use alloc::string::String;
use alloc::vec::Vec;
use core::{alloc::Layout, num::NonZeroUsize, ptr::NonNull, time::Duration};

use crab_usb as crabusb;

struct TrueosCrabKernel;

static TRUEOS_CRAB_KERNEL: TrueosCrabKernel = TrueosCrabKernel;

impl crabusb::DmaOp for TrueosCrabKernel {
    fn page_size(&self) -> usize {
        4096
    }

    unsafe fn map_single(
        &self,
        dma_mask: u64,
        addr: NonNull<u8>,
        size: NonZeroUsize,
        align: usize,
        direction: crabusb::DmaDirection,
    ) -> Result<crabusb::DmaMapHandle, crabusb::DmaError> {
        let size = size.get();
        let layout = Layout::from_size_align(size, align.max(1))?;
        let phys = crate::phys::virt_to_phys_checked(addr.as_ptr())
            .ok_or(crabusb::DmaError::NullPointer)?;
        let dma_addr = crabusb::DmaAddr::from(phys);
        let end = phys
            .checked_add(size.saturating_sub(1) as u64)
            .ok_or(crabusb::DmaError::DmaMaskNotMatch {
                addr: dma_addr,
                mask: dma_mask,
            })?;
        if end > dma_mask || (align > 1 && !phys.is_multiple_of(align as u64)) {
            let max_phys = Some(dma_mask.checked_add(1).unwrap_or(u64::MAX));
            let (bounce_phys, bounce_virt) =
                crate::dma::alloc_with_max(size, layout.align(), max_phys)
                    .ok_or(crabusb::DmaError::NoMemory)?;
            let bounce = NonNull::new(bounce_virt).ok_or(crabusb::DmaError::NullPointer)?;

            if matches!(
                direction,
                crabusb::DmaDirection::ToDevice | crabusb::DmaDirection::Bidirectional
            ) {
                unsafe {
                    core::ptr::copy_nonoverlapping(addr.as_ptr(), bounce.as_ptr(), size);
                }
            }

            if crate::logflag::USB_MASS_UAS_TRACE_LOGS {
                crate::log!(
                    "crabusb: dma remap size={} align={} orig_phys=0x{:X} bounce_phys=0x{:X}\n",
                    size,
                    layout.align(),
                    phys,
                    bounce_phys
                );
            }

            return Ok(unsafe {
                crabusb::DmaMapHandle::new(
                    addr,
                    crabusb::DmaAddr::from(bounce_phys),
                    layout,
                    Some(bounce),
                )
            });
        }
        Ok(unsafe { crabusb::DmaMapHandle::new(addr, dma_addr, layout, None) })
    }

    unsafe fn unmap_single(&self, handle: crabusb::DmaMapHandle) {
        if let Some(virt) = handle.alloc_virt() {
            crate::dma::dealloc(virt.as_ptr(), handle.size());
        }
    }

    unsafe fn alloc_coherent(
        &self,
        dma_mask: u64,
        layout: Layout,
    ) -> Option<crabusb::DmaHandle> {
        let max_phys = Some(
            dma_mask
                .checked_add(1)
                .unwrap_or(u64::MAX)
                .min(0x1_0000_0000),
        );
        let (phys, virt) = crate::dma::alloc_with_max(layout.size(), layout.align(), max_phys)?;
        let ptr = NonNull::new(virt)?;
        Some(unsafe { crabusb::DmaHandle::new(ptr, crabusb::DmaAddr::from(phys), layout) })
    }

    unsafe fn dealloc_coherent(&self, handle: crabusb::DmaHandle) {
        crate::dma::dealloc(handle.as_ptr().as_ptr(), handle.size());
    }
}

impl crabusb::KernelOp for TrueosCrabKernel {
    fn delay(&self, duration: Duration) {
        let delay_ms = duration.as_millis().try_into().unwrap_or(u64::MAX);
        let _ = crate::wait::spin_until_timeout_no_exec(delay_ms.max(1), || false);
    }
}

pub fn known_xhci_host_inputs() -> Option<(crabusb::Mmio, &'static dyn crabusb::KernelOp)> {
    let dev = known_xhci_device()?;
    crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);
    let (bar, phys) = first_mmio_bar(&dev)?;
    let size = crate::pci::bar_size_bytes(dev.bus, dev.slot, dev.function, bar)
        .unwrap_or(0x10000)
        .max(0x1000) as usize;
    let mmio = crate::pci::mmio::map_mmio_region_exact(phys, size).ok()?;
    Some((mmio, &TRUEOS_CRAB_KERNEL))
}

fn known_xhci_device() -> Option<crate::pci::PciDevice> {
    crate::pci::with_devices(|devices| {
        devices
            .iter()
            .copied()
            .find(|dev| dev.class == 0x0c && dev.subclass == 0x03 && dev.prog_if == 0x30)
    })
}

