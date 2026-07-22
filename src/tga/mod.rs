use atomic_waker::AtomicWaker;
use core::future::poll_fn;
use core::ptr::{NonNull, read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence};
use core::task::Poll;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

// this connected to the FPGA-based "TGA" adapter in the FPGA lab, which implements a tiny MMIO protocol

use crate::pci::PciDevice;

const TGA_VENDOR_ID: u16 = 0x22c2; // DEC vendor:
const TGA_DEVICE_ID: u16 = 0x1100; // TGA adapter
const TGA_PCI_OWNER: &str = "tga";
const TGA_EXPECTED_BAR0_SIZE: u64 = 1024; // 1 KiB
const TGA_EXPECTED_BAR2_SIZE: u64 = trueos_fpga_abi::BAR2_LFM25_STREAM_BYTES as u64;

// Minimal TGA contract (we control both ends):
// - BAR0 is MMIO
// - BAR0 + 0x00 is a 32-bit LED bitfield
//   - bit0..bit5: usr_led0..usr_led5
//   - other bits ignored
// - BAR0 + 0x20 is a 32-bit read-only liveness magic ("TGAT")
// - BAR0 + 0x80 is the function-call doorbell
// - BAR0 + 0x100..0x1FF is one fixed, inline work package
// - BAR0 + 0x200..0x27F is the read-only firmware manifest
//
// The LED/magic plane stays independent of function execution so it remains useful when
// function firmware wedges.  There is no FPGA-side command processor, DMA requester, or
// virtual memory.
const TGA_LED_SET_OFF: usize = trueos_fpga_abi::BAR0_LED_OFFSET;
const TGA_MAGIC_OFF: usize = trueos_fpga_abi::BAR0_LIVENESS_MAGIC_OFFSET;
const TGA_OFFLOAD_DOORBELL_OFF: usize = trueos_fpga_abi::BAR0_CALL_DOORBELL_OFFSET;
const TGA_OFFLOAD_IRQ_ACK_OFF: usize = trueos_fpga_abi::BAR0_CALL_IRQ_ACK_OFFSET;
const TGA_OFFLOAD_IRQ_RETIRE_COUNT_OFF: usize = trueos_fpga_abi::BAR0_CALL_IRQ_RETIRE_COUNT_OFFSET;
const TGA_OFFLOAD_IRQ_REQUEST_COUNT_OFF: usize =
    trueos_fpga_abi::BAR0_CALL_IRQ_REQUEST_COUNT_OFFSET;
const TGA_OFFLOAD_IRQ_CONTROLLER_ACK_COUNT_OFF: usize =
    trueos_fpga_abi::BAR0_CALL_IRQ_CONTROLLER_ACK_COUNT_OFFSET;
const TGA_OFFLOAD_IRQ_STATE_OFF: usize = trueos_fpga_abi::BAR0_CALL_IRQ_STATE_OFFSET;
const TGA_OFFLOAD_WORK_PACKAGE_OFF: usize = trueos_fpga_abi::BAR0_WORK_PACKAGE_OFFSET;
const TGA_FIRMWARE_MANIFEST_OFF: usize = trueos_fpga_abi::BAR0_FIRMWARE_MANIFEST_OFFSET;
const TGA_DBG_RX_CAPTURE_COUNT_OFF: usize = 0x060;
const TGA_DBG_WRITE_COUNT_OFF: usize = 0x064;
const TGA_DBG_WORD30_WRITE_COUNT_OFF: usize = 0x068;
const TGA_DBG_WORD30_LAST_PAYLOAD_OFF: usize = 0x06c;
const TGA_DBG_WORD30_STORAGE_OFF: usize = 0x070;
const TGA_DBG_RX_FIFO_STATE_OFF: usize = 0x074;
const TGA_DBG_RX_ERROR_COUNT_OFF: usize = 0x078;

const TGA_OFFLOAD_DOORBELL_MAGIC: u32 = 0x4C4C_4143; // "CALL"
const TGA_BOOT_MMIO_TOUCH_ENABLED: bool = false;
// Raw LED writes are a transport-debug fallback only. Normal blinking comes from
// fpga_offload::led_step_heartbeat so it proves the complete function-call path.
const TGA_HEARTBEAT_MMIO_ENABLED: bool = false;
const TGA_MAGIC_EXPECTED: u32 = 0x5453_4154;
pub(crate) const TGA_COMPLETION_VECTOR: u8 = 0x42;

struct Tga {
    bus: u8,
    slot: u8,
    function: u8,
    bar_phys: u64,
    bar_size: u64,
    bar_is_64: bool,
    bar_assignment: TgaBarAssignment,
    mmio_base: usize,
    stream_bar_phys: Option<u64>,
    stream_mmio_base: Option<usize>,
    led_reg: usize,
    magic_reg: usize,
    offload_work_package_reg: usize,
    offload_doorbell_reg: usize,
    offload_irq_ack_reg: usize,
    firmware_manifest_reg: usize,
}

#[derive(Copy, Clone)]
struct TgaHotplugSnapshot {
    bus: u8,
    slot: u8,
    function: u8,
    bar_phys: u64,
    bar_size: u64,
    bar_is_64: bool,
    bar_assignment: TgaBarAssignment,
    mmio_base: usize,
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

// Safety: `Tga` contains an MMIO pointer and is always accessed behind the `TGA` mutex.
unsafe impl Send for Tga {}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum OffloadTransportError {
    Offline,
    InvalidPackage,
    WriteVerification {
        word: u8,
        observed: u32,
        expected: u32,
        rx_captures: u32,
        rx_capture_delta: u32,
        decoded_writes: u32,
        decoded_write_delta: u32,
        word30_writes: u32,
        word30_write_delta: u32,
        word30_last_payload: u32,
        word30_storage: u32,
        rx_fifo_state: u32,
        rx_errors: u32,
    },
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct CompletionIrqHardwareStats {
    pub retirements: u32,
    pub requests: u32,
    pub controller_acks: u32,
    pub state: u32,
}

impl Tga {
    #[inline(always)]
    fn write_led(&self, value: u32) {
        unsafe { write_volatile(self.led_reg as *mut u32, value) };
    }

    #[inline(always)]
    fn read_reg(reg: usize) -> u32 {
        unsafe { read_volatile(reg as *const u32) }
    }

    #[inline(always)]
    fn write_reg(reg: usize, value: u32) {
        unsafe { write_volatile(reg as *mut u32, value) };
    }

    #[inline(always)]
    fn write_reg64(reg: usize, value: u64) {
        unsafe { write_volatile(reg as *mut u64, value) };
    }

    fn protocol_magic(&self) -> u32 {
        Self::read_reg(self.magic_reg)
    }

