#![allow(dead_code, unused_macros)]
extern crate alloc;

use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};
use futures::{FutureExt, future::BoxFuture};
use id_arena::{Arena, Id};
use std::sync::{Arc, Mutex};

macro_rules! info { ($($args:tt)*) => {{ let _ = format_args!($($args)*); }} }
macro_rules! warn { ($($args:tt)*) => {{ let _ = format_args!($($args)*); }} }
macro_rules! error { ($($args:tt)*) => {{ let _ = format_args!($($args)*); }} }
macro_rules! debug { ($($args:tt)*) => {{ let _ = format_args!($($args)*); }} }
macro_rules! trace { ($($args:tt)*) => {{ let _ = format_args!($($args)*); }} }

#[derive(Debug)]
enum USBError {
    Transaction,
    HubScan,
    HubConfiguration,
    Other(anyhow::Error),
}
impl core::fmt::Display for USBError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
@ENUMERATION_OWNER@
mod hub_types { @HUB_TYPES@
}
use hub_types::Speed;
#[allow(unused_imports)]
mod enumeration_diagnostics { @ENUMERATION_DIAGNOSTICS@
}
type ConfigurationDescriptor = ();
#[derive(Clone)]
struct DeviceDescriptor {
    hub: bool,
    fail_hub_configuration: bool,
}

mod device {
    use super::*;
    #[derive(Clone)]
    pub struct DeviceTopology {
        pub hub_ports: Option<u8>,
        pub root_port_id: u8,
        pub port_id: u8,
        pub route_string: u32,
        pub speed: Speed,
        pub parent_hub_slot_id: Option<u8>,
        pub path: Vec<u8>,
    }
}

#[derive(Clone, Default)]
struct HubInfo {
    hub_depth: isize,
    port_id: u8,
    parent: Option<Id<Hub>>,
    speed: Speed,
    slot_id: u8,
    tt: UsbTt,
}
#[derive(Clone, Default)]
struct UsbTt {
    multi: bool,
}
struct PortChangeInfo {
    root_port_id: u8,
    port_id: u8,
    port_speed: Speed,
    tt_port_on_hub: Option<u8>,
}
struct DeviceAddressInfo {
    root_port_id: u8,
    port_speed: Speed,
    parent_hub: Option<Id<Hub>>,
    port_id: u8,
    infos: BTreeMap<Id<Hub>, HubInfo>,
}

trait DeviceOp: Send {
    fn id(&self) -> usize;
    fn descriptor(&self) -> &DeviceDescriptor;
    fn configuration_descriptors(&self) -> &[ConfigurationDescriptor] {
        &[]
    }
}
struct Device(Box<dyn DeviceOp>);
impl From<Box<dyn DeviceOp>> for Device {
    fn from(device: Box<dyn DeviceOp>) -> Self {
        Self(device)
    }
}
struct FakeDevice {
    id: usize,
    descriptor: DeviceDescriptor,
}
impl DeviceOp for FakeDevice {
    fn id(&self) -> usize {
        self.id
    }
    fn descriptor(&self) -> &DeviceDescriptor {
        &self.descriptor
    }
}

trait DeviceInfoOp {
    fn id(&self) -> usize;
    fn topology(&self) -> &device::DeviceTopology;
}
struct DeviceInfo {
    id: usize,
    topology: device::DeviceTopology,
}
impl DeviceInfo {
    fn new(id: usize, _: DeviceDescriptor, _: &[()], topology: device::DeviceTopology) -> Self {
        Self { id, topology }
    }
}
impl DeviceInfoOp for DeviceInfo {
    fn id(&self) -> usize {
        self.id
    }
    fn topology(&self) -> &device::DeviceTopology {
        &self.topology
    }
}
enum ProbedDeviceInfoOp {
    Device(Box<dyn DeviceInfoOp>),
    Hub(Box<dyn DeviceInfoOp>),
}
impl ProbedDeviceInfoOp {
    fn info(&self) -> &dyn DeviceInfoOp {
        match self {
            Self::Device(info) | Self::Hub(info) => info.as_ref(),
        }
    }
}

