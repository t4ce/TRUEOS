//! One RTL8125B MSI route with task-owned RX/TX processing.
//!
//! The ISR only masks/acknowledges owned causes and publishes readiness. It
//! takes no network/device mutex and never allocates or polls an executor.

use atomic_waker::AtomicWaker;
use core::{future::poll_fn, sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering}, task::Poll};
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

mod registers;
use registers::Registers;

pub(crate) const NETWORK_VECTOR: u8 = 0x45;
const _: () = {
    assert!(NETWORK_VECTOR != crate::chronos::CHRONOS_TIMER_VECTOR);
    assert!(NETWORK_VECTOR != crate::remote_work_wake::REMOTE_WORK_WAKE_VECTOR);
    assert!(NETWORK_VECTOR != crate::hv::control_kick::LIFECYCLE_KICK_VECTOR);
    assert!(NETWORK_VECTOR != crate::live_update::RENDEZVOUS_VECTOR);
    assert!(NETWORK_VECTOR != crate::intel::interrupts::GRAPHICS_VECTOR);
};
static CONFIGURING: AtomicBool = AtomicBool::new(false);
static READY: AtomicBool = AtomicBool::new(false);
static FAULTED: AtomicBool = AtomicBool::new(false);
static DEVICE: AtomicUsize = AtomicUsize::new(usize::MAX);
static MMIO: AtomicUsize = AtomicUsize::new(0);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static WAKER: AtomicWaker = AtomicWaker::new();
static INTERRUPTS: AtomicU64 = AtomicU64::new(0);
static REARM_RACES: AtomicU64 = AtomicU64::new(0);
static LAST_STATUS: AtomicUsize = AtomicUsize::new(0);
static HANDLERS: AtomicUsize = AtomicUsize::new(0);

struct Mmio(usize);
impl Registers for Mmio {
    fn read(&mut self, offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.0 + offset) as *const u32) }
    }
    fn write(&mut self, offset: usize, value: u32) {
        unsafe { core::ptr::write_volatile((self.0 + offset) as *mut u32, value) }
    }
}

pub(crate) fn interrupt_install(idt: &mut InterruptDescriptorTable) {
    idt[NETWORK_VECTOR].set_handler_fn(network_isr);
}

fn available_msi(bus: u8, slot: u8, function: u8) -> Option<u16> {
    if crate::pci::config_read_u16(bus, slot, function, 0x06) & (1 << 4) == 0 { return None; }
    let mut capability = u16::from(crate::pci::config_read_u8(bus, slot, function, 0x34) & !3);
    let mut msi = None;
    for _ in 0..48 {
        if !(0x40..0x100).contains(&capability) { break; }
        let kind = crate::pci::config_read_u8(bus, slot, function, capability);
        let control = crate::pci::config_read_u16(bus, slot, function, capability + 2);
        if kind == 0x05 {
            if control & 1 != 0 { return None; }
            msi = Some(capability);
        } else if kind == 0x11 && control & (1 << 15) != 0 { return None; }
        capability = u16::from(crate::pci::config_read_u8(bus, slot, function, capability + 1) & !3);
    }
    msi
}

/// Initialization is serialized before network tasks are published. Only the
/// physical 10ec:8125 rev05 path is admitted; other NICs retain their poller.
/// An enabled MSI/MSI-X belonging to another owner is never taken over.
pub(crate) fn configure_r8125(index: usize, bus: u8, slot: u8, function: u8, mmio: usize) -> bool {
    if is_ready(index) { return true; }
    if mmio == 0 || index >= super::adapter::MAX_NET_DEVICES
        || crate::pci::config_read_u32(bus, slot, function, 0) != 0x8125_10ec
        || crate::pci::config_read_u8(bus, slot, function, 8) != 5
        || READY.load(Ordering::Acquire)
        || CONFIGURING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err()
    { return false; }
    let admitted = configure(index, bus, slot, function, mmio);
    CONFIGURING.store(false, Ordering::Release);
    if !admitted {
        crate::log_warn!(target: "net";
            "net/irq: RTL8125 route unavailable dev={} bdf={:02x}:{:02x}.{} fallback=polled\n", index, bus, slot, function);
    }
    admitted
}

