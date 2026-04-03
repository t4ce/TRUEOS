use alloc::vec::Vec;
use core::alloc::Layout;
use core::cmp::min;
use core::future::Future;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use core::task::Poll;
use core::time::Duration;

use crab_usb::{EndpointKind, Event, EventHandler, KernelOp, USBHost, usb_if};
use dma_api::{DmaAddr, DmaDirection, DmaError, DmaHandle, DmaMapHandle, DmaOp};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

#[path = "sound/mod.rs"]
pub(crate) mod sound;

pub(super) struct TrueosCrabUsbKernel;

pub(super) static CRABUSB_KERNEL: TrueosCrabUsbKernel = TrueosCrabUsbKernel;

use super::xhci::MAX_XHCI_CONTROLLERS;

static INITIAL_SNAPSHOT_LOGGED: [AtomicBool; MAX_XHCI_CONTROLLERS] =
    [const { AtomicBool::new(false) }; MAX_XHCI_CONTROLLERS];
static EVENT_HANDLER_READY: [AtomicBool; MAX_XHCI_CONTROLLERS] =
    [const { AtomicBool::new(false) }; MAX_XHCI_CONTROLLERS];
static CONTROLLER_PHASE: [AtomicU32; MAX_XHCI_CONTROLLERS] =
    [const { AtomicU32::new(0) }; MAX_XHCI_CONTROLLERS];
static EVENT_HANDLER: [Mutex<Option<EventHandler>>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(None) }; MAX_XHCI_CONTROLLERS];
static PROBE_REQUESTED: [AtomicBool; MAX_XHCI_CONTROLLERS] =
    [const { AtomicBool::new(false) }; MAX_XHCI_CONTROLLERS];
static ROOT_PORT_CHANGE_SEEN: [AtomicBool; MAX_XHCI_CONTROLLERS] =
    [const { AtomicBool::new(false) }; MAX_XHCI_CONTROLLERS];
static NO_PORT_CHANGE_HINT_LOGGED: [AtomicBool; MAX_XHCI_CONTROLLERS] =
    [const { AtomicBool::new(false) }; MAX_XHCI_CONTROLLERS];
static ROOT_HUB_LIFECYCLE_STAGE: [AtomicU32; MAX_XHCI_CONTROLLERS] =
    [const { AtomicU32::new(0) }; MAX_XHCI_CONTROLLERS];
static AUDIO_STREAM_REQUESTED: AtomicBool = AtomicBool::new(false);
static AUDIO_STREAM_ACTIVE: AtomicBool = AtomicBool::new(false);
static TRUEKEY_STREAM_REQUESTED: AtomicBool = AtomicBool::new(false);
static TRUEKEY_STREAM_ACTIVE: AtomicBool = AtomicBool::new(false);
static BOUNCE_MAPPINGS: Mutex<Vec<BounceMapping>> = Mutex::new(Vec::new());
static EMPTY_PROBE_STREAK: [AtomicU32; MAX_XHCI_CONTROLLERS] =
    [const { AtomicU32::new(0) }; MAX_XHCI_CONTROLLERS];
static LAST_PROBE_STATE: [AtomicU32; MAX_XHCI_CONTROLLERS] =
    [const { AtomicU32::new(0) }; MAX_XHCI_CONTROLLERS]; // 0=init, 1=ok, 2=empty, 3=error
static LAST_PROBE_DEVICE_COUNT: [AtomicU32; MAX_XHCI_CONTROLLERS] =
    [const { AtomicU32::new(0) }; MAX_XHCI_CONTROLLERS];
static PROBE_FAIL_STREAK: [AtomicU32; MAX_XHCI_CONTROLLERS] =
    [const { AtomicU32::new(0) }; MAX_XHCI_CONTROLLERS];
static TLB_DEVICES: [Mutex<Vec<super::TlbUsbDevice>>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(Vec::new()) }; MAX_XHCI_CONTROLLERS];
static TLB_TOPOLOGY: [Mutex<Vec<super::TlbUsbTopologyNode>>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(Vec::new()) }; MAX_XHCI_CONTROLLERS];

#[derive(Clone, Copy)]
pub(super) struct UsbRuntimeDiag {
    pub event_handler_ready: bool,
    pub probe_requested: bool,
    pub root_port_change_seen: bool,
    pub controller_phase: &'static str,
    pub root_hub_lifecycle: &'static str,
    pub empty_probe_streak: u32,
    pub probe_fail_streak: u32,
    pub last_probe_state: &'static str,
    pub last_probe_device_count: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ControllerPhase {
    Startup = 0,
    FirstContact = 1,
    Quarantined = 2,
    Validated = 3,
    Recoverable = 4,
}

fn controller_phase_name(phase: ControllerPhase) -> &'static str {
    match phase {
        ControllerPhase::Startup => "startup",
        ControllerPhase::FirstContact => "first-contact",
        ControllerPhase::Quarantined => "quarantined",
        ControllerPhase::Validated => "validated",
        ControllerPhase::Recoverable => "recoverable",
    }
}

fn controller_phase_from_raw(raw: u32) -> ControllerPhase {
    match raw {
        1 => ControllerPhase::FirstContact,
        2 => ControllerPhase::Quarantined,
        3 => ControllerPhase::Validated,
        4 => ControllerPhase::Recoverable,
        _ => ControllerPhase::Startup,
    }
}

fn controller_phase(controller_id: usize) -> ControllerPhase {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return ControllerPhase::Startup;
    }
    controller_phase_from_raw(CONTROLLER_PHASE[controller_id].load(Ordering::Acquire))
}

fn set_controller_phase(controller_id: usize, phase: ControllerPhase, reason: &'static str) {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return;
    }
    let prev = controller_phase(controller_id);
    if prev != phase {
        crate::log!(
            "crabusb: controller {} phase {} -> {} ({})\n",
            controller_id,
            controller_phase_name(prev),
            controller_phase_name(phase),
            reason
        );
    }
    CONTROLLER_PHASE[controller_id].store(phase as u32, Ordering::Release);
}

fn mark_controller_validated(controller_id: usize, reason: &'static str) {
    let phase = controller_phase(controller_id);
    if !matches!(phase, ControllerPhase::Validated | ControllerPhase::Recoverable) {
        set_controller_phase(controller_id, ControllerPhase::Validated, reason);
    }
}

fn mark_controller_quarantined(controller_id: usize, reason: &'static str) {
    set_controller_phase(controller_id, ControllerPhase::Quarantined, reason);
}

fn mark_controller_recoverable(controller_id: usize, reason: &'static str) {
    set_controller_phase(controller_id, ControllerPhase::Recoverable, reason);
}

fn controller_can_rebind(controller_id: usize) -> bool {
    matches!(
        controller_phase(controller_id),
        ControllerPhase::Validated | ControllerPhase::Recoverable
    )
}

fn controller_needs_quarantine(controller_id: usize) -> bool {
    matches!(
        controller_phase(controller_id),
        ControllerPhase::Startup | ControllerPhase::FirstContact | ControllerPhase::Quarantined
    )
}

const DEMO_WAV_EMBEDDED: &[u8] = b""; // temporary empty audio payload
const AUDIO_FRAME_BYTES: usize = 4; // s16le stereo
const TRUEKEY_VENDOR_ID: u16 = 0x303A;
const TRUEKEY_PRODUCT_ID: u16 = 0x1001;
const TRUEKEY_STREAM_CHUNK: usize = 512;
const HID_INTERRUPT_TIMEOUT_MS: u64 = 1000;
const CRABUSB_PROBE_TIMEOUT_MS: u64 = 2500;
const CRABUSB_INITIAL_SETTLE_MS: u64 = 250;
const CRABUSB_PROBE_QUIET_MS: u64 = 150;
const CRABUSB_INTEL_INITIAL_SETTLE_MS: u64 = 1000;
const CRABUSB_INTEL_PROBE_QUIET_MS: u64 = 750;
const CRABUSB_INTEL_SKIP_PROBE_EXPERIMENT: bool = true;
const CRABUSB_INTEL_SKIP_PROBE_REARM_MS: u64 = 1000;
const CRABUSB_INTEL_SKIP_EVENT_HANDLER_EXPERIMENT: bool = true;
const CRABUSB_INTEL_PORT_POWER_HOLDOFF_EXPERIMENT: bool = true;
const CRABUSB_QUICK_STOP_WINDOW_MS: u64 = 1000;
const CRABUSB_QUICK_STOP_BACKOFF_MS: u64 = 2000;
const CRABUSB_MAX_QUICK_STOP_REBINDS: u32 = 3;
const CRABUSB_INTEL_QUIESCENT_EXPERIMENT: bool = true;
const CRABUSB_INTEL_QUIESCENT_EXPERIMENT_MS: u64 = 1500;
const CRABUSB_INTEL_QUIESCENT_POLL_MS: u64 = 25;
const ROOT_HUB_LIFECYCLE_INIT: u32 = 0;
const ROOT_HUB_LIFECYCLE_BOUND: u32 = 1;
const ROOT_HUB_LIFECYCLE_ROOT_CHANGE: u32 = 2;
const ROOT_HUB_LIFECYCLE_SETTLING: u32 = 3;
const ROOT_HUB_LIFECYCLE_FIRST_CONTACT: u32 = 4;
const ROOT_HUB_LIFECYCLE_STEADY: u32 = 5;

fn probe_state_name(code: u32) -> &'static str {
    match code {
        0 => "init",
        1 => "ok",
        2 => "empty",
        3 => "error",
        4 => "timeout",
        5 => "steady",
        _ => "unknown",
    }
}

#[inline]
fn intel_quiescent_experiment_enabled(device_id: u16) -> bool {
    CRABUSB_INTEL_QUIESCENT_EXPERIMENT && device_id != 0x7A60
}

fn cached_device_count(controller_id: usize) -> usize {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return 0;
    }
    TLB_DEVICES[controller_id].lock().len()
}

fn root_hub_lifecycle_name(stage: u32) -> &'static str {
    match stage {
        ROOT_HUB_LIFECYCLE_INIT => "init",
        ROOT_HUB_LIFECYCLE_BOUND => "bound",
        ROOT_HUB_LIFECYCLE_ROOT_CHANGE => "root-change",
        ROOT_HUB_LIFECYCLE_SETTLING => "settling",
        ROOT_HUB_LIFECYCLE_FIRST_CONTACT => "first-contact",
        ROOT_HUB_LIFECYCLE_STEADY => "steady",
        _ => "unknown",
    }
}

fn root_hub_lifecycle_bucket(stage: u32) -> &'static str {
    match stage {
        ROOT_HUB_LIFECYCLE_INIT | ROOT_HUB_LIFECYCLE_BOUND => "before-root-change",
        ROOT_HUB_LIFECYCLE_ROOT_CHANGE | ROOT_HUB_LIFECYCLE_SETTLING => "during-root-progression",
        ROOT_HUB_LIFECYCLE_FIRST_CONTACT | ROOT_HUB_LIFECYCLE_STEADY => "after-first-contact",
        _ => "unknown",
    }
}

fn reset_root_hub_lifecycle(controller_id: usize, stage: u32, reason: &str) {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return;
    }

    let prev = ROOT_HUB_LIFECYCLE_STAGE[controller_id].swap(stage, Ordering::AcqRel);
    if prev != stage {
        crate::log!(
            "crabusb: controller {} root-hub lifecycle {} -> {} ({})\n",
            controller_id,
            root_hub_lifecycle_name(prev),
            root_hub_lifecycle_name(stage),
            reason
        );
    }
}

fn advance_root_hub_lifecycle(controller_id: usize, stage: u32, reason: &str) {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return;
    }

    loop {
        let prev = ROOT_HUB_LIFECYCLE_STAGE[controller_id].load(Ordering::Acquire);
        if stage <= prev {
            return;
        }
        if ROOT_HUB_LIFECYCLE_STAGE[controller_id]
            .compare_exchange(prev, stage, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            crate::log!(
                "crabusb: controller {} root-hub lifecycle {} -> {} ({})\n",
                controller_id,
                root_hub_lifecycle_name(prev),
                root_hub_lifecycle_name(stage),
                reason
            );
            return;
        }
    }
}

fn root_hub_lifecycle_stage(controller_id: usize) -> u32 {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return ROOT_HUB_LIFECYCLE_INIT;
    }
    ROOT_HUB_LIFECYCLE_STAGE[controller_id].load(Ordering::Acquire)
}

fn root_hub_lifecycle_summary(controller_id: usize) -> &'static str {
    root_hub_lifecycle_name(root_hub_lifecycle_stage(controller_id))
}

fn root_hub_lifecycle_bucket_for_controller(controller_id: usize) -> &'static str {
    root_hub_lifecycle_bucket(root_hub_lifecycle_stage(controller_id))
}

fn log_probe_progress(prefix: &str) {
    let progress = crab_usb::debug_usb_probe_progress();
    let submit = crab_usb::debug_last_submit();
    let event = crab_usb::debug_last_event();
    crate::log!(
        "crabusb: {} stage={} root_port={} port={} slot={} detail={} submit[dci={} dir={} len={} ptr=0x{:X}] event[slot={} ep={} cc={} residual={} ptr=0x{:X}]\n",
        prefix,
        crab_usb::debug_usb_probe_stage_name(progress.stage),
        progress.root_port,
        progress.port,
        progress.slot,
        progress.detail,
        submit.dci,
        submit.direction,
        submit.len,
        submit.ptr,
        event.slot_id,
        event.ep_id,
        event.completion_code,
        event.residual,
        event.ptr,
    );
}

#[inline]
unsafe fn read_mmio32(base: *const u8, offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile(base.add(offset) as *const u32) }
}

#[inline]
unsafe fn read_mmio64(base: *const u8, offset: usize) -> u64 {
    let lo = unsafe { read_mmio32(base, offset) } as u64;
    let hi = unsafe { read_mmio32(base, offset + 4) } as u64;
    lo | (hi << 32)
}