trait HubOp: Send {
    fn slot_id(&self) -> u8;
    fn num_ports(&self) -> Option<u8> {
        None
    }
    fn changed_ports(&mut self) -> BoxFuture<'_, Result<Vec<PortChangeInfo>, USBError>>;
    fn init(&mut self, info: HubInfo) -> BoxFuture<'_, Result<HubInfo, USBError>> {
        async { Ok(info) }.boxed()
    }
}
struct Hub {
    backend: Box<dyn HubOp>,
    info: HubInfo,
}
impl Hub {
    fn new(
        backend: Box<dyn HubOp>,
        _: &BTreeMap<Id<Hub>, HubInfo>,
        port_id: u8,
        parent: Option<Id<Hub>>,
    ) -> Self {
        Self {
            backend,
            info: HubInfo {
                hub_depth: 0,
                port_id,
                parent,
                ..Default::default()
            },
        }
    }
}
struct FakeHub {
    ports: Vec<u8>,
    fail: bool,
}
impl HubOp for FakeHub {
    fn slot_id(&self) -> u8 {
        0
    }
    fn changed_ports(&mut self) -> BoxFuture<'_, Result<Vec<PortChangeInfo>, USBError>> {
        let ports = std::mem::take(&mut self.ports);
        let fail = self.fail;
        async move {
            if fail {
                return Err(USBError::HubScan);
            }
            Ok(ports
                .into_iter()
                .map(|port_id| PortChangeInfo {
                    root_port_id: port_id,
                    port_id,
                    port_speed: Speed::High,
                    tt_port_on_hub: None,
                })
                .collect())
        }
        .boxed()
    }
}
struct HubDevice {
    device: Device,
}
impl HubDevice {
    fn is_hub(desc: &DeviceDescriptor, _: &[()]) -> Option<()> {
        desc.hub.then_some(())
    }
    fn new(device: Device, _: (), _: u8, _: u8, _: &()) -> Self {
        Self { device }
    }
}
impl HubOp for HubDevice {
    fn slot_id(&self) -> u8 {
        self.device.0.id() as u8
    }
    fn changed_ports(&mut self) -> BoxFuture<'_, Result<Vec<PortChangeInfo>, USBError>> {
        async { Ok(Vec::new()) }.boxed()
    }
    fn init(&mut self, info: HubInfo) -> BoxFuture<'_, Result<HubInfo, USBError>> {
        let fail = self.device.0.descriptor().fail_hub_configuration;
        async move {
            if fail {
                Err(USBError::HubConfiguration)
            } else {
                Ok(info)
            }
        }
        .boxed()
    }
}

trait CoreOp {
    fn new_addressed_device(
        &mut self,
        info: DeviceAddressInfo,
    ) -> BoxFuture<'_, Result<Box<dyn DeviceOp>, USBError>>;
    fn kernel(&self) -> &() {
        &()
    }
    fn record_enumeration_failure(
        &mut self,
        _: u8,
        _: u8,
        _: Option<u8>,
        _: &'static str,
        _: &dyn core::fmt::Display,
    ) {
    }
}
struct FakeBackend {
    failures: Vec<u8>,
    bad_hubs: Vec<u8>,
    attempts: Arc<Mutex<Vec<u8>>>,
}
impl CoreOp for FakeBackend {
    fn new_addressed_device(
        &mut self,
        info: DeviceAddressInfo,
    ) -> BoxFuture<'_, Result<Box<dyn DeviceOp>, USBError>> {
        self.attempts.lock().unwrap().push(info.root_port_id);
        let result = if self.failures.contains(&info.root_port_id) {
            Err(USBError::Transaction)
        } else {
            Ok(Box::new(FakeDevice {
                id: info.root_port_id as usize,
                descriptor: DeviceDescriptor {
                    hub: self.bad_hubs.contains(&info.root_port_id),
                    fail_hub_configuration: self.bad_hubs.contains(&info.root_port_id),
                },
            }) as Box<dyn DeviceOp>)
        };
        async move { result }.boxed()
    }
}
struct Core {
    backend: Box<dyn CoreOp>,
    hubs: Arena<Hub>,
    root_hub: Option<Id<Hub>>,
    inited_devices: BTreeMap<usize, Box<dyn DeviceOp>>,
    quarantined_hubs: Vec<EnumerationOwner<Hub>>,
}
impl Core {
    @CORE_METHODS@
}

fn fixture(ports: &[u8], failures: &[u8]) -> (Core, Arc<Mutex<Vec<u8>>>) {
    let attempts = Arc::new(Mutex::new(Vec::new()));
    let mut hubs = Arena::new();
    let root_hub = hubs.alloc(Hub {
        backend: Box::new(FakeHub {
            ports: ports.to_vec(),
            fail: false,
        }),
        info: HubInfo {
            hub_depth: -1,
            port_id: 0,
            parent: None,
            ..Default::default()
        },
    });
    let core = Core {
        backend: Box::new(FakeBackend {
            failures: failures.to_vec(),
            bad_hubs: Vec::new(),
            attempts: attempts.clone(),
        }),
        hubs,
        root_hub: Some(root_hub),
        inited_devices: BTreeMap::new(),
        quarantined_hubs: Vec::new(),
    };
    (core, attempts)
}

#[test]
fn first_device_failure_does_not_consume_later_ports() {
    let (mut core, attempts) = fixture(&[2, 6, 7, 10], &[2]);
    let result = futures::executor::block_on(core.probe_devices()).unwrap();
    assert_eq!(*attempts.lock().unwrap(), [2, 6, 7, 10]);
    assert_eq!(
        result
            .iter()
            .map(|info| info.info().id())
            .collect::<Vec<_>>(),
        [6, 7, 10]
    );
    assert_eq!(
        core.inited_devices.keys().copied().collect::<Vec<_>>(),
        [6, 7, 10]
    );
    assert!(
        futures::executor::block_on(core.probe_devices())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        *attempts.lock().unwrap(),
        [2, 6, 7, 10],
        "failed port is not blindly retried"
    );
}

#[test]
fn later_device_failure_keeps_earlier_results_and_continues() {
    let (mut core, attempts) = fixture(&[2, 6, 7], &[6]);
    let result = futures::executor::block_on(core.probe_devices()).unwrap();
    assert_eq!(*attempts.lock().unwrap(), [2, 6, 7]);
    assert_eq!(
        result
            .iter()
            .map(|info| info.info().id())
            .collect::<Vec<_>>(),
        [2, 7]
    );
    assert_eq!(
        core.inited_devices.keys().copied().collect::<Vec<_>>(),
        [2, 7]
    );
    assert_eq!(result[1].info().topology().path, [7]);
}