fn configure(index: usize, bus: u8, slot: u8, function: u8, mmio: usize) -> bool {
    let Some(capability) = available_msi(bus, slot, function) else { return false; };
    let Some(destination) = crate::percpu::cpu_slots().iter().find(|cpu| cpu.slot == 0)
        .map(|cpu| cpu.lapic_id).filter(|id| *id <= u8::MAX as u32) else { return false; };
    let mut registers = Mmio(mmio);
    if registers.read(registers::MASK) != 0 { return false; }
    registers::mask(&mut registers);
    let command = crate::pci::config_read_u16(bus, slot, function, 0x04);
    let control = crate::pci::config_read_u16(bus, slot, function, capability + 2);
    let address = crate::pci::config_read_u32(bus, slot, function, capability + 4);
    let is_64 = control & (1 << 7) != 0;
    let high = if is_64 { crate::pci::config_read_u32(bus, slot, function, capability + 8) } else { 0 };
    let data_offset = if is_64 { 12 } else { 8 };
    let data = crate::pci::config_read_u16(bus, slot, function, capability + data_offset);
    DEVICE.store(index, Ordering::Release);
    MMIO.store(mmio, Ordering::Release);
    if !crate::pci::enable_single_msi(bus, slot, function, NETWORK_VECTOR, destination) {
        crate::pci::config_write_u16(bus, slot, function, capability + 2, control & !1);
        crate::pci::config_write_u32(bus, slot, function, capability + 4, address);
        if is_64 { crate::pci::config_write_u32(bus, slot, function, capability + 8, high); }
        crate::pci::config_write_u16(bus, slot, function, capability + data_offset, data);
        crate::pci::config_write_u16(bus, slot, function, 0x04, command);
        MMIO.store(0, Ordering::Release);
        DEVICE.store(usize::MAX, Ordering::Release);
        return false;
    }
    // Leave masked until the service's initial descriptor drain and rearm.
    READY.store(true, Ordering::Release);
    crate::log_info!(target: "net";
        "net/irq: RTL8125 MSI owned dev={} bdf={:02x}:{:02x}.{} vector=0x{:02x} destination={} mask=0x{:02x} scheduling=deferred-rx-tx\n",
        index, bus, slot, function, NETWORK_VECTOR, destination, registers::OWNED_EVENTS);
    true
}

/// Ownership remains stable on fault: the service retains RX responsibility
/// and explicitly falls back to polling instead of reviving a retired task.
pub(crate) fn is_ready(index: usize) -> bool {
    READY.load(Ordering::Acquire) && DEVICE.load(Ordering::Acquire) == index
}
pub(crate) fn is_faulted(index: usize) -> bool { is_ready(index) && FAULTED.load(Ordering::Acquire) }
pub(crate) fn generation(_index: usize) -> u64 { GENERATION.load(Ordering::Acquire) }
pub(crate) async fn wait_after(index: usize, observed: u64) {
    poll_fn(|cx| {
        if generation(index) != observed || is_faulted(index) { return Poll::Ready(()); }
        WAKER.register(cx.waker());
        if generation(index) != observed || is_faulted(index) { Poll::Ready(()) } else { Poll::Pending }
    }).await;
}
pub(crate) fn mask(index: usize) {
    if is_ready(index) { registers::mask(&mut Mmio(MMIO.load(Ordering::Acquire))); }
}
pub(crate) fn rearm(index: usize) -> bool {
    if !is_ready(index) || is_faulted(index) { return false; }
    let observed = generation(index);
    let pending = match registers::rearm(&mut Mmio(MMIO.load(Ordering::Acquire)), || {
        HANDLERS.load(Ordering::SeqCst) != 0 || generation(index) != observed
    }) {
        Ok(pending) => { if pending { REARM_RACES.fetch_add(1, Ordering::Relaxed); } pending }
        Err(()) => { FAULTED.store(true, Ordering::Release); notify(); true }
    };
    if is_faulted(index) {
        // A concurrent handler may have latched the fault before our arm
        // write. Its mask must remain effective throughout fallback polling.
        mask(index);
        return true;
    }
    pending
}
fn notify() { GENERATION.fetch_add(1, Ordering::AcqRel); WAKER.wake(); }

#[derive(Clone, Copy, Debug)]
pub(crate) struct Snapshot { pub interrupts: u64, pub rearm_races: u64, pub last_status: u32, pub faulted: bool }
pub(crate) fn snapshot() -> Snapshot {
    Snapshot { interrupts: INTERRUPTS.load(Ordering::Acquire), rearm_races: REARM_RACES.load(Ordering::Acquire),
        last_status: LAST_STATUS.load(Ordering::Acquire) as u32, faulted: FAULTED.load(Ordering::Acquire) }
}

extern "x86-interrupt" fn network_isr(_frame: InterruptStackFrame) {
    HANDLERS.fetch_add(1, Ordering::SeqCst);
    if READY.load(Ordering::Acquire) && !FAULTED.load(Ordering::Acquire) {
        match registers::dispatch(&mut Mmio(MMIO.load(Ordering::Acquire))) {
            Ok(0) => {}
            Ok(status) => { INTERRUPTS.fetch_add(1, Ordering::Relaxed); LAST_STATUS.store(status as usize, Ordering::Release); notify(); }
            Err(()) => { FAULTED.store(true, Ordering::Release); notify(); }
        }
    }
    // Publish the generation before retiring the in-flight marker. Rearm can
    // therefore recognize every interval of a legitimate ISR masking the NIC.
    HANDLERS.fetch_sub(1, Ordering::SeqCst);
    crate::remote_work_wake::local_eoi();
}