fn log_xhci_runtime_snapshot(controller_id: usize, prefix: &str) {
    let Some(info) = super::controller_by_index(controller_id) else {
        return;
    };

    let progress = crab_usb::debug_usb_probe_progress();
    let root_port = if progress.root_port != 0 {
        progress.root_port
    } else {
        progress.port
    };

    unsafe {
        let mmio = info.mmio_base.as_ptr() as *const u8;
        let caplen = (read_mmio32(mmio, 0x00) & 0xFF) as usize;
        let dboff = (read_mmio32(mmio, 0x14) & !0x3) as usize;
        let rtsoff = (read_mmio32(mmio, 0x18) & !0x1F) as usize;

        let usbcmd = read_mmio32(mmio, caplen);
        let usbsts = read_mmio32(mmio, caplen + 0x04);
        let crcr = read_mmio64(mmio, caplen + 0x18);
        let dcbaap = read_mmio64(mmio, caplen + 0x30);
        let config = read_mmio32(mmio, caplen + 0x38);

        let iman = read_mmio32(mmio, rtsoff + 0x20);
        let imod = read_mmio32(mmio, rtsoff + 0x24);
        let erstsz = read_mmio32(mmio, rtsoff + 0x28);
        let erstba = read_mmio64(mmio, rtsoff + 0x30);
        let erdp = read_mmio64(mmio, rtsoff + 0x38);

        crate::log!(
            "crabusb: {} xhci regs ctrl={} mmio={:p} caplen=0x{:X} dboff=0x{:X} rtsoff=0x{:X} usbcmd=0x{:08X} usbsts=0x{:08X} crcr=0x{:016X} dcbaap=0x{:016X} config=0x{:08X} iman=0x{:08X} imod=0x{:08X} erstsz=0x{:08X} erstba=0x{:016X} erdp=0x{:016X}\n",
            prefix,
            controller_id,
            info.mmio_base,
            caplen,
            dboff,
            rtsoff,
            usbcmd,
            usbsts,
            crcr,
            dcbaap,
            config,
            iman,
            imod,
            erstsz,
            erstba,
            erdp,
        );

        if root_port != 0 {
            let portsc_off = caplen + 0x400 + ((root_port as usize - 1) * 0x10);
            let portsc = read_mmio32(mmio, portsc_off);
            let portpmsc = read_mmio32(mmio, portsc_off + 0x04);
            let portli = read_mmio32(mmio, portsc_off + 0x08);
            crate::log!(
                "crabusb: {} xhci port ctrl={} root_port={} portsc=0x{:08X} portpmsc=0x{:08X} portli=0x{:08X}\n",
                prefix,
                controller_id,
                root_port,
                portsc,
                portpmsc,
                portli,
            );
        }
    }
}

#[derive(Clone, Copy)]
struct XhciStatusBits {
    hc_halted: bool,
    host_system_error: bool,
    controller_not_ready: bool,
    host_controller_error: bool,
}

fn xhci_status_bits(controller_id: usize) -> Option<XhciStatusBits> {
    let info = super::controller_by_index(controller_id)?;
    let mmio = info.mmio_base.as_ptr() as *const u8;
    unsafe {
        let caplen = (read_mmio32(mmio, 0x00) & 0xFF) as usize;
        let usbsts = read_mmio32(mmio, caplen + 0x04);
        Some(XhciStatusBits {
            hc_halted: (usbsts & (1 << 0)) != 0,
            host_system_error: (usbsts & (1 << 2)) != 0,
            controller_not_ready: (usbsts & (1 << 11)) != 0,
            host_controller_error: (usbsts & (1 << 12)) != 0,
        })
    }
}

fn xhci_fatal_probe_state(controller_id: usize) -> Option<&'static str> {
    let status = xhci_status_bits(controller_id)?;
    let submit = crab_usb::debug_last_submit();
    if status.host_system_error {
        Some("host-system-error")
    } else if status.host_controller_error {
        Some("host-controller-error")
    } else if status.hc_halted && submit.ptr != 0 {
        Some("controller-halted-after-submit")
    } else if status.controller_not_ready && submit.ptr != 0 {
        Some("controller-not-ready-after-submit")
    } else {
        None
    }
}

fn xhci_fatal_init_state(controller_id: usize) -> Option<&'static str> {
    let status = xhci_status_bits(controller_id)?;
    if status.host_system_error {
        Some("host-system-error")
    } else if status.host_controller_error {
        Some("host-controller-error")
    } else {
        None
    }
}

fn log_xhci_status_bits(controller_id: usize, prefix: &str) {
    let Some(status) = xhci_status_bits(controller_id) else {
        return;
    };
    crate::log!(
        "crabusb: {} xhci status ctrl={} phase={} halted={} hse={} cnr={} hce={}\n",
        prefix,
        controller_id,
        controller_phase_name(controller_phase(controller_id)),
        status.hc_halted,
        status.host_system_error,
        status.controller_not_ready,
        status.host_controller_error,
    );
}

fn xhci_set_interrupter_enable(controller_id: usize, enabled: bool) -> bool {
    let info = match super::controller_by_index(controller_id) {
        Some(info) => info,
        None => return false,
    };
    let mmio = info.mmio_base.as_ptr() as *mut u8;
    unsafe {
        let caplen = (read_mmio32(mmio.cast_const(), 0x00) & 0xFF) as usize;
        let usbcmd_off = caplen;
        let mut usbcmd = read_mmio32(mmio.cast_const(), usbcmd_off);
        const XHCI_USBCMD_INTE: u32 = 1 << 2;
        if enabled {
            usbcmd |= XHCI_USBCMD_INTE;
        } else {
            usbcmd &= !XHCI_USBCMD_INTE;
        }
        write_mmio32(mmio, usbcmd_off, usbcmd);
        let _ = read_mmio32(mmio.cast_const(), usbcmd_off);
    }
    true
}

fn controller_initial_settle_ms(vendor_id: u16) -> u64 {
    if vendor_id == 0x8086 {
        CRABUSB_INTEL_INITIAL_SETTLE_MS
    } else {
        CRABUSB_INITIAL_SETTLE_MS
    }
}

fn controller_probe_quiet_ms(vendor_id: u16) -> u64 {
    if vendor_id == 0x8086 {
        CRABUSB_INTEL_PROBE_QUIET_MS
    } else {
        CRABUSB_PROBE_QUIET_MS
    }
}

fn intel_skip_event_handler_experiment(vendor_id: u16) -> bool {
    vendor_id == 0x8086 && CRABUSB_INTEL_SKIP_EVENT_HANDLER_EXPERIMENT
}

fn xhci_any_connected_root_port(controller_id: usize) -> Option<u8> {
    let info = super::controller_by_index(controller_id)?;
    let mmio = info.mmio_base.as_ptr() as *const u8;
    unsafe {
        let caplen = (read_mmio32(mmio, 0x00) & 0xFF) as usize;
        let hcsparams1 = read_mmio32(mmio, 0x04);
        let port_count = ((hcsparams1 >> 24) & 0xff) as usize;
        for port_idx in 0..port_count {
            let portsc_off = caplen + 0x400 + (port_idx * 0x10);
            let portsc = read_mmio32(mmio, portsc_off);
            if (portsc & 0x1) != 0 {
                return Some((port_idx + 1) as u8);
            }
        }
    }
    None
}

fn xhci_set_all_root_port_power(controller_id: usize, enabled: bool) -> Option<usize> {
    let info = super::controller_by_index(controller_id)?;
    let mmio = info.mmio_base.as_ptr() as *mut u8;
    let mut changed = 0usize;
    unsafe {
        let caplen = (read_mmio32(mmio.cast_const(), 0x00) & 0xFF) as usize;
        let hcsparams1 = read_mmio32(mmio.cast_const(), 0x04);
        let port_count = ((hcsparams1 >> 24) & 0xff) as usize;
        const PORTSC_PP: u32 = 1 << 9;
        for port_idx in 0..port_count {
            let portsc_off = caplen + 0x400 + (port_idx * 0x10);
            let mut portsc = read_mmio32(mmio.cast_const(), portsc_off);
            let before = (portsc & PORTSC_PP) != 0;
            if enabled {
                portsc |= PORTSC_PP;
            } else {
                portsc &= !PORTSC_PP;
            }
            let after = (portsc & PORTSC_PP) != 0;
            if before != after {
                write_mmio32(mmio, portsc_off, portsc);
                let _ = read_mmio32(mmio.cast_const(), portsc_off);
                changed += 1;
            }
        }
    }
    Some(changed)
}

#[inline]
unsafe fn write_mmio32(base: *mut u8, offset: usize, value: u32) {
    unsafe { core::ptr::write_volatile(base.add(offset) as *mut u32, value) };
}

fn is_pre_command_root_hub_wait() -> bool {
    let progress = crab_usb::debug_usb_probe_progress();
    let submit = crab_usb::debug_last_submit();
    progress.stage == 1 && submit.ptr == 0
}

#[derive(Copy, Clone)]
struct BounceMapping {
    orig_virt: usize,
    bounce_virt: usize,
    size: usize,
    direction: DmaDirection,
}

#[inline]
fn crabusb_dma_cache_flush(addr: NonNull<u8>, size: usize) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use core::arch::x86_64::{_mm_clflush, _mm_mfence};

        if size == 0 {
            return;
        }

        let line = 64usize;
        let start = (addr.as_ptr() as usize) & !(line - 1);
        let end = (addr.as_ptr() as usize).saturating_add(size);
        let mut ptr = start;
        while ptr < end {
            _mm_clflush(ptr as *const _);
            ptr = ptr.saturating_add(line);
        }
        _mm_mfence();
    }
}

#[inline]
fn crabusb_dma_cache_invalidate(addr: NonNull<u8>, size: usize) {
    crabusb_dma_cache_flush(addr, size);
}

impl DmaOp for TrueosCrabUsbKernel {
    fn page_size(&self) -> usize {
        4096
    }

    fn flush(&self, addr: NonNull<u8>, size: usize) {
        crabusb_dma_cache_flush(addr, size);
    }

    fn invalidate(&self, addr: NonNull<u8>, size: usize) {
        crabusb_dma_cache_invalidate(addr, size);
    }

    fn flush_invalidate(&self, addr: NonNull<u8>, size: usize) {
        crabusb_dma_cache_flush(addr, size);
    }

    unsafe fn map_single(
        &self,
        dma_mask: u64,
        addr: NonNull<u8>,
        size: NonZeroUsize,
        align: usize,
        _direction: DmaDirection,
    ) -> Result<DmaMapHandle, DmaError> {
        let required_align = align.max(1);
        let layout =
            Layout::from_size_align(size.get(), required_align).map_err(DmaError::LayoutError)?;
        let phys = crate::phys::virt_to_phys_checked(addr.as_ptr()).ok_or(DmaError::NoMemory)?;

        let aligned = phys.is_multiple_of(required_align as u64);
        let in_mask = phys
            .checked_add(size.get().saturating_sub(1) as u64)
            .map(|end| end <= dma_mask)
            .unwrap_or(false);

        if aligned && in_mask {
            return Ok(unsafe { DmaMapHandle::new(addr, DmaAddr::from(phys), layout, None) });
        }

        let max_phys_exclusive = if dma_mask == u64::MAX {
            None
        } else {
            dma_mask.checked_add(1)
        };
        let (bounce_phys, bounce_virt) =
            crate::dma::alloc_with_max(layout.size(), layout.align(), max_phys_exclusive)
                .ok_or(DmaError::NoMemory)?;
        let bounce_virt = NonNull::new(bounce_virt).ok_or(DmaError::NoMemory)?;

        if matches!(_direction, DmaDirection::ToDevice | DmaDirection::Bidirectional) {
            unsafe {
                core::ptr::copy_nonoverlapping(addr.as_ptr(), bounce_virt.as_ptr(), layout.size())
            };
        }

        BOUNCE_MAPPINGS.lock().push(BounceMapping {
            orig_virt: addr.as_ptr() as usize,
            bounce_virt: bounce_virt.as_ptr() as usize,
            size: layout.size(),
            direction: _direction,
        });

        Ok(unsafe {
            DmaMapHandle::new(addr, DmaAddr::from(bounce_phys), layout, Some(bounce_virt))
        })
    }

    unsafe fn unmap_single(&self, handle: DmaMapHandle) {
        if let Some(alloc_virt) = handle.alloc_virt() {
            let mapping = {
                let mut mappings = BOUNCE_MAPPINGS.lock();
                mappings
                    .iter()
                    .position(|entry| entry.bounce_virt == alloc_virt.as_ptr() as usize)
                    .map(|idx| mappings.swap_remove(idx))
            };

            if let Some(mapping) = mapping
                && matches!(
                    mapping.direction,
                    DmaDirection::FromDevice | DmaDirection::Bidirectional
                )
            {
                core::ptr::copy_nonoverlapping(
                    mapping.bounce_virt as *const u8,
                    mapping.orig_virt as *mut u8,
                    mapping.size,
                );
            }

            crate::dma::dealloc(alloc_virt.as_ptr(), handle.size());
        }
    }

    unsafe fn alloc_coherent(&self, dma_mask: u64, layout: Layout) -> Option<DmaHandle> {
        let max_phys_exclusive = if dma_mask == u64::MAX {
            None
        } else {
            dma_mask.checked_add(1)
        };
        let (phys, virt) =
            crate::dma::alloc_with_max(layout.size(), layout.align(), max_phys_exclusive)?;
        let virt = NonNull::new(virt)?;
        Some(unsafe { DmaHandle::new(virt, DmaAddr::from(phys), layout) })
    }

    unsafe fn dealloc_coherent(&self, handle: DmaHandle) {
        crate::dma::dealloc(handle.as_ptr().as_ptr(), handle.size());
    }
}

impl KernelOp for TrueosCrabUsbKernel {
    fn delay(&self, duration: Duration) {
        let millis = duration.as_millis();
        if millis == 0 {
            return;
        }
        let timeout_ms = millis.min(u128::from(u64::MAX)) as u64;
        let _ = crate::wait::spin_until_timeout(timeout_ms, || false);
    }
}

#[derive(Copy, Clone)]
struct PreferredAlt {
    configuration_index: u8,
    configuration_value: u8,
    interface_number: u8,
    alternate_setting: u8,
    class: u8,
    subclass: u8,
    protocol: u8,
    has_iso_out: bool,
    endpoint_count: usize,
}

#[derive(Copy, Clone)]
struct IsoOutEndpoint {
    address: u8,
    max_packet_size: u16,
    interval: u8,
}

#[derive(Copy, Clone)]
struct UacFeatureUnitTarget {
    ac_interface: u8,
    unit_id: u8,
    source_id: u8,
    supports_mute: bool,
    supports_volume: bool,
}

#[derive(Copy, Clone)]
struct UacAudioControlTarget {
    ac_interface: u8,
    uac2: bool,
    clock_source_id: Option<u8>,
    playback_stream_link: Option<u8>,
    feature_unit: Option<UacFeatureUnitTarget>,
}