#[test]
fn one_hub_scan_failure_keeps_other_hubs_available() {
    let (mut core, attempts) = fixture(&[2], &[]);
    core.hubs.alloc(Hub {
        backend: Box::new(FakeHub {
            ports: vec![],
            fail: true,
        }),
        info: HubInfo {
            hub_depth: 0,
            port_id: 3,
            parent: core.root_hub,
            ..Default::default()
        },
    });
    core.hubs.alloc(Hub {
        backend: Box::new(FakeHub {
            ports: vec![6],
            fail: false,
        }),
        info: HubInfo {
            hub_depth: -1,
            port_id: 0,
            parent: None,
            ..Default::default()
        },
    });
    let result = futures::executor::block_on(core.probe_devices()).unwrap();
    assert_eq!(*attempts.lock().unwrap(), [2, 6]);
    assert_eq!(
        result
            .iter()
            .map(|info| info.info().id())
            .collect::<Vec<_>>(),
        [2, 6]
    );
}

#[test]
fn bad_hub_configuration_keeps_other_ports_and_retains_owner() {
    let (mut core, attempts) = fixture(&[2, 6, 7], &[]);
    core.backend = Box::new(FakeBackend {
        failures: vec![],
        bad_hubs: vec![6],
        attempts: attempts.clone(),
    });
    let result = futures::executor::block_on(core.probe_devices()).unwrap();
    assert_eq!(*attempts.lock().unwrap(), [2, 6, 7]);
    assert_eq!(
        result
            .iter()
            .map(|info| info.info().id())
            .collect::<Vec<_>>(),
        [2, 7]
    );
    assert_eq!(core.quarantined_hubs.len(), 1);
    assert_eq!(core.quarantined_hubs[0].get_mut().backend.slot_id(), 6);
}

mod admission {
    use super::{DeviceAddressInfo, EnumerationOwner, USBError};
    use std::{
        collections::BTreeSet,
        sync::{Arc, Mutex},
    };
    type Result<T = ()> = core::result::Result<T, USBError>;

    #[derive(Default)]
    struct Allocator {
        active_slots: BTreeSet<usize>,
        released_dma: Vec<usize>,
    }
    #[derive(Clone, Copy)]
    enum Completion {
        Success,
        Failure(&'static str),
        Pending(&'static str),
    }
    impl Completion {
        fn immediate(self, stage: &'static str) -> Result {
            match self {
                Self::Failure(fault) if fault == stage => Err(USBError::Transaction),
                _ => Ok(()),
            }
        }
        async fn complete(self, stage: &'static str) -> Result {
            if matches!(self, Self::Pending(fault) if fault == stage) {
                core::future::pending::<()>().await;
            }
            self.immediate(stage)
        }
    }
    trait DeviceOp {
        fn id(&self) -> usize;
    }
    struct Dci;
    impl Dci {
        const CTRL: Self = Self;
    }
    struct EndpointInfo;
    impl EndpointInfo {
        fn control() -> Self {
            Self
        }
    }
    struct RawEndpoint {
        completion: Completion,
    }
    struct Endpoint {
        completion: Completion,
    }
    #[derive(Debug)]
    struct DescriptorBase;
    struct Descriptor {
        num_configurations: u8,
    }
    struct Configuration {
        configuration_value: u8,
    }
    impl Endpoint {
        fn new(_: EndpointInfo, ep: RawEndpoint) -> Self {
            Self {
                completion: ep.completion,
            }
        }
        async fn get_configuration_descriptor(&mut self, _: u8) -> Result<Configuration> {
            self.completion.complete("get-config-descriptor").await?;
            Ok(Configuration {
                configuration_value: 1,
            })
        }
    }
    struct Device {
        slot: usize,
        allocator: Arc<Mutex<Allocator>>,
        enumeration_stage: &'static str,
        completion: Completion,
        port_speed: super::Speed,
        ctrl_ep: Option<Endpoint>,
        desc: Descriptor,
        config_desc: Vec<Configuration>,
    }
    impl Device {
        async fn new(host: &mut Xhci) -> Result<Self> {
            let mut allocator = host.allocator.lock().unwrap();
            let slot = (1..)
                .find(|slot| !allocator.active_slots.contains(slot))
                .unwrap();
            allocator.active_slots.insert(slot);
            Ok(Self {
                slot,
                allocator: host.allocator.clone(),
                enumeration_stage: "allocate-ep0",
                completion: host.completion,
                port_speed: super::Speed::Full,
                ctrl_ep: None,
                desc: Descriptor {
                    num_configurations: 1,
                },
                config_desc: Vec::new(),
            })
        }
        @DEVICE_INIT@
        fn new_ep(&mut self, _: Dci) -> Result<RawEndpoint> {
            self.completion.immediate("allocate-ep0")?;
            Ok(RawEndpoint {
                completion: self.completion,
            })
        }
        fn control_endpoint_mut(&mut self) -> &mut Endpoint {
            self.ctrl_ep.as_mut().unwrap()
        }
        async fn address(&mut self, _: &mut Xhci, _: &DeviceAddressInfo) -> Result {
            self.completion.complete("address-device").await
        }
        async fn get_device_descriptor_base(&mut self) -> Result<DescriptorBase> {
            self.completion
                .complete("get-device-descriptor-base")
                .await?;
            Ok(DescriptorBase)
        }
        async fn setup_max_packet(&mut self, _: DescriptorBase) -> Result {
            self.completion.complete("evaluate-ep0").await
        }
        async fn get_configuration(&mut self) -> Result<u8> {
            self.completion.complete("get-configuration").await?;
            Ok(0)
        }
        async fn read_descriptor(&mut self) -> Result {
            self.completion.complete("get-device-descriptor").await
        }
        async fn _set_configuration(&mut self, _: u8) -> Result {
            self.completion.complete("set-configuration").await
        }
        fn enumeration_stage(&self) -> &'static str {
            self.enumeration_stage
        }
    }
    impl DeviceOp for Device {
        fn id(&self) -> usize {
            self.slot
        }
    }
    impl Drop for Device {
        fn drop(&mut self) {
            let mut allocator = self.allocator.lock().unwrap();
            assert!(allocator.active_slots.remove(&self.slot));
            allocator.released_dma.push(self.slot);
        }
    }
    struct Xhci {
        allocator: Arc<Mutex<Allocator>>,
        completion: Completion,
        quarantined_devices: Vec<EnumerationOwner<Device>>,
        enumeration: super::enumeration_diagnostics::XhciEnumerationDiagnostics,
    }
    impl Xhci {
        @NEW_DEVICE@
    }
    fn host(completion: Completion) -> Xhci {
        Xhci {
            allocator: Arc::default(),
            completion,
            quarantined_devices: Vec::new(),
            enumeration: Default::default(),
        }
    }
    fn address(port: u8) -> DeviceAddressInfo {
        DeviceAddressInfo {
            root_port_id: port,
            port_id: port,
            port_speed: super::Speed::High,
            parent_hub: None,
            infos: Default::default(),
        }
    }

