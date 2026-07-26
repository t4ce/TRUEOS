//! Minimal host seam for the dormant TGA PCI card.
//!
//! TRUEOS owns only discovery, hotplug, BAR0 mapping, MSI completion, a raw LED
//! fallback, and two small fixed calls. The firmware/toolchain/model experiment
//! lives outside this repository.

pub(crate) mod protocol;

use atomic_waker::AtomicWaker;
use core::future::poll_fn;
use core::ptr::{NonNull, read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence};
use core::task::Poll;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

use crate::pci::PciDevice;
use protocol::{FunctionId, WorkPackage, WorkState};

const TGA_VENDOR_ID: u16 = 0x22c2;
const TGA_DEVICE_ID: u16 = 0x1100;
const TGA_PCI_OWNER: &str = "tga";
const TGA_EXPECTED_BAR0_SIZE: u64 = 1024;
const TGA_MAGIC_EXPECTED: u32 = protocol::HEARTBEAT_REPLY;
pub(crate) const TGA_COMPLETION_VECTOR: u8 = 0x42;

const TGA_PRESENCE_PROBE_MS: u64 = 1000;
const TGA_OFFLINE_RETRY_MS: u64 = 250;
const TGA_PRESENCE_MISS_THRESHOLD: u8 = 3;
const PCI_COMMAND_IO_SPACE: u16 = 1 << 0;
const PCI_COMMAND_MEM_SPACE: u16 = 1 << 1;

struct Tga {
    bus: u8,
    slot: u8,
    function: u8,
    bar_phys: u64,
    bar_size: u64,
    bar_is_64: bool,
    bar_assignment: TgaBarAssignment,
    mmio_base: usize,
    led_reg: usize,
    magic_reg: usize,
    work_package_reg: usize,
    doorbell_reg: usize,
    irq_ack_reg: usize,
}
// Safety: the MMIO addresses are accessed only while the device is published
// behind `TGA`, and all callers serialize through that mutex.
unsafe impl Send for Tga {}

#[derive(Copy, Clone)]
struct TgaHotplugSnapshot {
    bus: u8,
    slot: u8,
    function: u8,
    bar_phys: u64,
    bar_size: u64,
    bar_is_64: bool,
    bar_assignment: TgaBarAssignment,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum TgaBarAssignment {
    Firmware,
    Restored,
    Allocated,
}

impl TgaBarAssignment {
    const fn label(self) -> &'static str {
        match self {
            Self::Firmware => "firmware",
            Self::Restored => "restored",
            Self::Allocated => "allocated",
        }
    }
}

impl Tga {
    #[inline(always)]
    fn read_reg(reg: usize) -> u32 {
        unsafe { read_volatile(reg as *const u32) }
    }

    #[inline(always)]
    fn write_reg(reg: usize, value: u32) {
        unsafe { write_volatile(reg as *mut u32, value) };
    }