#[derive(Copy, Clone)]
struct UacStreamFormat {
    channels: u8,
    subframe_bytes: u8,
    bit_resolution: u8,
    supports_48k: bool,
}

#[derive(Copy, Clone)]
struct UacStreamTarget {
    sync_type: u8,
    has_feedback_ep: bool,
    max_packet_payload: u16,
    format: Option<UacStreamFormat>,
}

#[derive(Copy, Clone)]
struct UacStreamCandidate {
    interface_number: u8,
    alternate_setting: u8,
    endpoint_address: u8,
    sync_type: u8,
    has_feedback_ep: bool,
    max_packet_payload: u16,
    terminal_link: Option<u8>,
    format: Option<UacStreamFormat>,
}

fn uac_sync_type_name(sync_type: u8) -> &'static str {
    match sync_type {
        0 => "no-sync",
        1 => "async",
        2 => "adaptive",
        3 => "synchronous",
        _ => "unknown",
    }
}

#[derive(Copy, Clone)]
struct TruekeyTarget {
    interface_number: u8,
    alternate_setting: u8,
    out_endpoint: u8,
    out_max_packet_size: u16,
}

fn tlb_transfer_type_name(transfer_type: usb_if::descriptor::EndpointType) -> &'static str {
    match transfer_type {
        usb_if::descriptor::EndpointType::Control => "ctrl",
        usb_if::descriptor::EndpointType::Isochronous => "iso",
        usb_if::descriptor::EndpointType::Bulk => "bulk",
        usb_if::descriptor::EndpointType::Interrupt => "intr",
    }
}

fn tlb_speed_name(speed: usb_if::Speed) -> &'static str {
    match speed {
        usb_if::Speed::Low => "low",
        usb_if::Speed::Full => "full",
        usb_if::Speed::High => "high",
        usb_if::Speed::Wireless => "wireless",
        usb_if::Speed::SuperSpeed => "super",
        usb_if::Speed::SuperSpeedPlus => "super+",
    }
}

fn topology_path_string(topology: &crab_usb::device::DeviceTopology) -> alloc::string::String {
    if topology.path.is_empty() {
        return alloc::format!("rp{}", topology.root_port_id);
    }

    let mut out = alloc::format!("rp{}", topology.root_port_id);
    for hop in topology.path.iter() {
        out.push_str(&alloc::format!("->hub{}:p{}", hop.slot_id, hop.port_id));
    }
    out
}

fn topology_location_string(
    location: &crab_usb::topology::DeviceLocation,
) -> alloc::string::String {
    let mut out = alloc::format!("rp{}", location.root_port);
    for port in location.path.iter().skip(1) {
        out.push_str(&alloc::format!("->p{}", port));
    }
    out
}

fn push_tlb_topology_node(
    out: &mut Vec<super::TlbUsbTopologyNode>,
    node: super::TlbUsbTopologyNode,
) {
    let exists = out.iter().any(|known| {
        known.controller_index == node.controller_index
            && known.kind == node.kind
            && known.slot_id == node.slot_id
            && known.root_port_id == node.root_port_id
            && known.port_id == node.port_id
            && known.parent_slot_id == node.parent_slot_id
    });
    if !exists {
        out.push(node);
    }
}

fn snapshot_tlb_topology(
    controller_id: usize,
    topology: &crab_usb::topology::DeviceTree,
) -> Vec<super::TlbUsbTopologyNode> {
    let mut out = Vec::new();

    for node in topology.iter() {
        push_tlb_topology_node(
            &mut out,
            super::TlbUsbTopologyNode {
                controller_index: controller_id,
                kind: super::TlbUsbTopologyNodeKind::RootPort,
                slot_id: None,
                root_port_id: node.location.root_port,
                port_id: node.location.root_port,
                depth: 0,
                parent_slot_id: None,
                vendor_id: None,
                product_id: None,
                class: None,
                subclass: None,
                protocol: None,
                speed: "unknown",
            },
        );

        push_tlb_topology_node(
            &mut out,
            super::TlbUsbTopologyNode {
                controller_index: controller_id,
                kind: if node.is_hub {
                    super::TlbUsbTopologyNodeKind::Hub
                } else {
                    super::TlbUsbTopologyNodeKind::Device
                },
                slot_id: Some(node.id.raw()),
                root_port_id: node.location.root_port,
                port_id: node.port,
                depth: node.location.path.len().saturating_sub(1) as u8,
                parent_slot_id: node.parent.map(|id| id.raw()),
                vendor_id: Some(node.descriptor.vendor_id),
                product_id: Some(node.descriptor.product_id),
                class: Some(node.descriptor.class),
                subclass: Some(node.descriptor.subclass),
                protocol: Some(node.descriptor.protocol),
                speed: "unknown",
            },
        );
    }

    out.sort_by_key(|node| {
        (
            node.controller_index,
            node.root_port_id,
            node.depth,
            node.parent_slot_id.unwrap_or(0),
            node.slot_id.unwrap_or(0),
            node.port_id,
        )
    });

    out
}

fn snapshot_tlb_device(controller_id: usize, dev: &crab_usb::DeviceInfo) -> super::TlbUsbDevice {
    let desc = dev.descriptor();
    let topology = dev.topology();
    let location = dev.location();
    let mut configurations = Vec::new();
    for cfg in dev.configurations().iter() {
        let mut interfaces = Vec::new();
        for iface_group in cfg.interfaces.iter() {
            for alt in iface_group.alt_settings.iter() {
                let mut endpoints = Vec::new();
                for ep in alt.endpoints.iter() {
                    endpoints.push(super::TlbUsbEndpoint {
                        address: ep.address,
                        transfer_type: tlb_transfer_type_name(ep.transfer_type),
                        max_packet_size: ep.max_packet_size,
                        interval: ep.interval,
                    });
                }
                interfaces.push(super::TlbUsbInterface {
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    class: alt.class,
                    subclass: alt.subclass,
                    protocol: alt.protocol,
                    endpoints,
                });
            }
        }
        configurations.push(super::TlbUsbConfiguration {
            configuration_value: cfg.configuration_value,
            attributes: cfg.attributes,
            max_power: cfg.max_power,
            interfaces,
        });
    }

    super::TlbUsbDevice {
        controller_index: controller_id,
        stable_id: location.device_id().raw(),
        slot_id: dev.id() as u32,
        root_port_id: topology.root_port_id,
        route_string: location.route_string,
        path: location.path,
        port_id: topology.port_id,
        speed: tlb_speed_name(topology.port_speed),
        parent_hub_slot_id: topology.parent_hub_slot_id.map(u32::from),
        hub_path: topology
            .path
            .iter()
            .map(|hop| super::TlbUsbPathHop {
                slot_id: u32::from(hop.slot_id),
                port_id: hop.port_id,
                hub_depth: hop.hub_depth,
                speed: tlb_speed_name(hop.speed),
            })
            .collect(),
        vendor_id: desc.vendor_id,
        product_id: desc.product_id,
        class: desc.class,
        subclass: desc.subclass,
        protocol: desc.protocol,
        num_configurations: desc.num_configurations,
        max_packet_size_0: desc.max_packet_size_0,
        configurations,
    }
}

fn update_tlb_devices(controller_id: usize, devices: &[crab_usb::DeviceInfo]) {
    let mut cache = TLB_DEVICES[controller_id].lock();
    for dev in devices.iter() {
        let snapshot = snapshot_tlb_device(controller_id, dev);
        if let Some(existing) = cache
            .iter_mut()
            .find(|known| known.slot_id == snapshot.slot_id)
        {
            *existing = snapshot;
        } else {
            cache.push(snapshot);
        }
    }
}

fn update_tlb_topology(controller_id: usize, topology: &crab_usb::topology::DeviceTree) {
    let mut cache = TLB_TOPOLOGY[controller_id].lock();
    *cache = snapshot_tlb_topology(controller_id, topology);
}

async fn refresh_tlb_topology(host: &mut USBHost, controller_id: usize) {
    match host.topology().await {
        Ok(topology) => update_tlb_topology(controller_id, &topology),
        Err(err) => {
            crate::log!(
                "crabusb: topology refresh failed on controller {}: {:?}\n",
                controller_id,
                err
            );
        }
    }
}

async fn handle_detected_device(
    host: &mut USBHost,
    spawner: &Spawner,
    controller_id: usize,
    dev_idx: usize,
    dev: &crab_usb::DeviceInfo,
) {
    let desc = dev.descriptor();
    let topology = dev.topology();
    let location = dev.location();
    crate::log!(
        "crabusb: dev {:04X}:{:04X} slot={} stable=0x{:08X} topo={} route=0x{:06X} port={} speed={}\n",
        desc.vendor_id,
        desc.product_id,
        dev.id(),
        dev.stable_id().raw(),
        topology_location_string(&location),
        location.route_string,
        topology.port_id,
        tlb_speed_name(topology.port_speed),
    );

    if desc.vendor_id == TRUEKEY_VENDOR_ID && desc.product_id == TRUEKEY_PRODUCT_ID {
        maybe_start_truekey_bridge(host, dev).await;
        return;
    }
    let mut handled_any = false;

    if super::hid::leds::maybe_start_led_controller(host, dev, spawner, controller_id as u32).await
    {
        handled_any = true;
    }
    if super::hid::mediacontrol::maybe_start_media_control(host, dev, spawner, controller_id as u32)
        .await
    {
        handled_any = true;
    }
    if super::hid::boot::maybe_start_hid_boot_streams(host, dev, spawner, controller_id as u32)
        .await
    {
        handled_any = true;
    }
    if descriptor_has_audio_candidate(dev) {
        sound::maybe_start_target_audio(host, dev, spawner).await;
        handled_any = true;
    }
    if super::midi::maybe_start_midi(host, dev, spawner, controller_id as u32).await {
        handled_any = true;
    }
    if super::pen::maybe_start_mass_storage(host, dev, spawner, controller_id as u32).await {
        handled_any = true;
    }
    if !handled_any {
        log_opened_device_graph(host, dev_idx, dev).await;
    }
}

fn descriptor_has_audio_candidate(dev_info: &crab_usb::DeviceInfo) -> bool {
    dev_info.interface_descriptors().any(|iface| {
        iface.class == 0x01
            || iface.endpoints.iter().any(|ep| {
                ep.transfer_type == usb_if::descriptor::EndpointType::Isochronous
                    && ep.direction == usb_if::transfer::Direction::Out
            })
    })
}

async fn with_timeout_or_none<F: Future>(fut: F, timeout_ms: u64) -> Option<F::Output> {
    let mut fut = core::pin::pin!(fut);
    let mut timeout = core::pin::pin!(Timer::after(EmbassyDuration::from_millis(timeout_ms)));

    core::future::poll_fn(|cx| {
        if let Poll::Ready(out) = fut.as_mut().poll(cx) {
            return Poll::Ready(Some(out));
        }
        if timeout.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

fn parse_wav_pcm_s16_stereo_48k(bytes: &[u8]) -> Option<(usize, usize)> {
    fn le_u16(s: &[u8]) -> Option<u16> {
        if s.len() < 2 {
            return None;
        }
        Some(u16::from_le_bytes([s[0], s[1]]))
    }

    fn le_u32(s: &[u8]) -> Option<u32> {
        if s.len() < 4 {
            return None;
        }
        Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }

    let mut off = 12usize;
    let mut fmt_ok = false;
    let mut data: Option<(usize, usize)> = None;
    while off + 8 <= bytes.len() {
        let id = &bytes[off..off + 4];
        let sz = le_u32(&bytes[off + 4..off + 8])? as usize;
        let payload = off + 8;
        let end = payload.saturating_add(sz);
        if end > bytes.len() {
            return None;
        }

        if id == b"fmt " {
            if sz < 16 {
                return None;
            }
            let fmt = &bytes[payload..payload + sz];
            let audio_fmt = le_u16(&fmt[0..2])?;
            let channels = le_u16(&fmt[2..4])?;
            let rate = le_u32(&fmt[4..8])?;
            let bits = le_u16(&fmt[14..16])?;
            if audio_fmt == 1 && channels == 2 && rate == 48_000 && bits == 16 {
                fmt_ok = true;
            } else {
                return None;
            }
        } else if id == b"data" {
            data = Some((payload, sz));
            if fmt_ok {
                break;
            }
        }

        off = end + (sz & 1);
    }

    if !fmt_ok {
        return None;
    }
    data
}

fn pick_preferred_alt(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Option<PreferredAlt> {
    let mut best: Option<PreferredAlt> = None;

    for (config_index, config) in configs.iter().enumerate() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                let has_iso_out = alt.endpoints.iter().any(|ep| {
                    ep.transfer_type == usb_if::descriptor::EndpointType::Isochronous
                        && ep.direction == usb_if::transfer::Direction::Out
                });

                let score = if has_iso_out {
                    100
                } else if alt.class == 0x01 && alt.subclass == 0x02 {
                    50
                } else if alt.class == 0x01 {
                    25
                } else {
                    0
                };

                let candidate = PreferredAlt {
                    configuration_index: config_index as u8,
                    configuration_value: config.configuration_value,
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    class: alt.class,
                    subclass: alt.subclass,
                    protocol: alt.protocol,
                    has_iso_out,
                    endpoint_count: alt.endpoints.len(),
                };

                let replace = match best {
                    None => true,
                    Some(current) => {
                        let current_score = if current.has_iso_out {
                            100
                        } else if current.class == 0x01 && current.subclass == 0x02 {
                            50
                        } else if current.class == 0x01 {
                            25
                        } else {
                            0
                        };

                        score > current_score
                            || (score == current_score
                                && candidate.endpoint_count > current.endpoint_count)
                            || (score == current_score
                                && candidate.endpoint_count == current.endpoint_count
                                && candidate.alternate_setting > current.alternate_setting)
                    }
                };

                if replace {
                    best = Some(candidate);
                }
            }
        }
    }

    best
}

fn le_u16_at(bytes: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*bytes.get(off)?, *bytes.get(off + 1)?]))
}

async fn fetch_raw_configuration_bytes(
    device: &mut crab_usb::Device,
    configuration_index: u8,
) -> Option<Vec<u8>> {
    let mut header = [0u8; 9];
    device
        .ep_ctrl()
        .get_descriptor(
            usb_if::descriptor::DescriptorType::CONFIGURATION,
            configuration_index,
            0,
            &mut header,
        )
        .await
        .ok()?;

    let total_length = usize::from(le_u16_at(&header, 2)?);
    if total_length < header.len() {
        return None;
    }

    let mut full = Vec::from_iter(core::iter::repeat_n(0u8, total_length));
    device
        .ep_ctrl()
        .get_descriptor(
            usb_if::descriptor::DescriptorType::CONFIGURATION,
            configuration_index,
            0,
            full.as_mut_slice(),
        )
        .await
        .ok()?;
    Some(full)
}

