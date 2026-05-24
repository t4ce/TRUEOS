use alloc::string::String;
use alloc::vec::Vec;

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
    use core::sync::atomic::{AtomicU64, Ordering};

    static VIRTUAL_CURSOR_SEQ: AtomicU64 = AtomicU64::new(1);

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

    pub fn pop_cursor_event() -> Option<TrueosHidCursorEvent> {
        None
    }

    pub fn read_cursor_events_since(
        read_seq: u64,
        _out: &mut [TrueosHidCursorEvent],
    ) -> (u64, u32, usize) {
        (read_seq.max(VIRTUAL_CURSOR_SEQ.load(Ordering::Relaxed)), 0, 0)
    }

    pub fn inject_virtual_cursor_event(
        _slot_id: u32,
        _nx: f64,
        _ny: f64,
        _buttons_down: u32,
        _wheel: i16,
        _flags: u32,
    ) {
        VIRTUAL_CURSOR_SEQ.fetch_add(1, Ordering::Relaxed);
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
