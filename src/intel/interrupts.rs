//! Shared graphics MSI owner, initially dispatching only WD0 Frame Complete.
//!
//! Gen12 ADL-S uses the Gen11 interrupt tree (Linux v6.12 i915_irq.c and
//! i915_pci.c). Register definitions: TGL PRM Vol 2c pp419, 442, 1039,
//! 1677-1678; WD event definitions: Vol 2d p1155. This is the full-frame
//! interrupt route, independent of the undocumented WD/VDEnc pointer mode.
//!
//! The ISR takes no kernel mutex, allocates nothing, and never enters display
//! state, PCI configuration, GuC CTB handling or the scheduler's task polling.
//! It acknowledges only its owned WD leaf and parent. Future GT/GuC routes
//! must be added to this dispatcher, rather than taking over the same MSI.

use atomic_waker::AtomicWaker;
use core::future::{Future, poll_fn};
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use core::task::Poll;
use spin::Mutex;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

mod demux;
mod generation;
use demux::*;

// 0x40: Chronos, 0x41: remote work, 0x42: lifecycle, 0x43: rendezvous.
pub(crate) const GRAPHICS_VECTOR: u8 = 0x44;
const _: () = {
    assert!(GRAPHICS_VECTOR != crate::chronos::CHRONOS_TIMER_VECTOR);
    assert!(GRAPHICS_VECTOR != crate::remote_work_wake::REMOTE_WORK_WAKE_VECTOR);
    assert!(GRAPHICS_VECTOR != crate::hv::control_kick::LIFECYCLE_KICK_VECTOR);
    assert!(GRAPHICS_VECTOR != crate::live_update::RENDEZVOUS_VECTOR);
};

static MMIO_BASE: AtomicUsize = AtomicUsize::new(0);
static CONFIGURING: AtomicBool = AtomicBool::new(false);
static HANDLERS: AtomicUsize = AtomicUsize::new(0);
static WD_POWERED: AtomicBool = AtomicBool::new(false);
static ROUTE_ENABLED: AtomicBool = AtomicBool::new(false);
static FAULTED: AtomicBool = AtomicBool::new(false);
static WD_GENERATION: generation::Generation = generation::Generation::new();
static WD_WAKER: AtomicWaker = AtomicWaker::new();
static INTERRUPTS: AtomicU64 = AtomicU64::new(0);
static COMPLETIONS: AtomicU64 = AtomicU64::new(0);
static LAST_MASTER: AtomicU32 = AtomicU32::new(0);
static LAST_DISPLAY: AtomicU32 = AtomicU32::new(0);
static LAST_MISC: AtomicU32 = AtomicU32::new(0);
static LAST_WD: AtomicU32 = AtomicU32::new(0);

struct Control {
    msi_owned: bool,
    enabled: bool,
    saved_master: u32,
    saved_display: u32,
    saved_misc_mask: u32,
    saved_misc_enable: u32,
    saved_wd_mask: u32,
}

static CONTROL: Mutex<Control> = Mutex::new(Control {
    msi_owned: false,
    enabled: false,
    saved_master: 0,
    saved_display: 0,
    saved_misc_mask: 0,
    saved_misc_enable: 0,
    saved_wd_mask: 0,
});

struct Mmio(usize);
impl Registers for Mmio {
    fn read(&mut self, register: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.0 + register) as *const u32) }
    }
    fn write(&mut self, register: usize, value: u32) {
        unsafe { core::ptr::write_volatile((self.0 + register) as *mut u32, value) }
    }
}

pub(crate) fn interrupt_install(idt: &mut InterruptDescriptorTable) {
    idt[GRAPHICS_VECTOR].set_handler_fn(graphics_isr);
}

/// Quiesce direct ISR MMIO before changing masks or WD's power lease. The
/// control mutex is task-only. An ISR arriving during configuration exits
/// without MMIO, and does not re-enable the masked master.
fn quiesce(registers: &mut Mmio) {
    CONFIGURING.store(true, Ordering::SeqCst);
    registers.write(GFX_MASTER, 0);
    let _ = registers.read(GFX_MASTER);
    while HANDLERS.load(Ordering::SeqCst) != 0 {
        core::hint::spin_loop();
    }
    // An ISR may have passed its CONFIGURING check just before publication
    // and re-enabled the master while draining. Close that race after it exits.
    registers.write(GFX_MASTER, 0);
    let _ = registers.read(GFX_MASTER);
}