fn parse_uac_stream_target(
    raw_cfg: &[u8],
    interface_number: u8,
    alternate_setting: u8,
    endpoint_address: u8,
) -> Option<UacStreamTarget> {
    let mut idx = 0usize;
    let mut current_if: Option<(u8, u8, u8, u8)> = None;
    let mut pending_out: Option<(u8, u16, u8)> = None;
    let mut has_feedback_ep = false;

    while idx + 2 <= raw_cfg.len() {
        let len = usize::from(raw_cfg[idx]);
        if len < 2 || idx + len > raw_cfg.len() {
            break;
        }

        match raw_cfg[idx + 1] {
            0x04 if len >= 9 => {
                current_if =
                    Some((raw_cfg[idx + 2], raw_cfg[idx + 3], raw_cfg[idx + 5], raw_cfg[idx + 6]));
                pending_out = None;
                has_feedback_ep = false;
            }
            0x05 if len >= 7 => {
                let ep_addr = raw_cfg[idx + 2];
                let bm_attr = raw_cfg[idx + 3];
                let xfer_type = bm_attr & 0x3;
                let sync_type = (bm_attr >> 2) & 0x3;
                let usage_type = (bm_attr >> 4) & 0x3;
                let max_packet = le_u16_at(raw_cfg, idx + 4).unwrap_or(0) & 0x07FF;
                let dir_in = (ep_addr & 0x80) != 0;

                if !dir_in && xfer_type == 0x01 {
                    pending_out = Some((ep_addr, max_packet, sync_type));
                }
                if dir_in && xfer_type == 0x01 && usage_type == 0x01 {
                    has_feedback_ep = true;
                }
            }
            _ => {}
        }

        if let (Some((ifnum, alt, class, subclass)), Some((ep_addr, max_packet, sync_type))) =
            (current_if, pending_out)
            && ifnum == interface_number
            && alt == alternate_setting
            && class == 0x01
            && subclass == 0x02
            && ep_addr == endpoint_address
        {
            return Some(UacStreamTarget {
                sync_type,
                has_feedback_ep,
                max_packet_payload: max_packet,
                format: None,
            });
        }

        idx += len;
    }

    None
}

fn parse_uac_stream_candidates(raw_cfg: &[u8]) -> Vec<UacStreamCandidate> {
    const CS_INTERFACE: u8 = 0x24;
    const UAC_AS_GENERAL: u8 = 0x01;
    const UAC_AS_FORMAT_TYPE: u8 = 0x02;

    let mut out = Vec::new();
    let mut idx = 0usize;
    let mut current_if: Option<(u8, u8, u8, u8)> = None;
    let mut current_terminal_link: Option<u8> = None;
    let mut current_format: Option<UacStreamFormat> = None;
    let mut pending_out: Option<(u8, u16, u8)> = None;
    let mut has_feedback_ep = false;

    let mut flush_current = |out: &mut Vec<UacStreamCandidate>,
                             current_if: Option<(u8, u8, u8, u8)>,
                             terminal_link: Option<u8>,
                             format: Option<UacStreamFormat>,
                             pending_out: Option<(u8, u16, u8)>,
                             has_feedback_ep: bool| {
        if let (Some((ifnum, alt, class, subclass)), Some((ep_addr, max_packet, sync_type))) =
            (current_if, pending_out)
            && class == 0x01
            && subclass == 0x02
        {
            out.push(UacStreamCandidate {
                interface_number: ifnum,
                alternate_setting: alt,
                endpoint_address: ep_addr,
                sync_type,
                has_feedback_ep,
                max_packet_payload: max_packet,
                terminal_link,
                format,
            });
        }
    };

    while idx + 2 <= raw_cfg.len() {
        let len = usize::from(raw_cfg[idx]);
        if len < 2 || idx + len > raw_cfg.len() {
            break;
        }

        match raw_cfg[idx + 1] {
            0x04 if len >= 9 => {
                flush_current(
                    &mut out,
                    current_if,
                    current_terminal_link,
                    current_format,
                    pending_out,
                    has_feedback_ep,
                );
                current_if =
                    Some((raw_cfg[idx + 2], raw_cfg[idx + 3], raw_cfg[idx + 5], raw_cfg[idx + 6]));
                current_terminal_link = None;
                current_format = None;
                pending_out = None;
                has_feedback_ep = false;
            }
            CS_INTERFACE if current_if.is_some() && len >= 4 => match raw_cfg[idx + 2] {
                UAC_AS_GENERAL => {
                    current_terminal_link = Some(raw_cfg[idx + 3]);
                }
                UAC_AS_FORMAT_TYPE if len >= 8 && raw_cfg[idx + 3] == 0x01 => {
                    let channels = raw_cfg[idx + 4];
                    let subframe_bytes = raw_cfg[idx + 5];
                    let bit_resolution = raw_cfg[idx + 6];
                    let freq_type = usize::from(raw_cfg[idx + 7]);
                    let supports_48k = if freq_type == 0 {
                        if len >= 14 {
                            let min_rate = u32::from(raw_cfg[idx + 8])
                                | (u32::from(raw_cfg[idx + 9]) << 8)
                                | (u32::from(raw_cfg[idx + 10]) << 16);
                            let max_rate = u32::from(raw_cfg[idx + 11])
                                | (u32::from(raw_cfg[idx + 12]) << 8)
                                | (u32::from(raw_cfg[idx + 13]) << 16);
                            (min_rate..=max_rate).contains(&48_000)
                        } else {
                            false
                        }
                    } else {
                        let mut found_48k = false;
                        for sample_idx in 0..freq_type {
                            let off = idx + 8 + sample_idx * 3;
                            if off + 3 > idx + len {
                                break;
                            }
                            let rate = u32::from(raw_cfg[off])
                                | (u32::from(raw_cfg[off + 1]) << 8)
                                | (u32::from(raw_cfg[off + 2]) << 16);
                            if rate == 48_000 {
                                found_48k = true;
                                break;
                            }
                        }
                        found_48k
                    };

                    current_format = Some(UacStreamFormat {
                        channels,
                        subframe_bytes,
                        bit_resolution,
                        supports_48k,
                    });
                }
                _ => {}
            },
            0x05 if len >= 7 => {
                let ep_addr = raw_cfg[idx + 2];
                let bm_attr = raw_cfg[idx + 3];
                let xfer_type = bm_attr & 0x3;
                let sync_type = (bm_attr >> 2) & 0x3;
                let usage_type = (bm_attr >> 4) & 0x3;
                let max_packet = le_u16_at(raw_cfg, idx + 4).unwrap_or(0) & 0x07FF;
                let dir_in = (ep_addr & 0x80) != 0;

                if !dir_in && xfer_type == 0x01 {
                    pending_out = Some((ep_addr, max_packet, sync_type));
                }
                if dir_in && xfer_type == 0x01 && usage_type == 0x01 {
                    has_feedback_ep = true;
                }
            }
            _ => {}
        }

        idx += len;
    }

    flush_current(
        &mut out,
        current_if,
        current_terminal_link,
        current_format,
        pending_out,
        has_feedback_ep,
    );
    out
}

fn log_uac_topology(raw_cfg: &[u8], vendor_id: u16, product_id: u16) {
    if !crate::logflag::USB_AUDIO_DEBUG_LOGS {
        return;
    }

    const CS_INTERFACE: u8 = 0x24;
    const UAC_AC_INPUT_TERMINAL: u8 = 0x02;
    const UAC_AC_OUTPUT_TERMINAL: u8 = 0x03;
    const UAC_AC_FEATURE_UNIT: u8 = 0x06;
    const UAC_AS_GENERAL: u8 = 0x01;
    const UAC_AS_FORMAT_TYPE: u8 = 0x02;

    let mut idx = 0usize;
    let mut current_if: Option<(u8, u8, u8, u8)> = None;

    while idx + 2 <= raw_cfg.len() {
        let len = usize::from(raw_cfg[idx]);
        if len < 2 || idx + len > raw_cfg.len() {
            break;
        }

        match raw_cfg[idx + 1] {
            0x04 if len >= 9 => {
                current_if =
                    Some((raw_cfg[idx + 2], raw_cfg[idx + 3], raw_cfg[idx + 5], raw_cfg[idx + 6]));
            }
            CS_INTERFACE if len >= 3 => match raw_cfg[idx + 2] {
                UAC_AC_INPUT_TERMINAL if len >= 8 => crate::log!(
                    "crabusb: audio-topology {:04X}:{:04X} input-term id={} type=0x{:04X} assoc={} if={}/{}\n",
                    vendor_id,
                    product_id,
                    raw_cfg[idx + 3],
                    le_u16_at(raw_cfg, idx + 4).unwrap_or(0),
                    raw_cfg[idx + 6],
                    current_if.map(|v| v.0).unwrap_or(0),
                    current_if.map(|v| v.1).unwrap_or(0)
                ),
                UAC_AC_OUTPUT_TERMINAL if len >= 9 => crate::log!(
                    "crabusb: audio-topology {:04X}:{:04X} output-term id={} type=0x{:04X} source={} assoc={} if={}/{}\n",
                    vendor_id,
                    product_id,
                    raw_cfg[idx + 3],
                    le_u16_at(raw_cfg, idx + 4).unwrap_or(0),
                    raw_cfg[idx + 7],
                    raw_cfg[idx + 8],
                    current_if.map(|v| v.0).unwrap_or(0),
                    current_if.map(|v| v.1).unwrap_or(0)
                ),
                UAC_AC_FEATURE_UNIT if len >= 6 => crate::log!(
                    "crabusb: audio-topology {:04X}:{:04X} feature-unit id={} source={} if={}/{}\n",
                    vendor_id,
                    product_id,
                    raw_cfg[idx + 3],
                    raw_cfg[idx + 4],
                    current_if.map(|v| v.0).unwrap_or(0),
                    current_if.map(|v| v.1).unwrap_or(0)
                ),
                UAC_AS_GENERAL if len >= 4 => {
                    if let Some((ifnum, alt, class, subclass)) = current_if
                        && class == 0x01
                        && subclass == 0x02
                    {
                        crate::log!(
                            "crabusb: audio-topology {:04X}:{:04X} as-general if#{} alt={} terminal_link={}\n",
                            vendor_id,
                            product_id,
                            ifnum,
                            alt,
                            raw_cfg[idx + 3]
                        );
                    }
                }
                UAC_AS_FORMAT_TYPE if len >= 8 => {
                    if let Some((ifnum, alt, class, subclass)) = current_if
                        && class == 0x01
                        && subclass == 0x02
                        && raw_cfg[idx + 3] == 0x01
                    {
                        crate::log!(
                            "crabusb: audio-topology {:04X}:{:04X} as-format if#{} alt={} channels={} subframe={} bits={} freq_type={}\n",
                            vendor_id,
                            product_id,
                            ifnum,
                            alt,
                            raw_cfg[idx + 4],
                            raw_cfg[idx + 5],
                            raw_cfg[idx + 6],
                            raw_cfg[idx + 7]
                        );
                    }
                }
                _ => {}
            },
            _ => {}
        }

        idx += len;
    }
}