fn first_mmio_bar(dev: &crate::pci::PciDevice) -> Option<(u8, u64)> {
    for bar in 0..6u8 {
        let (lo, hi) = crate::pci::read_bar_raw(dev.bus, dev.slot, dev.function, bar);
        if lo == 0 || (lo & 0x1) != 0 {
            continue;
        }
        let phys = ((hi.unwrap_or(0) as u64) << 32) | ((lo & !0xf) as u64);
        if phys != 0 {
            return Some((bar, phys));
        }
    }
    None
}

pub mod xhci {
    pub const MAX_XHCI_CONTROLLERS: usize = 0;
}

#[derive(Clone, Debug, Default)]
pub struct UsbControllerInfo {
    pub index: usize,
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub controller_phase: &'static str,
    pub root_hub_lifecycle: &'static str,
    pub event_ready: bool,
    pub root_port_change_seen: bool,
    pub empty_probe_streak: u32,
}

pub fn pci_usb_controllers() -> Vec<UsbControllerInfo> {
    Vec::new()
}

pub fn discover_first_controller() -> Option<UsbControllerInfo> {
    None
}

pub async fn crabusb_bsp_service(_index: usize) {
    core::future::pending::<()>().await;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TlbUsbTopologyNodeKind {
    RootPort,
    Hub,
    Device,
}

#[derive(Clone, Debug, Default)]
pub struct UsbDeviceSummary {
    pub root_port_id: u8,
    pub port: u8,
    pub slot_id: u8,
    pub route_string: u32,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub class: Option<u8>,
    pub subclass: Option<u8>,
    pub protocol: Option<u8>,
    pub kind: &'static str,
    pub product: Option<String>,
    pub stable_id: u32,
}

#[derive(Clone, Debug, Default)]
pub struct TlbUsbEndpoint {
    pub address: u8,
    pub transfer_type: &'static str,
    pub max_packet_size: u16,
    pub interval: u8,
}

#[derive(Clone, Debug, Default)]
pub struct TlbUsbInterface {
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub endpoints: Vec<TlbUsbEndpoint>,
}

#[derive(Clone, Debug, Default)]
pub struct TlbUsbConfiguration {
    pub configuration_value: u8,
    pub attributes: u8,
    pub max_power: u8,
    pub interfaces: Vec<TlbUsbInterface>,
}

#[derive(Clone, Debug, Default)]
pub struct TlbUsbHubPathHop {
    pub slot_id: u8,
    pub port_id: u8,
    pub hub_depth: u8,
    pub speed: &'static str,
}

#[derive(Clone, Debug, Default)]
pub struct TlbUsbDevice {
    pub stable_id: u32,
    pub slot_id: u8,
    pub root_port_id: u8,
    pub port_id: u8,
    pub route_string: u32,
    pub speed: &'static str,
    pub vendor_id: u16,
    pub product_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub num_configurations: u8,
    pub max_packet_size_0: u8,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub path: Vec<u8>,
    pub parent_hub_slot_id: Option<u8>,
    pub hub_path: Vec<TlbUsbHubPathHop>,
    pub configurations: Vec<TlbUsbConfiguration>,
}

#[derive(Clone, Debug)]
pub struct TlbUsbTopologyNode {
    pub kind: TlbUsbTopologyNodeKind,
    pub controller_index: usize,
    pub root_port_id: u8,
    pub port_id: u8,
    pub depth: u8,
    pub slot_id: Option<u8>,
    pub parent_slot_id: Option<u8>,
    pub speed: &'static str,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub class: Option<u8>,
    pub subclass: Option<u8>,
    pub protocol: Option<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct TlbUsbSnapshot {
    pub controllers: Vec<UsbControllerInfo>,
    pub devices: Vec<TlbUsbDevice>,
    pub topology: Vec<TlbUsbTopologyNode>,
    pub probe_device_count: Option<usize>,
    pub probe_error: Option<&'static str>,
}

pub fn tlb_usb_snapshot() -> TlbUsbSnapshot {
    TlbUsbSnapshot::default()
}

pub fn crabusb_observed_device_summaries(
    _controller_index: usize,
) -> Result<Vec<UsbDeviceSummary>, &'static str> {
    Ok(Vec::new())
}

pub fn crabusb_observed_devices(
    _controller_index: usize,
) -> Result<Vec<TlbUsbDevice>, &'static str> {
    Ok(Vec::new())
}

#[derive(Clone, Debug, Default)]
pub struct UsbRuntimeDiag {
    pub probe_requested: bool,
    pub probe_fail_streak: u32,
    pub early_fatal_rebind_streak: u32,
    pub last_probe_state: &'static str,
    pub last_probe_device_count: usize,
    pub recovery_quiescent_before_bind: bool,
    pub recovery_quiescent_ms: u64,
    pub recovery_initial_settle_ms: u64,
    pub recovery_probe_quiet_ms: u64,
    pub recovery_skip_delayed_event_handler: bool,
}