    #[test]
    fn init_failure_retains_dma_slot_and_identifies_the_failing_stage() {
        for stage in [
            "allocate-ep0",
            "address-device",
            "get-device-descriptor-base",
            "evaluate-ep0",
            "get-configuration",
            "get-device-descriptor",
            "get-config-descriptor",
            "set-configuration",
        ] {
            let mut host = host(Completion::Failure(stage));
            let error = match futures::executor::block_on(host.new_device(address(6))) {
                Ok(_) => panic!("fault injection unexpectedly succeeded"),
                Err(error) => error,
            };
            let detail = error.to_string();
            assert!(detail.contains("root_port=6"), "{detail}");
            assert!(detail.contains("port=6"), "{detail}");
            assert!(detail.contains("slot=1"), "{detail}");
            assert!(detail.contains(&format!("stage={stage}")), "{detail}");
            assert_eq!(host.quarantined_devices.len(), 1);
            assert_eq!(host.enumeration.failure_count, 1);
            let evidence = host.enumeration.first_failure.clone().unwrap();
            assert_eq!(
                (
                    evidence.root_port_id,
                    evidence.port_id,
                    evidence.slot_id,
                    evidence.stage
                ),
                (6, 6, Some(1), stage)
            );
            assert!(host.allocator.lock().unwrap().released_dma.is_empty());
            host.completion = Completion::Success;
            let device = futures::executor::block_on(host.new_device(address(7))).unwrap();
            assert_eq!(device.id(), 2, "failed device's live slot cannot be reused");
            assert_eq!(
                host.enumeration.first_failure,
                Some(evidence.clone()),
                "success preserves prior evidence"
            );
            assert_eq!(host.enumeration.last_failure, Some(evidence));
            drop(device);
            let allocator = host.allocator.clone();
            drop(host);
            assert_eq!(allocator.lock().unwrap().active_slots, BTreeSet::from([1]));
            assert_eq!(allocator.lock().unwrap().released_dma, [2]);
        }
    }

    #[test]
    fn cancellation_keeps_controller_visible_dma_and_slot_occupied() {
        let mut host = host(Completion::Pending("address-device"));
        let mut future = Box::pin(host.new_device(address(2)));
        assert!(
            core::future::Future::poll(
                future.as_mut(),
                &mut core::task::Context::from_waker(core::task::Waker::noop())
            )
            .is_pending()
        );
        drop(future);
        assert_eq!(
            host.allocator.lock().unwrap().active_slots,
            BTreeSet::from([1])
        );
        assert!(host.allocator.lock().unwrap().released_dma.is_empty());
        host.completion = Completion::Success;
        let device = futures::executor::block_on(host.new_device(address(6))).unwrap();
        assert_eq!(
            device.id(),
            2,
            "cancelled admission cannot release a slot without teardown"
        );
        drop(device);
        assert_eq!(
            host.allocator.lock().unwrap().active_slots,
            BTreeSet::from([1])
        );
    }
}

mod wire_values {
    use super::{
        USBError,
        hub_types::{HubDescriptor, PortStatus, PortStatusChange, Speed},
    };
    use anyhow::anyhow;
    type Result<T, E = USBError> = core::result::Result<T, E>;
    @EP0_PACKET_SIZE@
    @HUB_HELPERS@
    use super::DeviceAddressInfo;
    @TRANSACTION_TRANSLATOR@

