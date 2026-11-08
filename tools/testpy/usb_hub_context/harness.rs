#![allow(dead_code, unused_macros)]
extern crate alloc;

use anyhow::anyhow;
use std::sync::{Arc, Mutex};
use usb_if::{
    err::{TransferError, USBError},
    host::{
        ControlSetup,
        hub::{HubDescriptor, Speed},
    },
    transfer::{Recipient, Request, RequestType},
};
use xhci::{
    context::{Input32Byte, InputHandler},
    ring::trb::command,
};

type Result<T = (), E = USBError> = core::result::Result<T, E>;
macro_rules! debug { ($($args:tt)*) => {{ let _ = format_args!($($args)*); }} }
macro_rules! info { ($($args:tt)*) => {{ let _ = format_args!($($args)*); }} }
macro_rules! trace { ($($args:tt)*) => {{ let _ = format_args!($($args)*); }} }
macro_rules! warn { ($($args:tt)*) => {{ let _ = format_args!($($args)*); }} }
fn mb() {}

#[derive(Clone, Debug)]
@HUB_PARAMS@
mod backend {
    pub mod ty {
        pub use crate::HubParams;
    }
}

#[derive(Clone, Copy)]
struct SlotId(u8);
impl SlotId {
    fn as_u8(self) -> u8 {
        self.0
    }
}
impl From<SlotId> for u8 {
    fn from(slot: SlotId) -> Self {
        slot.0
    }
}
#[derive(Debug)]
enum Event {
    Command {
        command: command::Allowed,
        input: Input32Byte,
    },
    SetInterface {
        interface: u16,
        alternate: u16,
        accepted: bool,
    },
    EndpointClaim {
        interface: u8,
        alternate: u8,
    },
    HubPower,
}
struct ContextData {
    input: Arc<Mutex<Input32Byte>>,
}
impl ContextData {
    fn with_input(&mut self, f: impl FnOnce(&mut dyn InputHandler)) {
        f(&mut *self.input.lock().unwrap());
    }
    fn input_bus_addr(&self) -> u64 {
        0x1000
    }
    @CONTEXT_CHANGE@
}
struct CommandRing {
    input: Arc<Mutex<Input32Byte>>,
    events: Arc<Mutex<Vec<Event>>>,
}
impl CommandRing {
    async fn cmd_request(&self, command: command::Allowed) -> Result {
        self.events.lock().unwrap().push(Event::Command {
            command,
            input: *self.input.lock().unwrap(),
        });
        Ok(())
    }
}
struct XhciDevice {
    id: SlotId,
    ctx: ContextData,
    cmd: CommandRing,
    port_speed: Speed,
    xhci_version: u16,
}
impl XhciDevice {
    @UPDATE_HUB@
    @EVALUATE@
}
@TT_ENCODE@
@TT_DECODE@

#[derive(Clone, Copy)]
enum SelectionFault {
    Unsupported,
    Stall,
    Timeout,
}
struct Endpoint {
    events: Arc<Mutex<Vec<Event>>>,
    selection_fault: Option<SelectionFault>,
}
impl Endpoint {
    async fn control_out(
        &mut self,
        setup: ControlSetup,
        data: &[u8],
    ) -> Result<usize, TransferError> {
        assert!(matches!(setup.request_type, RequestType::Standard));
        assert!(matches!(setup.recipient, Recipient::Interface));
        assert!(matches!(setup.request, Request::SetInterface));
        assert!(data.is_empty());
        self.events.lock().unwrap().push(Event::SetInterface {
            interface: setup.index,
            alternate: setup.value,
            accepted: self.selection_fault.is_none(),
        });
        match self.selection_fault {
            Some(SelectionFault::Unsupported) => return Err(TransferError::NotSupported),
            Some(SelectionFault::Stall) => return Err(TransferError::Stall),
            Some(SelectionFault::Timeout) => return Err(TransferError::Timeout),
            None => {}
        }
        Ok(0)
    }
}
struct DeviceDescriptor {
    protocol: u8,
}
struct Device {
    native: XhciDevice,
    ctrl: Endpoint,
    desc: DeviceDescriptor,
}
impl Device {
    fn descriptor(&self) -> &DeviceDescriptor {
        &self.desc
    }
    fn ctrl_ep_mut(&mut self) -> &mut Endpoint {
        &mut self.ctrl
    }
    async fn update_hub(&mut self, params: HubParams) -> Result {
        self.native.update_hub_inner(params).await
    }
    async fn claim_interface(&mut self, interface: u8, alternate: u8) -> Result {
        // Claiming a USB interface includes xHCI endpoint admission. This is
        // intentionally distinct from sending only USB SET_INTERFACE on EP0.
        self.native
            .cmd
            .events
            .lock()
            .unwrap()
            .push(Event::EndpointClaim {
                interface,
                alternate,
            });
        self.native.ctx.perper_change();
        self.ctrl
            .control_out(
                ControlSetup {
                    request_type: RequestType::Standard,
                    recipient: Recipient::Interface,
                    request: Request::SetInterface,
                    value: u16::from(alternate),
                    index: u16::from(interface),
                },
                &[],
            )
            .await?;
        self.native.ctx.with_input(|input| {
            input.control_mut().set_add_context_flag(3);
            input.device_mut().slot_mut().set_context_entries(4);
        });
        self.native
            .cmd
            .cmd_request(command::Allowed::ConfigureEndpoint(
                *command::ConfigureEndpoint::default()
                    .set_slot_id(self.native.id.into())
                    .set_input_context_pointer(self.native.ctx.input_bus_addr()),
            ))
            .await
    }
}