    fn firmware_manifest_mismatch(&self) -> Option<(usize, u32, u32)> {
        let expected = &trueos_fpga_abi::builtins::FIRMWARE_MANIFEST
            as *const trueos_fpga_abi::FirmwareManifest as *const u32;
        let word_count = core::mem::size_of::<trueos_fpga_abi::FirmwareManifest>() / 4;
        fence(Ordering::Acquire);
        for index in 0..word_count {
            let observed = Self::read_reg(self.firmware_manifest_reg + index * 4);
            let wanted = unsafe { expected.add(index).read() };
            if observed != wanted {
                return Some((index, observed, wanted));
            }
        }
        None
    }
}

static TGA: Mutex<Option<Tga>> = Mutex::new(None);
static TGA_LAST_MAP: Mutex<Option<(u64, usize)>> = Mutex::new(None);
static TGA_LAST_STREAM_MAP: Mutex<Option<(u64, usize)>> = Mutex::new(None);
static TGA_LAST_DISCONNECT: Mutex<Option<TgaHotplugSnapshot>> = Mutex::new(None);
static TGA_LAST_WORKING_LEASE: Mutex<Option<TgaHotplugSnapshot>> = Mutex::new(None);
static TGA_LINK_RECOVERY_ATTEMPTED: AtomicBool = AtomicBool::new(false);

// Heartbeat policy: write a visible changing pattern as a "driver alive" indicator.
// We send 0..31 (wrap) so the FPGA can display the low 5 bits.
static TGA_HEARTBEAT_COUNTER: AtomicU32 = AtomicU32::new(0);
static TGA_LIVENESS_LOGGED: AtomicBool = AtomicBool::new(false);
static TGA_CONNECTION_GENERATION: AtomicU32 = AtomicU32::new(0);
static TGA_IRQ_CONFIGURED: AtomicBool = AtomicBool::new(false);
static TGA_IRQ_CONFIG_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);
static TGA_COMPLETION_IRQ_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static TGA_COMPLETION_IRQ_COUNT: AtomicU64 = AtomicU64::new(0);
static TGA_COMPLETION_IRQ_WAKER: AtomicWaker = AtomicWaker::new();
static TGA_OFFLOAD_WRITE_REPAIR_COUNT: AtomicU64 = AtomicU64::new(0);
static TGA_OFFLOAD_WRITE_REPAIR_LOGGED: AtomicBool = AtomicBool::new(false);
static TGA_OFFLOAD_BATCH_FALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);
static TGA_OFFLOAD_BATCH_DISABLED: AtomicBool = AtomicBool::new(false);
static TGA_OFFLOAD_BATCH_FALLBACK_LOGGED: AtomicBool = AtomicBool::new(false);

const TGA_HEARTBEAT_PERIOD_MS: u64 = 100;
const TGA_HEARTBEAT_LOG_EVERY_WRITES: u32 = 50;
const TGA_PRESENCE_PROBE_PERIOD_MS: u64 = 1000;
const TGA_OFFLINE_RETRY_MS: u64 = 250;
const TGA_PRESENCE_MISS_THRESHOLD: u8 = 10;

const PCI_COMMAND_IO_SPACE: u16 = 1 << 0;
const PCI_COMMAND_MEM_SPACE: u16 = 1 << 1;

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

pub(crate) fn offload_write_repair_count() -> u64 {
    TGA_OFFLOAD_WRITE_REPAIR_COUNT.load(Ordering::Acquire)
}

pub(crate) fn offload_batch_fallback_count() -> u64 {
    TGA_OFFLOAD_BATCH_FALLBACK_COUNT.load(Ordering::Acquire)
}

pub(crate) fn completion_interrupt_configured() -> bool {
    TGA_IRQ_CONFIGURED.load(Ordering::Acquire)
}

pub(crate) fn completion_irq_hardware_stats() -> Option<CompletionIrqHardwareStats> {
    let guard = TGA.lock();
    let tga = guard.as_ref()?;
    fence(Ordering::Acquire);
    Some(CompletionIrqHardwareStats {
        retirements: Tga::read_reg(tga.mmio_base + TGA_OFFLOAD_IRQ_RETIRE_COUNT_OFF),
        requests: Tga::read_reg(tga.mmio_base + TGA_OFFLOAD_IRQ_REQUEST_COUNT_OFF),
        controller_acks: Tga::read_reg(tga.mmio_base + TGA_OFFLOAD_IRQ_CONTROLLER_ACK_COUNT_OFF),
        state: Tga::read_reg(tga.mmio_base + TGA_OFFLOAD_IRQ_STATE_OFF),
    })
}

/// Clear any sticky device-side completion and return the interrupt sequence
/// against which the next single-slot submission must wait.
pub(crate) fn arm_offload_interrupt() -> Result<u64, OffloadTransportError> {
    if !completion_interrupt_configured() {
        return Err(OffloadTransportError::Offline);
    }
    ack_offload_interrupt()?;
    fence(Ordering::SeqCst);
    Ok(TGA_COMPLETION_IRQ_SEQUENCE.load(Ordering::Acquire))
}

/// Sleep the worker until the ISR advances the hardware completion sequence.
/// AtomicWaker keeps the ISR free of MMIO, allocation, callbacks, and spin locks.
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

    // Drop any terminal status left by an earlier software generation before
    // unmasking MSI in config space.
    Tga::write_reg(tga.offload_irq_ack_reg, 1);
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
                "tga: endpoint protocol ready but single-vector MSI setup failed bdf={:02X}:{:02X}.{}; completion transport remains unpublished\n",
                tga.bus,
                tga.slot,
                tga.function
            );
        }
        return false;
    }

    TGA_IRQ_CONFIG_FAILURE_LOGGED.store(false, Ordering::Release);
    TGA_IRQ_CONFIGURED.store(true, Ordering::Release);
    let command = crate::pci::config_read_u16(tga.bus, tga.slot, tga.function, 0x04);
    crate::log!(
        "tga: completion interrupt enabled mode=msi vector=0x{:02X} destination_apic={} command=0x{:04X} requester=msi-only bdf={:02X}:{:02X}.{}\n",
        TGA_COMPLETION_VECTOR,
        destination_apic_id,
        command,
        tga.bus,
        tga.slot,
        tga.function
    );
    true
}

fn tga_bar_size_bytes(bus: u8, slot: u8, function: u8, bar_index: u8) -> Option<u64> {
    // BAR sizing writes can confuse some devices if decode is enabled.
    // Also, some endpoints incorrectly return a 0 upper mask for 64-bit BAR sizing.
    // We harden both issues locally for TGA bring-up.
    let cmd_before = crate::pci::config_read_u16(bus, slot, function, 0x04);
    if cmd_before == 0xFFFF {
        return None;
    }

    let cmd_disabled = cmd_before & !(PCI_COMMAND_IO_SPACE | PCI_COMMAND_MEM_SPACE);
    if cmd_disabled != cmd_before {
        crate::pci::config_write_u16(bus, slot, function, 0x04, cmd_disabled);
    }

    if bar_index >= 6 {
        crate::pci::config_write_u16(bus, slot, function, 0x04, cmd_before);
        return None;
    }
    let off = 0x10u16 + u16::from(bar_index) * 4;
    let orig_lo = crate::pci::config_read_u32(bus, slot, function, off);
    if orig_lo == 0xFFFF_FFFF {
        crate::pci::config_write_u16(bus, slot, function, 0x04, cmd_before);
        return None;
    }
    if (orig_lo & 0x1) != 0 {
        crate::pci::config_write_u16(bus, slot, function, 0x04, cmd_before);
        return None;
    }

    let is_64 = ((orig_lo >> 1) & 0x3) == 0x2;
    let orig_hi = if is_64 {
        crate::pci::config_read_u32(bus, slot, function, off + 4)
    } else {
        0
    };

    crate::pci::config_write_u32(bus, slot, function, off, 0xFFFF_FFF0);
    if is_64 {
        crate::pci::config_write_u32(bus, slot, function, off + 4, 0xFFFF_FFFF);
    }

    let mask_lo = crate::pci::config_read_u32(bus, slot, function, off);
    let mask_hi = if is_64 {
        crate::pci::config_read_u32(bus, slot, function, off + 4)
    } else {
        0
    };

    crate::pci::config_write_u32(bus, slot, function, off, orig_lo);
    if is_64 {
        crate::pci::config_write_u32(bus, slot, function, off + 4, orig_hi);
    }

    crate::pci::config_write_u16(bus, slot, function, 0x04, cmd_before);

    if is_64 {
        let size_mask_lo = mask_lo & !0xFu32;
        if size_mask_lo == 0 {
            return None;
        }

        // If the upper mask comes back 0, compute the size from the low dword only.
        // For small (<4GiB) 64-bit BARs, a conforming device typically returns 0xFFFF_FFFF
        // in the upper mask during sizing. We've observed 0 here from the endpoint.
        if mask_hi == 0 {
            return Some((!size_mask_lo).wrapping_add(1) as u64);
        }

        let size_mask = ((mask_hi as u64) << 32) | (size_mask_lo as u64);
        if size_mask == 0 {
            return None;
        }
        let size = (!size_mask).wrapping_add(1);

        // Extra guard: if the computed size looks like the "0xFFFFFFFF...." pattern,
        // fall back to the low-dword-only calculation.
        if (size >> 32) == 0xFFFF_FFFF {
            return Some((!size_mask_lo).wrapping_add(1) as u64);
        }

        Some(size)
    } else {
        let size_mask = mask_lo & !0xFu32;
        if size_mask == 0 {
            return None;
        }
        Some((!size_mask).wrapping_add(1) as u64)
    }
}