    #[test]
    fn transaction_translator_uses_child_port_on_the_nearest_high_speed_hub() {
        let (mut core, _) = super::fixture(&[], &[]);
        let parent = core.root_hub;
        let hub1 = core.hubs.alloc(super::Hub {
            backend: Box::new(super::FakeHub {
                ports: vec![],
                fail: false,
            }),
            info: super::HubInfo {
                parent,
                slot_id: 2,
                hub_depth: 0,
                port_id: 6,
                speed: Speed::High,
                tt: super::UsbTt { multi: true },
            },
        });
        let hub2 = core.hubs.alloc(super::Hub {
            backend: Box::new(super::FakeHub {
                ports: vec![],
                fail: false,
            }),
            info: super::HubInfo {
                parent: Some(hub1),
                slot_id: 4,
                hub_depth: 1,
                port_id: 2,
                speed: Speed::High,
                tt: super::UsbTt { multi: true },
            },
        });
        let mut info = DeviceAddressInfo {
            root_port_id: 6,
            port_id: 3,
            port_speed: Speed::Full,
            parent_hub: Some(hub2),
            infos: core.hub_infos(),
        };
        assert_eq!(
            transaction_translator(&info),
            Some((4, 3, true)),
            "Laview child is on HP2734 port3, not its upstream port2"
        );
        info.port_speed = Speed::Low;
        assert_eq!(transaction_translator(&info), Some((4, 3, true)));
        info.port_speed = Speed::High;
        assert_eq!(transaction_translator(&info), None);
        info.port_speed = Speed::Full;
        info.parent_hub = parent;
        assert_eq!(
            transaction_translator(&info),
            None,
            "direct root children need no external TT"
        );
        info.parent_hub = Some(hub2);
        info.infos.get_mut(&hub2).unwrap().speed = Speed::Full;
        assert_eq!(
            transaction_translator(&info),
            Some((2, 2, true)),
            "FS hub shares its HS ancestor's upstream port TT"
        );
    }

    #[test]
    fn superspeed_ep0_exponent_is_decoded_while_usb2_uses_bytes() {
        for speed in [Speed::SuperSpeed, Speed::SuperSpeedPlus] {
            assert_eq!(ep0_max_packet_size(speed, 9).unwrap(), 512);
            assert!(ep0_max_packet_size(speed, 8).is_err());
            assert!(ep0_max_packet_size(speed, 64).is_err());
        }
        for bytes in [8, 16, 32, 64] {
            assert_eq!(
                ep0_max_packet_size(Speed::Full, bytes).unwrap(),
                bytes as u16
            );
        }
        assert_eq!(ep0_max_packet_size(Speed::High, 64).unwrap(), 64);
        assert_eq!(ep0_max_packet_size(Speed::Low, 8).unwrap(), 8);
        assert!(ep0_max_packet_size(Speed::High, 9).is_err());
        assert!(ep0_max_packet_size(Speed::Low, 64).is_err());
        assert!(ep0_max_packet_size(Speed::Full, 0).is_err());
    }

    #[test]
    fn nine_byte_usb2_hub_descriptor_is_copied_without_adjacent_bytes() {
        let wire = [9, 0x29, 4, 0x12, 0, 50, 100, 0, 0xff, 0xa5, 0xa5, 0xa5];
        let desc = parse_hub_descriptor(&wire[..9], false).unwrap();
        assert_eq!(desc.bNbrPorts, 4);
        assert_eq!(desc.hub_characteristics(), 0x12);
        let representation = unsafe {
            core::slice::from_raw_parts(
                core::ptr::addr_of!(desc).cast::<u8>(),
                core::mem::size_of::<HubDescriptor>(),
            )
        };
        assert_eq!(&representation[..9], &wire[..9]);
        assert!(
            representation[9..].iter().all(|byte| *byte == 0),
            "short wire descriptor cannot read adjacent allocation data"
        );
    }

    #[test]
    fn hub_descriptors_validate_wire_length_type_and_port_bitmap_extent() {
        let usb2 = [9, 0x29, 4, 0, 0, 50, 0, 0, 0xff];
        for len in 0..9 {
            assert!(parse_hub_descriptor(&usb2[..len], false).is_err());
        }
        let mut malformed = usb2;
        malformed[0] = 8;
        assert!(parse_hub_descriptor(&malformed, false).is_err());
        malformed[0] = 10;
        assert!(parse_hub_descriptor(&malformed, false).is_err());
        malformed = usb2;
        malformed[2] = 8; // needs two bytes in each variable bitmap
        assert!(parse_hub_descriptor(&malformed, false).is_err());
        malformed[2] = 0;
        assert!(parse_hub_descriptor(&malformed, false).is_err());
        assert!(parse_hub_descriptor(&usb2, true).is_err());
        let usb3 = [12, 0x2a, 6, 0, 0, 50, 0, 7, 0x34, 0x12, 0xfe, 0];
        let desc = parse_hub_descriptor(&usb3, true).unwrap();
        assert_eq!(desc.bNbrPorts, 6);
        assert_eq!(unsafe { desc.u.ss }.hub_delay(), 0x1234);
        assert!(parse_hub_descriptor(&usb3, false).is_err());
        assert!(parse_hub_descriptor(&usb3[..11], true).is_err());
    }