pub fn crabusb_runtime_diag(_controller_index: usize) -> UsbRuntimeDiag {
    UsbRuntimeDiag::default()
}

#[derive(Clone, Debug, Default)]
pub struct XhciPortDiag {
    pub port_id: u8,
    pub portsc: u32,
    pub portpmsc: u32,
    pub portli: u32,
}

#[derive(Clone, Debug, Default)]
pub struct XhciMmioDiag {
    pub caplen: u8,
    pub hcsparams1: u32,
    pub hccparams1: u32,
    pub dboff: u32,
    pub rtsoff: u32,
    pub usbcmd: u32,
    pub usbsts: u32,
    pub crcr: u64,
    pub dcbaap: u64,
    pub config: u32,
    pub iman: u32,
    pub imod: u32,
    pub erstsz: u32,
    pub erstba: u64,
    pub erdp: u64,
    pub ports: Vec<XhciPortDiag>,
}

pub fn controller_mmio_diag(_controller_index: usize) -> Option<XhciMmioDiag> {
    None
}

pub mod class {
    #[derive(Clone, Copy, Debug)]
    pub struct UsbClassTriple {
        class: u8,
        subclass: u8,
        protocol: u8,
    }

    #[derive(Clone, Copy, Debug)]
    pub struct UsbBaseClass {
        code: u8,
    }

    #[derive(Clone, Copy, Debug)]
    pub struct DescriptorUsage;

    impl UsbClassTriple {
        pub const fn from_codes(class: u8, subclass: u8, protocol: u8) -> Self {
            Self {
                class,
                subclass,
                protocol,
            }
        }

        pub const fn base_class(self) -> UsbBaseClass {
            UsbBaseClass { code: self.class }
        }

        pub const fn short_name(self) -> &'static str {
            let _ = self.subclass;
            let _ = self.protocol;
            "USB"
        }

        pub const fn description(self) -> &'static str {
            let _ = self;
            "disabled"
        }
    }

    impl UsbBaseClass {
        pub const fn code(self) -> u8 {
            self.code
        }

        pub const fn descriptor_usage(self) -> DescriptorUsage {
            let _ = self;
            DescriptorUsage
        }

        pub const fn description(self) -> &'static str {
            let _ = self;
            "disabled"
        }
    }

    impl DescriptorUsage {
        pub const fn as_str(self) -> &'static str {
            let _ = self;
            "disabled"
        }
    }
}

pub mod input {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct MouseEvent {
        pub buttons: u8,
        pub dx: i8,
        pub dy: i8,
        pub wheel: i8,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct TabletEvent {
        pub slot_id: u32,
        pub buttons: u32,
        pub report_id: u8,
        pub x_raw: u16,
        pub y_raw: u16,
        pub x_norm_q15: u16,
        pub y_norm_q15: u16,
        pub flags: u32,
    }

    pub fn pop_mouse_event() -> Option<MouseEvent> {
        None
    }

    pub fn pop_tablet_event() -> Option<TabletEvent> {
        None
    }
}

pub mod hid {
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicU64, Ordering};
    use spin::Mutex;

    static VIRTUAL_CURSOR_SEQ: AtomicU64 = AtomicU64::new(1);
    const CURSOR_EVENT_CAP: usize = crate::allcaps::input::HID_CURSOR_EVENT_RING_CAP;
    const USB3_MOUSE_CONTROLLER_ID: u32 = 3;

    static CURSOR_EVENTS: Mutex<Vec<TrueosHidCursorEvent>> = Mutex::new(Vec::new());
    static MOUSE_STATES: Mutex<Vec<MouseCursorState>> = Mutex::new(Vec::new());

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct TrueosHidCursorEvent {
        pub seq: u64,
        pub controller_id: u32,
        pub slot_id: u32,
        pub ep_target: u32,
        pub hid_kind: u8,
        pub buttons_down: u32,
        pub wheel: i16,
        pub flags: u32,
    }

    #[derive(Clone, Copy, Debug)]
    struct MouseCursorState {
        controller_id: u32,
        slot_id: u32,
        ep_target: u32,
        x: f64,
        y: f64,
    }

    pub fn pop_cursor_event() -> Option<TrueosHidCursorEvent> {
        let mut events = CURSOR_EVENTS.lock();
        if events.is_empty() {
            None
        } else {
            Some(events.remove(0))
        }
    }

