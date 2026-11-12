//! Shared graphics MSI owner, dispatching WD0 Frame Complete and VCS0 MI_USER_INTERRUPT.
//!
//! Gen12 ADL-S uses the Gen11 interrupt tree (Linux v6.12 i915_irq.c and
//! i915_pci.c). Register definitions: TGL PRM Vol 2c pp419, 442, 1039,
//! 1677-1678; WD event definitions: Vol 2d p1155. This is the full-frame
//! interrupt route, independent of the undocumented WD/VDEnc pointer mode.
//!
//! The ISR takes no kernel mutex, allocates nothing, and never enters display
//! state, PCI configuration, GuC CTB handling or the scheduler's task polling.
//! It acknowledges only owned WD and VCS0 user events. GuC/context-switch routes
//! remain separate; new owners must join this dispatcher, never replace its MSI.

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
static WD_ROUTE_ENABLED: AtomicBool = AtomicBool::new(false);
static VCS0_ROUTE_ENABLED: AtomicBool = AtomicBool::new(false);
static IDENTITY_TIMEOUT_CYCLES: AtomicU64 = AtomicU64::new(1);
static VCS0_GENERATION: generation::Generation = generation::Generation::new();
static VCS0_WAKER: AtomicWaker = AtomicWaker::new();
static VCS0_COMPLETIONS: AtomicU64 = AtomicU64::new(0);
static LAST_GT_BANK: AtomicU32 = AtomicU32::new(0);
static LAST_GT_IDENTITY: AtomicU32 = AtomicU32::new(0);
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
    wd_enabled: bool,
    vcs0_enabled: bool,
    saved_master: u32,
    saved_display: u32,
    saved_misc_mask: u32,
    saved_misc_enable: u32,
    saved_wd_mask: u32,
}

static CONTROL: Mutex<Control> = Mutex::new(Control {
    msi_owned: false,
    wd_enabled: false,
    vcs0_enabled: false,
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
    fn timestamp(&mut self) -> u64 {
        unsafe { core::arch::x86_64::_rdtsc() }
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
        "intel/irq: route rejected stage={} register=0x{:X} value=0x{:08X} pointer_mode=unchanged\n",
        stage, register, value);
    false
}

impl Control {
    fn routes(&self) -> Routes {
        Routes {
            wd: self.wd_enabled,
            vcs0: self.vcs0_enabled,
        }
    }
    fn master_enable(&self) -> u32 {
        self.routes().master_enable(self.saved_master)
    }
}

fn platform_ready(dev: super::Dev) -> bool {
    if crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 0) != 0x4680_8086
        || dev.mmio_len < REQUIRED_MMIO_BYTES
        || FAULTED.load(Ordering::Acquire)
    {
        return reject("platform-or-bar-or-fault", 0, dev.device_id as u32);
    }
    true
}