#[derive(Clone, Copy, Debug)]
struct UsbTt {
    multi: bool,
    think_time_ns: usize,
}
#[derive(Clone, Copy, Debug)]
struct HubInfo {
    hub_depth: isize,
    speed: Speed,
    tt: UsbTt,
}
struct HubSettings {
    config_value: u8,
    interface_number: u8,
    alt_setting: u8,
}
#[derive(Debug, PartialEq)]
enum HubState {
    Uninitialized,
    Running,
}
struct Port;
impl Port {
    fn new(_: u8) -> Self {
        Self
    }
}
struct Inner {
    descriptor: HubDescriptor,
    num_ports: u8,
    dev: Device,
    parent_hub_slot_id: u8,
    root_port_id: u8,
    ports: Vec<Port>,
    state: HubState,
}
struct HubStatus;
impl HubStatus {
    fn local_power_source(&self) -> bool {
        false
    }
    fn over_current(&self) -> bool {
        false
    }
}
struct HubDevice {
    settings: HubSettings,
    data: Inner,
}
const HUB_PR_FS: u8 = 0;
const HUB_PR_HS_SINGLE_TT: u8 = 1;
const HUB_PR_HS_MULTI_TT: u8 = 2;
const HUB_PR_SS: u8 = 3;
impl HubDevice {
    @CONFIGURE_HUB@
    async fn get_hub_descriptor(&mut self) -> Result<HubDescriptor> {
        Ok(self.data.descriptor)
    }
    fn hub_descriptor(&self) -> &HubDescriptor {
        &self.data.descriptor
    }
    async fn get_hub_status(&mut self) -> Result<HubStatus> {
        Ok(HubStatus)
    }
    fn is_superspeed(&self) -> bool {
        self.data.dev.desc.protocol == 3
    }
    async fn set_hub_depth(&mut self, _: u8) -> Result {
        Ok(())
    }
    async fn hub_power_on(&mut self) -> Result {
        self.data
            .dev
            .native
            .cmd
            .events
            .lock()
            .unwrap()
            .push(Event::HubPower);
        Ok(())
    }
}