fn finish_configuration(registers: &mut Mmio, master: u32) {
    CONFIGURING.store(false, Ordering::SeqCst);
    if !FAULTED.load(Ordering::Acquire) {
        registers.write(GFX_MASTER, master & MASTER_ENABLE);
        let _ = registers.read(GFX_MASTER);
        if FAULTED.load(Ordering::Acquire) {
            registers.write(GFX_MASTER, 0);
            let _ = registers.read(GFX_MASTER);
        }
    }
}

fn available_msi(dev: super::Dev) -> Option<u16> {
    if crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x06) & (1 << 4) == 0 {
        return None;
    }
    let mut capability =
        u16::from(crate::pci::config_read_u8(dev.bus, dev.slot, dev.function, 0x34) & !3);
    let mut msi = None;
    for _ in 0..48 {
        if !(0x40..0x100).contains(&capability) {
            break;
        }
        let kind = crate::pci::config_read_u8(dev.bus, dev.slot, dev.function, capability);
        let control = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, capability + 2);
        if kind == 0x05 {
            if control & 1 != 0 {
                return None;
            }
            msi = Some(capability);
        } else if kind == 0x11 && control & (1 << 15) != 0 {
            return None;
        }
        capability = u16::from(
            crate::pci::config_read_u8(dev.bus, dev.slot, dev.function, capability + 1) & !3,
        );
    }
    msi
}

fn reject(stage: &str, register: usize, value: u32) -> bool {
    crate::log_warn!(target: "gfx";
        "intel/irq: WD route rejected stage={} register=0x{:X} value=0x{:08X} pointer_mode=unchanged\n",
        stage, register, value);
    false
}