fn parse_uac_audio_controls(raw_cfg: &[u8]) -> Option<UacAudioControlTarget> {
    const USB_CLASS_AUDIO: u8 = 0x01;
    const USB_SUBCLASS_AUDIOCONTROL: u8 = 0x01;
    const CS_INTERFACE: u8 = 0x24;
    const UAC_AC_HEADER: u8 = 0x01;
    const UAC_AC_OUTPUT_TERMINAL: u8 = 0x03;
    const UAC_AC_FEATURE_UNIT: u8 = 0x06;
    const UAC2_CLOCK_SOURCE: u8 = 0x0A;

    let mut idx = 0usize;
    let mut current_ac_if_number: Option<u8> = None; // Track AC interface number
    let mut current_ac_is_uac2 = false;
    let mut playback_output_source_id: Option<u8> = None;
    let mut first_fu: Option<UacFeatureUnitTarget> = None;
    let mut feature_units: Vec<UacFeatureUnitTarget> = Vec::new();
    let mut clock_source_id: Option<u8> = None;
    let mut discovered: Option<UacAudioControlTarget> = None;

    while idx + 2 <= raw_cfg.len() {
        let len = usize::from(raw_cfg[idx]);
        if len < 2 || idx + len > raw_cfg.len() {
            break;
        }

        match raw_cfg[idx + 1] {
            0x04 if len >= 9 => {
                let interface_number = raw_cfg[idx + 2];
                let alternate_setting = raw_cfg[idx + 3];
                let class = raw_cfg[idx + 5];
                let subclass = raw_cfg[idx + 6];

                let is_ac_interface =
                    class == USB_CLASS_AUDIO && subclass == USB_SUBCLASS_AUDIOCONTROL;

                if let Some(prev_if_num) = current_ac_if_number {
                    if interface_number != prev_if_num {
                        let playback_fu = playback_output_source_id.and_then(|output_source_id| {
                            feature_units
                                .iter()
                                .copied()
                                .find(|feature| feature.unit_id == output_source_id)
                        });

                        discovered = Some(UacAudioControlTarget {
                            ac_interface: prev_if_num,
                            uac2: current_ac_is_uac2,
                            clock_source_id,
                            playback_stream_link: playback_fu.map(|feature| feature.source_id),
                            feature_unit: playback_fu.or(first_fu),
                        });

                        current_ac_if_number = None;
                        current_ac_is_uac2 = false;
                        playback_output_source_id = None;
                        first_fu = None;
                        feature_units.clear();
                        clock_source_id = None;
                    }
                }

                if is_ac_interface && alternate_setting == 0 && current_ac_if_number.is_none() {
                    current_ac_if_number = Some(interface_number);
                }
            }
            CS_INTERFACE if current_ac_if_number.is_some() && len >= 3 => {
                let subtype = raw_cfg[idx + 2];
                match subtype {
                    UAC_AC_HEADER if len >= 5 => {
                        current_ac_is_uac2 = le_u16_at(raw_cfg, idx + 3).unwrap_or(0) >= 0x0200;
                    }
                    UAC_AC_OUTPUT_TERMINAL if len >= 9 => {
                        let terminal_id = raw_cfg[idx + 3];
                        let terminal_type = le_u16_at(raw_cfg, idx + 4).unwrap_or(0);
                        let source_id = raw_cfg[idx + 7];
                        // Prefer the last non-USB-streaming output terminal as the playback sink.
                        if !matches!(terminal_type, 0x0101 | 0x0102 | 0x0201) {
                            let _ = terminal_id;
                            playback_output_source_id = Some(source_id);
                        }
                    }
                    UAC_AC_FEATURE_UNIT if len >= 7 => {
                        let unit_id = raw_cfg[idx + 3];
                        let source_id = raw_cfg[idx + 4];
                        let master_controls = if current_ac_is_uac2 {
                            if len < 10 {
                                idx += len;
                                continue;
                            }
                            u32::from(raw_cfg[idx + 5])
                                | (u32::from(raw_cfg[idx + 6]) << 8)
                                | (u32::from(raw_cfg[idx + 7]) << 16)
                                | (u32::from(raw_cfg[idx + 8]) << 24)
                        } else {
                            let control_size = usize::from(raw_cfg[idx + 5]);
                            let controls_off = idx + 6;
                            let controls_end = controls_off.saturating_add(control_size);
                            if controls_end > idx + len {
                                idx += len;
                                continue;
                            }
                            let mut controls = 0u32;
                            for (shift, b) in raw_cfg[controls_off..controls_end].iter().enumerate()
                            {
                                controls |= u32::from(*b) << (shift * 8);
                            }
                            controls
                        };

                        let candidate = UacFeatureUnitTarget {
                            ac_interface: current_ac_if_number.unwrap_or(0),
                            unit_id,
                            source_id,
                            supports_mute: (master_controls & 0x01) != 0,
                            supports_volume: (master_controls & 0x02) != 0,
                        };

                        if first_fu.is_none() {
                            first_fu = Some(candidate);
                        }
                        feature_units.push(candidate);
                    }
                    UAC2_CLOCK_SOURCE if current_ac_is_uac2 && len >= 4 => {
                        clock_source_id = Some(raw_cfg[idx + 3]);
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        idx += len;
    }

    if let Some(ac_if) = current_ac_if_number {
        let playback_fu = playback_output_source_id.and_then(|output_source_id| {
            feature_units
                .iter()
                .copied()
                .find(|feature| feature.unit_id == output_source_id)
        });

        discovered = Some(UacAudioControlTarget {
            ac_interface: ac_if,
            uac2: current_ac_is_uac2,
            clock_source_id,
            playback_stream_link: playback_fu.map(|feature| feature.source_id),
            feature_unit: playback_fu.or(first_fu),
        });
    }

    discovered
}

async fn configure_uac_playback_controls(
    device: &mut crab_usb::Device,
    vendor_id: u16,
    product_id: u16,
    raw_cfg: &[u8],
) {
    async fn try_set_uac_fu_mute(
        device: &mut crab_usb::Device,
        vendor_id: u16,
        product_id: u16,
        feature: UacFeatureUnitTarget,
        channel: u8,
        muted: bool,
    ) {
        const UAC_CONTROL_TIMEOUT_MS: u64 = 200;
        let value = 0x0100u16 | u16::from(channel);
        let payload = [if muted { 1 } else { 0 }];
        match with_timeout_or_none(
            device.control_out(
                usb_if::host::ControlSetup {
                    request_type: usb_if::transfer::RequestType::Class,
                    recipient: usb_if::transfer::Recipient::Interface,
                    request: usb_if::transfer::Request::Other(0x01),
                    value,
                    index: (u16::from(feature.unit_id) << 8) | u16::from(feature.ac_interface),
                },
                &payload,
            ),
            UAC_CONTROL_TIMEOUT_MS,
        )
        .await
        {
            Some(Ok(_)) => crate::log!(
                "crabusb: target {:04X}:{:04X} audio mute ok fu={} ac_if={} ch={} muted={}\n",
                vendor_id,
                product_id,
                feature.unit_id,
                feature.ac_interface,
                channel,
                muted
            ),
            Some(Err(err)) => crate::log!(
                "crabusb: target {:04X}:{:04X} audio mute failed fu={} ac_if={} ch={} muted={} err={:?}\n",
                vendor_id,
                product_id,
                feature.unit_id,
                feature.ac_interface,
                channel,
                muted,
                err
            ),
            None => crate::log!(
                "crabusb: target {:04X}:{:04X} audio mute timeout fu={} ac_if={} ch={} muted={}\n",
                vendor_id,
                product_id,
                feature.unit_id,
                feature.ac_interface,
                channel,
                muted
            ),
        }
    }

    async fn try_get_uac_fu_mute(
        device: &mut crab_usb::Device,
        vendor_id: u16,
        product_id: u16,
        feature: UacFeatureUnitTarget,
        channel: u8,
    ) {
        const UAC_CONTROL_TIMEOUT_MS: u64 = 200;
        let value = 0x0100u16 | u16::from(channel);
        let mut payload = [0u8; 1];
        match with_timeout_or_none(
            device.control_in(
                usb_if::host::ControlSetup {
                    request_type: usb_if::transfer::RequestType::Class,
                    recipient: usb_if::transfer::Recipient::Interface,
                    request: usb_if::transfer::Request::Other(0x81),
                    value,
                    index: (u16::from(feature.unit_id) << 8) | u16::from(feature.ac_interface),
                },
                &mut payload,
            ),
            UAC_CONTROL_TIMEOUT_MS,
        )
        .await
        {
            Some(Ok(read)) => crate::log!(
                "crabusb: target {:04X}:{:04X} audio mute-read fu={} ac_if={} ch={} bytes={} muted={}\n",
                vendor_id,
                product_id,
                feature.unit_id,
                feature.ac_interface,
                channel,
                read,
                payload[0] != 0
            ),
            Some(Err(err)) => crate::log!(
                "crabusb: target {:04X}:{:04X} audio mute-read failed fu={} ac_if={} ch={} err={:?}\n",
                vendor_id,
                product_id,
                feature.unit_id,
                feature.ac_interface,
                channel,
                err
            ),
            None => crate::log!(
                "crabusb: target {:04X}:{:04X} audio mute-read timeout fu={} ac_if={} ch={}\n",
                vendor_id,
                product_id,
                feature.unit_id,
                feature.ac_interface,
                channel
            ),
        }
    }

    async fn try_get_uac_fu_volume(
        device: &mut crab_usb::Device,
        vendor_id: u16,
        product_id: u16,
        feature: UacFeatureUnitTarget,
        channel: u8,
        request: u8,
        label: &'static str,
    ) -> Option<i16> {
        const UAC_CONTROL_TIMEOUT_MS: u64 = 200;
        let value = 0x0200u16 | u16::from(channel);
        let mut payload = [0u8; 2];
        match with_timeout_or_none(
            device.control_in(
                usb_if::host::ControlSetup {
                    request_type: usb_if::transfer::RequestType::Class,
                    recipient: usb_if::transfer::Recipient::Interface,
                    request: usb_if::transfer::Request::Other(request),
                    value,
                    index: (u16::from(feature.unit_id) << 8) | u16::from(feature.ac_interface),
                },
                &mut payload,
            ),
            UAC_CONTROL_TIMEOUT_MS,
        )
        .await
        {
            Some(Ok(read)) if read >= 2 => {
                let value_db256 = i16::from_le_bytes(payload);
                if crate::logflag::USB_AUDIO_DEBUG_LOGS {
                    crate::log!(
                        "crabusb: target {:04X}:{:04X} audio volume-{} fu={} ac_if={} ch={} value_db256={}\n",
                        vendor_id,
                        product_id,
                        label,
                        feature.unit_id,
                        feature.ac_interface,
                        channel,
                        value_db256
                    );
                }
                Some(value_db256)
            }
            Some(Ok(read)) => {
                if crate::logflag::USB_AUDIO_DEBUG_LOGS {
                    crate::log!(
                        "crabusb: target {:04X}:{:04X} audio volume-{} short fu={} ac_if={} ch={} bytes={}\n",
                        vendor_id,
                        product_id,
                        label,
                        feature.unit_id,
                        feature.ac_interface,
                        channel,
                        read
                    );
                }
                None
            }
            Some(Err(err)) => {
                if crate::logflag::USB_AUDIO_DEBUG_LOGS {
                    crate::log!(
                        "crabusb: target {:04X}:{:04X} audio volume-{} failed fu={} ac_if={} ch={} err={:?}\n",
                        vendor_id,
                        product_id,
                        label,
                        feature.unit_id,
                        feature.ac_interface,
                        channel,
                        err
                    );
                }
                None
            }
            None => {
                if crate::logflag::USB_AUDIO_DEBUG_LOGS {
                    crate::log!(
                        "crabusb: target {:04X}:{:04X} audio volume-{} timeout fu={} ac_if={} ch={}\n",
                        vendor_id,
                        product_id,
                        label,
                        feature.unit_id,
                        feature.ac_interface,
                        channel
                    );
                }
                None
            }
        }
    }

    let Some(controls) = parse_uac_audio_controls(raw_cfg) else {
        crate::log!(
            "crabusb: target {:04X}:{:04X} no audio control entities found\n",
            vendor_id,
            product_id
        );
        return;
    };
    async fn try_set_uac_fu_volume(
        device: &mut crab_usb::Device,
        vendor_id: u16,
        product_id: u16,
        feature: UacFeatureUnitTarget,
        channel: u8,
        value_db256: i16,
    ) {
        const UAC_CONTROL_TIMEOUT_MS: u64 = 200;
        let value = 0x0200u16 | u16::from(channel);
        match with_timeout_or_none(
            device.control_out(
                usb_if::host::ControlSetup {
                    request_type: usb_if::transfer::RequestType::Class,
                    recipient: usb_if::transfer::Recipient::Interface,
                    request: usb_if::transfer::Request::Other(0x01),
                    value,
                    index: (u16::from(feature.unit_id) << 8) | u16::from(feature.ac_interface),
                },
                &value_db256.to_le_bytes(),
            ),
            UAC_CONTROL_TIMEOUT_MS,
        )
        .await
        {
            Some(Ok(_)) => {
                if crate::logflag::USB_AUDIO_DEBUG_LOGS {
                    crate::log!(
                        "crabusb: target {:04X}:{:04X} audio volume ok fu={} ac_if={} ch={} value_db256={}\n",
                        vendor_id,
                        product_id,
                        feature.unit_id,
                        feature.ac_interface,
                        channel,
                        value_db256
                    );
                }
            }
            Some(Err(err)) => crate::log!(
                "crabusb: target {:04X}:{:04X} audio volume failed fu={} ac_if={} ch={} value_db256={} err={:?}\n",
                vendor_id,
                product_id,
                feature.unit_id,
                feature.ac_interface,
                channel,
                value_db256,
                err
            ),
            None => crate::log!(
                "crabusb: target {:04X}:{:04X} audio volume timeout fu={} ac_if={} ch={} value_db256={}\n",
                vendor_id,
                product_id,
                feature.unit_id,
                feature.ac_interface,
                channel,
                value_db256
            ),
        }
    }

    if crate::logflag::USB_AUDIO_DEBUG_LOGS {
        crate::log!(
            "crabusb: target {:04X}:{:04X} audio control ac_if={} uac2={} clock={} feature={}\n",
            vendor_id,
            product_id,
            controls.ac_interface,
            controls.uac2,
            controls.clock_source_id.unwrap_or(0),
            controls.feature_unit.map(|f| f.unit_id).unwrap_or(0)
        );
    }

    if controls.uac2
        && let Some(clock_id) = controls.clock_source_id
    {
        match device
            .control_out(
                usb_if::host::ControlSetup {
                    request_type: usb_if::transfer::RequestType::Class,
                    recipient: usb_if::transfer::Recipient::Interface,
                    request: usb_if::transfer::Request::Other(0x01),
                    value: 0x0100,
                    index: (u16::from(clock_id) << 8) | u16::from(controls.ac_interface),
                },
                &48_000u32.to_le_bytes(),
            )
            .await
        {
            Ok(_) => crate::log!(
                "crabusb: target {:04X}:{:04X} audio clock-rate ok clock={} ac_if={} hz=48000\n",
                vendor_id,
                product_id,
                clock_id,
                controls.ac_interface
            ),
            Err(err) => crate::log!(
                "crabusb: target {:04X}:{:04X} audio clock-rate failed clock={} ac_if={} hz=48000 err={:?}\n",
                vendor_id,
                product_id,
                clock_id,
                controls.ac_interface,
                err
            ),
        }
    }

    let Some(feature) = controls.feature_unit else {
        crate::log!(
            "crabusb: target {:04X}:{:04X} no playback feature unit found\n",
            vendor_id,
            product_id
        );
        return;
    };

    if crate::logflag::USB_AUDIO_DEBUG_LOGS {
        crate::log!(
            "crabusb: target {:04X}:{:04X} audio feature unit id={} ac_if={} mute={} volume={}\n",
            vendor_id,
            product_id,
            feature.unit_id,
            feature.ac_interface,
            feature.supports_mute,
            feature.supports_volume
        );
    }

    if feature.supports_mute {
        try_set_uac_fu_mute(device, vendor_id, product_id, feature, 0, false).await;
        if crate::logflag::USB_AUDIO_DEBUG_LOGS {
            try_get_uac_fu_mute(device, vendor_id, product_id, feature, 0).await;
        }
    }

    if feature.supports_volume {
        try_set_uac_fu_volume(device, vendor_id, product_id, feature, 0, 0).await;
    }

    for channel in [0u8, 1, 2] {
        let current =
            try_get_uac_fu_volume(device, vendor_id, product_id, feature, channel, 0x81, "cur")
                .await;
        let min =
            try_get_uac_fu_volume(device, vendor_id, product_id, feature, channel, 0x82, "min")
                .await;
        let max =
            try_get_uac_fu_volume(device, vendor_id, product_id, feature, channel, 0x83, "max")
                .await;
        let _ = try_get_uac_fu_volume(device, vendor_id, product_id, feature, channel, 0x84, "res")
            .await;

        if let (Some(current), Some(min), Some(max)) = (current, min, max) {
            let target = if (min..=max).contains(&0) { 0 } else { max };
            if crate::logflag::USB_AUDIO_DEBUG_LOGS {
                crate::log!(
                    "crabusb: target {:04X}:{:04X} audio volume-plan fu={} ac_if={} ch={} current={} min={} max={} target={}\n",
                    vendor_id,
                    product_id,
                    feature.unit_id,
                    feature.ac_interface,
                    channel,
                    current,
                    min,
                    max,
                    target
                );
            }
            if target != current {
                try_set_uac_fu_volume(device, vendor_id, product_id, feature, channel, target)
                    .await;
                if crate::logflag::USB_AUDIO_DEBUG_LOGS {
                    let _ = try_get_uac_fu_volume(
                        device, vendor_id, product_id, feature, channel, 0x81, "cur",
                    )
                    .await;
                }
            }
        }
    }
}

fn find_iso_out_endpoint(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
    interface_number: u8,
    alternate_setting: u8,
) -> Option<IsoOutEndpoint> {
    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            if interface.interface_number != interface_number {
                continue;
            }
            for alt in interface.alt_settings.iter() {
                if alt.alternate_setting != alternate_setting {
                    continue;
                }
                for ep in alt.endpoints.iter() {
                    if ep.transfer_type == usb_if::descriptor::EndpointType::Isochronous
                        && ep.direction == usb_if::transfer::Direction::Out
                    {
                        return Some(IsoOutEndpoint {
                            address: ep.address,
                            max_packet_size: ep.max_packet_size,
                            interval: ep.interval,
                        });
                    }
                }
            }
        }
    }
    None
}

fn fill_audio_packet(out: &mut [u8], wav: &[u8], wav_cursor: &mut usize) {
    let mut copied = 0usize;
    while copied + AUDIO_FRAME_BYTES <= out.len() {
        if *wav_cursor + AUDIO_FRAME_BYTES > wav.len() {
            *wav_cursor = 0;
        }
        out[copied..copied + AUDIO_FRAME_BYTES]
            .copy_from_slice(&wav[*wav_cursor..*wav_cursor + AUDIO_FRAME_BYTES]);
        *wav_cursor += AUDIO_FRAME_BYTES;
        copied += AUDIO_FRAME_BYTES;
    }
    out[copied..].fill(0);
}

fn audio_packet_level_probe(packet: &[u8]) -> (i16, u32) {
    let mut peak = 0i16;
    let mut sum_abs = 0u32;
    let mut samples = 0u32;

    for smp in packet.chunks_exact(2) {
        let v = i16::from_le_bytes([smp[0], smp[1]]);
        let a = v.unsigned_abs() as u32;
        if a > peak as u32 {
            peak = i16::try_from(a).unwrap_or(i16::MAX);
        }
        sum_abs = sum_abs.saturating_add(a);
        samples = samples.saturating_add(1);
    }

    let mean_abs = if samples == 0 { 0 } else { sum_abs / samples };
    (peak, mean_abs)
}

fn choose_audio_packet_bytes(frame_bytes: usize, endpoint_payload_limit: usize) -> usize {
    let frame_bytes = frame_bytes.max(1);
    let payload_limit = endpoint_payload_limit.max(frame_bytes);

    // For 48 kHz PCM, nominal payload is either per 1ms frame (FS) or per 125us microframe (HS/SS).
    let nominal_1ms = (48_000usize * frame_bytes) / 1_000;
    let nominal_125us = (48_000usize * frame_bytes) / 8_000;

    let candidate = if payload_limit >= nominal_1ms {
        nominal_1ms
    } else if payload_limit >= nominal_125us {
        nominal_125us
    } else {
        payload_limit
    };

    let aligned = (candidate / frame_bytes) * frame_bytes;
    aligned.max(frame_bytes).min(payload_limit)
}

fn pick_truekey_target(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Option<TruekeyTarget> {
    let mut best: Option<(u32, TruekeyTarget)> = None;

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                let Some(out_ep) = alt.endpoints.iter().find(|ep| {
                    ep.direction == usb_if::transfer::Direction::Out
                        && ep.transfer_type == usb_if::descriptor::EndpointType::Bulk
                }) else {
                    continue;
                };

                let mut score = 10u32;
                if alt.class == 0x0A {
                    score += 100;
                } else if alt.class == 0x02 {
                    score += 70;
                } else if alt.class == 0xFF {
                    score += 40;
                }
                score += alt.endpoints.len() as u32;
                score += u32::from(alt.alternate_setting);

                let target = TruekeyTarget {
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    out_endpoint: out_ep.address,
                    out_max_packet_size: out_ep.max_packet_size,
                };

                match best {
                    Some((best_score, _)) if best_score >= score => {}
                    _ => best = Some((score, target)),
                }
            }
        }
    }

    best.map(|(_, target)| target)
}