fn write_led_raw(value: u32) {
    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return;
    };
    tga.write_led(value);
}

fn write_heartbeat_led(value: u32, count: u32) {
    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return;
    };
    tga.write_led(value);
    if count % TGA_HEARTBEAT_LOG_EVERY_WRITES == 0 {
        let bus = tga.bus;
        let slot = tga.slot;
        let function = tga.function;
        let bar_phys = tga.bar_phys;
        let led_reg = tga.led_reg;
        drop(guard);
        crate::log!(
            "tga: heartbeat mmio write count={} led=0x{:02X} bdf={:02X}:{:02X}.{} bar0=0x{:016X} virt=0x{:016X}\n",
            count,
            value,
            bus,
            slot,
            function,
            bar_phys,
            led_reg
        );
    }
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
            "tga: liveness reply=yep-alive magic=0x{:08X} bundle=matched bdf={:02X}:{:02X}.{}\n",
            magic,
            tga.bus,
            tga.slot,
            tga.function
        );
    } else {
        crate::log_warn!(
            "tga: liveness mismatch magic=0x{:08X} expected=0x{:08X} bdf={:02X}:{:02X}.{}; flash matching TRUEGA firmware\n",
            magic,
            TGA_MAGIC_EXPECTED,
            tga.bus,
            tga.slot,
            tga.function
        );
    }
}

fn candidate_bundle_ready(tga: &Tga) -> bool {
    let magic = tga.protocol_magic();
    if magic != TGA_MAGIC_EXPECTED {
        // A live SRAM program can expose PCI config space a few seconds before the
        // new fabric has finished configuring. Keep that candidate private and
        // retry it instead of publishing an unusable transport connection.
        if !TGA_LIVENESS_LOGGED.swap(true, Ordering::AcqRel) {
            crate::log_warn!(
                "tga: endpoint present but protocol not ready magic=0x{:08X} expected=0x{:08X} bdf={:02X}:{:02X}.{}; retrying without publishing connection\n",
                magic,
                TGA_MAGIC_EXPECTED,
                tga.bus,
                tga.slot,
                tga.function
            );
        }
        return false;
    }

    if let Some((word, observed, expected)) = tga.firmware_manifest_mismatch() {
        if !TGA_LIVENESS_LOGGED.swap(true, Ordering::AcqRel) {
            crate::log_warn!(
                "tga: firmware bundle mismatch manifest_word={} observed=0x{:08X} expected=0x{:08X} bdf={:02X}:{:02X}.{}; transport remains unpublished\n",
                word,
                observed,
                expected,
                tga.bus,
                tga.slot,
                tga.function
            );
        }
        return false;
    }

    true
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
        mmio_base: tga.mmio_base,
    }
}

fn log_reconnect_delta(prev: TgaHotplugSnapshot, now: &Tga) {
    let bdf_changed = prev.bus != now.bus || prev.slot != now.slot || prev.function != now.function;
    let bar_phys_changed = prev.bar_phys != now.bar_phys;
    let bar_size_changed = prev.bar_size != now.bar_size;
    let bar_mode_changed = prev.bar_is_64 != now.bar_is_64;
    let assign_changed = prev.bar_assignment != now.bar_assignment;
    let map_changed = prev.mmio_base != now.mmio_base;

    if !(bdf_changed
        || bar_phys_changed
        || bar_size_changed
        || bar_mode_changed
        || assign_changed
        || map_changed)
    {
        crate::log_warn!(
            "tga: hotplug event (warn marks a significant low-level event, not a detected fault): endpoint reconnect completed with stable resources bdf={:02X}:{:02X}.{} bar0=0x{:016X} size=0x{:X} map=0x{:X}\n",
            now.bus,
            now.slot,
            now.function,
            now.bar_phys,
            now.bar_size,
            now.mmio_base
        );
        return;
    }

    crate::log_warn!(
        "tga: hotplug event (warn marks a significant low-level event, not a detected fault): endpoint reconnect completed; bdf {:02X}:{:02X}.{} -> {:02X}:{:02X}.{} bar0 0x{:016X} -> 0x{:016X} size 0x{:X} -> 0x{:X} mode {} -> {} assign {} -> {} map 0x{:X} -> 0x{:X}\n",
        prev.bus,
        prev.slot,
        prev.function,
        now.bus,
        now.slot,
        now.function,
        prev.bar_phys,
        now.bar_phys,
        prev.bar_size,
        now.bar_size,
        if prev.bar_is_64 { "64b" } else { "32b" },
        if now.bar_is_64 { "64b" } else { "32b" },
        prev.bar_assignment.label(),
        now.bar_assignment.label(),
        prev.mmio_base,
        now.mmio_base
    );
}