/// Called with WD's power wells held, before any capture trigger. A physical
/// device check is essential: the experimental PCI boundary aliases TGL as
/// 0x4680, and must not silently admit it to this ADL-S hardware stage.
pub(super) fn enable_wd(dev: super::Dev) -> bool {
    if crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 0) != 0x4680_8086
        || dev.mmio_len < REQUIRED_MMIO_BYTES
        || FAULTED.load(Ordering::Acquire)
    {
        return reject("platform-or-bar-or-fault", 0, dev.device_id as u32);
    }
    let mut control = CONTROL.lock();
    if control.enabled {
        return true;
    }
    let mut registers = Mmio(dev.mmio as usize);
    let previous_master = registers.read(GFX_MASTER);
    // Another enabled interrupt owner needs an explicit shared registration;
    // do not repoint an active MSI or adopt its top-level enable.
    if previous_master & MASTER_ENABLE != 0 {
        return reject("unowned-master-enabled", GFX_MASTER, previous_master);
    }
    let msi_capability = if !control.msi_owned {
        let Some(capability) = available_msi(dev) else {
            return reject("msi-unavailable-or-owned", 0, 0);
        };
        Some(capability)
    } else {
        None
    };
    quiesce(&mut registers);
    control.saved_master = previous_master & MASTER_ENABLE;
    control.saved_display = registers.read(DISPLAY_CONTROL) & MASTER_ENABLE;
    control.saved_misc_mask = registers.read(MISC_MASK) & MISC_WD0;
    control.saved_misc_enable = registers.read(MISC_ENABLE) & MISC_WD0;
    control.saved_wd_mask = registers.read(WD_MASK) & WD_FRAME_COMPLETE;
    if let Some((register, value)) = unowned_enable(&mut registers) {
        finish_configuration(&mut registers, control.saved_master);
        return reject("unowned-source-enabled", register, value);
    }

    // Drain only stale owned status while no capture can be in flight.
    registers.write(WD_IDENTITY, WD_FRAME_COMPLETE);
    let remaining_wd = registers.read(WD_IDENTITY);
    if remaining_wd == 0 {
        registers.write(MISC_IDENTITY, MISC_WD0);
        let _ = registers.read(MISC_IDENTITY);
    }
    let remaining_master = registers.read(GFX_MASTER) & !MASTER_ENABLE;
    let remaining_display = registers.read(DISPLAY_CONTROL) & !MASTER_ENABLE;
    if remaining_wd != 0 || remaining_master != 0 || remaining_display != 0 {
        finish_configuration(&mut registers, control.saved_master);
        return reject("unowned-source-pending", GFX_MASTER, remaining_master);
    }
    MMIO_BASE.store(dev.mmio as usize, Ordering::Release);
    if !control.msi_owned {
        let destination = crate::percpu::cpu_slots()
            .iter()
            .find(|slot| slot.slot == 0)
            .map(|slot| slot.lapic_id);
        let Some(destination) = destination.filter(|destination| *destination <= u8::MAX as u32)
        else {
            finish_configuration(&mut registers, control.saved_master);
            return reject("msi-destination-unavailable", 0, 0);
        };
        let capability = msi_capability.expect("unowned MSI was checked before configuration");
        let saved_command = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);
        let saved_msi_control =
            crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, capability + 2);
        let saved_address =
            crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, capability + 4);
        let is_64 = saved_msi_control & (1 << 7) != 0;
        let saved_high = if is_64 {
            crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, capability + 8)
        } else {
            0
        };
        let data_offset = if is_64 { 12 } else { 8 };
        let saved_data =
            crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, capability + data_offset);
        if !crate::pci::enable_single_msi(
            dev.bus,
            dev.slot,
            dev.function,
            GRAPHICS_VECTOR,
            destination,
        ) {
            // The PCI helper can fail readback after partial programming.
            // Restore its disabled capability before allowing another attempt.
            crate::pci::config_write_u16(
                dev.bus,
                dev.slot,
                dev.function,
                capability + 2,
                saved_msi_control & !1,
            );
            crate::pci::config_write_u32(
                dev.bus,
                dev.slot,
                dev.function,
                capability + 4,
                saved_address,
            );
            if is_64 {
                crate::pci::config_write_u32(
                    dev.bus,
                    dev.slot,
                    dev.function,
                    capability + 8,
                    saved_high,
                );
            }
            crate::pci::config_write_u16(
                dev.bus,
                dev.slot,
                dev.function,
                capability + data_offset,
                saved_data,
            );
            crate::pci::config_write_u16(dev.bus, dev.slot, dev.function, 0x04, saved_command);
            finish_configuration(&mut registers, control.saved_master);
            return reject("msi-programming-readback", 0, 0);
        }
        control.msi_owned = true;
    }
    WD_POWERED.store(true, Ordering::Release);
    let wd_mask = registers.read(WD_MASK) & !WD_FRAME_COMPLETE;
    registers.write(WD_MASK, wd_mask);
    let misc_mask = registers.read(MISC_MASK) & !MISC_WD0;
    registers.write(MISC_MASK, misc_mask);
    let misc_enable = registers.read(MISC_ENABLE) | MISC_WD0;
    registers.write(MISC_ENABLE, misc_enable);
    registers.write(DISPLAY_CONTROL, MASTER_ENABLE);
    let _ = registers.read(DISPLAY_CONTROL);
    control.enabled = true;
    ROUTE_ENABLED.store(true, Ordering::Release);
    finish_configuration(&mut registers, MASTER_ENABLE);
    crate::log_info!(target: "gfx";
        "intel/irq: wd0 route enabled vector=0x{:02X} event=frame-complete-bit7 master=gen11 physical=adls-msi wait=atomic-waker watchdog=one-shot\n", GRAPHICS_VECTOR);
    true
}

/// Mask and drain WD while its registers remain powered. All in-flight ISR
/// register accesses have retired before this returns and power can be freed.
pub(super) fn disable_wd(dev: super::Dev) {
    let mut control = CONTROL.lock();
    if !control.enabled {
        return;
    }
    let mut registers = Mmio(dev.mmio as usize);
    quiesce(&mut registers);
    ROUTE_ENABLED.store(false, Ordering::Release);
    let wd_mask = (registers.read(WD_MASK) & !WD_FRAME_COMPLETE) | control.saved_wd_mask;
    registers.write(WD_MASK, wd_mask);
    let misc_enable = (registers.read(MISC_ENABLE) & !MISC_WD0) | control.saved_misc_enable;
    registers.write(MISC_ENABLE, misc_enable);
    let misc_mask = (registers.read(MISC_MASK) & !MISC_WD0) | control.saved_misc_mask;
    registers.write(MISC_MASK, misc_mask);
    registers.write(WD_IDENTITY, WD_FRAME_COMPLETE);
    if registers.read(WD_IDENTITY) == 0 {
        registers.write(MISC_IDENTITY, MISC_WD0);
        let _ = registers.read(MISC_IDENTITY);
    }
    WD_POWERED.store(false, Ordering::Release);
    registers.write(DISPLAY_CONTROL, control.saved_display);
    let _ = registers.read(DISPLAY_CONTROL);
    control.enabled = false;
    finish_configuration(&mut registers, control.saved_master);
    WD_GENERATION.notify();
    WD_WAKER.wake();
    crate::log_info!(target: "gfx";
        "intel/irq: wd0 route disabled snapshot={:?}\n", snapshot());
}