async fn stream_truekey_logs(
    device: &mut crab_usb::Device,
    vendor_id: u16,
    product_id: u16,
    target: TruekeyTarget,
) {
    let endpoint_kind = match device.get_endpoint(target.out_endpoint).await {
        Ok(kind) => kind,
        Err(err) => {
            crate::log!(
                "crabusb: truekey {:04X}:{:04X} ep=0x{:02X} open failed: {:?}\n",
                vendor_id,
                product_id,
                target.out_endpoint,
                err
            );
            return;
        }
    };

    let crab_usb::EndpointKind::BulkOut(mut bulk_out) = endpoint_kind else {
        crate::log!(
            "crabusb: truekey {:04X}:{:04X} ep=0x{:02X} is not bulk-out\n",
            vendor_id,
            product_id,
            target.out_endpoint
        );
        return;
    };

    TRUEKEY_STREAM_ACTIVE.store(true, Ordering::Release);
    crate::log!(
        "crabusb: truekey streaming start {:04X}:{:04X} if#{} alt={} ep=0x{:02X} mps={}\n",
        vendor_id,
        product_id,
        target.interface_number,
        target.alternate_setting,
        target.out_endpoint,
        target.out_max_packet_size
    );

    let chunk_limit = min(TRUEKEY_STREAM_CHUNK, usize::from(target.out_max_packet_size.max(1)));
    let mut cursor = 0usize;

    loop {
        let snapshot = crate::globalog::snapshot();
        if cursor > snapshot.len() {
            cursor = snapshot.len();
        }

        if cursor == snapshot.len() {
            Timer::after(EmbassyDuration::from_millis(50)).await;
            continue;
        }

        let end = min(snapshot.len(), cursor + chunk_limit);
        match bulk_out.submit_and_wait(&snapshot[cursor..end]).await {
            Ok(sent) if sent > 0 => {
                cursor += sent;
            }
            Ok(_) => {
                Timer::after(EmbassyDuration::from_millis(1)).await;
            }
            Err(err) => {
                crate::log!(
                    "crabusb: truekey streaming stopped {:04X}:{:04X} ep=0x{:02X} err={:?}\n",
                    vendor_id,
                    product_id,
                    target.out_endpoint,
                    err
                );
                break;
            }
        }
    }

    TRUEKEY_STREAM_ACTIVE.store(false, Ordering::Release);
}

async fn truekey_logdrain_task(
    device: &mut crab_usb::Device,
    vendor_id: u16,
    product_id: u16,
    target: TruekeyTarget,
) {
    crate::log!(
        "crabusb: truekey {:04X}:{:04X} handoff -> logdrain if#{} alt={} ep=0x{:02X}\n",
        vendor_id,
        product_id,
        target.interface_number,
        target.alternate_setting,
        target.out_endpoint
    );
    stream_truekey_logs(device, vendor_id, product_id, target).await;
}

async fn maybe_start_truekey_bridge(host: &mut USBHost, dev_info: &crab_usb::DeviceInfo) {
    if !TRUEKEY_STREAM_REQUESTED.load(Ordering::Acquire)
        || TRUEKEY_STREAM_ACTIVE.load(Ordering::Acquire)
    {
        return;
    }

    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    if vendor_id != TRUEKEY_VENDOR_ID || product_id != TRUEKEY_PRODUCT_ID {
        return;
    }

    crate::log!("crabusb: truekey {:04X}:{:04X} candidate found\n", vendor_id, product_id);

    let mut device = match host.open_device(dev_info).await {
        Ok(device) => device,
        Err(err) => {
            crate::log!(
                "crabusb: truekey {:04X}:{:04X} open failed: {:?}\n",
                vendor_id,
                product_id,
                err
            );
            return;
        }
    };

    let configs = device.configurations().to_vec();
    crate::log!(
        "crabusb: truekey {:04X}:{:04X} inspecting {} config(s)\n",
        vendor_id,
        product_id,
        configs.len()
    );
    let Some(target) = pick_truekey_target(&configs) else {
        crate::log!(
            "crabusb: truekey {:04X}:{:04X} no bulk-out sink target found\n",
            vendor_id,
            product_id
        );
        return;
    };

    crate::log!(
        "crabusb: truekey {:04X}:{:04X} selected if#{} alt={} ep=0x{:02X}\n",
        vendor_id,
        product_id,
        target.interface_number,
        target.alternate_setting,
        target.out_endpoint
    );

    crate::log!(
        "crabusb: truekey {:04X}:{:04X} data claim begin if#{} alt={}\n",
        vendor_id,
        product_id,
        target.interface_number,
        target.alternate_setting
    );
    match device
        .claim_interface(target.interface_number, target.alternate_setting)
        .await
    {
        Ok(()) => {
            crate::log!(
                "crabusb: truekey {:04X}:{:04X} ownership if#{} alt={} ep=0x{:02X}\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.alternate_setting,
                target.out_endpoint
            );
            truekey_logdrain_task(&mut device, vendor_id, product_id, target).await;
        }
        Err(err) => crate::log!(
            "crabusb: truekey {:04X}:{:04X} data if#{} alt={} claim failed: {:?}\n",
            vendor_id,
            product_id,
            target.interface_number,
            target.alternate_setting,
            err
        ),
    }
}

async fn log_opened_device_graph(
    host: &mut USBHost,
    dev_idx: usize,
    dev_info: &crab_usb::DeviceInfo,
) {
    let device = match host.open_device(dev_info).await {
        Ok(device) => device,
        Err(err) => {
            let desc = dev_info.descriptor();
            crate::log!(
                "crabusb: open dev#{} {:04X}:{:04X} failed: {:?}\n",
                dev_idx,
                desc.vendor_id,
                desc.product_id,
                err
            );
            return;
        }
    };

    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    crate::log!(
        "crabusb: open dev#{} slot={} vid={:04X} pid={:04X} mfg={}\n",
        dev_idx,
        device.slot_id(),
        vendor_id,
        product_id,
        device.manufacturer().unwrap_or("-")
    );

    let configs = device.configurations().to_vec();
    if let Some(target) = super::mass::pick_mass_target(&configs) {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} cfg={} if#{} alt={} bulk_in=0x{:02X} bulk_out=0x{:02X} class={:02X} subclass={:02X} proto={:02X}\n",
            vendor_id,
            product_id,
            target.configuration_value,
            target.interface_number,
            target.alternate_setting,
            target.bulk_in,
            target.bulk_out,
            target.class,
            target.subclass,
            target.protocol
        );
    }
    if let Some(preferred) = pick_preferred_alt(&configs)
        && (preferred.has_iso_out || preferred.class == 0x01)
    {
        crate::log!(
            "crabusb: audio-candidate {:04X}:{:04X} if#{} alt={} class={:02X} subclass={:02X} proto={:02X} iso_out={}\n",
            vendor_id,
            product_id,
            preferred.interface_number,
            preferred.alternate_setting,
            preferred.class,
            preferred.subclass,
            preferred.protocol,
            preferred.has_iso_out
        );
    }

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                crate::log!(
                    "crabusb: open dev#{} if#{} alt={} desc-only class={:02X} subclass={:02X} proto={:02X}\n",
                    dev_idx,
                    alt.interface_number,
                    alt.alternate_setting,
                    alt.class,
                    alt.subclass,
                    alt.protocol
                );
                for ep in alt.endpoints.iter() {
                    let ep_num = ep.address & 0x0F;
                    crate::log!(
                        "crabusb: open dev#{} if#{} alt={} ep=0x{:02X} num={} desc-only mps={} interval={}\n",
                        dev_idx,
                        alt.interface_number,
                        alt.alternate_setting,
                        ep.address,
                        ep_num,
                        ep.max_packet_size,
                        ep.interval
                    );
                }
            }
        }
    }
}