fn log_tga_state(prefix: &str, tga: &Tga) {
    crate::log!(
        "tga: {} bdf={:02X}:{:02X}.{} bar0=0x{:016X} size=0x{:X} {} {} map=0x{:X}\n",
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

pub fn tga_led_write(value: u32) {
    write_led_raw(value);
}

pub fn tga_led_set(on: bool) {
    tga_led_write(if on { 1 } else { 0 });
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Lfm25StreamResult {
    pub gate_q30: i64,
    pub up_q30: i64,
    pub result_q30: i64,
    pub error_code: u32,
}

pub(crate) fn lfm25_stream_available() -> bool {
    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return false;
    };
    tga.stream_bar_phys.is_some()
        && tga.stream_mmio_base.is_some()
        && Tga::read_reg(tga.mmio_base + trueos_fpga_abi::BAR0_LFM25_STREAM_CAPABILITY_OFFSET)
            == trueos_fpga_abi::LFM25_STREAM_CAPABILITY_MAGIC
}

pub(crate) fn lfm25_stream_write_blocks(
    buffer_offset: usize,
    blocks: &[[u8; trueos_fpga_abi::lfm25::Q8_0_BLOCK_BYTES]],
) -> Result<(), OffloadTransportError> {
    if blocks.is_empty()
        || blocks.len() > 144
        || !matches!(
            buffer_offset,
            trueos_fpga_abi::BAR2_LFM25_STREAM_ACTIVATION_OFFSET
                | trueos_fpga_abi::BAR2_LFM25_STREAM_WEIGHT0_OFFSET
                | trueos_fpga_abi::BAR2_LFM25_STREAM_WEIGHT1_OFFSET
        )
    {
        return Err(OffloadTransportError::InvalidPackage);
    }

    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return Err(OffloadTransportError::Offline);
    };
    let Some(stream_base) = tga.stream_mmio_base else {
        return Err(OffloadTransportError::Offline);
    };
    if Tga::read_reg(tga.mmio_base + trueos_fpga_abi::BAR0_LFM25_STREAM_STATE_OFFSET)
        == trueos_fpga_abi::Lfm25StreamState::Busy as u32
    {
        return Err(OffloadTransportError::InvalidPackage);
    }

    for (block_index, block) in blocks.iter().enumerate() {
        let slot = stream_base
            + buffer_offset
            + block_index * trueos_fpga_abi::BAR2_LFM25_STREAM_BLOCK_STRIDE;
        for qword_index in 0..4 {
            let byte_index = qword_index * 8;
            let mut bytes = [0u8; 8];
            let available = block.len().saturating_sub(byte_index).min(8);
            if available != 0 {
                bytes[..available].copy_from_slice(&block[byte_index..byte_index + available]);
            }
            Tga::write_reg64(slot + qword_index * 8, u64::from_le_bytes(bytes));
        }
        let mut tail = [0u8; 4];
        tail[..2].copy_from_slice(&block[32..34]);
        Tga::write_reg(slot + 32, u32::from_le_bytes(tail));
    }
    fence(Ordering::Release);
    Ok(())
}

pub(crate) fn lfm25_stream_write_block_bytes(
    buffer_offset: usize,
    bytes: &[u8],
) -> Result<(), OffloadTransportError> {
    let block_bytes = trueos_fpga_abi::lfm25::Q8_0_BLOCK_BYTES;
    let block_count = bytes.len() / block_bytes;
    if bytes.len() % block_bytes != 0
        || block_count == 0
        || block_count > 144
        || !matches!(
            buffer_offset,
            trueos_fpga_abi::BAR2_LFM25_STREAM_WEIGHT0_OFFSET
                | trueos_fpga_abi::BAR2_LFM25_STREAM_WEIGHT1_OFFSET
        )
    {
        return Err(OffloadTransportError::InvalidPackage);
    }

    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return Err(OffloadTransportError::Offline);
    };
    let Some(stream_base) = tga.stream_mmio_base else {
        return Err(OffloadTransportError::Offline);
    };
    if Tga::read_reg(tga.mmio_base + trueos_fpga_abi::BAR0_LFM25_STREAM_STATE_OFFSET)
        == trueos_fpga_abi::Lfm25StreamState::Busy as u32
    {
        return Err(OffloadTransportError::InvalidPackage);
    }

    for (block_index, block) in bytes.chunks_exact(block_bytes).enumerate() {
        let slot = stream_base
            + buffer_offset
            + block_index * trueos_fpga_abi::BAR2_LFM25_STREAM_BLOCK_STRIDE;
        for qword_index in 0..4 {
            let byte_index = qword_index * 8;
            let mut word = [0u8; 8];
            let available = block.len().saturating_sub(byte_index).min(8);
            if available != 0 {
                word[..available].copy_from_slice(&block[byte_index..byte_index + available]);
            }
            Tga::write_reg64(slot + qword_index * 8, u64::from_le_bytes(word));
        }
        let mut tail = [0u8; 4];
        tail[..2].copy_from_slice(&block[32..34]);
        Tga::write_reg(slot + 32, u32::from_le_bytes(tail));
    }
    fence(Ordering::Release);
    Ok(())
}

pub(crate) fn start_lfm25_stream_row(mode: u32, row: u32) -> Result<(), OffloadTransportError> {
    if !matches!(
        mode,
        trueos_fpga_abi::LFM25_STREAM_MODE_GATE_UP_SILU | trueos_fpga_abi::LFM25_STREAM_MODE_DOWN
    ) {
        return Err(OffloadTransportError::InvalidPackage);
    }
    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return Err(OffloadTransportError::Offline);
    };
    if tga.stream_mmio_base.is_none()
        || Tga::read_reg(tga.mmio_base + trueos_fpga_abi::BAR0_LFM25_STREAM_CAPABILITY_OFFSET)
            != trueos_fpga_abi::LFM25_STREAM_CAPABILITY_MAGIC
    {
        return Err(OffloadTransportError::Offline);
    }

    let control = mode | trueos_fpga_abi::LFM25_STREAM_CONTROL_INTERRUPT_ENABLE;
    Tga::write_reg(tga.mmio_base + trueos_fpga_abi::BAR0_LFM25_STREAM_CONTROL_OFFSET, control);
    Tga::write_reg(tga.mmio_base + trueos_fpga_abi::BAR0_LFM25_STREAM_ROW_OFFSET, row);
    fence(Ordering::SeqCst);
    // This non-posted BAR0 read is the producer commit: all preceding BAR2
    // posted writes must be visible before the doorbell can reach the endpoint.
    let capability =
        Tga::read_reg(tga.mmio_base + trueos_fpga_abi::BAR0_LFM25_STREAM_CAPABILITY_OFFSET);
    if capability != trueos_fpga_abi::LFM25_STREAM_CAPABILITY_MAGIC {
        return Err(OffloadTransportError::Offline);
    }
    fence(Ordering::Release);
    Tga::write_reg(
        tga.mmio_base + trueos_fpga_abi::BAR0_LFM25_STREAM_DOORBELL_OFFSET,
        trueos_fpga_abi::LFM25_STREAM_DOORBELL_MAGIC,
    );
    Ok(())
}

pub(crate) fn lfm25_stream_state()
-> Result<trueos_fpga_abi::Lfm25StreamState, OffloadTransportError> {
    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return Err(OffloadTransportError::Offline);
    };
    fence(Ordering::Acquire);
    let raw = Tga::read_reg(tga.mmio_base + trueos_fpga_abi::BAR0_LFM25_STREAM_STATE_OFFSET);
    trueos_fpga_abi::Lfm25StreamState::from_raw(raw).ok_or(OffloadTransportError::InvalidPackage)
}

pub(crate) fn read_lfm25_stream_result() -> Result<Lfm25StreamResult, OffloadTransportError> {
    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return Err(OffloadTransportError::Offline);
    };
    fence(Ordering::Acquire);
    let read_i64 = |low_offset: usize, high_offset: usize| {
        let low = Tga::read_reg(tga.mmio_base + low_offset) as u64;
        let high = Tga::read_reg(tga.mmio_base + high_offset) as u64;
        ((high << 32) | low) as i64
    };
    Ok(Lfm25StreamResult {
        gate_q30: read_i64(
            trueos_fpga_abi::BAR0_LFM25_STREAM_GATE_LO_OFFSET,
            trueos_fpga_abi::BAR0_LFM25_STREAM_GATE_HI_OFFSET,
        ),
        up_q30: read_i64(
            trueos_fpga_abi::BAR0_LFM25_STREAM_UP_LO_OFFSET,
            trueos_fpga_abi::BAR0_LFM25_STREAM_UP_HI_OFFSET,
        ),
        result_q30: read_i64(
            trueos_fpga_abi::BAR0_LFM25_STREAM_RESULT_LO_OFFSET,
            trueos_fpga_abi::BAR0_LFM25_STREAM_RESULT_HI_OFFSET,
        ),
        error_code: Tga::read_reg(tga.mmio_base + trueos_fpga_abi::BAR0_LFM25_STREAM_ERROR_OFFSET),
    })
}