    fn protocol_magic(&self) -> u32 {
        Self::read_reg(self.magic_reg)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum OffloadTransportError {
    Offline,
    InvalidPackage,
    WriteVerification {
        word: u8,
        observed: u32,
        expected: u32,
    },
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Status {
    pub online: bool,
    pub protocol_alive: bool,
    pub generation: u32,
    pub bdf: Option<(u8, u8, u8)>,
    pub bar_phys: Option<u64>,
    pub bar_size: Option<u64>,
    pub msi_ready: bool,
    pub interrupts: u64,
}

static TGA: Mutex<Option<Tga>> = Mutex::new(None);
static TGA_LAST_MAP: Mutex<Option<(u64, usize)>> = Mutex::new(None);
static TGA_LAST_DISCONNECT: Mutex<Option<TgaHotplugSnapshot>> = Mutex::new(None);
static TGA_LAST_WORKING_LEASE: Mutex<Option<TgaHotplugSnapshot>> = Mutex::new(None);
static TGA_LINK_RECOVERY_ATTEMPTED: AtomicBool = AtomicBool::new(false);
static TGA_LIVENESS_LOGGED: AtomicBool = AtomicBool::new(false);
static TGA_CONNECTION_GENERATION: AtomicU32 = AtomicU32::new(0);
static TGA_IRQ_CONFIGURED: AtomicBool = AtomicBool::new(false);
static TGA_IRQ_CONFIG_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);
static TGA_COMPLETION_IRQ_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static TGA_COMPLETION_IRQ_COUNT: AtomicU64 = AtomicU64::new(0);
static TGA_COMPLETION_IRQ_WAKER: AtomicWaker = AtomicWaker::new();

pub(crate) fn interrupt_install(idt: &mut InterruptDescriptorTable) {
    idt[TGA_COMPLETION_VECTOR].set_handler_fn(tga_completion_isr);
}

#[allow(non_snake_case)]
extern "x86-interrupt" fn tga_completion_isr(_stack_frame: InterruptStackFrame) {
    TGA_COMPLETION_IRQ_COUNT.fetch_add(1, Ordering::Relaxed);
    TGA_COMPLETION_IRQ_SEQUENCE.fetch_add(1, Ordering::Release);
    TGA_COMPLETION_IRQ_WAKER.wake();
    crate::remote_work_wake::local_eoi();
}

pub(crate) fn completion_interrupt_count() -> u64 {
    TGA_COMPLETION_IRQ_COUNT.load(Ordering::Acquire)
}

pub(crate) fn completion_interrupt_configured() -> bool {
    TGA_IRQ_CONFIGURED.load(Ordering::Acquire)
}

pub(crate) fn arm_offload_interrupt() -> Result<u64, OffloadTransportError> {
    if !completion_interrupt_configured() {
        return Err(OffloadTransportError::Offline);
    }
    ack_offload_interrupt()?;
    fence(Ordering::SeqCst);
    Ok(TGA_COMPLETION_IRQ_SEQUENCE.load(Ordering::Acquire))
}

pub(crate) async fn wait_for_completion_interrupt(after: u64) -> u64 {
    poll_fn(|cx| {
        let observed = TGA_COMPLETION_IRQ_SEQUENCE.load(Ordering::Acquire);
        if observed != after {
            return Poll::Ready(observed);
        }
        TGA_COMPLETION_IRQ_WAKER.register(cx.waker());
        let observed = TGA_COMPLETION_IRQ_SEQUENCE.load(Ordering::Acquire);
        if observed != after {
            Poll::Ready(observed)
        } else {
            Poll::Pending
        }
    })
    .await
}

fn wake_completion_waiter_offline() {
    TGA_IRQ_CONFIGURED.store(false, Ordering::Release);
    TGA_COMPLETION_IRQ_SEQUENCE.fetch_add(1, Ordering::Release);
    TGA_COMPLETION_IRQ_WAKER.wake();
}

fn configure_completion_interrupt(tga: &Tga) -> bool {
    let Some(destination_apic_id) = crate::percpu::cpu_slots()
        .iter()
        .find(|cpu| cpu.slot == 0)
        .map(|cpu| cpu.lapic_id)
    else {
        return false;
    };

    Tga::write_reg(tga.irq_ack_reg, 1);
    fence(Ordering::SeqCst);
    if !crate::pci::enable_single_msi(
        tga.bus,
        tga.slot,
        tga.function,
        TGA_COMPLETION_VECTOR,
        destination_apic_id,
    ) {
        if !TGA_IRQ_CONFIG_FAILURE_LOGGED.swap(true, Ordering::AcqRel) {
            crate::log_warn!(
                "tga: MSI setup failed bdf={:02X}:{:02X}.{}; RPC remains offline\n",
                tga.bus,
                tga.slot,
                tga.function
            );
        }
        return false;
    }

    TGA_IRQ_CONFIG_FAILURE_LOGGED.store(false, Ordering::Release);
    TGA_IRQ_CONFIGURED.store(true, Ordering::Release);
    crate::log!(
        "tga: MSI ready vector=0x{:02X} destination_apic={} bdf={:02X}:{:02X}.{}\n",
        TGA_COMPLETION_VECTOR,
        destination_apic_id,
        tga.bus,
        tga.slot,
        tga.function
    );
    true
}

fn tga_bar_size_bytes(bus: u8, slot: u8, function: u8, bar_index: u8) -> Option<u64> {
    let command = crate::pci::config_read_u16(bus, slot, function, 0x04);
    if command == 0xFFFF || bar_index >= 6 {
        return None;
    }
    let disabled = command & !(PCI_COMMAND_IO_SPACE | PCI_COMMAND_MEM_SPACE);
    if disabled != command {
        crate::pci::config_write_u16(bus, slot, function, 0x04, disabled);
    }

    let offset = 0x10u16 + u16::from(bar_index) * 4;
    let original_low = crate::pci::config_read_u32(bus, slot, function, offset);
    if original_low == 0xFFFF_FFFF || original_low & 1 != 0 {
        crate::pci::config_write_u16(bus, slot, function, 0x04, command);
        return None;
    }
    let is_64 = (original_low >> 1) & 3 == 2;
    let original_high = if is_64 {
        crate::pci::config_read_u32(bus, slot, function, offset + 4)
    } else {
        0
    };

    crate::pci::config_write_u32(bus, slot, function, offset, 0xFFFF_FFF0);
    if is_64 {
        crate::pci::config_write_u32(bus, slot, function, offset + 4, 0xFFFF_FFFF);
    }
    let mask_low = crate::pci::config_read_u32(bus, slot, function, offset);
    let mask_high = if is_64 {
        crate::pci::config_read_u32(bus, slot, function, offset + 4)
    } else {
        0
    };

    crate::pci::config_write_u32(bus, slot, function, offset, original_low);
    if is_64 {
        crate::pci::config_write_u32(bus, slot, function, offset + 4, original_high);
    }
    crate::pci::config_write_u16(bus, slot, function, 0x04, command);

    let low = mask_low & !0xFu32;
    if low == 0 {
        return None;
    }
    if !is_64 || mask_high == 0 {
        return Some((!low).wrapping_add(1) as u64);
    }
    let mask = (u64::from(mask_high) << 32) | u64::from(low);
    let size = (!mask).wrapping_add(1);
    if size >> 32 == 0xFFFF_FFFF {
        Some((!low).wrapping_add(1) as u64)
    } else {
        Some(size)
    }
}

fn snapshot_from_tga(tga: &Tga) -> TgaHotplugSnapshot {
    TgaHotplugSnapshot {
        bus: tga.bus,
        slot: tga.slot,
        function: tga.function,
        bar_phys: tga.bar_phys,
        bar_size: tga.bar_size,
        bar_is_64: tga.bar_is_64,
        bar_assignment: tga.bar_assignment,
    }
}

fn log_tga_state(prefix: &str, tga: &Tga) {
    crate::log!(
        "tga: {} bdf={:02X}:{:02X}.{} bar0=0x{:016X} size=0x{:X} mode={} assignment={} map=0x{:X}\n",
        prefix,
        tga.bus,
        tga.slot,
        tga.function,
        tga.bar_phys,
        tga.bar_size,
        if tga.bar_is_64 { "64b" } else { "32b" },
        tga.bar_assignment.label(),
        tga.mmio_base
    );
}

fn log_reconnect_delta(previous: TgaHotplugSnapshot, now: &Tga) {
    crate::log_warn!(
        "tga: hotplug reconnect bdf={:02X}:{:02X}.{}->{:02X}:{:02X}.{} bar0=0x{:016X}->0x{:016X} size=0x{:X}->0x{:X} assignment={}->{}\n",
        previous.bus,
        previous.slot,
        previous.function,
        now.bus,
        now.slot,
        now.function,
        previous.bar_phys,
        now.bar_phys,
        previous.bar_size,
        now.bar_size,
        previous.bar_assignment.label(),
        now.bar_assignment.label()
    );
}

fn log_liveness_once() {
    if TGA_LIVENESS_LOGGED.swap(true, Ordering::AcqRel) {
        return;
    }
    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        TGA_LIVENESS_LOGGED.store(false, Ordering::Release);
        return;
    };
    let magic = tga.protocol_magic();
    if magic == TGA_MAGIC_EXPECTED {
        *TGA_LAST_WORKING_LEASE.lock() = Some(snapshot_from_tga(tga));
        crate::log!(
            "tga: liveness ok magic=0x{:08X} bdf={:02X}:{:02X}.{}\n",
            magic,
            tga.bus,
            tga.slot,
            tga.function
        );
    } else {
        crate::log_warn!(
            "tga: liveness mismatch observed=0x{:08X} expected=0x{:08X} bdf={:02X}:{:02X}.{}\n",
            magic,
            TGA_MAGIC_EXPECTED,
            tga.bus,
            tga.slot,
            tga.function
        );
    }
}

fn candidate_protocol_ready(tga: &Tga) -> bool {
    let magic = tga.protocol_magic();
    if magic == TGA_MAGIC_EXPECTED {
        true
    } else {
        if !TGA_LIVENESS_LOGGED.swap(true, Ordering::AcqRel) {
            crate::log_warn!(
                "tga: endpoint present but BAR0 protocol is not ready magic=0x{:08X}; retrying\n",
                magic
            );
        }
        false
    }
}

pub fn tga_led_write(value: u32) {
    if let Some(tga) = TGA.lock().as_ref() {
        Tga::write_reg(tga.led_reg, value);
    }
}

pub fn tga_led_set(on: bool) {
    tga_led_write(u32::from(on));
}

pub fn protocol_alive() -> bool {
    TGA.lock()
        .as_ref()
        .is_some_and(|tga| tga.protocol_magic() == TGA_MAGIC_EXPECTED)
}

pub fn is_online() -> bool {
    TGA.lock().is_some()
}

pub(crate) fn connection_generation() -> u32 {
    TGA_CONNECTION_GENERATION.load(Ordering::Acquire)
}

pub(crate) fn status() -> Status {
    let guard = TGA.lock();
    let (online, protocol_alive, bdf, bar_phys, bar_size) = match guard.as_ref() {
        Some(tga) => (
            true,
            tga.protocol_magic() == TGA_MAGIC_EXPECTED,
            Some((tga.bus, tga.slot, tga.function)),
            Some(tga.bar_phys),
            Some(tga.bar_size),
        ),
        None => (false, false, None, None, None),
    };
    Status {
        online,
        protocol_alive,
        generation: connection_generation(),
        bdf,
        bar_phys,
        bar_size,
        msi_ready: completion_interrupt_configured(),
        interrupts: completion_interrupt_count(),
    }
}

pub(crate) fn submit_offload_work_package(
    package: &WorkPackage,
) -> Result<(), OffloadTransportError> {
    if package.magic != protocol::WORK_PACKAGE_MAGIC
        || package.abi_version != protocol::ABI_VERSION
        || FunctionId::new(package.function).is_none()
        || package.state != WorkState::HostReady as u32
        || package.input_len as usize > protocol::INLINE_INPUT_BYTES
        || package.output_capacity as usize > protocol::INLINE_OUTPUT_BYTES
    {
        return Err(OffloadTransportError::InvalidPackage);
    }

    let guard = TGA.lock();
    let tga = guard.as_ref().ok_or(OffloadTransportError::Offline)?;
    let source = package as *const WorkPackage as *const u32;
    const HEADER_WORDS: usize = core::mem::offset_of!(WorkPackage, reserved_header) / 4;
    const INPUT_WORD: usize = core::mem::offset_of!(WorkPackage, input) / 4;
    let input_words = (package.input_len as usize).div_ceil(4);
    let total_words = HEADER_WORDS + input_words;

    for sequence in 0..total_words {
        let index = if sequence < HEADER_WORDS {
            sequence
        } else {
            INPUT_WORD + sequence - HEADER_WORDS
        };
        let expected = unsafe { source.add(index).read() };
        let register = tga.work_package_reg + index * 4;
        let mut observed = 0;
        for _ in 0..8 {
            Tga::write_reg(register, expected);
            fence(Ordering::SeqCst);
            observed = Tga::read_reg(register);
            if observed == expected {
                break;
            }
        }
        if observed != expected {
            return Err(OffloadTransportError::WriteVerification {
                word: index as u8,
                observed,
                expected,
            });
        }
    }

    fence(Ordering::Release);
    Tga::write_reg(tga.doorbell_reg, protocol::CALL_DOORBELL_MAGIC);
    Ok(())
}

pub(crate) fn offload_work_state() -> Result<WorkState, OffloadTransportError> {
    let guard = TGA.lock();
    let tga = guard.as_ref().ok_or(OffloadTransportError::Offline)?;
    fence(Ordering::Acquire);
    WorkState::from_raw(Tga::read_reg(tga.work_package_reg + protocol::WORK_PACKAGE_STATE_OFFSET))
        .ok_or(OffloadTransportError::InvalidPackage)
}

pub(crate) fn read_offload_work_package() -> Result<WorkPackage, OffloadTransportError> {
    let guard = TGA.lock();
    let tga = guard.as_ref().ok_or(OffloadTransportError::Offline)?;
    let mut package = WorkPackage::ZEROED;
    let destination = &mut package as *mut WorkPackage as *mut u32;
    const HEADER_WORDS: usize = core::mem::offset_of!(WorkPackage, reserved_header) / 4;
    const OUTPUT_WORD: usize = core::mem::offset_of!(WorkPackage, output) / 4;
    fence(Ordering::Acquire);
    for index in 0..HEADER_WORDS {
        unsafe {
            destination
                .add(index)
                .write(Tga::read_reg(tga.work_package_reg + index * 4));
        }
    }
    let output_len = package.output_len as usize;
    if output_len > protocol::INLINE_OUTPUT_BYTES {
        return Err(OffloadTransportError::InvalidPackage);
    }
    for index in 0..output_len.div_ceil(4) {
        unsafe {
            destination
                .add(OUTPUT_WORD + index)
                .write(Tga::read_reg(tga.work_package_reg + (OUTPUT_WORD + index) * 4));
        }
    }
    Ok(package)
}

pub(crate) fn ack_offload_interrupt() -> Result<(), OffloadTransportError> {
    let guard = TGA.lock();
    let tga = guard.as_ref().ok_or(OffloadTransportError::Offline)?;
    Tga::write_reg(tga.irq_ack_reg, 1);
    Ok(())
}

fn is_tga(device: &PciDevice) -> bool {
    device.vendor == TGA_VENDOR_ID && device.device == TGA_DEVICE_ID
}

fn is_present(tga: &Tga) -> bool {
    crate::pci::config_read_u16(tga.bus, tga.slot, tga.function, 0) != 0xFFFF
}

fn bring_online(device: &PciDevice) -> Option<Tga> {
    if crate::pci::config_read_u16(device.bus, device.slot, device.function, 0) != TGA_VENDOR_ID
        || crate::pci::config_read_u16(device.bus, device.slot, device.function, 2) != TGA_DEVICE_ID
    {
        return None;
    }
    crate::pci::enable_mem_space_only(device.bus, device.slot, device.function);

    let (mut bar_low, mut bar_high) =
        crate::pci::read_bar0_raw(device.bus, device.slot, device.function);
    if bar_low == 0xFFFF_FFFF || bar_low & 1 != 0 || (bar_low >> 1) & 3 != 2 {
        return None;
    }
    let bar_is_64 = true;
    let bar_size = tga_bar_size_bytes(device.bus, device.slot, device.function, 0)
        .unwrap_or(TGA_EXPECTED_BAR0_SIZE);
    let mut bar_phys = u64::from(bar_low & !0xF) | (u64::from(bar_high?) << 32);
    let mut assignment = TgaBarAssignment::Firmware;

    if bar_phys == 0 || bar_phys >= 0x40_0000_0000 {
        let alignment = TGA_EXPECTED_BAR0_SIZE.max(0x1000);
        let previous = TGA_LAST_WORKING_LEASE
            .lock()
            .as_ref()
            .copied()
            .filter(|old| {
                old.bus == device.bus
                    && old.slot == device.slot
                    && old.function == device.function
                    && old.bar_size == bar_size
                    && old.bar_is_64
                    && old.bar_phys != 0
                    && old.bar_phys % alignment == 0
            });
        let base = if let Some(old) = previous {
            assignment = TgaBarAssignment::Restored;
            old.bar_phys
        } else {
            assignment = TgaBarAssignment::Allocated;
            crate::pci::alloc_hotplug_mmio_base(device.bus, TGA_EXPECTED_BAR0_SIZE, alignment)?
        };
        let new_low = (base as u32 & !0xF) | (bar_low & 0xF);
        crate::pci::config_write_u32(device.bus, device.slot, device.function, 0x10, new_low);
        crate::pci::config_write_u32(
            device.bus,
            device.slot,
            device.function,
            0x14,
            (base >> 32) as u32,
        );
        (bar_low, bar_high) = crate::pci::read_bar0_raw(device.bus, device.slot, device.function);
        if bar_low == 0xFFFF_FFFF || bar_low & 1 != 0 {
            return None;
        }
        bar_phys = u64::from(bar_low & !0xF) | (u64::from(bar_high?) << 32);
        if bar_phys == 0 {
            return None;
        }
        crate::pci::enable_mem_space_only(device.bus, device.slot, device.function);
    }

    if assignment == TgaBarAssignment::Firmware
        && TGA_LAST_WORKING_LEASE.lock().as_ref().is_some_and(|old| {
            old.bus == device.bus
                && old.slot == device.slot
                && old.function == device.function
                && old.bar_phys == bar_phys
        })
    {
        assignment = TgaBarAssignment::Restored;
    }

    let mapped = match *TGA_LAST_MAP.lock() {
        Some((last_phys, last_base)) if last_phys == bar_phys => {
            NonNull::new(last_base as *mut u8)?
        }
        _ => {
            let mapping = crate::pci::mmio::map_mmio_region_exact(bar_phys, 0x1000).ok()?;
            *TGA_LAST_MAP.lock() = Some((bar_phys, mapping.as_ptr() as usize));
            mapping
        }
    };
    let base = mapped.as_ptr() as usize;
    Some(Tga {
        bus: device.bus,
        slot: device.slot,
        function: device.function,
        bar_phys,
        bar_size,
        bar_is_64,
        bar_assignment: assignment,
        mmio_base: base,
        led_reg: base + protocol::BAR0_LED_OFFSET,
        magic_reg: base + protocol::BAR0_LIVENESS_MAGIC_OFFSET,
        work_package_reg: base + protocol::BAR0_WORK_PACKAGE_OFFSET,
        doorbell_reg: base + protocol::BAR0_CALL_DOORBELL_OFFSET,
        irq_ack_reg: base + protocol::BAR0_CALL_IRQ_ACK_OFFSET,
    })
}

pub fn try_init() -> bool {
    if is_online() {
        return true;
    }

    let mut devices_empty = false;
    crate::pci::with_devices(|devices| devices_empty = devices.is_empty());
    if devices_empty {
        crate::pci::enumerate_impl();
    }
    let mut found = None;
    let mut device_count = 0;
    crate::pci::with_devices(|devices| {
        device_count = devices.len();
        found = devices.iter().copied().find(is_tga);
    });
    let Some(device) = found else {
        if crate::log_os::flags::BOOT_INFO_LOGS {
            crate::log_os::flags::TGA_MISSING_LOG_ONCE.call_once(|| {
                crate::log!(
                    "tga: PCI device not found vid=0x{:04X} did=0x{:04X} scanned={}\n",
                    TGA_VENDOR_ID,
                    TGA_DEVICE_ID,
                    device_count
                );
            });
        }
        return false;
    };

    if let Err(error) = crate::pci::claim_device(&device, TGA_PCI_OWNER) {
        crate::log_warn!(
            "tga: PCI claim rejected bdf={:02X}:{:02X}.{} error={:?}\n",
            device.bus,
            device.slot,
            device.function,
            error
        );
        return false;
    }
    let Some(tga) = bring_online(&device) else {
        let _ = crate::pci::release_device_claim(
            device.bus,
            device.slot,
            device.function,
            TGA_PCI_OWNER,
        );
        return false;
    };
    if !candidate_protocol_ready(&tga) || !configure_completion_interrupt(&tga) {
        let _ = crate::pci::release_device_claim(
            device.bus,
            device.slot,
            device.function,
            TGA_PCI_OWNER,
        );
        return false;
    }

    if let Some(previous) = TGA_LAST_DISCONNECT.lock().take() {
        log_reconnect_delta(previous, &tga);
    } else {
        log_tga_state("connected", &tga);
    }
    *TGA.lock() = Some(tga);
    TGA_CONNECTION_GENERATION.fetch_add(1, Ordering::AcqRel);
    TGA_LIVENESS_LOGGED.store(false, Ordering::Release);
    log_liveness_once();
    true
}

pub fn init_once() {
    crate::log_info!(
        target: "boot";
        "tga: PCI/hotplug ownership is deferred to the service task\n"
    );
}

fn disconnect() {
    let mut guard = TGA.lock();
    let Some(old) = guard.take() else {
        return;
    };
    let _ = crate::pci::release_device_claim(old.bus, old.slot, old.function, TGA_PCI_OWNER);
    *TGA_LAST_DISCONNECT.lock() = Some(snapshot_from_tga(&old));
    TGA_LINK_RECOVERY_ATTEMPTED.store(false, Ordering::Release);
    TGA_LIVENESS_LOGGED.store(false, Ordering::Release);
    wake_completion_waiter_offline();
    crate::log_warn!(
        "tga: hotplug disconnect bdf={:02X}:{:02X}.{}\n",
        old.bus,
        old.slot,
        old.function
    );
}

#[embassy_executor::task]
pub(crate) async fn tga_task() {
    crate::log_os::flags::TGA_TASK_STARTED_LOG_ONCE.call_once(|| {
        crate::log_info!(target: "boot"; "tga: PCI/hotplug service started\n");
    });
    let mut presence_misses = 0u8;
    loop {
        if !is_online() {
            crate::pci::enumerate_impl();
            let mut initialized = try_init();
            if !initialized
                && let Some(previous) = TGA_LAST_DISCONNECT.lock().as_ref().copied()
                && TGA_LINK_RECOVERY_ATTEMPTED
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                if let Ok(recovery) = crate::pci::recover_dedicated_downstream_link(
                    previous.bus,
                    previous.slot,
                    previous.function,
                ) {
                    Timer::after(EmbassyDuration::from_millis(100)).await;
                    crate::pci::enumerate_impl();
                    initialized = try_init();
                    crate::log_warn!(
                        "tga: link retrain bridge={:02X}:{:02X}.{} result={}\n",
                        recovery.bridge_bus,
                        recovery.bridge_slot,
                        recovery.bridge_function,
                        if initialized { "online" } else { "offline" }
                    );
                }
            }
            presence_misses = 0;
            Timer::after(EmbassyDuration::from_millis(TGA_OFFLINE_RETRY_MS)).await;
            continue;
        }

        Timer::after(EmbassyDuration::from_millis(TGA_PRESENCE_PROBE_MS)).await;
        let present = TGA.lock().as_ref().is_some_and(is_present);
        if present {
            presence_misses = 0;
            log_liveness_once();
            continue;
        }
        presence_misses = presence_misses.saturating_add(1);
        if presence_misses >= TGA_PRESENCE_MISS_THRESHOLD {
            disconnect();
            presence_misses = 0;
        }
    }
}