fn fixture(
    version: u16,
    protocol: u8,
    think_bits: u8,
    accept_multi_tt: bool,
) -> (HubDevice, Arc<Mutex<Vec<Event>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let input = Arc::new(Mutex::new(Input32Byte::new_32byte()));
    input
        .lock()
        .unwrap()
        .device_mut()
        .slot_mut()
        .set_context_entries(1);
    let ctx = ContextData {
        input: input.clone(),
    };
    let cmd = CommandRing {
        input,
        events: events.clone(),
    };
    let port_speed = match protocol {
        0 => Speed::Full,
        3 => Speed::SuperSpeed,
        _ => Speed::High,
    };
    let native = XhciDevice {
        id: SlotId(4),
        ctx,
        cmd,
        port_speed,
        xhci_version: version,
    };
    let mut bytes = [0u8; core::mem::size_of::<HubDescriptor>()];
    bytes[..7].copy_from_slice(&[9, 0x29, 4, (think_bits & 3) << 5, 0, 50, 0]);
    let descriptor = unsafe { core::ptr::read_unaligned(bytes.as_ptr().cast::<HubDescriptor>()) };
    let hub = HubDevice {
        settings: HubSettings {
            config_value: 1,
            interface_number: 4,
            alt_setting: 0,
        },
        data: Inner {
            descriptor,
            num_ports: 0,
            dev: Device {
                native,
                ctrl: Endpoint {
                    events: events.clone(),
                    selection_fault: if accept_multi_tt {
                        None
                    } else {
                        Some(SelectionFault::Unsupported)
                    },
                },
                desc: DeviceDescriptor { protocol },
            },
            parent_hub_slot_id: 0,
            root_port_id: 6,
            ports: Vec::new(),
            state: HubState::Uninitialized,
        },
    };
    (hub, events)
}
fn info() -> HubInfo {
    HubInfo {
        hub_depth: 0,
        speed: Speed::High,
        tt: UsbTt {
            multi: false,
            think_time_ns: 0,
        },
    }
}
fn commands(events: &[Event]) -> Vec<(&command::Allowed, &Input32Byte)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Command { command, input } => Some((command, input)),
            _ => None,
        })
        .collect()
}
fn assert_hub_context(input: &Input32Byte, multi: bool, think: u8) {
    let slot = input.device().slot();
    assert!(slot.hub());
    assert_eq!(slot.multi_tt(), multi);
    assert_eq!(slot.number_of_ports(), 4);
    assert_eq!(slot.tt_think_time(), think);
    assert_eq!(
        input.control().as_ref()[1],
        1,
        "hub metadata command adds only Slot Context A0"
    );
    assert_eq!(
        input.control().as_ref()[0],
        0,
        "hub metadata command drops no endpoint context"
    );
}

#[test]
fn modern_hub_context_uses_configure_endpoint_with_complete_slot_metadata() {
    for version in [0x96, 0x100, 0x110, 0x120] {
        for think in 0..4 {
            let (mut hub, events) = fixture(version, HUB_PR_HS_SINGLE_TT, think, true);
            let result = futures::executor::block_on(hub.configure(info())).unwrap();
            assert_eq!(result.tt.think_time_ns, 666 * (usize::from(think) + 1));
            let events = events.lock().unwrap();
            let commands = commands(&events);
            assert_eq!(commands.len(), 1);
            match commands[0].0 {
                command::Allowed::ConfigureEndpoint(command) => {
                    assert_eq!(command.slot_id(), 4);
                    assert_eq!(command.input_context_pointer(), 0x1000);
                    assert!(!command.deconfigure());
                }
                other => panic!("modern hub metadata requires ConfigureEndpoint, got {other:?}"),
            }
            assert_hub_context(commands[0].1, false, think);
            assert_eq!(hub.data.state, HubState::Running);
        }
    }
}

#[test]
fn multi_tt_selection_does_not_admit_endpoint_contexts_before_hub_metadata() {
    let (mut hub, events) = fixture(0x110, HUB_PR_HS_MULTI_TT, 3, true);
    let result = futures::executor::block_on(hub.configure(info())).unwrap();
    assert!(result.tt.multi);
    let events = events.lock().unwrap();
    assert!(matches!(
        events[0],
        Event::SetInterface {
            interface: 4,
            alternate: 1,
            accepted: true
        }
    ));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::EndpointClaim { .. }))
    );
    let commands = commands(&events);
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        commands[0].0,
        command::Allowed::ConfigureEndpoint(_)
    ));
    assert_hub_context(commands[0].1, true, 3);
}

#[test]
fn refused_multi_tt_selection_keeps_single_tt_slot_metadata() {
    let (mut hub, events) = fixture(0x110, HUB_PR_HS_MULTI_TT, 2, false);
    // Prove that falling back to single-TT also clears stale MTT state.
    hub.data.dev.native.ctx.with_input(|input| {
        input.device_mut().slot_mut().set_multi_tt();
    });
    let result = futures::executor::block_on(hub.configure(info())).unwrap();
    assert!(!result.tt.multi);
    let events = events.lock().unwrap();
    let commands = commands(&events);
    assert_eq!(commands.len(), 1);
    assert_hub_context(commands[0].1, false, 2);
}