pub(crate) fn lfm25_stream_completion_count() -> Result<u32, OffloadTransportError> {
    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return Err(OffloadTransportError::Offline);
    };
    Ok(Tga::read_reg(tga.mmio_base + trueos_fpga_abi::BAR0_LFM25_STREAM_COMPLETION_COUNT_OFFSET))
}

/// Copy one complete call into the fixed BAR window and hand it to the FPGA.
pub(crate) fn submit_offload_work_package(
    package: &trueos_fpga_abi::WorkPackage,
) -> Result<(), OffloadTransportError> {
    if package.magic != trueos_fpga_abi::WORK_PACKAGE_MAGIC
        || package.abi_version != trueos_fpga_abi::ABI_VERSION
        || trueos_fpga_abi::FunctionId::new(package.function).is_none()
        || package.state != trueos_fpga_abi::WorkState::HostReady as u32
        || package.input_len as usize > trueos_fpga_abi::INLINE_INPUT_BYTES
    {
        return Err(OffloadTransportError::InvalidPackage);
    }

    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return Err(OffloadTransportError::Offline);
    };
    let source = package as *const trueos_fpga_abi::WorkPackage as *const u32;
    // Only header words 0..9 and the declared input are meaningful requests.
    // Reserved header words 10..15 are deliberately not transported.
    const REQUEST_HEADER_WORDS: usize =
        core::mem::offset_of!(trueos_fpga_abi::WorkPackage, reserved_header) / 4;
    const REQUEST_INPUT_WORD: usize =
        core::mem::offset_of!(trueos_fpga_abi::WorkPackage, input) / 4;
    let input_word_count = (package.input_len as usize).div_ceil(4);
    let request_word_count = REQUEST_HEADER_WORDS + input_word_count;
    let request_word_index = |sequence: usize| {
        if sequence < REQUEST_HEADER_WORDS {
            sequence
        } else {
            REQUEST_INPUT_WORD + sequence - REQUEST_HEADER_WORDS
        }
    };

    // A non-posted read of the endpoint's decoded-write counter is both an
    // ordering barrier for preceding posted writes and proof that every TLP in
    // the small batch reached the BAR decoder. PCIe protects the address and
    // payload themselves. If the endpoint cannot absorb even this bounded
    // burst, restart the complete request through the proven per-word
    // write/read repair path and disable batching for the rest of this boot.
    const POSTED_WRITE_BATCH_WORDS: usize = 4;
    let mut batch_ok = !TGA_OFFLOAD_BATCH_DISABLED.load(Ordering::Acquire);
    if batch_ok {
        let mut decoded_expected = Tga::read_reg(tga.mmio_base + TGA_DBG_WRITE_COUNT_OFF);
        let mut batch_start = 0;
        while batch_start < request_word_count {
            let batch_end = (batch_start + POSTED_WRITE_BATCH_WORDS).min(request_word_count);
            for sequence in batch_start..batch_end {
                let index = request_word_index(sequence);
                let expected = unsafe { source.add(index).read() };
                Tga::write_reg(tga.offload_work_package_reg + index * 4, expected);
            }
            fence(Ordering::SeqCst);
            decoded_expected = decoded_expected.wrapping_add((batch_end - batch_start) as u32);
            let decoded_observed = Tga::read_reg(tga.mmio_base + TGA_DBG_WRITE_COUNT_OFF);
            if decoded_observed != decoded_expected {
                batch_ok = false;
                TGA_OFFLOAD_BATCH_DISABLED.store(true, Ordering::Release);
                TGA_OFFLOAD_BATCH_FALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
                if !TGA_OFFLOAD_BATCH_FALLBACK_LOGGED.swap(true, Ordering::AcqRel) {
                    crate::log_warn!(
                        "tga: posted BAR batch was not fully decoded; using per-word verified submission for this boot\n"
                    );
                }
                break;
            }
            batch_start = batch_end;
        }
    }

    if batch_ok {
        fence(Ordering::Release);
        Tga::write_reg(tga.offload_doorbell_reg, TGA_OFFLOAD_DOORBELL_MAGIC);
        return Ok(());
    }

    // Conservative recovery path. A new word-0 write starts the request again;
    // every meaningful word is acknowledged before the next one is presented.
    const WRITE_REPAIR_ATTEMPTS: usize = 8;
    let rx_captures_before = Tga::read_reg(tga.mmio_base + TGA_DBG_RX_CAPTURE_COUNT_OFF);
    let decoded_writes_before = Tga::read_reg(tga.mmio_base + TGA_DBG_WRITE_COUNT_OFF);
    let word30_writes_before = Tga::read_reg(tga.mmio_base + TGA_DBG_WORD30_WRITE_COUNT_OFF);
    let mut repaired_any = false;
    for sequence in 0..request_word_count {
        let index = request_word_index(sequence);
        let expected = unsafe { source.add(index).read() };
        let register = tga.offload_work_package_reg + index * 4;
        let mut observed = 0;
        for attempt in 0..WRITE_REPAIR_ATTEMPTS {
            Tga::write_reg(register, expected);
            if attempt != 0 {
                TGA_OFFLOAD_WRITE_REPAIR_COUNT.fetch_add(1, Ordering::Relaxed);
            }
            fence(Ordering::SeqCst);
            observed = Tga::read_reg(register);
            if observed == expected {
                repaired_any |= attempt != 0;
                break;
            }
        }
        if observed != expected {
            // Snapshot the endpoint before releasing TGA to the heartbeat
            // client. These counters separate a missing posted write from a
            // stale read completion without changing the failing traffic.
            let rx_captures = Tga::read_reg(tga.mmio_base + TGA_DBG_RX_CAPTURE_COUNT_OFF);
            let decoded_writes = Tga::read_reg(tga.mmio_base + TGA_DBG_WRITE_COUNT_OFF);
            let word30_writes = Tga::read_reg(tga.mmio_base + TGA_DBG_WORD30_WRITE_COUNT_OFF);
            let rx_capture_delta = rx_captures.wrapping_sub(rx_captures_before);
            let decoded_write_delta = decoded_writes.wrapping_sub(decoded_writes_before);
            let word30_write_delta = word30_writes.wrapping_sub(word30_writes_before);
            let word30_last_payload =
                Tga::read_reg(tga.mmio_base + TGA_DBG_WORD30_LAST_PAYLOAD_OFF);
            let word30_storage = Tga::read_reg(tga.mmio_base + TGA_DBG_WORD30_STORAGE_OFF);
            let rx_fifo_state = Tga::read_reg(tga.mmio_base + TGA_DBG_RX_FIFO_STATE_OFF);
            let rx_errors = Tga::read_reg(tga.mmio_base + TGA_DBG_RX_ERROR_COUNT_OFF);
            return Err(OffloadTransportError::WriteVerification {
                word: index as u8,
                observed,
                expected,
                rx_captures,
                rx_capture_delta,
                decoded_writes,
                decoded_write_delta,
                word30_writes,
                word30_write_delta,
                word30_last_payload,
                word30_storage,
                rx_fifo_state,
                rx_errors,
            });
        }
    }
    if repaired_any && !TGA_OFFLOAD_WRITE_REPAIR_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log_warn!(
            "tga: repaired stale BAR work-package word before doorbell; request integrity preserved\n"
        );
    }

    fence(Ordering::Release);
    Tga::write_reg(tga.offload_doorbell_reg, TGA_OFFLOAD_DOORBELL_MAGIC);
    Ok(())
}