async fn probe_and_log(host: &mut USBHost, spawner: &Spawner, controller_id: usize) -> bool {
    match crate::wait::select2(
        host.probe_devices(),
        Timer::after(EmbassyDuration::from_millis(CRABUSB_PROBE_TIMEOUT_MS)),
    )
    .await
    {
        crate::wait::Either::First(res) => match res {
            Ok(devices) => {
                mark_controller_validated(controller_id, "probe completed successfully");
                if devices.is_empty() {
                    let cached = cached_device_count(controller_id);
                    if cached > 0 {
                        LAST_PROBE_STATE[controller_id].store(5, Ordering::Release);
                        LAST_PROBE_DEVICE_COUNT[controller_id]
                            .store(cached as u32, Ordering::Release);
                        PROBE_FAIL_STREAK[controller_id].store(0, Ordering::Release);
                        EMPTY_PROBE_STREAK[controller_id].store(0, Ordering::Release);
                        NO_PORT_CHANGE_HINT_LOGGED[controller_id].store(false, Ordering::Release);
                        advance_root_hub_lifecycle(
                            controller_id,
                            ROOT_HUB_LIFECYCLE_STEADY,
                            "empty probe after first device contact",
                        );
                        return false;
                    }

                    LAST_PROBE_STATE[controller_id].store(2, Ordering::Release);
                    LAST_PROBE_DEVICE_COUNT[controller_id].store(0, Ordering::Release);
                    PROBE_FAIL_STREAK[controller_id].store(0, Ordering::Release);
                    let streak =
                        EMPTY_PROBE_STREAK[controller_id].fetch_add(1, Ordering::AcqRel) + 1;
                    if streak.is_multiple_of(25) {
                        crate::log!(
                            "crabusb: controller {} empty probe streak={} root_port_change_seen={} event_ready={}\n",
                            controller_id,
                            streak,
                            ROOT_PORT_CHANGE_SEEN[controller_id].load(Ordering::Acquire),
                            EVENT_HANDLER_READY[controller_id].load(Ordering::Acquire),
                        );
                    }
                    if !ROOT_PORT_CHANGE_SEEN[controller_id].load(Ordering::Acquire)
                        && NO_PORT_CHANGE_HINT_LOGGED[controller_id]
                            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                            .is_ok()
                    {
                        crate::log!(
                            "crabusb: no root-port change events observed; controller may be empty or downstream devices are not handed to the guest\n"
                        );
                    }

                    if ROOT_PORT_CHANGE_SEEN[controller_id].load(Ordering::Acquire)
                        && streak.is_multiple_of(100)
                    {
                        crate::log!(
                            "crabusb: controller {} forcing host rebind after persistent empty probes (streak={})\n",
                            controller_id,
                            streak
                        );
                        uninstall_event_handler(controller_id);
                    }
                    if ROOT_PORT_CHANGE_SEEN[controller_id].load(Ordering::Acquire) {
                        advance_root_hub_lifecycle(
                            controller_id,
                            ROOT_HUB_LIFECYCLE_SETTLING,
                            "empty probe while waiting for first device contact",
                        );
                    }
                    false
                } else {
                    LAST_PROBE_STATE[controller_id].store(1, Ordering::Release);
                    LAST_PROBE_DEVICE_COUNT[controller_id]
                        .store(devices.len() as u32, Ordering::Release);
                    PROBE_FAIL_STREAK[controller_id].store(0, Ordering::Release);
                    EMPTY_PROBE_STREAK[controller_id].store(0, Ordering::Release);
                    NO_PORT_CHANGE_HINT_LOGGED[controller_id].store(false, Ordering::Release);
                    if cached_device_count(controller_id) == 0 {
                        advance_root_hub_lifecycle(
                            controller_id,
                            ROOT_HUB_LIFECYCLE_FIRST_CONTACT,
                            "probe discovered first devices",
                        );
                    } else {
                        advance_root_hub_lifecycle(
                            controller_id,
                            ROOT_HUB_LIFECYCLE_STEADY,
                            "probe rediscovered devices",
                        );
                    }
                    mark_controller_validated(controller_id, "probe discovered devices");
                    update_tlb_devices(controller_id, &devices);
                    refresh_tlb_topology(host, controller_id).await;
                    crate::log!("crabusb: discovered {} new device(s)\n", devices.len());
                    for dev in devices.iter() {
                        let desc = dev.descriptor();
                        crate::log!(
                            "crabusb: dev {:04X}:{:04X} class={:02X} subclass={:02X} proto={:02X}\n",
                            desc.vendor_id,
                            desc.product_id,
                            desc.class,
                            desc.subclass,
                            desc.protocol
                        );
                    }
                    for (dev_idx, dev) in devices.iter().enumerate() {
                        handle_detected_device(host, spawner, controller_id, dev_idx, dev).await;
                    }
                    true
                }
            }
            Err(err) => {
                LAST_PROBE_STATE[controller_id].store(3, Ordering::Release);
                LAST_PROBE_DEVICE_COUNT[controller_id].store(0, Ordering::Release);
                log_probe_progress("probe failed progress");
                log_xhci_runtime_snapshot(controller_id, "probe failed");
                if is_pre_command_root_hub_wait() {
                    PROBE_FAIL_STREAK[controller_id].store(0, Ordering::Release);
                    crate::log!(
                        "crabusb: controller {} root-hub settle still in progress; ignoring pre-command probe error: {:?}\n",
                        controller_id,
                        err
                    );
                    return false;
                }
                let fail_streak =
                    PROBE_FAIL_STREAK[controller_id].fetch_add(1, Ordering::AcqRel) + 1;
                crate::log!(
                    "crabusb: controller {} probe failed: {:?} (streak={})\n",
                    controller_id,
                    err,
                    fail_streak
                );
                if let Some(reason) = xhci_fatal_probe_state(controller_id) {
                    crate::log!(
                        "crabusb: controller {} fatal xhci state during probe failure ({}); rebinding immediately\n",
                        controller_id,
                        reason
                    );
                    PROBE_FAIL_STREAK[controller_id].store(0, Ordering::Release);
                    uninstall_event_handler(controller_id);
                    return false;
                }
                if fail_streak >= 2 {
                    crate::log!(
                        "crabusb: controller {} forcing host rebind after repeated probe failures\n",
                        controller_id
                    );
                    PROBE_FAIL_STREAK[controller_id].store(0, Ordering::Release);
                    uninstall_event_handler(controller_id);
                }
                false
            }
        },
        crate::wait::Either::Second(_) => {
            LAST_PROBE_STATE[controller_id].store(4, Ordering::Release);
            LAST_PROBE_DEVICE_COUNT[controller_id].store(0, Ordering::Release);
            log_probe_progress("probe timeout progress");
            log_xhci_runtime_snapshot(controller_id, "probe timeout");
            if is_pre_command_root_hub_wait() {
                PROBE_FAIL_STREAK[controller_id].store(0, Ordering::Release);
                crate::log!(
                    "crabusb: controller {} root-hub settle still in progress; ignoring pre-command probe timeout after {}ms\n",
                    controller_id,
                    CRABUSB_PROBE_TIMEOUT_MS
                );
                return false;
            }
            let fail_streak = PROBE_FAIL_STREAK[controller_id].fetch_add(1, Ordering::AcqRel) + 1;
            crate::log!(
                "crabusb: controller {} probe timeout after {}ms (streak={})\n",
                controller_id,
                CRABUSB_PROBE_TIMEOUT_MS,
                fail_streak
            );
            if let Some(reason) = xhci_fatal_probe_state(controller_id) {
                crate::log!(
                    "crabusb: controller {} fatal xhci state during probe timeout ({}); rebinding immediately\n",
                    controller_id,
                    reason
                );
                PROBE_FAIL_STREAK[controller_id].store(0, Ordering::Release);
                uninstall_event_handler(controller_id);
                return false;
            }
            if fail_streak >= 2 {
                crate::log!(
                    "crabusb: controller {} forcing host rebind after repeated probe timeouts\n",
                    controller_id
                );
                PROBE_FAIL_STREAK[controller_id].store(0, Ordering::Release);
                uninstall_event_handler(controller_id);
            }
            false
        }
    }
}

async fn crab_scout_once(host: &mut USBHost, info: super::TlbUsbController, spawner: &Spawner) {
    if INITIAL_SNAPSHOT_LOGGED[info.index]
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    crate::log!("crabusb: scout begin (timeout={}ms)\n", CRABUSB_PROBE_TIMEOUT_MS);
    match crate::wait::select2(
        host.probe_devices(),
        Timer::after(EmbassyDuration::from_millis(CRABUSB_PROBE_TIMEOUT_MS)),
    )
    .await
    {
        crate::wait::Either::First(res) => match res {
            Ok(devices) => {
                LAST_PROBE_DEVICE_COUNT[info.index].store(devices.len() as u32, Ordering::Release);
                mark_controller_validated(info.index, "scout probe completed");
                if devices.is_empty() {
                    LAST_PROBE_STATE[info.index].store(2, Ordering::Release);
                    advance_root_hub_lifecycle(
                        info.index,
                        ROOT_HUB_LIFECYCLE_SETTLING,
                        "scout found no devices yet",
                    );
                } else {
                    LAST_PROBE_STATE[info.index].store(1, Ordering::Release);
                    advance_root_hub_lifecycle(
                        info.index,
                        ROOT_HUB_LIFECYCLE_FIRST_CONTACT,
                        "scout discovered first devices",
                    );
                }
                update_tlb_devices(info.index, &devices);
                refresh_tlb_topology(host, info.index).await;
                crate::log!("crabusb: scout devices={}\n", devices.len());
                for (dev_idx, dev) in devices.iter().enumerate() {
                    let desc = dev.descriptor();
                    let topology = dev.topology();
                    let location = dev.location();
                    crate::log!(
                        "crabusb: scout dev#{} vid={:04X} pid={:04X} class={:02X} subclass={:02X} proto={:02X} cfgs={} stable=0x{:08X} topo={} route=0x{:06X} port={} speed={}\n",
                        dev_idx,
                        desc.vendor_id,
                        desc.product_id,
                        desc.class,
                        desc.subclass,
                        desc.protocol,
                        dev.configurations().len(),
                        dev.stable_id().raw(),
                        topology_location_string(&location),
                        location.route_string,
                        topology.port_id,
                        tlb_speed_name(topology.port_speed),
                    );
                    for iface in dev.interface_descriptors() {
                        crate::log!(
                            "crabusb: scout dev#{} if#{} alt={} class={:02X} subclass={:02X} proto={:02X} eps={}\n",
                            dev_idx,
                            iface.interface_number,
                            iface.alternate_setting,
                            iface.class,
                            iface.subclass,
                            iface.protocol,
                            iface.endpoints.len()
                        );
                    }
                }
                for (dev_idx, dev) in devices.iter().enumerate() {
                    handle_detected_device(host, spawner, info.index, dev_idx, dev).await;
                }
            }
            Err(err) => {
                LAST_PROBE_STATE[info.index].store(3, Ordering::Release);
                LAST_PROBE_DEVICE_COUNT[info.index].store(0, Ordering::Release);
                crate::log!("crabusb: scout probe failed: {:?}\n", err);
                if let Some(reason) = xhci_fatal_probe_state(info.index) {
                    crate::log!(
                        "crabusb: controller {} fatal xhci state during scout failure ({}); rebinding immediately\n",
                        info.index,
                        reason
                    );
                    uninstall_event_handler(info.index);
                }
            }
        },
        crate::wait::Either::Second(_) => {
            LAST_PROBE_STATE[info.index].store(4, Ordering::Release);
            LAST_PROBE_DEVICE_COUNT[info.index].store(0, Ordering::Release);
            log_probe_progress("scout timeout progress");
            log_xhci_runtime_snapshot(info.index, "scout timeout");
            crate::log!("crabusb: scout probe timeout after {}ms\n", CRABUSB_PROBE_TIMEOUT_MS);
            if let Some(reason) = xhci_fatal_probe_state(info.index) {
                crate::log!(
                    "crabusb: controller {} fatal xhci state during scout timeout ({}); rebinding immediately\n",
                    info.index,
                    reason
                );
                uninstall_event_handler(info.index);
            }
        }
    }
    crate::log!("crabusb: scout end\n");
}

fn install_event_handler(controller_id: usize, handler: EventHandler) {
    set_controller_phase(controller_id, ControllerPhase::Startup, "event handler installed");
    reset_root_hub_lifecycle(controller_id, ROOT_HUB_LIFECYCLE_BOUND, "event handler installed");
    *EVENT_HANDLER[controller_id].lock() = Some(handler);
    EVENT_HANDLER_READY[controller_id].store(true, Ordering::Release);
}

fn uninstall_event_handler(controller_id: usize) {
    EVENT_HANDLER_READY[controller_id].store(false, Ordering::Release);
    reset_root_hub_lifecycle(controller_id, ROOT_HUB_LIFECYCLE_INIT, "event handler removed");
    *EVENT_HANDLER[controller_id].lock() = None;
}

async fn wait_for_manual_rebind(controller_id: usize) {
    crate::log!(
        "crabusb: controller {} quarantined; auto-rebind disabled until manual rebind request\n",
        controller_id
    );
    loop {
        if controller_can_rebind(controller_id) {
            crate::log!(
                "crabusb: controller {} leaving quarantine after manual rebind request\n",
                controller_id
            );
            return;
        }
        Timer::after(EmbassyDuration::from_secs(1)).await;
    }
}

#[embassy_executor::task(pool_size = MAX_XHCI_CONTROLLERS)]
pub async fn event_pump_task(controller_id: usize) {
    loop {
        if !EVENT_HANDLER_READY[controller_id].load(Ordering::Acquire) {
            Timer::after(EmbassyDuration::from_millis(10)).await;
            continue;
        }

        let event = {
            let guard = EVENT_HANDLER[controller_id].lock();
            guard.as_ref().map(|handler| handler.handle_event())
        };

        match event {
            Some(Event::Nothing) | None => {
                Timer::after(EmbassyDuration::from_millis(1)).await;
            }
            Some(Event::PortChange { port }) => {
                let first_change =
                    !ROOT_PORT_CHANGE_SEEN[controller_id].swap(true, Ordering::AcqRel);
                let queued_probe =
                    !PROBE_REQUESTED[controller_id].swap(true, Ordering::AcqRel);
                if first_change || queued_probe {
                    crate::log!(
                        "crabusb: pump port change on controller {} root port {} first_change={} queued_probe={}\n",
                        controller_id,
                        port,
                        first_change,
                        queued_probe
                    );
                }
                NO_PORT_CHANGE_HINT_LOGGED[controller_id].store(false, Ordering::Release);
                advance_root_hub_lifecycle(
                    controller_id,
                    ROOT_HUB_LIFECYCLE_ROOT_CHANGE,
                    "root-port change observed",
                );
                Timer::after(EmbassyDuration::from_millis(1)).await;
            }
            Some(Event::Stopped) => {
                crate::log!("crabusb: pump observed stopped event\n");
                log_xhci_status_bits(controller_id, "pump stopped");
                crate::log!(
                    "crabusb: controller {} stopped event phase={} lifecycle={} bucket={}\n",
                    controller_id,
                    controller_phase_name(controller_phase(controller_id)),
                    root_hub_lifecycle_summary(controller_id),
                    root_hub_lifecycle_bucket_for_controller(controller_id)
                );
                if controller_can_rebind(controller_id) {
                    mark_controller_recoverable(controller_id, "pump observed stopped event");
                    uninstall_event_handler(controller_id);
                    Timer::after(EmbassyDuration::from_millis(10)).await;
                } else {
                    mark_controller_quarantined(
                        controller_id,
                        "pump observed stopped before validation",
                    );
                    crate::log!(
                        "crabusb: controller {} stopped before validation; quarantining instead of rebinding\n",
                        controller_id
                    );
                    Timer::after(EmbassyDuration::from_millis(CRABUSB_QUICK_STOP_BACKOFF_MS)).await;
                }
            }
        }
    }
}