pub(super) fn wd_generation() -> u64 {
    WD_GENERATION.observe()
}
pub(super) fn wd_route_ready() -> bool {
    ROUTE_ENABLED.load(Ordering::Acquire) && !FAULTED.load(Ordering::Acquire)
}

/// One waiter follows the serialized WD owner. A generation check on both
/// sides of waker registration closes the trigger/register interrupt race.
/// This does not consume or fabricate a completed frame.
pub(super) async fn wait_wd_after(
    observed: u64,
    mut watchdog: Pin<&mut trueos_time::Timer>,
) -> bool {
    poll_fn(|cx| {
        if WD_GENERATION.changed_after_register(observed, || WD_WAKER.register(cx.waker()))
            || !wd_route_ready()
        {
            return Poll::Ready(true);
        }
        watchdog.as_mut().poll(cx).map(|()| false)
    })
    .await
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Snapshot {
    pub interrupts: u64,
    pub completions: u64,
    pub faulted: bool,
    pub master: u32,
    pub display: u32,
    pub misc: u32,
    pub wd: u32,
}
pub(crate) fn snapshot() -> Snapshot {
    Snapshot {
        interrupts: INTERRUPTS.load(Ordering::Acquire),
        completions: COMPLETIONS.load(Ordering::Acquire),
        faulted: FAULTED.load(Ordering::Acquire),
        master: LAST_MASTER.load(Ordering::Acquire),
        display: LAST_DISPLAY.load(Ordering::Acquire),
        misc: LAST_MISC.load(Ordering::Acquire),
        wd: LAST_WD.load(Ordering::Acquire),
    }
}

extern "x86-interrupt" fn graphics_isr(_frame: InterruptStackFrame) {
    HANDLERS.fetch_add(1, Ordering::SeqCst);
    let base = MMIO_BASE.load(Ordering::Acquire);
    if base != 0
        && !CONFIGURING.load(Ordering::SeqCst)
        && !FAULTED.load(Ordering::Acquire)
        && ROUTE_ENABLED.load(Ordering::Acquire)
    {
        INTERRUPTS.fetch_add(1, Ordering::Relaxed);
        let mut registers = Mmio(base);
        let result = dispatch(&mut registers, WD_POWERED.load(Ordering::Acquire));
        LAST_MASTER.store(result.master, Ordering::Release);
        LAST_DISPLAY.store(result.display, Ordering::Release);
        LAST_MISC.store(result.misc, Ordering::Release);
        LAST_WD.store(result.wd, Ordering::Release);
        if result.unexpected {
            FAULTED.store(true, Ordering::Release);
            // Configuration may have released CONFIGURING and enabled the
            // master while this handler was decoding the pending source.
            registers.write(GFX_MASTER, 0);
            let _ = registers.read(GFX_MASTER);
        }
        if result.frame_complete {
            COMPLETIONS.fetch_add(1, Ordering::Relaxed);
        }
        if result.master & MASTER_DISPLAY != 0 && !CONFIGURING.load(Ordering::SeqCst) {
            registers.write(DISPLAY_CONTROL, MASTER_ENABLE);
        }
        if !result.unexpected
            && !CONFIGURING.load(Ordering::SeqCst)
            && !FAULTED.load(Ordering::Acquire)
        {
            registers.write(GFX_MASTER, MASTER_ENABLE);
            let _ = registers.read(GFX_MASTER);
        }
        if result.frame_complete || result.unexpected {
            WD_GENERATION.notify();
            // AtomicWaker never waits for an interrupted registration. TRUEOS
            // executor wakers enqueue tasks; they do not poll them in the ISR.
            WD_WAKER.wake();
        }
    }
    HANDLERS.fetch_sub(1, Ordering::SeqCst);
    crate::remote_work_wake::local_eoi();
}