#[test]
fn failed_multi_tt_control_transfer_stops_before_host_context_admission() {
    for fault in [SelectionFault::Stall, SelectionFault::Timeout] {
        let (mut hub, events) = fixture(0x110, HUB_PR_HS_MULTI_TT, 2, true);
        hub.data.dev.ctrl.selection_fault = Some(fault);
        let error = futures::executor::block_on(hub.configure(info())).unwrap_err();
        assert!(error.to_string().contains("SET_INTERFACE"));
        let events = events.lock().unwrap();
        assert!(
            commands(&events).is_empty(),
            "unrecovered EP0 failure must not be treated as single-TT success"
        );
        assert!(!events.iter().any(|event| matches!(event, Event::HubPower)));
        assert_eq!(hub.data.state, HubState::Uninitialized);
    }
}

#[test]
fn legacy_095_hub_metadata_still_uses_evaluate_context() {
    let (mut hub, events) = fixture(0x95, HUB_PR_HS_SINGLE_TT, 1, true);
    futures::executor::block_on(hub.configure(info())).unwrap();
    let events = events.lock().unwrap();
    let commands = commands(&events);
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        commands[0].0,
        command::Allowed::EvaluateContext(_)
    ));
    assert_hub_context(commands[0].1, false, 1);
}

#[test]
fn ordinary_ep0_evaluation_remains_evaluate_context_on_modern_controllers() {
    let (mut hub, events) = fixture(0x110, HUB_PR_HS_SINGLE_TT, 0, true);
    hub.data.dev.native.ctx.perper_change();
    hub.data.dev.native.ctx.with_input(|input| {
        input.control_mut().set_add_context_flag(1);
        input.device_mut().endpoint_mut(1).set_max_packet_size(64);
    });
    futures::executor::block_on(hub.data.dev.native.evaluate()).unwrap();
    let events = events.lock().unwrap();
    let commands = commands(&events);
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        commands[0].0,
        command::Allowed::EvaluateContext(_)
    ));
    assert_eq!(commands[0].1.device().endpoint(1).max_packet_size(), 64);
    assert_eq!(commands[0].1.control().as_ref()[1], 3);
}

#[test]
fn transaction_translator_think_time_roundtrips_every_wire_encoding() {
    for bits in 0..4u16 {
        let nanoseconds = hub_tt_think_time_ns((bits << 5) | 0x801f);
        assert_eq!(nanoseconds, 666 * (usize::from(bits) + 1));
        assert_eq!(xhci_tt_think_time(nanoseconds as u16), bits as u8);
    }
}

#[test]
fn full_speed_and_superspeed_hubs_clear_stale_tt_fields() {
    for protocol in [HUB_PR_FS, HUB_PR_SS] {
        let (mut hub, events) = fixture(0x110, protocol, 3, true);
        hub.data.dev.native.ctx.with_input(|input| {
            let slot = input.device_mut().slot_mut();
            slot.set_multi_tt();
            slot.set_tt_think_time(3);
        });
        let result = futures::executor::block_on(hub.configure(info())).unwrap();
        assert_eq!(result.tt.think_time_ns, 0);
        assert!(!result.tt.multi);
        let events = events.lock().unwrap();
        assert_hub_context(commands(&events)[0].1, false, 0);
    }
}

#[test]
fn subsequent_endpoint_configuration_retains_the_committed_hub_metadata() {
    let (mut hub, events) = fixture(0x110, HUB_PR_HS_MULTI_TT, 2, true);
    futures::executor::block_on(hub.configure(info())).unwrap();
    futures::executor::block_on(hub.data.dev.claim_interface(4, 1)).unwrap();
    let events = events.lock().unwrap();
    let commands = commands(&events);
    assert_eq!(commands.len(), 2);
    assert_hub_context(commands[0].1, true, 2);
    let later = commands[1].1;
    assert!(later.device().slot().hub());
    assert!(later.device().slot().multi_tt());
    assert_eq!(later.device().slot().number_of_ports(), 4);
    assert_eq!(later.device().slot().tt_think_time(), 2);
    assert_eq!(later.control().as_ref()[1], 1 | (1 << 3));
}