#[embassy_executor::task(pool_size = MAX_XHCI_CONTROLLERS)]
pub async fn bsp_service(controller_index: usize, spawner: Spawner) {
    const OFFLINE_RETRY_MS: u64 = 1000;

    TRUEKEY_STREAM_REQUESTED.store(true, Ordering::Release);
    let mut quick_stop_streak = 0u32;

    loop {
        if controller_needs_quarantine(controller_index)
            && matches!(controller_phase(controller_index), ControllerPhase::Quarantined)
        {
            wait_for_manual_rebind(controller_index).await;
        }

        let Some(info) = super::controller_by_index(controller_index) else {
            if crate::logflag::USB_VERBOSE {
                crate::log!(
                    "crabusb: controller {} not available yet; retrying\n",
                    controller_index
                );
            }
            Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
            continue;
        };

        crate::log!(
            "crabusb: BSP service binding controller {} at {:02X}:{:02X}.{} vid={:04X} pid={:04X} mmio={:p}\n",
            info.index,
            info.bus,
            info.slot,
            info.function,
            info.vendor_id,
            info.device_id,
            info.mmio_base
        );

        if info.vendor_id == 0x8086 {
            let flr = crate::pci::try_function_level_reset(info.bus, info.slot, info.function);
            crate::log!("crabusb: controller {} intel pre-init flr={}\n", info.index, flr);
        }

        crate::pci::enable_mem_and_bus_master(info.bus, info.slot, info.function);

        let mmio = info.mmio_base;

        let mut host = match USBHost::new_xhci(mmio, &CRABUSB_KERNEL) {
            Ok(host) => host,
            Err(err) => {
                crate::log!(
                    "crabusb: failed to create host for controller {}: {:?}\n",
                    info.index,
                    err
                );
                Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
                continue;
            }
        };

        if let Err(err) = host.init().await {
            crate::log!("crabusb: host init failed for controller {}: {:?}\n", info.index, err);
            log_xhci_status_bits(info.index, "host init failed");
            if let Some(reason) = xhci_fatal_init_state(info.index) {
                mark_controller_quarantined(info.index, "fatal xhci state during host init");
                crate::log!(
                    "crabusb: controller {} fatal xhci state during host init ({}); auto-rebind disabled\n",
                    info.index,
                    reason
                );
                wait_for_manual_rebind(info.index).await;
                continue;
            }
            Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
            continue;
        }

        set_controller_phase(info.index, ControllerPhase::FirstContact, "host init completed");
        crate::log!("crabusb: host init ok controller {} awaiting root-hub events\n", info.index);

        ROOT_PORT_CHANGE_SEEN[info.index].store(false, Ordering::Release);
        NO_PORT_CHANGE_HINT_LOGGED[info.index].store(false, Ordering::Release);
        EMPTY_PROBE_STREAK[info.index].store(0, Ordering::Release);
        LAST_PROBE_STATE[info.index].store(0, Ordering::Release);
        LAST_PROBE_DEVICE_COUNT[info.index].store(0, Ordering::Release);
        PROBE_FAIL_STREAK[info.index].store(0, Ordering::Release);
        TLB_DEVICES[info.index].lock().clear();
        TLB_TOPOLOGY[info.index].lock().clear();
        advance_root_hub_lifecycle(
            info.index,
            ROOT_HUB_LIFECYCLE_SETTLING,
            "initial settle window started",
        );

        if info.vendor_id == 0x8086 && CRABUSB_INTEL_PORT_POWER_HOLDOFF_EXPERIMENT {
            let changed = xhci_set_all_root_port_power(info.index, false).unwrap_or(0);
            crate::log!(
                "crabusb: controller {} intel port-power holdoff active changed_ports={}\n",
                info.index,
                changed
            );
        }

        if info.vendor_id == 0x8086 && intel_quiescent_experiment_enabled(info.device_id) {
            let masked = xhci_set_interrupter_enable(info.index, false);
            crate::log!(
                "crabusb: controller {} intel quiescent experiment begin masked={} duration={}ms\n",
                info.index,
                masked,
                CRABUSB_INTEL_QUIESCENT_EXPERIMENT_MS
            );

            let quiet_started_at = Instant::now();
            let mut quiet_failed = false;
            while quiet_started_at.elapsed()
                < EmbassyDuration::from_millis(CRABUSB_INTEL_QUIESCENT_EXPERIMENT_MS)
            {
                if let Some(status) = xhci_status_bits(info.index) {
                    if status.host_system_error || status.host_controller_error || status.hc_halted
                    {
                        log_xhci_status_bits(info.index, "intel quiescent failed");
                        crate::log!(
                            "crabusb: controller {} intel quiescent experiment failed after {}ms\n",
                            info.index,
                            quiet_started_at.elapsed().as_millis()
                        );
                        mark_controller_quarantined(
                            info.index,
                            "intel quiescent experiment observed controller stop",
                        );
                        quiet_failed = true;
                        break;
                    }
                }
                Timer::after(EmbassyDuration::from_millis(
                    CRABUSB_INTEL_QUIESCENT_POLL_MS,
                ))
                .await;
            }

            if quiet_failed {
                wait_for_manual_rebind(info.index).await;
                continue;
            }

            crate::log!(
                "crabusb: controller {} intel quiescent experiment survived {}ms\n",
                info.index,
                quiet_started_at.elapsed().as_millis()
            );
            let _ = xhci_set_interrupter_enable(info.index, true);
        } else if info.vendor_id == 0x8086 && CRABUSB_INTEL_QUIESCENT_EXPERIMENT {
            crate::log!(
                "crabusb: controller {} intel quiescent experiment skipped for device {:04X}\n",
                info.index,
                info.device_id
            );
        }

        let skip_event_handler = intel_skip_event_handler_experiment(info.vendor_id);
        if skip_event_handler {
            crate::log!(
                "crabusb: controller {} intel skip-event-handler experiment active\n",
                info.index
            );
            reset_root_hub_lifecycle(
                info.index,
                ROOT_HUB_LIFECYCLE_BOUND,
                "event handler intentionally skipped",
            );
        } else {
            install_event_handler(info.index, host.create_event_handler());
        }
        let bind_started_at = Instant::now();
        let initial_settle_ms = controller_initial_settle_ms(info.vendor_id);
        let probe_quiet_ms = controller_probe_quiet_ms(info.vendor_id);
        Timer::after(EmbassyDuration::from_millis(initial_settle_ms)).await;
        if info.vendor_id != 0x8086 {
            crab_scout_once(&mut host, info, &spawner).await;
        } else {
            PROBE_REQUESTED[info.index].store(true, Ordering::Release);
            crate::log!(
                "crabusb: controller {} intel deferred scout; waiting for quiet root-port settle {}ms\n",
                info.index,
                probe_quiet_ms
            );
        }

        let mut idle_ticks = 0u32;
        let mut probe_quiet_until: Option<Instant> = None;
        loop {
            if skip_event_handler {
                if let Some(reason) = xhci_fatal_init_state(info.index) {
                    log_xhci_status_bits(info.index, "manual poll stopped");
                    mark_controller_quarantined(
                        info.index,
                        "manual poll observed stopped without event handler",
                    );
                    crate::log!(
                        "crabusb: controller {} manual poll observed fatal xhci state ({}); stopping without event handler\n",
                        info.index,
                        reason
                    );
                    break;
                }

                if let Some(port) = xhci_any_connected_root_port(info.index) {
                    let first_change =
                        !ROOT_PORT_CHANGE_SEEN[info.index].swap(true, Ordering::AcqRel);
                    let queued_probe =
                        !PROBE_REQUESTED[info.index].swap(true, Ordering::AcqRel);
                    if first_change || queued_probe {
                        crate::log!(
                            "crabusb: manual root-port poll on controller {} root port {} first_change={} queued_probe={}\n",
                            info.index,
                            port,
                            first_change,
                            queued_probe
                        );
                    }
                    advance_root_hub_lifecycle(
                        info.index,
                        ROOT_HUB_LIFECYCLE_ROOT_CHANGE,
                        "manual root-port poll observed connection",
                    );
                }
            }

            if !EVENT_HANDLER_READY[info.index].load(Ordering::Acquire) {
                if skip_event_handler {
                    Timer::after(EmbassyDuration::from_millis(50)).await;
                } else {
                    let quick_stop = bind_started_at.elapsed()
                        < EmbassyDuration::from_millis(CRABUSB_QUICK_STOP_WINDOW_MS);
                    if quick_stop {
                        quick_stop_streak = quick_stop_streak.saturating_add(1);
                    } else {
                        quick_stop_streak = 0;
                    }
                    crate::log!(
                        "crabusb: event handler stopped; rediscovering controller (quick_stop={} streak={}) phase={} lifecycle={} bucket={}\n",
                        quick_stop,
                        quick_stop_streak,
                        controller_phase_name(controller_phase(info.index)),
                        root_hub_lifecycle_summary(info.index),
                        root_hub_lifecycle_bucket_for_controller(info.index)
                    );
                    if quick_stop_streak > CRABUSB_MAX_QUICK_STOP_REBINDS {
                        crate::log!(
                            "crabusb: controller {} hit repeated immediate-stop loop; backing off {}ms before rebind\n",
                            info.index,
                            CRABUSB_QUICK_STOP_BACKOFF_MS
                        );
                        Timer::after(EmbassyDuration::from_millis(CRABUSB_QUICK_STOP_BACKOFF_MS))
                            .await;
                        quick_stop_streak = 0;
                    }
                    break;
                }
            }

            if PROBE_REQUESTED[info.index].swap(false, Ordering::AcqRel) {
                advance_root_hub_lifecycle(
                    info.index,
                    ROOT_HUB_LIFECYCLE_SETTLING,
                    "probe requested after root-port change",
                );
                probe_quiet_until =
                    Some(Instant::now() + EmbassyDuration::from_millis(probe_quiet_ms));
                idle_ticks = 0;
            }

            if let Some(deadline) = probe_quiet_until {
                if Instant::now() >= deadline {
                    if info.vendor_id == 0x8086
                        && CRABUSB_INTEL_SKIP_PROBE_EXPERIMENT
                        && !matches!(controller_phase(info.index), ControllerPhase::Validated)
                    {
                        crate::log!(
                            "crabusb: controller {} intel skip-probe experiment rearming quiet window {}ms\n",
                            info.index,
                            CRABUSB_INTEL_SKIP_PROBE_REARM_MS
                        );
                        probe_quiet_until = Some(
                            Instant::now()
                                + EmbassyDuration::from_millis(CRABUSB_INTEL_SKIP_PROBE_REARM_MS),
                        );
                        continue;
                    }
                    crate::log!("crabusb: servicing settled probe on controller {}\n", info.index);
                    probe_quiet_until = None;
                    let _ = probe_and_log(&mut host, &spawner, info.index).await;
                    continue;
                } else {
                    Timer::after(EmbassyDuration::from_millis(10)).await;
                    continue;
                }
            }

            idle_ticks = idle_ticks.wrapping_add(1);
            if idle_ticks >= 300 {
                idle_ticks = 0;
                if cached_device_count(info.index) == 0 {
                    let _ = probe_and_log(&mut host, &spawner, info.index).await;
                }
            }
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }
        uninstall_event_handler(info.index);
        Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
    }
}

#[embassy_executor::task]
pub async fn audio_task() {
    crate::log!("crabusb: audio service armed\n");
    loop {
        Timer::after(EmbassyDuration::from_secs(5)).await;
        if crate::logflag::USB_AUDIO_DEBUG_LOGS && AUDIO_STREAM_ACTIVE.load(Ordering::Acquire) {
            crate::log!("crabusb: audio service streaming\n");
        }
    }
}

#[embassy_executor::task]
pub async fn truekey_task() {
    TRUEKEY_STREAM_REQUESTED.store(true, Ordering::Release);
    crate::log!("crabusb: truekey service armed\n");
    loop {
        Timer::after(EmbassyDuration::from_secs(5)).await;
        if TRUEKEY_STREAM_ACTIVE.load(Ordering::Acquire) {
            crate::log!("crabusb: truekey service streaming\n");
        }
    }
}

pub(super) fn diag_counters(controller_id: usize) -> Option<(bool, bool, u32)> {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return None;
    }

    Some((
        EVENT_HANDLER_READY[controller_id].load(Ordering::Acquire),
        ROOT_PORT_CHANGE_SEEN[controller_id].load(Ordering::Acquire),
        EMPTY_PROBE_STREAK[controller_id].load(Ordering::Acquire),
    ))
}

pub(super) fn diag_devices(controller_id: usize) -> Vec<super::TlbUsbDevice> {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return Vec::new();
    }

    TLB_DEVICES[controller_id].lock().clone()
}

pub(super) fn diag_topology(controller_id: usize) -> Vec<super::TlbUsbTopologyNode> {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return Vec::new();
    }

    TLB_TOPOLOGY[controller_id].lock().clone()
}

pub(super) fn diag_probe_error(controller_id: usize) -> Option<&'static str> {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return None;
    }

    match LAST_PROBE_STATE[controller_id].load(Ordering::Acquire) {
        2 => Some("empty"),
        3 => Some("probe_failed"),
        4 => Some("timeout"),
        _ => None,
    }
}

pub(super) fn diag_probe_device_count(controller_id: usize) -> Option<u32> {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return None;
    }

    Some(LAST_PROBE_DEVICE_COUNT[controller_id].load(Ordering::Acquire))
}

pub(super) fn request_probe(controller_id: usize) -> bool {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return false;
    }

    PROBE_REQUESTED[controller_id].store(true, Ordering::Release);
    true
}

pub(super) fn request_rebind(controller_id: usize) -> bool {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return false;
    }

    crate::log!("crabusb: live rebind requested on controller {}\n", controller_id);
    mark_controller_recoverable(controller_id, "live rebind requested");
    uninstall_event_handler(controller_id);
    true
}

pub(super) fn runtime_diag(controller_id: usize) -> Option<UsbRuntimeDiag> {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return None;
    }

    Some(UsbRuntimeDiag {
        event_handler_ready: EVENT_HANDLER_READY[controller_id].load(Ordering::Acquire),
        probe_requested: PROBE_REQUESTED[controller_id].load(Ordering::Acquire),
        root_port_change_seen: ROOT_PORT_CHANGE_SEEN[controller_id].load(Ordering::Acquire),
        controller_phase: controller_phase_name(controller_phase(controller_id)),
        root_hub_lifecycle: root_hub_lifecycle_summary(controller_id),
        empty_probe_streak: EMPTY_PROBE_STREAK[controller_id].load(Ordering::Acquire),
        probe_fail_streak: PROBE_FAIL_STREAK[controller_id].load(Ordering::Acquire),
        last_probe_state: probe_state_name(LAST_PROBE_STATE[controller_id].load(Ordering::Acquire)),
        last_probe_device_count: LAST_PROBE_DEVICE_COUNT[controller_id].load(Ordering::Acquire),
    })
}

pub(super) fn diag_phase(controller_id: usize) -> Option<&'static str> {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return None;
    }

    Some(controller_phase_name(controller_phase(controller_id)))
}

pub(super) fn diag_root_hub_lifecycle(controller_id: usize) -> Option<&'static str> {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return None;
    }

    Some(root_hub_lifecycle_summary(controller_id))
}