/// Read only the ownership/completion flag from the call window.
pub(crate) fn offload_work_state() -> Result<trueos_fpga_abi::WorkState, OffloadTransportError> {
    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return Err(OffloadTransportError::Offline);
    };
    fence(Ordering::Acquire);
    let raw =
        Tga::read_reg(tga.offload_work_package_reg + trueos_fpga_abi::WORK_PACKAGE_STATE_OFFSET);
    trueos_fpga_abi::WorkState::from_raw(raw).ok_or(OffloadTransportError::InvalidPackage)
}

/// Read the completed package fields consumed by the ABI decoder.
///
/// The request input, reserved header, and unused tail of the 96-byte output are
/// already known or irrelevant after retirement. Avoiding those non-posted BAR
/// reads matters for fine-grained accelerator calls: a 20-byte slot-2 result now
/// needs 15 completion dwords instead of copying all 64 dwords.
pub(crate) fn read_offload_work_package()
-> Result<trueos_fpga_abi::WorkPackage, OffloadTransportError> {
    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return Err(OffloadTransportError::Offline);
    };
    let mut package = trueos_fpga_abi::WorkPackage::ZEROED;
    let destination = &mut package as *mut trueos_fpga_abi::WorkPackage as *mut u32;
    const HEADER_WORDS: usize =
        core::mem::offset_of!(trueos_fpga_abi::WorkPackage, reserved_header) / 4;
    const OUTPUT_WORD: usize = core::mem::offset_of!(trueos_fpga_abi::WorkPackage, output) / 4;
    fence(Ordering::Acquire);
    for index in 0..HEADER_WORDS {
        let value = Tga::read_reg(tga.offload_work_package_reg + index * 4);
        unsafe { destination.add(index).write(value) };
    }
    let output_len = package.output_len as usize;
    if output_len <= trueos_fpga_abi::INLINE_OUTPUT_BYTES {
        for index in 0..output_len.div_ceil(4) {
            let value = Tga::read_reg(tga.offload_work_package_reg + (OUTPUT_WORD + index) * 4);
            unsafe { destination.add(OUTPUT_WORD + index).write(value) };
        }
    }
    Ok(package)
}

/// Acknowledge a completion interrupt after the service has consumed the result.
/// Polling firmware does not require this operation.
pub(crate) fn ack_offload_interrupt() -> Result<(), OffloadTransportError> {
    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return Err(OffloadTransportError::Offline);
    };
    Tga::write_reg(tga.offload_irq_ack_reg, 1);
    Ok(())
}

pub fn protocol_alive() -> bool {
    TGA.lock()
        .as_ref()
        .map(|tga| tga.protocol_magic() == TGA_MAGIC_EXPECTED)
        .unwrap_or(false)
}

pub fn try_init() -> bool {
    if is_online() {
        return true;
    }

    // Ensure PCI enumeration happened at least once.
    let mut device_count: usize = 0;
    crate::pci::with_devices(|devices| {
        device_count = devices.len();
    });
    if device_count == 0 {
        crate::pci::enumerate_impl();
    }

    let mut found: Option<PciDevice> = None;
    crate::pci::with_devices(|devices| {
        found = devices.iter().copied().find(is_tga);
    });
    let Some(dev) = found else {
        if crate::log_os::flags::BOOT_INFO_LOGS {
            crate::log_os::flags::TGA_MISSING_LOG_ONCE.call_once(|| {
                crate::log!(
                    "tga: device not found (vid=0x{:04X} did=0x{:04X}, scanned {} devices)\n",
                    TGA_VENDOR_ID,
                    TGA_DEVICE_ID,
                    device_count
                );
            });
        }
        return false;
    };

    if let Err(error) = crate::pci::claim_device(&dev, TGA_PCI_OWNER) {
        crate::log!(
            "tga: PCI claim rejected bdf={:02X}:{:02X}.{} error={:?}\n",
            dev.bus,
            dev.slot,
            dev.function,
            error
        );
        return false;
    }

    let Some(tga) = bring_online(&dev) else {
        let _ = crate::pci::release_device_claim(dev.bus, dev.slot, dev.function, TGA_PCI_OWNER);
        return false;
    };

    if !candidate_bundle_ready(&tga) {
        let _ = crate::pci::release_device_claim(dev.bus, dev.slot, dev.function, TGA_PCI_OWNER);
        return false;
    }

    if !configure_completion_interrupt(&tga) {
        TGA_IRQ_CONFIGURED.store(false, Ordering::Release);
        let _ = crate::pci::release_device_claim(dev.bus, dev.slot, dev.function, TGA_PCI_OWNER);
        return false;
    }

    if let Some(prev) = TGA_LAST_DISCONNECT.lock().take() {
        log_reconnect_delta(prev, &tga);
    } else {
        log_tga_state("connected", &tga);
    }

    *TGA.lock() = Some(tga);
    TGA_CONNECTION_GENERATION.fetch_add(1, Ordering::AcqRel);
    TGA_LIVENESS_LOGGED.store(false, Ordering::Release);
    log_liveness_once();
    if TGA_BOOT_MMIO_TOUCH_ENABLED {
        // Keep contract explicit when MMIO touch is enabled: default to LED off.
        tga_led_set(false);
    }
    true
}

pub fn init_once() {
    crate::log_info!(
        target: "boot";
        "tga: init_once deferred; task owns VID/PID claim and hotplug probe\n"
    );
}

fn is_present(tga: &Tga) -> bool {
    crate::pci::config_read_u16(tga.bus, tga.slot, tga.function, 0x00) != 0xFFFF
}

pub fn is_online() -> bool {
    TGA.lock().is_some()
}

pub(crate) fn connection_generation() -> u32 {
    TGA_CONNECTION_GENERATION.load(Ordering::Acquire)
}

fn is_tga(dev: &PciDevice) -> bool {
    dev.vendor == TGA_VENDOR_ID && dev.device == TGA_DEVICE_ID
}