    pub fn read_cursor_events_since(
        read_seq: u64,
        out: &mut [TrueosHidCursorEvent],
    ) -> (u64, u32, usize) {
        let events = CURSOR_EVENTS.lock();
        let next_seq = VIRTUAL_CURSOR_SEQ
            .load(Ordering::Relaxed)
            .saturating_sub(1)
            .max(read_seq);
        let first_seq = events.first().map(|event| event.seq).unwrap_or(next_seq);
        let dropped = if read_seq != 0 && read_seq.saturating_add(1) < first_seq {
            first_seq.saturating_sub(read_seq.saturating_add(1)) as u32
        } else {
            0
        };

        let mut wrote = 0usize;
        for event in events.iter().filter(|event| event.seq > read_seq) {
            if wrote >= out.len() {
                break;
            }
            out[wrote] = *event;
            wrote += 1;
        }
        (next_seq, dropped, wrote)
    }

    pub fn inject_virtual_cursor_event(
        _slot_id: u32,
        _nx: f64,
        _ny: f64,
        _buttons_down: u32,
        _wheel: i16,
        _flags: u32,
    ) {
        push_cursor_event(TrueosHidCursorEvent {
            seq: 0,
            controller_id: 0,
            slot_id: _slot_id,
            ep_target: 0,
            hid_kind: crate::r::cursor::HID_KIND_VIRTUAL_CURSOR,
            buttons_down: _buttons_down,
            wheel: _wheel,
            flags: _flags,
        });
    }

    pub fn inject_usb3_mouse_relative_event(
        slot_id: u32,
        ep_target: u32,
        dx: i8,
        dy: i8,
        buttons_down: u32,
        wheel: i16,
        flags: u32,
    ) {
        let (x, y) = update_mouse_cursor(USB3_MOUSE_CONTROLLER_ID, slot_id, ep_target, dx, dy);
        crate::r::cursor::upsert_snapshot(
            USB3_MOUSE_CONTROLLER_ID,
            slot_id,
            ep_target,
            crate::r::cursor::HID_KIND_MOUSE,
            x,
            y,
            buttons_down,
        );
        push_cursor_event(TrueosHidCursorEvent {
            seq: 0,
            controller_id: USB3_MOUSE_CONTROLLER_ID,
            slot_id,
            ep_target,
            hid_kind: crate::r::cursor::HID_KIND_MOUSE,
            buttons_down,
            wheel,
            flags,
        });
    }

    fn push_cursor_event(mut event: TrueosHidCursorEvent) {
        event.seq = VIRTUAL_CURSOR_SEQ.fetch_add(1, Ordering::Relaxed);
        let mut events = CURSOR_EVENTS.lock();
        if events.len() >= CURSOR_EVENT_CAP {
            let _ = events.remove(0);
        }
        events.push(event);
    }

    fn update_mouse_cursor(
        controller_id: u32,
        slot_id: u32,
        ep_target: u32,
        dx: i8,
        dy: i8,
    ) -> (f64, f64) {
        let (view_w, view_h) = crate::intel::active_scanout_dimensions()
            .map(|(w, h)| (w.max(1) as f64, h.max(1) as f64))
            .unwrap_or((1920.0, 1080.0));
        let mut states = MOUSE_STATES.lock();
        if let Some(state) = states.iter_mut().find(|state| {
            state.controller_id == controller_id
                && state.slot_id == slot_id
                && state.ep_target == ep_target
        }) {
            state.x = clamp01(state.x + (dx as f64 / view_w));
            state.y = clamp01(state.y + (dy as f64 / view_h));
            return (state.x, state.y);
        }

        let mut state = MouseCursorState {
            controller_id,
            slot_id,
            ep_target,
            x: 0.5,
            y: 0.5,
        };
        state.x = clamp01(state.x + (dx as f64 / view_w));
        state.y = clamp01(state.y + (dy as f64 / view_h));
        let out = (state.x, state.y);
        states.push(state);
        out
    }

    #[inline]
    fn clamp01(value: f64) -> f64 {
        if value < 0.0 {
            0.0
        } else if value > 1.0 {
            1.0
        } else {
            value
        }
    }
}

pub mod hut {
    use alloc::vec::Vec;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct KeyboardSnapshot {
        pub key_down_bits: [u32; 8],
    }

    pub fn keyboards_snapshot() -> Vec<KeyboardSnapshot> {
        Vec::new()
    }
}

pub mod midi {
    #[derive(Clone, Copy, Debug)]
    pub struct PianoHeldSnapshot {
        pub seq: u16,
        pub len: usize,
        pub notes: [u8; 16],
        pub velocities: [u8; 16],
    }

    impl Default for PianoHeldSnapshot {
        fn default() -> Self {
            Self {
                seq: 0,
                len: 0,
                notes: [0; 16],
                velocities: [0; 16],
            }
        }
    }

    pub fn piano_connected() -> bool {
        false
    }

    pub fn piano_held_snapshot() -> Option<PianoHeldSnapshot> {
        None
    }
}