    #[test]
    fn superspeed_hub_power_and_link_bits_do_not_make_a_low_speed_child() {
        let status = decode_hub_port_status(0x0203, true);
        assert!(status.connected && status.enabled && status.powered);
        assert!(!status.low_speed && !status.high_speed && !status.suspended);
        assert_eq!(status.speed, Speed::SuperSpeed);
        assert!(decode_hub_port_status(0x0203 | (3 << 5), true).suspended);
        assert!(!decode_hub_port_status(0x0203 | (2 << 5), true).suspended);
        assert!(
            !decode_hub_port_status(0x0103, true).powered,
            "USB3 bit8 is not port power"
        );
        let usb2_low = decode_hub_port_status(0x0303, false);
        assert!(usb2_low.powered && usb2_low.low_speed);
        assert_eq!(usb2_low.speed, Speed::Low);
        let usb2_high = decode_hub_port_status(0x0503, false);
        assert!(usb2_high.powered && usb2_high.high_speed);
        assert_eq!(usb2_high.speed, Speed::High);
    }
}

mod external_ports {
    use super::{
        PortChangeInfo, USBError,
        hub_types::{PortFeature, PortStatus, PortStatusChange, Speed},
    };
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum PortState {
        Uninit,
        Probed,
    }
    struct Port {
        status: PortStatus,
        state: PortState,
    }
    struct Device;
    impl Device {
        fn slot_id(&self) -> u8 {
            4
        }
    }
    struct Inner {
        num_ports: u8,
        ports: Vec<Port>,
        root_port_id: u8,
        dev: Device,
    }
    struct HubDevice {
        data: Inner,
        fail_status: Option<u8>,
        fail_ack: Option<u8>,
        attempts: Vec<u8>,
    }
    impl HubDevice {
        @EXTERNAL_PORT_METHODS@
        async fn get_port_status(
            &mut self,
            port: u8,
        ) -> Result<(PortStatus, PortStatusChange), USBError> {
            self.attempts.push(port);
            if self.fail_status == Some(port) {
                return Err(USBError::Transaction);
            }
            Ok((
                status(),
                PortStatusChange {
                    connection_changed: true,
                    ..no_change()
                },
            ))
        }
        async fn clear_port_feature(&mut self, port: u8, _: PortFeature) -> Result<(), USBError> {
            if self.fail_ack == Some(port) {
                return Err(USBError::Transaction);
            }
            Ok(())
        }
        async fn handle_port_connection(
            &mut self,
            port: u8,
            _: &PortStatus,
        ) -> Result<PortChangeInfo, USBError> {
            Ok(PortChangeInfo {
                root_port_id: self.data.root_port_id,
                port_id: port,
                port_speed: Speed::High,
                tt_port_on_hub: None,
            })
        }
    }
    fn no_change() -> PortStatusChange {
        PortStatusChange {
            connection_changed: false,
            enabled_changed: false,
            reset_complete: false,
            suspend_changed: false,
            over_current_changed: false,
        }
    }
    fn status() -> PortStatus {
        PortStatus {
            connected: true,
            enabled: true,
            suspended: false,
            over_current: false,
            resetting: false,
            powered: true,
            low_speed: false,
            high_speed: true,
            speed: Speed::High,
            change: no_change(),
        }
    }
    fn fixture(fail_status: Option<u8>, fail_ack: Option<u8>) -> HubDevice {
        HubDevice {
            data: Inner {
                num_ports: 3,
                root_port_id: 6,
                dev: Device,
                ports: (0..3)
                    .map(|_| Port {
                        status: status(),
                        state: PortState::Uninit,
                    })
                    .collect(),
            },
            fail_status,
            fail_ack,
            attempts: Vec::new(),
        }
    }

    #[test]
    fn failing_external_hub_port_does_not_drop_earlier_or_later_candidates() {
        for failed in [1, 2, 3] {
            let mut hub = fixture(Some(failed), None);
            let candidates = futures::executor::block_on(hub.changed_ports()).unwrap();
            assert_eq!(hub.attempts, [1, 2, 3]);
            assert_eq!(
                candidates
                    .iter()
                    .map(|port| port.port_id)
                    .collect::<Vec<_>>(),
                (1..=3).filter(|port| *port != failed).collect::<Vec<_>>()
            );
            assert_eq!(
                hub.data.ports[(failed - 1) as usize].state,
                PortState::Uninit
            );
        }
    }