fn bring_lfm25_stream_bar_online(dev: &PciDevice) -> Option<(u64, usize)> {
    let size = tga_bar_size_bytes(dev.bus, dev.slot, dev.function, 2)?;
    if size != TGA_EXPECTED_BAR2_SIZE {
        crate::log_warn!(
            "tga: optional BAR2 size mismatch bdf={:02X}:{:02X}.{} probed=0x{:X} expected=0x{:X}; row streamer disabled\n",
            dev.bus,
            dev.slot,
            dev.function,
            size,
            TGA_EXPECTED_BAR2_SIZE
        );
        return None;
    }

    let (mut bar_lo, mut bar_hi) = crate::pci::read_bar_raw(dev.bus, dev.slot, dev.function, 2);
    if bar_lo == 0xFFFF_FFFF || (bar_lo & 0x1) != 0 || ((bar_lo >> 1) & 0x3) != 0x2 {
        return None;
    }
    if (bar_lo & 0x8) == 0 {
        crate::log_warn!(
            "tga: optional BAR2 is not prefetchable bdf={:02X}:{:02X}.{}; row streamer disabled\n",
            dev.bus,
            dev.slot,
            dev.function
        );
        return None;
    }

    let mut phys = ((bar_hi? as u64) << 32) | ((bar_lo as u64) & !0xFu64);
    if phys == 0 || phys >= 0x40_0000_0000 {
        let base = crate::pci::alloc_hotplug_mmio_base(
            dev.bus,
            TGA_EXPECTED_BAR2_SIZE,
            TGA_EXPECTED_BAR2_SIZE,
        )?;
        let new_lo = ((base as u32) & !0xFu32) | (bar_lo & 0xFu32);
        crate::pci::config_write_u32(dev.bus, dev.slot, dev.function, 0x18, new_lo);
        crate::pci::config_write_u32(dev.bus, dev.slot, dev.function, 0x1C, (base >> 32) as u32);
        crate::pci::enable_mem_space_only(dev.bus, dev.slot, dev.function);
        (bar_lo, bar_hi) = crate::pci::read_bar_raw(dev.bus, dev.slot, dev.function, 2);
        if bar_lo == 0xFFFF_FFFF || (bar_lo & 0x1) != 0 || ((bar_lo >> 1) & 0x3) != 0x2 {
            return None;
        }
        phys = ((bar_hi? as u64) << 32) | ((bar_lo as u64) & !0xFu64);
        if phys == 0 {
            return None;
        }
    }

    let mapped = {
        let last = *TGA_LAST_STREAM_MAP.lock();
        if let Some((last_phys, last_base)) = last {
            if last_phys == phys {
                NonNull::new(last_base as *mut u8)?
            } else {
                let mapping = crate::pci::mmio::map_mmio_region_exact(
                    phys,
                    trueos_fpga_abi::BAR2_LFM25_STREAM_BYTES,
                )
                .ok()?;
                *TGA_LAST_STREAM_MAP.lock() = Some((phys, mapping.as_ptr() as usize));
                mapping
            }
        } else {
            let mapping = crate::pci::mmio::map_mmio_region_exact(
                phys,
                trueos_fpga_abi::BAR2_LFM25_STREAM_BYTES,
            )
            .ok()?;
            *TGA_LAST_STREAM_MAP.lock() = Some((phys, mapping.as_ptr() as usize));
            mapping
        }
    };
    Some((phys, mapped.as_ptr() as usize))
}