/// Enter with the task-only control lock, leave successful admission quiesced.
/// Existing owned sources may become pending; they are serviced after enabling
/// the master again, never discarded as part of adding a different route.
fn prepare_owner(
    control: &mut Control,
    dev: super::Dev,
    registers: &mut Mmio,
    wd_powered: bool,
) -> bool {
    let owned = control.routes();
    let previous_master = registers.read(GFX_MASTER);
    if !owned.any() && previous_master & MASTER_ENABLE != 0 {
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
    quiesce(registers);
    if FAULTED.load(Ordering::Acquire) {
        finish_configuration(registers, control.master_enable());
        return reject("dispatcher-faulted", GFX_MASTER, previous_master);
    }
    if !owned.any() {
        control.saved_master = previous_master & MASTER_ENABLE;
    }
    if let Some((register, value)) = unowned_enable(registers, owned, wd_powered) {
        finish_configuration(registers, control.master_enable());
        return reject("unowned-source-enabled", register, value);
    }
    let pending = registers.read(GFX_MASTER) & !MASTER_ENABLE;
    if pending & !owned.master_bits() != 0 {
        finish_configuration(registers, control.master_enable());
        return reject("unowned-source-pending", GFX_MASTER, pending);
    }
    IDENTITY_TIMEOUT_CYCLES.store((crate::time::tsc_hz() / 10_000).max(1), Ordering::Release);
    MMIO_BASE.store(dev.mmio as usize, Ordering::Release);
    if !control.msi_owned {
        let destination = crate::percpu::cpu_slots()
            .iter()
            .find(|slot| slot.slot == 0)
            .map(|slot| slot.lapic_id);
        let Some(destination) = destination.filter(|destination| *destination <= u8::MAX as u32)
        else {
            finish_configuration(registers, control.master_enable());
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
            finish_configuration(registers, control.master_enable());
            return reject("msi-programming-readback", 0, 0);
        }
        control.msi_owned = true;
    }
    true
}

/// Own host user interrupts for the retained VCS0 encoder context. This GT
/// lease intentionally survives WD capture rollovers and WD power teardown.
/// It neither enables nor acknowledges GuC or context-switch interrupt bits.
pub(super) fn enable_vcs0_user(dev: super::Dev) -> bool {
    if !platform_ready(dev) {
        return false;
    }
    let mut control = CONTROL.lock();
    if control.vcs0_enabled {
        return true;
    }
    let fused = crate::intel::media_vdbox_mask(dev);
    if fused & 1 == 0 || fused & !0x5 != 0 {
        return reject("unsupported-vcs-topology", 0x9140, fused as u32);
    }
    let mut registers = Mmio(dev.mmio as usize);
    let wd_powered = WD_POWERED.load(Ordering::Acquire);
    if !prepare_owner(&mut control, dev, &mut registers, wd_powered) {
        return false;
    }
    if !configure_vcs0_user(&mut registers, fused & 4 != 0) {
        finish_configuration(&mut registers, control.master_enable());
        return reject(
            "vcs0-user-register-readback",
            VIDEO_ENABLE,
            registers.read(VIDEO_ENABLE),
        );
    }
    control.vcs0_enabled = true;
    VCS0_ROUTE_ENABLED.store(true, Ordering::Release);
    finish_configuration(&mut registers, control.master_enable());
    crate::log_info!(target: "gfx";
        "intel/irq: vcs0 user route enabled vector=0x{:02X} event=mi-user-bit0 gt_bank=1 gt_bit=0 identity_class=1 identity_instance=0 vcs2_user_masked={} lifetime=retained-encoder-context guc_irq_bits=unchanged wait=atomic-waker watchdog=one-shot\n",
        GRAPHICS_VECTOR, (fused & 4 != 0) as u8);
    true
}

/// WD admission holds its display power wells before calling this function.
pub(super) fn enable_wd(dev: super::Dev) -> bool {
    if !platform_ready(dev) {
        return false;
    }
    let mut control = CONTROL.lock();
    if control.wd_enabled {
        return true;
    }
    let mut registers = Mmio(dev.mmio as usize);
    if !prepare_owner(&mut control, dev, &mut registers, true) {
        return false;
    }
    if let Some((register, value)) = unowned_wd_enable(&mut registers) {
        finish_configuration(&mut registers, control.master_enable());
        return reject("unowned-wd-source-enabled", register, value);
    }
    control.saved_display = registers.read(DISPLAY_CONTROL) & MASTER_ENABLE;
    control.saved_misc_mask = registers.read(MISC_MASK) & MISC_WD0;
    control.saved_misc_enable = registers.read(MISC_ENABLE) & MISC_WD0;
    control.saved_wd_mask = registers.read(WD_MASK) & WD_FRAME_COMPLETE;
    // No capture is in flight. Drain the owned stale WD leaf and parent only.
    registers.write(WD_IDENTITY, WD_FRAME_COMPLETE);
    let remaining = registers.read(WD_IDENTITY);
    if remaining != 0 {
        finish_configuration(&mut registers, control.master_enable());
        return reject("unowned-wd-source-pending", WD_IDENTITY, remaining);
    }
    registers.write(MISC_IDENTITY, MISC_WD0);
    let _ = registers.read(MISC_IDENTITY);
    WD_POWERED.store(true, Ordering::Release);
    let wd_mask = registers.read(WD_MASK) & !WD_FRAME_COMPLETE;
    registers.write(WD_MASK, wd_mask);
    let misc_mask = registers.read(MISC_MASK) & !MISC_WD0;
    registers.write(MISC_MASK, misc_mask);
    let misc_enable = registers.read(MISC_ENABLE) | MISC_WD0;
    registers.write(MISC_ENABLE, misc_enable);
    registers.write(DISPLAY_CONTROL, MASTER_ENABLE);
    let _ = registers.read(DISPLAY_CONTROL);
    control.wd_enabled = true;
    WD_ROUTE_ENABLED.store(true, Ordering::Release);
    finish_configuration(&mut registers, control.master_enable());
    crate::log_info!(target: "gfx";
        "intel/irq: wd0 route enabled vector=0x{:02X} event=frame-complete-bit7 master=gen11 physical=adls-msi wait=atomic-waker watchdog=one-shot\n", GRAPHICS_VECTOR);
    true
}

/// Mask and drain WD while its registers remain powered. All in-flight ISR
/// register accesses have retired before this returns and power can be freed.
pub(super) fn disable_wd(dev: super::Dev) {
    let mut control = CONTROL.lock();
    if !control.wd_enabled {
        return;
    }
    let mut registers = Mmio(dev.mmio as usize);
    quiesce(&mut registers);
    WD_ROUTE_ENABLED.store(false, Ordering::Release);
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
    control.wd_enabled = false;
    finish_configuration(&mut registers, control.master_enable());
    WD_GENERATION.notify();
    WD_WAKER.wake();
    crate::log_info!(target: "gfx";
        "intel/irq: wd0 route disabled snapshot={:?}\n", snapshot());
}

pub(super) fn wd_generation() -> u64 {
    WD_GENERATION.observe()
}
pub(super) fn wd_route_ready() -> bool {
    WD_ROUTE_ENABLED.load(Ordering::Acquire) && !FAULTED.load(Ordering::Acquire)
}

pub(super) fn vcs0_generation() -> u64 {
    VCS0_GENERATION.observe()
}
pub(super) fn vcs0_route_ready() -> bool {
    VCS0_ROUTE_ENABLED.load(Ordering::Acquire) && !FAULTED.load(Ordering::Acquire)
}

/// A notification has no sequence meaning. The encoder must observe the
/// generation before reading its owned, ordered GGTT breadcrumb, then recheck
/// that exact submission sequence on every wake and on the final watchdog.
pub(super) async fn wait_vcs0_after(
    observed: u64,
    mut watchdog: Pin<&mut trueos_time::Timer>,
) -> bool {
    poll_fn(|cx| {
        if VCS0_GENERATION.changed_after_register(observed, || VCS0_WAKER.register(cx.waker()))
            || !vcs0_route_ready()
        {
            return Poll::Ready(true);
        }
        watchdog.as_mut().poll(cx).map(|()| false)
    })
    .await
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
    pub vcs0_completions: u64,
    pub vcs0_route_enabled: bool,
    pub faulted: bool,
    pub master: u32,
    pub display: u32,
    pub misc: u32,
    pub wd: u32,
    pub gt_bank: u32,
    pub gt_identity: u32,
}
pub(crate) fn snapshot() -> Snapshot {
    Snapshot {
        interrupts: INTERRUPTS.load(Ordering::Acquire),
        completions: COMPLETIONS.load(Ordering::Acquire),
        vcs0_completions: VCS0_COMPLETIONS.load(Ordering::Acquire),
        vcs0_route_enabled: VCS0_ROUTE_ENABLED.load(Ordering::Acquire),
        faulted: FAULTED.load(Ordering::Acquire),
        master: LAST_MASTER.load(Ordering::Acquire),
        display: LAST_DISPLAY.load(Ordering::Acquire),
        misc: LAST_MISC.load(Ordering::Acquire),
        wd: LAST_WD.load(Ordering::Acquire),
        gt_bank: LAST_GT_BANK.load(Ordering::Acquire),
        gt_identity: LAST_GT_IDENTITY.load(Ordering::Acquire),
    }
}

extern "x86-interrupt" fn graphics_isr(_frame: InterruptStackFrame) {
    HANDLERS.fetch_add(1, Ordering::SeqCst);
    let base = MMIO_BASE.load(Ordering::Acquire);
    if base != 0
        && !CONFIGURING.load(Ordering::SeqCst)
        && !FAULTED.load(Ordering::Acquire)
        && (WD_ROUTE_ENABLED.load(Ordering::Acquire) || VCS0_ROUTE_ENABLED.load(Ordering::Acquire))
    {
        INTERRUPTS.fetch_add(1, Ordering::Relaxed);
        let mut registers = Mmio(base);
        let routes = Routes {
            wd: WD_ROUTE_ENABLED.load(Ordering::Acquire) && WD_POWERED.load(Ordering::Acquire),
            vcs0: VCS0_ROUTE_ENABLED.load(Ordering::Acquire),
        };
        let result = dispatch(
            &mut registers,
            routes,
            IDENTITY_TIMEOUT_CYCLES.load(Ordering::Acquire),
        );
        LAST_MASTER.store(result.master, Ordering::Release);
        LAST_DISPLAY.store(result.display, Ordering::Release);
        LAST_MISC.store(result.misc, Ordering::Release);
        LAST_WD.store(result.wd, Ordering::Release);
        LAST_GT_BANK.store(result.gt_bank, Ordering::Release);
        LAST_GT_IDENTITY.store(result.gt_identity, Ordering::Release);
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
        if result.vcs0_user {
            VCS0_COMPLETIONS.fetch_add(1, Ordering::Relaxed);
        }
        if routes.wd && result.master & MASTER_DISPLAY != 0 && !CONFIGURING.load(Ordering::SeqCst) {
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
        if result.vcs0_user || result.unexpected {
            VCS0_GENERATION.notify();
            VCS0_WAKER.wake();
        }
    }
    HANDLERS.fetch_sub(1, Ordering::SeqCst);
    crate::remote_work_wake::local_eoi();
}