    #[test]
    fn failed_change_ack_keeps_candidate_available_for_a_later_scan() {
        let mut hub = fixture(None, Some(2));
        let candidates = futures::executor::block_on(hub.changed_ports()).unwrap();
        assert_eq!(
            candidates
                .iter()
                .map(|port| port.port_id)
                .collect::<Vec<_>>(),
            [1, 3]
        );
        assert_eq!(hub.data.ports[1].state, PortState::Uninit);
        hub.fail_ack = None;
        let candidates = futures::executor::block_on(hub.changed_ports()).unwrap();
        assert_eq!(
            candidates
                .iter()
                .map(|port| port.port_id)
                .collect::<Vec<_>>(),
            [2]
        );
        assert!(
            hub.data
                .ports
                .iter()
                .all(|port| port.state == PortState::Probed)
        );
    }
}

mod root_ports {
    use super::{PortChangeInfo, Speed, USBError};
    use std::sync::atomic::{AtomicBool, Ordering};
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum PortState {
        Uninit,
        Reseted,
        Probed,
    }
    struct Port {
        port_id: u8,
        changed: AtomicBool,
        state: PortState,
        ever_probed: bool,
    }
    #[derive(Clone, Copy, Debug)]
    struct Portsc(u32);
    impl Portsc {
        fn current_connect_status(self) -> bool {
            self.0 & 1 != 0
        }
        fn port_enabled_disabled(self) -> bool {
            self.0 & 2 != 0
        }
        fn port_reset(self) -> bool {
            self.0 & (1 << 4) != 0
        }
        fn port_power(self) -> bool {
            self.0 & (1 << 9) != 0
        }
        fn set_port_power(&mut self) {
            self.0 |= 1 << 9;
        }
        fn set_0_port_enabled_disabled(&mut self) {
            self.0 &= !2;
        }
        fn set_port_reset(&mut self) {
            self.0 |= 1 << 4;
        }
        fn connect_status_change(self) -> bool {
            self.0 & (1 << 17) != 0
        }
        fn port_speed(self) -> u8 {
            ((self.0 >> 10) & 15) as u8
        }
    }
    struct PortRegister {
        portsc: Portsc,
    }
    struct RegisterSet(Vec<u32>);
    fn apply_portsc_write(before: u32, requested: u32) -> u32 {
        let mut after = before;
        if requested & 2 != 0 {
            after &= !2;
        }
        after &= !(requested & XhciRootHub::PORT_CHANGE_MASK);
        after = (after & !NeutralWriter::PORTSC_RWS_MASK)
            | (requested & NeutralWriter::PORTSC_RWS_MASK);
        after | (requested & ((1 << 4) | (1 << 31)))
    }
    impl RegisterSet {
        fn len(&self) -> usize {
            self.0.len()
        }
        fn read_volatile_at(&self, idx: usize) -> PortRegister {
            PortRegister {
                portsc: Portsc(self.0[idx]),
            }
        }
        fn update_volatile_at(&mut self, idx: usize, update: impl FnOnce(&mut PortRegister)) {
            let before = self.0[idx];
            let mut register = PortRegister {
                portsc: Portsc(before),
            };
            update(&mut register);
            self.0[idx] = apply_portsc_write(before, register.portsc.0);
        }
    }
    struct CapabilityLength;
    impl CapabilityLength {
        fn read_volatile(&self) -> Self {
            Self
        }
        fn get(self) -> u8 {
            0
        }
    }
    struct Capability {
        caplength: CapabilityLength,
    }
    struct NeutralWriter {
        mmio_base: usize,
        capability: Capability,
        port_register_set: RegisterSet,
    }
    fn mb() {}
    impl NeutralWriter {
        @NEUTRAL_PORTSC_WRITE@
    }
    struct Registers {
        port_register_set: RegisterSet,
    }
    struct XhciRootHub {
        ports: Vec<Port>,
        reg: Registers,
        resets: Vec<u8>,
        actions: Vec<(u8, u32, u32)>,
        writes: Vec<(u8, &'static str, u32)>,
        phases: Vec<(&'static str, Vec<u32>)>,
    }
    impl XhciRootHub {
        const PORT_CHANGE_MASK: u32 = 0x7f << 17;
        const PORT_PP: u32 = 1 << 9;
        const PORT_PR: u32 = 1 << 4;
        fn ports(&self) -> &[Port] {
            &self.ports
        }
        fn ports_mut(&mut self) -> &mut [Port] {
            &mut self.ports
        }
        fn root_port_ignored_by_physics(&self, _: u8) -> bool {
            false
        }
        fn log_port_status(&self, _: &str, _: u8) {}
        fn log_status_mut(&mut self, stage: &'static str) {
            self.phases
                .push((stage, self.reg.port_register_set.0.clone()));
        }
        fn fail_if_halted(&self, _: &str) -> Result<(), USBError> {
            Ok(())
        }
        fn request_root_port_reset(&mut self, port: u8) -> Result<(), USBError> {
            self.resets.push(port);
            self.reg.port_register_set.0[(port - 1) as usize] |= 1 << 4;
            Ok(())
        }
        fn write_portsc_action(
            &mut self,
            idx: usize,
            port: u8,
            action: &'static str,
            set: u32,
            ack: u32,
        ) -> Result<(), USBError> {
            self.actions.push((port, set, ack));
            let before = self.reg.port_register_set.0[idx];
            // Run the actual register writer against ordinary host memory.
            // Then apply register semantics: writing PED=1 disables the port,
            // change bits acknowledge only on 1, PP/PLS are ordinary RWS bits.
            let mut aperture = [0u32; 0x104];
            aperture[0x100] = before;
            let mut writer = NeutralWriter {
                mmio_base: aperture.as_mut_ptr() as usize,
                capability: Capability {
                    caplength: CapabilityLength,
                },
                port_register_set: RegisterSet(vec![before]),
            };
            let (_, requested, _) = writer.write_portsc_neutral(0, set, ack).unwrap();
            self.writes.push((port, action, requested));
            self.reg.port_register_set.0[idx] = apply_portsc_write(before, requested);
            Ok(())
        }
        @ROOT_PORT_METHODS@
    }
    fn fixture(raw: u32, state: PortState, ever_probed: bool) -> XhciRootHub {
        XhciRootHub {
            ports: vec![Port {
                port_id: 1,
                changed: AtomicBool::new(true),
                state,
                ever_probed,
            }],
            reg: Registers {
                port_register_set: RegisterSet(vec![raw]),
            },
            resets: Vec::new(),
            actions: Vec::new(),
            writes: Vec::new(),
            phases: Vec::new(),
        }
    }

    #[test]
    fn full_reset_does_not_disable_an_already_powered_superspeed_link_during_power_phase() {
        let before = 3 | (1 << 9) | (4 << 10) | (1 << 17) | (1 << 21);
        let mut hub = fixture(before, PortState::Uninit, false);
        hub.init_emulated_full_reset_ports().unwrap();
        let power_phase = hub
            .phases
            .iter()
            .find(|(stage, _)| *stage == "root-hub-after-port-power")
            .unwrap();
        assert_eq!(
            power_phase.1,
            [before],
            "power phase must preserve the inherited enabled link and pending changes"
        );
        assert_eq!(
            hub.writes.len(),
            1,
            "powered ports need only the deliberate reset request"
        );
        assert_eq!(hub.writes[0].1, "init-reset");
        let request = hub.writes[0].2;
        assert_eq!(
            request & ((1 << 1) | (1 << 31) | XhciRootHub::PORT_CHANGE_MASK),
            0,
            "reset must not replay PED, WPR, or change acknowledgements"
        );
        assert_ne!(request & (1 << 4), 0);
        assert_ne!(request & (1 << 9), 0);
    }

    #[test]
    fn full_reset_neutral_power_write_does_not_replay_rw1c_fields() {
        let before = 3 | (4 << 10) | (1 << 17) | (1 << 21);
        let mut hub = fixture(before, PortState::Uninit, false);
        hub.init_emulated_full_reset_ports().unwrap();
        let power_phase = hub
            .phases
            .iter()
            .find(|(stage, _)| *stage == "root-hub-after-port-power")
            .unwrap();
        assert_eq!(power_phase.1, [before | (1 << 9)]);
        assert_eq!(
            hub.writes
                .iter()
                .map(|(_, action, _)| *action)
                .collect::<Vec<_>>(),
            ["init-power-on", "init-reset"]
        );
        for (_, _, request) in &hub.writes {
            assert_eq!(
                request & ((1 << 1) | (1 << 31) | XhciRootHub::PORT_CHANGE_MASK),
                0
            );
            assert_ne!(request & (1 << 9), 0);
        }
        assert_eq!(
            hub.writes[0].2 & (1 << 4),
            0,
            "power phase does not assert reset"
        );
        assert_ne!(hub.writes[1].2 & (1 << 4), 0);
    }

    #[test]
    fn owned_root_port_survives_disconnect_and_reconnect_without_rearm() {
        // Actual disconnect, still-connected disabled, reset completion, and
        // reconnect CSC all leave the original slot's owner unretired.
        for raw in [1 << 17, 1 | (1 << 17), 3 | (1 << 21), 3 | (1 << 17)] {
            let mut hub = fixture(raw, PortState::Probed, true);
            hub.consume_port_change_edges().unwrap();
            assert_eq!(hub.ports[0].state, PortState::Probed);
            assert!(!hub.ports[0].changed.load(Ordering::Acquire));
            futures::executor::block_on(hub.handle_uninit()).unwrap();
            assert!(
                futures::executor::block_on(hub.handle_reseted())
                    .unwrap()
                    .is_empty()
            );
            assert!(hub.resets.is_empty());
            assert_eq!(
                hub.actions,
                [(1, 0, XhciRootHub::PORT_CHANGE_MASK)],
                "only acknowledge the edge"
            );
            // A stale software state cannot bypass the same ownership guard.
            hub.ports[0].state = PortState::Uninit;
            futures::executor::block_on(hub.handle_uninit()).unwrap();
            assert!(hub.resets.is_empty());
            hub.ports[0].state = PortState::Reseted;
            assert!(
                futures::executor::block_on(hub.handle_reseted())
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[test]
    fn untouched_root_port_still_resets_and_is_emitted_once() {
        let mut hub = fixture(1 | (3 << 10) | (1 << 17), PortState::Uninit, false);
        hub.consume_port_change_edges().unwrap();
        futures::executor::block_on(hub.handle_uninit()).unwrap();
        assert_eq!(hub.resets, [1]);
        hub.reg.port_register_set.0[0] = 3 | (3 << 10); // hardware reset completed
        futures::executor::block_on(hub.handle_uninit()).unwrap();
        let ports = futures::executor::block_on(hub.handle_reseted()).unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!((ports[0].root_port_id, ports[0].port_id), (1, 1));
        assert_eq!(ports[0].port_speed, Speed::High);
        assert_eq!(hub.ports[0].state, PortState::Probed);
        assert!(hub.ports[0].ever_probed);
        assert!(
            futures::executor::block_on(hub.handle_reseted())
                .unwrap()
                .is_empty()
        );
    }
}