fn bring_online(dev: &PciDevice) -> Option<Tga> {
    // Re-validate the device is still present at this BDF.
    // A return of 0xFFFF typically means config space read failed / no device.
    let vid_live = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x00);
    let did_live = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x02);
    if vid_live == 0xFFFF {
        return None;
    }
    if vid_live != TGA_VENDOR_ID || did_live != TGA_DEVICE_ID {
        return None;
    }

    let cmd_before = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);
    crate::pci::enable_mem_space_only(dev.bus, dev.slot, dev.function);
    let cmd_after = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);

    if cmd_before == 0xFFFF || cmd_after == 0xFFFF {
        return None;
    }

    let (mut bar_lo, mut bar_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
    if bar_lo == 0xFFFF_FFFF {
        return None;
    }
    if (bar_lo & 0x1) != 0 {
        return None;
    }

    let bar_is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
    if !bar_is_64 {
        crate::log!(
            "tga: unsupported BAR mode bdf={:02X}:{:02X}.{} (expected 64-bit BAR0/1)\n",
            dev.bus,
            dev.slot,
            dev.function
        );
        return None;
    }

    let mut bar_size = tga_bar_size_bytes(dev.bus, dev.slot, dev.function, 0).unwrap_or(0);
    if bar_size == 0 {
        bar_size = TGA_EXPECTED_BAR0_SIZE;
    } else if bar_size != TGA_EXPECTED_BAR0_SIZE {
        crate::log!(
            "tga: BAR0 size mismatch bdf={:02X}:{:02X}.{} probed=0x{:X} expected=0x{:X} (continuing)\n",
            dev.bus,
            dev.slot,
            dev.function,
            bar_size,
            TGA_EXPECTED_BAR0_SIZE
        );
    }

    let bar_hi_u32 = bar_hi?;

    let mut bar_phys = {
        let lo = (bar_lo as u64) & !0xFu64;
        let hi = bar_hi_u32 as u64;
        lo | (hi << 32)
    };

    let mut bar_assignment = TgaBarAssignment::Firmware;

    // Sanity check: if BAR is uninitialized (0) or at a suspiciously high address
    // (e.g. > 256 GiB), we force reassignment to our known-good 32-bit MMIO window.
    //
    // Context: we've observed the device reporting 0x3800_0000_0000 (approx 61 TiB).
    // This exceeds the physical address width of many hosts (e.g. 39-bit = 512 GiB)
    // and causes QEMU VFIO DMA map failures (error -22) when the guest enables
    // the BAR.
    if bar_phys == 0 || bar_phys >= 0x40_0000_0000 {
        // Hotplug path: firmware may not have assigned BARs for devices appearing later.
        // A transient SRAM reload is different: the same endpoint's firmware-routed
        // resource lease remains valid even though the endpoint temporarily forgot
        // its BAR registers. Restore that exact lease before considering allocation.
        // Generic allocation is reserved for a genuinely new endpoint or BAR shape.
        let size = TGA_EXPECTED_BAR0_SIZE;
        // Keep BAR base at least 4KiB aligned.
        // The current FPGA-side write decode matches BAR0 + 0x00 via address low bits,
        // so non-page-aligned hotplug bases (e.g. ...FC00) can miss that match.
        let align = TGA_EXPECTED_BAR0_SIZE.max(0x1000);

        let previous_lease = TGA_LAST_WORKING_LEASE
            .lock()
            .as_ref()
            .copied()
            .filter(|previous| {
                previous.bus == dev.bus
                    && previous.slot == dev.slot
                    && previous.function == dev.function
                    && previous.bar_size == bar_size
                    && previous.bar_is_64 == bar_is_64
                    && previous.bar_phys != 0
                    && previous.bar_phys % align == 0
            });
        let base = if let Some(previous) = previous_lease {
            bar_assignment = TgaBarAssignment::Restored;
            previous.bar_phys
        } else {
            bar_assignment = TgaBarAssignment::Allocated;
            crate::pci::alloc_hotplug_mmio_base(dev.bus, size, align)?
        };

        // Preserve the low BAR attribute bits (IO/type/prefetch) reported by the device.
        let new_lo = ((base as u32) & !0xFu32) | (bar_lo & 0xFu32);
        crate::pci::config_write_u32(dev.bus, dev.slot, dev.function, 0x10, new_lo);
        crate::pci::config_write_u32(dev.bus, dev.slot, dev.function, 0x14, (base >> 32) as u32);

        // Re-read and re-validate.
        (bar_lo, bar_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
        if bar_lo == 0xFFFF_FFFF {
            return None;
        }
        if (bar_lo & 0x1) != 0 {
            return None;
        }

        bar_phys = {
            let lo = (bar_lo as u64) & !0xFu64;
            let hi = bar_hi? as u64;
            lo | (hi << 32)
        };

        crate::pci::enable_mem_space_only(dev.bus, dev.slot, dev.function);
        if bar_phys == 0 {
            return None;
        }
    } else {
        // If the BAR was already valid, ensure the device is enabled now.
        crate::pci::enable_mem_space_only(dev.bus, dev.slot, dev.function);
    }

    // A retry after an early, not-yet-live candidate sees the BAR value that
    // the first attempt already restored. Preserve that reconnect provenance
    // instead of relabelling the same lease as a firmware assignment.
    if bar_assignment == TgaBarAssignment::Firmware {
        let disconnected = TGA_LAST_DISCONNECT.lock().as_ref().copied();
        let working = TGA_LAST_WORKING_LEASE.lock().as_ref().copied();
        if disconnected.is_some_and(|previous| {
            previous.bus == dev.bus
                && previous.slot == dev.slot
                && previous.function == dev.function
                && previous.bar_size == bar_size
                && previous.bar_is_64 == bar_is_64
        }) && working.is_some_and(|previous| {
            previous.bus == dev.bus
                && previous.slot == dev.slot
                && previous.function == dev.function
                && previous.bar_phys == bar_phys
                && previous.bar_size == bar_size
                && previous.bar_is_64 == bar_is_64
        }) {
            bar_assignment = TgaBarAssignment::Restored;
        }
    }

    // We only need the first few BAR0 registers, so mapping 1 page keeps it minimal.
    let mapped = {
        let last = *TGA_LAST_MAP.lock();
        if let Some((last_phys, last_base)) = last {
            if last_phys == bar_phys {
                NonNull::new(last_base as *mut u8)?
            } else {
                let m = crate::pci::mmio::map_mmio_region_exact(bar_phys, 0x1000).ok()?;
                *TGA_LAST_MAP.lock() = Some((bar_phys, m.as_ptr() as usize));
                m
            }
        } else {
            let m = crate::pci::mmio::map_mmio_region_exact(bar_phys, 0x1000).ok()?;
            *TGA_LAST_MAP.lock() = Some((bar_phys, m.as_ptr() as usize));
            m
        }
    };

    let base = mapped.as_ptr() as usize;
    let led_reg = base + TGA_LED_SET_OFF;
    let magic_reg = base + TGA_MAGIC_OFF;
    let offload_work_package_reg = base + TGA_OFFLOAD_WORK_PACKAGE_OFF;
    let offload_doorbell_reg = base + TGA_OFFLOAD_DOORBELL_OFF;
    let offload_irq_ack_reg = base + TGA_OFFLOAD_IRQ_ACK_OFF;
    let firmware_manifest_reg = base + TGA_FIRMWARE_MANIFEST_OFF;
    let stream_bar = bring_lfm25_stream_bar_online(dev);

    let tga = Tga {
        bus: dev.bus,
        slot: dev.slot,
        function: dev.function,
        bar_phys,
        bar_size,
        bar_is_64,
        bar_assignment,
        mmio_base: base,
        stream_bar_phys: stream_bar.map(|(phys, _)| phys),
        stream_mmio_base: stream_bar.map(|(_, mapped)| mapped),
        led_reg,
        magic_reg,
        offload_work_package_reg,
        offload_doorbell_reg,
        offload_irq_ack_reg,
        firmware_manifest_reg,
    };
    if TGA_BOOT_MMIO_TOUCH_ENABLED {
        tga.write_led(0);
    }
    Some(tga)
}

#[embassy_executor::task]
pub(crate) async fn tga_task() {
    crate::log_os::flags::TGA_TASK_STARTED_LOG_ONCE.call_once(|| {
        crate::log_info!(target: "boot"; "tga: task started\n");
    });
    let mut presence_miss_streak: u8 = 0;
    let period = EmbassyDuration::from_millis(TGA_HEARTBEAT_PERIOD_MS);
    let presence_probe_period = EmbassyDuration::from_millis(TGA_PRESENCE_PROBE_PERIOD_MS);
    let mut next_tick = Instant::now() + period;
    let mut next_presence_probe = Instant::now() + presence_probe_period;
    loop {
        if !is_online() {
            crate::pci::enumerate_impl();
            let mut initialized = try_init();
            let disconnected = TGA_LAST_DISCONNECT.lock().as_ref().copied();
            if !initialized
                && let Some(previous) = disconnected
                && TGA_LINK_RECOVERY_ATTEMPTED
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                match crate::pci::recover_dedicated_downstream_link(
                    previous.bus,
                    previous.slot,
                    previous.function,
                ) {
                    Ok(recovery) => {
                        // Link training is asynchronous. Do not block the kernel while the
                        // root port and freshly configured FPGA negotiate it.
                        Timer::after(EmbassyDuration::from_millis(100)).await;
                        crate::pci::enumerate_impl();
                        initialized = try_init();
                        if initialized {
                            crate::log_warn!(
                                target: "boot";
                                "tga: hotplug event (warn marks a significant low-level event, not a detected fault): upstream link retrain completed bridge={:02X}:{:02X}.{} target={:02X}:{:02X}.{} link_status_before=0x{:04X} result=endpoint-online\n",
                                recovery.bridge_bus,
                                recovery.bridge_slot,
                                recovery.bridge_function,
                                previous.bus,
                                previous.slot,
                                previous.function,
                                recovery.link_status_before
                            );
                        } else {
                            crate::log_warn!(
                                target: "boot";
                                "tga: hotplug recovery failed: upstream link retrain did not return endpoint bridge={:02X}:{:02X}.{} target={:02X}:{:02X}.{} link_status_before=0x{:04X}\n",
                                recovery.bridge_bus,
                                recovery.bridge_slot,
                                recovery.bridge_function,
                                previous.bus,
                                previous.slot,
                                previous.function,
                                recovery.link_status_before
                            );
                        }
                    }
                    Err(error) => {
                        crate::log_warn!(
                            target: "boot";
                            "tga: hotplug recovery skipped (warn marks a significant low-level event, not a detected fault): target={:02X}:{:02X}.{} reason={:?}\n",
                            previous.bus,
                            previous.slot,
                            previous.function,
                            error
                        );
                    }
                }
            }
            presence_miss_streak = 0;
            next_tick = Instant::now() + period;
            next_presence_probe = Instant::now() + presence_probe_period;
            Timer::after(EmbassyDuration::from_millis(TGA_OFFLINE_RETRY_MS)).await;
            continue;
        }

        let now = Instant::now();
        if now >= next_presence_probe {
            // Probe less frequently than heartbeat writes to keep LED cadence stable.
            let present = {
                let guard = TGA.lock();
                guard.as_ref().map(is_present).unwrap_or(false)
            };
            if !present {
                presence_miss_streak = presence_miss_streak.saturating_add(1);
                if presence_miss_streak >= TGA_PRESENCE_MISS_THRESHOLD {
                    {
                        let mut guard = TGA.lock();
                        if let Some(old) = guard.take() {
                            let _ = crate::pci::release_device_claim(
                                old.bus,
                                old.slot,
                                old.function,
                                TGA_PCI_OWNER,
                            );
                            *TGA_LAST_DISCONNECT.lock() = Some(snapshot_from_tga(&old));
                            TGA_LINK_RECOVERY_ATTEMPTED.store(false, Ordering::Release);
                            TGA_LIVENESS_LOGGED.store(false, Ordering::Release);
                            wake_completion_waiter_offline();
                        }
                    }
                    presence_miss_streak = 0;
                    next_tick = Instant::now() + period;
                    next_presence_probe = Instant::now() + presence_probe_period;
                    Timer::after(EmbassyDuration::from_millis(TGA_OFFLINE_RETRY_MS)).await;
                    continue;
                }
            } else {
                presence_miss_streak = 0;
            }
            next_presence_probe = now + presence_probe_period;
        }

        let t = TGA_HEARTBEAT_COUNTER.fetch_add(1, Ordering::Relaxed);
        if TGA_HEARTBEAT_MMIO_ENABLED {
            // Send 0..31 then wrap.
            write_heartbeat_led(t & 0x1F, t);
        }
        log_liveness_once();

        let now = Instant::now();
        if next_tick <= now {
            next_tick = now + period;
        }
        Timer::at(next_tick).await;
        next_tick += period;
    }
}
