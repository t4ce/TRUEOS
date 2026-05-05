use alloc::collections::BTreeMap;

use alloc::{sync::Arc, vec::Vec};
use core::fmt::Debug;
use core::time::Duration;

use futures::{FutureExt, future::BoxFuture};
use mbarrier::mb;
use spin::Mutex;
use usb_if::descriptor::DeviceDescriptorBase;
use usb_if::err::USBError;
use usb_if::{
    descriptor::{
        ConfigurationDescriptor, DescriptorType, DeviceDescriptor, EndpointDescriptor,
        EndpointType, InterfaceDescriptor,
    },
    host::{ControlSetup, hub::Speed},
    transfer::{Recipient, RequestType},
};
use xhci::ring::trb::command;

use super::{
    SlotId, Xhci,
    cmd::CommandRing,
    context::ContextData,
    endpoint::{Endpoint, EndpointDescriptorExt},
    parse_default_max_packet_size_from_port_speed,
    reg::SlotBell,
    transfer::TransferResultHandler,
};
use crate::DeviceAddressInfo;
use crate::backend::ty::HubParams;

use crate::osal::Kernel;
use crate::{
    backend::{
        Dci,
        ty::{
            DeviceOp,
            ep::{EndpointBase, EndpointControl},
        },
    },
    debug_record_stream_config,
    err::Result,
};

pub struct Device {
    id: SlotId,
    ctx: ContextData,
    desc: DeviceDescriptor,
    ctrl_ep: Option<EndpointControl>,
    transfer_result_handler: TransferResultHandler,
    bell: Arc<Mutex<SlotBell>>,
    kernel: Kernel,
    current_config_value: Option<u8>,
    config_desc: Vec<ConfigurationDescriptor>,
    port_speed: Speed,
    root_port_id: u8,
    port_id: u8,
    eps: BTreeMap<Dci, EndpointBase>,
    cmd: CommandRing,
}

#[derive(Clone, Copy)]
enum CommandLifecycleStage {
    AddressDevice,
    EvaluateContext,
    GetDeviceDescriptorBase,
    GetConfiguration,
    ReadDescriptor,
    GetConfigurationDescriptor,
    SetConfiguration,
    ConfigureEndpoint,
    ClaimInterface,
    ResetEndpoint,
    SetTrDequeuePointer,
    DisableSlot,
}

impl CommandLifecycleStage {
    fn name(self) -> &'static str {
        match self {
            Self::AddressDevice => "address-device",
            Self::EvaluateContext => "evaluate-context",
            Self::GetDeviceDescriptorBase => "get-device-descriptor-base",
            Self::GetConfiguration => "get-configuration",
            Self::ReadDescriptor => "read-descriptor",
            Self::GetConfigurationDescriptor => "get-configuration-descriptor",
            Self::SetConfiguration => "set-configuration",
            Self::ConfigureEndpoint => "configure-endpoint",
            Self::ClaimInterface => "claim-interface",
            Self::ResetEndpoint => "reset-endpoint",
            Self::SetTrDequeuePointer => "set-tr-dequeue-pointer",
            Self::DisableSlot => "disable-slot",
        }
    }

    fn progress(self) -> u32 {
        match self {
            Self::AddressDevice => 41,
            Self::EvaluateContext => 42,
            Self::GetDeviceDescriptorBase => 43,
            Self::GetConfiguration => 44,
            Self::ReadDescriptor => 45,
            Self::GetConfigurationDescriptor => 46,
            Self::SetConfiguration => 47,
            Self::ConfigureEndpoint => 48,
            Self::ClaimInterface => 49,
            Self::ResetEndpoint => 50,
            Self::SetTrDequeuePointer => 51,
            Self::DisableSlot => 52,
        }
    }
}

struct CommandLifecycle {
    slot_id: u8,
    root_port_id: u8,
    port_id: u8,
}

impl CommandLifecycle {
    fn new(slot_id: u8, root_port_id: u8, port_id: u8) -> Self {
        Self {
            slot_id,
            root_port_id,
            port_id,
        }
    }

    fn begin(&self, stage: CommandLifecycleStage, detail: u32) {
        crate::debug_set_usb_probe_progress(
            stage.progress(),
            self.root_port_id,
            self.port_id,
            self.slot_id,
            detail,
        );
        info!(
            "xhci lifecycle: slot={} root_port={} port={} stage={} begin detail={}\n",
            self.slot_id,
            self.root_port_id,
            self.port_id,
            stage.name(),
            detail,
        );
    }

    fn ok(&self, stage: CommandLifecycleStage, detail: u32) {
        crate::debug_set_usb_probe_progress(
            stage.progress(),
            self.root_port_id,
            self.port_id,
            self.slot_id,
            detail,
        );
        info!(
            "xhci lifecycle: slot={} root_port={} port={} stage={} ok detail={}\n",
            self.slot_id,
            self.root_port_id,
            self.port_id,
            stage.name(),
            detail,
        );
    }

    fn fail(&self, stage: CommandLifecycleStage, err: &(dyn Debug + Send + Sync)) {
        info!(
            "xhci lifecycle: slot={} root_port={} port={} stage={} failed: {:?}\n",
            self.slot_id,
            self.root_port_id,
            self.port_id,
            stage.name(),
            err,
        );
    }
}

impl Device {
    const LS_FS_ADDRESS_DEVICE_SETTLE_MS: u64 = 10;
    const LS_FS_EP0_REEVALUATE_SETTLE_MS: u64 = 2;

    pub(crate) async fn new(host: &mut Xhci) -> Result<Self> {
        let slot_id = host.device_slot_assignment().await?;
        crate::debug_set_usb_probe_progress(4, 0, 0, slot_id.as_u8(), 0);
        debug!("Slot {slot_id} assigned");
        let is_64 = host.is_64bit_ctx();
        debug!(
            "Creating new context for slot {slot_id}, {}",
            if is_64 { "64-bit" } else { "32-bit" }
        );
        let dma = host.kernel.clone();
        let ctx = host.dev_mut()?.new_ctx(slot_id, is_64, &dma)?;
        let bell = host.new_slot_bell(slot_id);
        let bell = Arc::new(Mutex::new(bell));
        // let port_speed = host.port_speed(port);
        let desc = unsafe { core::mem::zeroed() };

        Ok(Self {
            id: slot_id,
            ctx,
            bell,
            ctrl_ep: None,
            desc,
            kernel: dma,
            transfer_result_handler: host.transfer_result_handler.clone(),
            current_config_value: None,
            config_desc: vec![],
            port_speed: Speed::Full,
            root_port_id: 0,
            port_id: 0,
            eps: BTreeMap::new(),
            cmd: host.cmd.clone(),
        })
    }

    fn new_ep(&mut self, dci: Dci) -> Result<Endpoint> {
        let ep = Endpoint::new(self.id.as_u8(), dci, &self.kernel, self.bell.clone())?;
        self.transfer_result_handler
            .register_queue(self.id.as_u8(), dci.as_u8(), ep.ring());

        Ok(ep)
    }

    fn lifecycle(&self) -> CommandLifecycle {
        CommandLifecycle::new(self.id.as_u8(), self.root_port_id, self.port_id)
    }

    fn control_recovery_delay_ms(&self, short_ms: u64) -> u64 {
        if matches!(self.port_speed, Speed::Low | Speed::Full) {
            short_ms
        } else {
            0
        }
    }

    async fn abort_command_lifecycle(
        &mut self,
        lifecycle: &CommandLifecycle,
        stage: CommandLifecycleStage,
        err: &(dyn Debug + Send + Sync),
    ) {
        lifecycle.fail(stage, err);
        if let Err(close_err) = self.debug_close_slot_inner().await {
            info!(
                "xhci lifecycle: slot={} root_port={} port={} cleanup after {} failed: {:?}\n",
                lifecycle.slot_id,
                lifecycle.root_port_id,
                lifecycle.port_id,
                stage.name(),
                close_err
            );
        }
    }

    pub(crate) async fn init(&mut self, host: &mut Xhci, info: &DeviceAddressInfo) -> Result {
        info!(
            "crabusb/xhci/device: init begin slot={} root_port={} port={} speed={:?}",
            self.id.as_u8(),
            info.root_port_id,
            info.port_id,
            info.port_speed
        );
        self.root_port_id = info.root_port_id;
        self.port_id = info.port_id;
        // Keep the raw PORTSC.PortSpeed encoding for interval calculations
        self.port_speed = info.port_speed;
        // let speed = info.port_speed.to_xhci_portsc_value();

        let ep = self.new_ep(Dci::CTRL)?;
        self.ctrl_ep = Some(EndpointControl::new(ep));
        let lifecycle = self.lifecycle();
        self.address(host, info, &lifecycle).await?;
        // self.dump_device_out();
        let base = self.get_device_descriptor_base(info, &lifecycle).await?;
        debug!("Device Descriptor Base: {:#x?}", base);

        self.setup_max_packet(base, info, &lifecycle).await?;

        // 读取当前配置（应该返回 0，表示未配置）
        let current_config = self.get_configuration(info, &lifecycle).await?;
        debug!("Current configuration value: {}", current_config);

        self.read_descriptor(info, &lifecycle).await?;

        // 读取所有配置描述符
        for i in 0..self.desc.num_configurations {
            let config_desc = self
                .get_configuration_descriptor(i, info, &lifecycle)
                .await?;
            self.config_desc.push(config_desc);
        }

        // 设置配置为第一个配置（大多数设备只有一个配置）
        // 参考 USB 2.0 规范第 9.1.1 节和 u-boot 的 usb_set_configure_device
        if !self.config_desc.is_empty() {
            let config_value = self.config_desc[0].configuration_value;
            debug!("Setting device configuration to {}", config_value);
            self.set_configuration_with_lifecycle(config_value, info, &lifecycle)
                .await?;
        }

        info!(
            "crabusb/xhci/device: init end slot={} vid={:04x} pid={:04x} class={:02x} subclass={:02x} proto={:02x}",
            self.id.as_u8(),
            self.desc.vendor_id,
            self.desc.product_id,
            self.desc.class,
            self.desc.subclass,
            self.desc.protocol
        );
        lifecycle.ok(
            CommandLifecycleStage::ConfigureEndpoint,
            self.desc.num_configurations as u32,
        );
        Ok(())
    }

    async fn evaluate(&mut self) -> Result {
        let lifecycle = self.lifecycle();
        mb();
        debug!("Evaluating context for slot {}", self.id.as_u8());
        lifecycle.begin(CommandLifecycleStage::EvaluateContext, self.id.as_u8() as u32);
        let result = self
            .cmd
            .cmd_request(command::Allowed::EvaluateContext(
                *command::EvaluateContext::default()
                    .set_slot_id(self.id.into())
                    .set_input_context_pointer(self.ctx.input_bus_addr()),
            ))
            .await;
        match result {
            Ok(_) => {
                lifecycle.ok(CommandLifecycleStage::EvaluateContext, self.id.as_u8() as u32);
            }
            Err(err) => {
                lifecycle.fail(CommandLifecycleStage::EvaluateContext, &err);
                return Err(err.into());
            }
        }
        debug!("Evaluate context ok");
        Ok(())
    }

    async fn setup_max_packet(
        &mut self,
        desc: DeviceDescriptorBase,
        info: &DeviceAddressInfo,
        _lifecycle: &CommandLifecycle,
    ) -> Result {
        let lifecycle = self.lifecycle();
        // USB 3.x: bMaxPacketSize0 is a power-of-2 exponent (e.g. 9 → 2^9 = 512).
        // USB 2.0 and below: bMaxPacketSize0 is the literal byte count.
        let is_ss = matches!(info.port_speed, Speed::SuperSpeed | Speed::SuperSpeedPlus);
        let packet_size: u16 = if desc.max_packet_size_0 == 0 {
            8
        } else if is_ss && desc.max_packet_size_0 <= 15 {
            1u16 << desc.max_packet_size_0
        } else {
            desc.max_packet_size_0 as u16
        };

        let current_packet_size = parse_default_max_packet_size_from_port_speed(info.port_speed);

        if packet_size == current_packet_size {
            info!(
                "crabusb/xhci/device: slot={} root_port={} port={} skipping evaluate-context for ep0 mps {} (already programmed)",
                self.id.as_u8(),
                self.root_port_id,
                self.port_id,
                packet_size
            );
            return Ok(());
        }

        self.ctx.perper_change();

        let dci = Dci::CTRL;
        self.ctx.with_input(|input| {
            input.control_mut().clear_add_context_flag(0);
            input.control_mut().set_add_context_flag(1);

            let endpoint = input.device_mut().endpoint_mut(dci.as_usize());
            endpoint.set_max_packet_size(packet_size);
        });

        lifecycle.begin(CommandLifecycleStage::EvaluateContext, packet_size as u32);
        if let Err(err) = self.evaluate().await {
            lifecycle.fail(CommandLifecycleStage::EvaluateContext, &err);
            self.abort_command_lifecycle(&lifecycle, CommandLifecycleStage::EvaluateContext, &err)
                .await;
            return Err(err.into());
        }
        lifecycle.ok(CommandLifecycleStage::EvaluateContext, packet_size as u32);

        let settle_ms = self.control_recovery_delay_ms(Self::LS_FS_EP0_REEVALUATE_SETTLE_MS);
        if settle_ms != 0 {
            info!(
                "crabusb/xhci/device: slot={} root_port={} port={} settling {}ms after ep0 mps update",
                self.id.as_u8(),
                self.root_port_id,
                self.port_id,
                settle_ms
            );
            self.kernel.delay(Duration::from_millis(settle_ms));
        }

        Ok(())
    }

    async fn address(
        &mut self,
        host: &mut Xhci,
        info: &DeviceAddressInfo,
        _lifecycle: &CommandLifecycle,
    ) -> Result {
        let lifecycle = self.lifecycle();
        // 直接使用 DeviceSpeed 枚举计算默认 max packet size
        let max_packet_size = parse_default_max_packet_size_from_port_speed(info.port_speed);

        // Route String 由拓扑决定（root hub 端口不计入）
        let mut route_string = 0u32;
        let mut parent_id = info.parent_hub;
        let mut port_id = info.port_id;

        while let Some(pid) = parent_id {
            let parent_hub = info.infos.get(&pid).unwrap();
            if parent_hub.hub_depth == -1 {
                break;
            }
            if port_id > 15 {
                port_id = 15;
            }
            route_string |= (port_id as u32) << (parent_hub.hub_depth * 4);
            port_id = parent_hub.port_id;
            parent_id = parent_hub.parent;
        }

        let ctrl_ring_addr = self.ep_ctrl().raw.as_raw_mut::<Endpoint>().bus_addr();
        // ctrl dci
        let dci = Dci::CTRL;
        // 1. Allocate an Input Context data structure (6.2.5) and initialize all fields to
        // ‘0’.
        self.ctx.with_empty_input(|input| {
            let control_context = input.control_mut();
            // Initialize the Input Control Context (6.2.5.1) of the Input Context by
            // setting the A0 and A1 flags to ‘1’. These flags indicate that the Slot
            // Context and the Endpoint 0 Context of the Input Context are affected by
            // the command.
            control_context.set_add_context_flag(0);
            control_context.set_add_context_flag(1);
            for i in 2..32 {
                control_context.clear_drop_context_flag(i);
            }

            // Initialize the Input Slot Context data structure (6.2.2).
            // • Root Hub Port Number = Topology defined.
            // • Route String = Topology defined. Refer to section 8.9 in the USB3 spec. Note
            // that the Route String does not include the Root Hub Port Number.
            // • Context Entries = 1.
            let slot_context = input.device_mut().slot_mut();
            slot_context.clear_multi_tt();
            slot_context.clear_hub();
            slot_context.set_route_string(route_string);
            slot_context.set_context_entries(1);
            slot_context.set_max_exit_latency(0);
            slot_context.set_root_hub_port_number(info.root_port_id);
            slot_context.set_number_of_ports(0);
            slot_context.set_parent_hub_slot_id(0);

            // TT info is only valid for LS/FS devices behind a HS hub.
            if matches!(info.port_speed, Speed::Low | Speed::Full) {
                let mut parent_id = info.parent_hub;
                let mut tt_port = info.port_id;
                let mut hs_parent = None;

                while let Some(p) = parent_id {
                    let parent_hub = info.infos.get(&p).unwrap();
                    tt_port = parent_hub.port_id;
                    if parent_hub.hub_depth == -1 {
                        break;
                    }
                    if matches!(parent_hub.speed, Speed::High) {
                        hs_parent = Some(p);
                        break;
                    }
                    parent_id = parent_hub.parent;
                }

                if let Some(hs_id) = hs_parent {
                    let parent = info.infos.get(&hs_id).unwrap();
                    let slot_id = parent.slot_id;
                    if parent.tt.multi {
                        slot_context.set_multi_tt();
                    }

                    slot_context.set_parent_hub_slot_id(slot_id);
                    slot_context.set_parent_port_number(tt_port);
                    debug!(
                        "Setting parent_port_number (TT): {}, parent_hub_slot_id: {}",
                        tt_port, slot_id
                    );
                }
            }

            slot_context.set_tt_think_time(0);
            slot_context.set_interrupter_target(0);
            // 转换为 xHCI Slot Context 速度值
            slot_context.set_speed(info.port_speed.to_xhci_slot_value());

            // Initialize the Input default control Endpoint 0 Context (6.2.3).
            let endpoint_0 = input.device_mut().endpoint_mut(dci.as_usize());
            // • EP Type = Control.
            endpoint_0.set_endpoint_type(xhci::context::EndpointType::Control);
            // • Max Packet Size = The default maximum packet size for the Default Control Endpoint,
            //   as function of the PORTSC Port Speed field.
            endpoint_0.set_max_packet_size(max_packet_size);
            // • Max Burst Size = 0.
            endpoint_0.set_max_burst_size(0);
            // • TR Dequeue Pointer = Start address of first segment of the Default Control
            //   Endpoint Transfer Ring.
            endpoint_0.set_tr_dequeue_pointer(ctrl_ring_addr.raw());
            // • Dequeue Cycle State (DCS) = 1. Reflects Cycle bit state for valid TRBs written
            //   by software.
            // if ring_cycle_bit {
            endpoint_0.set_dequeue_cycle_state();
            // } else {
            //     endpoint_0.clear_dequeue_cycle_state();
            // }
            // • Interval = 0.
            endpoint_0.set_interval(0);
            // • Max Primary Streams (MaxPStreams) = 0.
            endpoint_0.set_max_primary_streams(0);
            // • Mult = 0.
            endpoint_0.set_mult(0);
            // • Error Count (CErr) = 3.
            endpoint_0.set_error_count(3);
            // • Average TRB Length = 8 (xHCI spec 6.2.3).
            endpoint_0.set_average_trb_length(8);
        });

        info!(
            "crabusb/xhci/device: address slot={} root_port={} port={} route=0x{:x} speed={:?} mps={}",
            self.id.as_u8(),
            info.root_port_id,
            info.port_id,
            route_string,
            info.port_speed,
            max_packet_size
        );

        mb();

        let input_bus_addr = self.ctx.input_bus_addr();
        trace!("Input context bus address: {input_bus_addr:#x?}");
        lifecycle.begin(CommandLifecycleStage::AddressDevice, max_packet_size as u32);
        let result = host
            .cmd_request(command::Allowed::AddressDevice(
                *command::AddressDevice::new()
                    .set_slot_id(self.id.into())
                    .set_input_context_pointer(input_bus_addr),
            ))
            .await;
        let result = match result {
            Ok(result) => result,
            Err(err) => {
                self.abort_command_lifecycle(&lifecycle, CommandLifecycleStage::AddressDevice, &err)
                    .await;
                return Err(err.into());
            }
        };

        lifecycle.ok(CommandLifecycleStage::AddressDevice, result.slot_id() as u32);

        info!(
            "crabusb/xhci/device: address ok slot={} completion={:?}",
            self.id.as_u8(),
            result.completion_code()
        );

        let settle_ms = if matches!(info.port_speed, Speed::Low | Speed::Full) {
            Self::LS_FS_ADDRESS_DEVICE_SETTLE_MS
        } else {
            0
        };
        if settle_ms != 0 {
            info!(
                "crabusb/xhci/device: slot={} root_port={} port={} settling {}ms after address-device",
                self.id.as_u8(),
                info.root_port_id,
                info.port_id,
                settle_ms
            );
            self.kernel.delay(Duration::from_millis(settle_ms));
        }

        Ok(())
    }

    async fn read_descriptor(
        &mut self,
        _info: &DeviceAddressInfo,
        _lifecycle: &CommandLifecycle,
    ) -> Result<()> {
        let lifecycle = self.lifecycle();
        lifecycle.begin(CommandLifecycleStage::ReadDescriptor, 0);
        match self.ep_ctrl().get_device_descriptor().await {
            Ok(desc) => {
                self.desc = desc;
                lifecycle.ok(
                    CommandLifecycleStage::ReadDescriptor,
                    self.desc.num_configurations as u32,
                );
            }
            Err(err) => {
                self.abort_command_lifecycle(&lifecycle, CommandLifecycleStage::ReadDescriptor, &err)
                    .await;
                return Err(err.into());
            }
        }
        Ok(())
    }
    async fn get_device_descriptor_base(
        &mut self,
        _info: &DeviceAddressInfo,
        _lifecycle: &CommandLifecycle,
    ) -> Result<DeviceDescriptorBase> {
        let lifecycle = self.lifecycle();
        let mut data = vec![0u8; 8];
        lifecycle.begin(CommandLifecycleStage::GetDeviceDescriptorBase, data.len() as u32);

        // DMA 传输
        if let Err(err) = self
            .ep_ctrl()
            .get_descriptor(DescriptorType::DEVICE, 0, 0, data.as_mut_slice())
            .await
        {
            self.abort_command_lifecycle(
                &lifecycle,
                CommandLifecycleStage::GetDeviceDescriptorBase,
                &err,
            )
            .await;
            return Err(err.into());
        }

        let desc = unsafe { *(data.as_mut_slice().as_ptr() as *const DeviceDescriptorBase) };
        lifecycle.ok(
            CommandLifecycleStage::GetDeviceDescriptorBase,
            u32::from(desc.max_packet_size_0),
        );

        Ok(desc)
    }

    async fn get_configuration(
        &mut self,
        _info: &DeviceAddressInfo,
        _lifecycle: &CommandLifecycle,
    ) -> Result<u8> {
        let lifecycle = self.lifecycle();
        lifecycle.begin(CommandLifecycleStage::GetConfiguration, 0);
        let val = match self.ep_ctrl().get_configuration().await {
            Ok(val) => val,
            Err(err) => {
                self.abort_command_lifecycle(&lifecycle, CommandLifecycleStage::GetConfiguration, &err)
                    .await;
                return Err(err.into());
            }
        };
        self.current_config_value = Some(val);
        lifecycle.ok(CommandLifecycleStage::GetConfiguration, u32::from(val));
        Ok(val)
    }

    async fn _set_configuration(&mut self, configuration_value: u8) -> Result {
        if self.current_config_value == Some(configuration_value) {
            info!(
                "crabusb/xhci/device: slot={} root_port={} port={} skipping set-configuration {} (already active)",
                self.id.as_u8(),
                self.root_port_id,
                self.port_id,
                configuration_value
            );
            return Ok(());
        }

        self.ctx.perper_change();
        if let Err(err) = self
            .ep_ctrl()
            .set_configuration(configuration_value)
            .await
        {
            return Err(err.into());
        }

        self.current_config_value = Some(configuration_value);

        self.ctx.with_input(|input| {
            let c = input.control_mut();
            c.set_configuration_value(configuration_value);
        });
        if let Err(err) = self.evaluate().await {
            return Err(err.into());
        }
        debug!("Device configuration set to {configuration_value}");
        Ok(())
    }

    async fn set_configuration_with_lifecycle(
        &mut self,
        configuration_value: u8,
        _info: &DeviceAddressInfo,
        lifecycle: &CommandLifecycle,
    ) -> Result {
        lifecycle.begin(CommandLifecycleStage::SetConfiguration, u32::from(configuration_value));
        match self._set_configuration(configuration_value).await {
            Ok(()) => {
                lifecycle.ok(CommandLifecycleStage::SetConfiguration, u32::from(configuration_value));
                Ok(())
            }
            Err(err) => {
                self.abort_command_lifecycle(lifecycle, CommandLifecycleStage::SetConfiguration, &err)
                    .await;
                Err(err.into())
            }
        }
    }

    async fn get_configuration_descriptor(
        &mut self,
        index: u8,
        _info: &DeviceAddressInfo,
        lifecycle: &CommandLifecycle,
    ) -> Result<ConfigurationDescriptor> {
        lifecycle.begin(
            CommandLifecycleStage::GetConfigurationDescriptor,
            u32::from(index),
        );
        match self.ep_ctrl().get_configuration_descriptor(index).await {
            Ok(desc) => {
                lifecycle.ok(
                    CommandLifecycleStage::GetConfigurationDescriptor,
                    u32::from(desc.configuration_value),
                );
                Ok(desc)
            }
            Err(err) => {
                self.abort_command_lifecycle(
                    lifecycle,
                    CommandLifecycleStage::GetConfigurationDescriptor,
                    &err,
                )
                .await;
                Err(err.into())
            }
        }
    }

    async fn _claim_interface(&mut self, interface: u8, alternate: u8) -> Result {
        let lifecycle = self.lifecycle();
        self.ctx.perper_change();
        lifecycle.begin(CommandLifecycleStage::ClaimInterface, u32::from(interface));
        self.ctx.with_input(|input| {
            let c = input.control_mut();
            c.set_interface_number(interface);
            c.set_alternate_setting(alternate);
        });

        // Alternate setting 0 is already the interface default. Some simple HID
        // devices, including emulated ones, may STALL an explicit SET_INTERFACE 0
        // even though the interface is otherwise usable. The userspace backend
        // already skips this request for alt 0, so keep the kernel xHCI path
        // consistent and only issue SET_INTERFACE for nonzero alternates.
        if alternate != 0 {
            if let Err(err) = self
                .ep_ctrl()
                .control_out(
                    ControlSetup {
                        request_type: RequestType::Standard,
                        recipient: Recipient::Interface,
                        request: usb_if::transfer::Request::SetInterface,
                        value: alternate as _, // alternate setting goes in value
                        index: interface as _, // interface number goes in index
                    },
                    &[],
                )
                .await
            {
                self.abort_command_lifecycle(&lifecycle, CommandLifecycleStage::ClaimInterface, &err)
                    .await;
                return Err(err.into());
            }
        }
        if let Err(err) = self.setup_all_endpoints(interface, alternate).await {
            self.abort_command_lifecycle(&lifecycle, CommandLifecycleStage::ClaimInterface, &err)
                .await;
            return Err(err.into());
        }
        lifecycle.ok(CommandLifecycleStage::ClaimInterface, u32::from(alternate));
        debug!("Interface {interface} set successfully");
        Ok(())
    }

    async fn setup_all_endpoints(&mut self, interface: u8, alternate: u8) -> Result {
        let lifecycle = self.lifecycle();
        let mut max_dci = 1;
        self.eps.clear();
        let mut configured_eps = Vec::new();

        for desc in self
            .find_interface_endpoints(interface, alternate)?
            .to_vec()
        {
            let dci = desc.dci();
            if dci > max_dci {
                max_dci = dci;
            }
            let mut ep_raw = self.new_ep(dci.into())?;
            let max_burst_size = self.xhci_bulk_max_burst_size(interface, alternate, &desc);
            if self.should_enable_skhynix_uas_streams(interface, alternate, &desc) {
                ep_raw.enable_primary_streams(32)?;
                for ring in ep_raw.stream_rings() {
                    self.transfer_result_handler
                        .add_queue(self.id.as_u8(), dci, ring);
                }
                let ring1_ptr = ep_raw
                    .stream_rings()
                    .next()
                    .map(|ring| ring.bus_addr().raw())
                    .unwrap_or(0);
                debug_record_stream_config(
                    self.id.as_u8(),
                    dci,
                    desc.address,
                    32,
                    ep_raw.max_primary_streams(),
                    max_burst_size,
                    desc.max_packet_size,
                    ep_raw.config_dequeue_pointer().raw(),
                    ring1_ptr,
                );
            }
            let ring_addr = ep_raw.config_dequeue_pointer();
            let has_primary_streams = ep_raw.has_primary_streams();
            let max_primary_streams = ep_raw.max_primary_streams();
            self.eps.insert(dci.into(), EndpointBase::new(ep_raw));

            let xhci_interval =
                self.calculate_xhci_interval(desc.interval, desc.transfer_type, desc.interval);
            configured_eps.push((
                desc,
                dci,
                ring_addr.raw(),
                xhci_interval,
                has_primary_streams,
                max_primary_streams,
                max_burst_size,
            ));
        }

        self.ctx.with_empty_input(|input| {
            input.control_mut().set_add_context_flag(0);

            input
                .device_mut()
                .slot_mut()
                .set_context_entries(max_dci + 1);

            for (
                desc,
                dci,
                ring_addr,
                xhci_interval,
                has_primary_streams,
                max_primary_streams,
                max_burst_size,
            ) in &configured_eps
            {
                input.control_mut().set_add_context_flag(*dci as _);

                debug!(
                    "init ep addr {:#x}  dci {} {:?}",
                    desc.address, dci, desc.transfer_type
                );

                let ep_mut = input.device_mut().endpoint_mut(*dci as _);

                debug!(
                    "Set XHCI interval: {} (original bInterval: {})",
                    xhci_interval, desc.interval
                );
                ep_mut.set_interval(*xhci_interval);
                ep_mut.set_endpoint_type(desc.endpoint_type());
                ep_mut.set_tr_dequeue_pointer(*ring_addr);
                ep_mut.set_max_packet_size(desc.max_packet_size);
                ep_mut.set_error_count(3);
                if *has_primary_streams {
                    ep_mut.clear_dequeue_cycle_state();
                    ep_mut.set_max_primary_streams(*max_primary_streams);
                    ep_mut.set_linear_stream_array();
                } else {
                    ep_mut.set_dequeue_cycle_state();
                    ep_mut.set_max_primary_streams(0);
                    ep_mut.clear_linear_stream_array();
                }
                ep_mut.set_mult(0);

                match desc.transfer_type {
                    EndpointType::Isochronous | EndpointType::Interrupt => {
                        // init for isoch/interrupt
                        ep_mut.set_max_packet_size(desc.max_packet_size & 0x7ff); //refer xhci page 162
                        ep_mut.set_max_burst_size(
                            ((desc.max_packet_size & 0x1800) >> 11).try_into().unwrap(),
                        );
                            // MaxESITPayload = (MaxBurst+1) * MaxPacketSize  (xHCI §6.2.3.8)
                            let mps = desc.max_packet_size & 0x7FF;
                            let burst = (desc.max_packet_size >> 11) & 0x3;
                            let esit = ((burst + 1) as u16).saturating_mul(mps);
                            ep_mut.set_max_endpoint_service_time_interval_payload_low(esit);
                    }
                    EndpointType::Bulk | EndpointType::Control => {
                        ep_mut.set_max_burst_size(*max_burst_size);
                        ep_mut.set_average_trb_length((desc.max_packet_size & 0x7ff) as _);
                    }
                }

                if let EndpointType::Isochronous = desc.transfer_type {
                    ep_mut.set_error_count(0);
                }
            }
        });
        mb();

        lifecycle.begin(CommandLifecycleStage::ConfigureEndpoint, max_dci as u32);
        let _result = match self
            .cmd
            .cmd_request(command::Allowed::ConfigureEndpoint(
                *command::ConfigureEndpoint::default()
                    .set_slot_id(self.id.into())
                    .set_input_context_pointer(self.ctx.input_bus_addr()),
            ))
            .await
        {
            Ok(result) => result,
            Err(err) => {
                self.abort_command_lifecycle(&lifecycle, CommandLifecycleStage::ConfigureEndpoint, &err)
                    .await;
                return Err(err.into());
            }
        };
        lifecycle.ok(CommandLifecycleStage::ConfigureEndpoint, max_dci as u32);

        Ok(())
    }

    fn find_interface_endpoints(
        &self,
        interface: u8,
        alternate: u8,
    ) -> Result<&[EndpointDescriptor]> {
        Ok(&self.find_interface_alt(interface, alternate)?.endpoints)
    }

    fn find_interface_alt(&self, interface: u8, alternate: u8) -> Result<&InterfaceDescriptor> {
        for config in &self.config_desc {
            for iface in &config.interfaces {
                if iface.interface_number == interface {
                    for alt in &iface.alt_settings {
                        if alt.alternate_setting == alternate {
                            return Ok(alt);
                        }
                    }
                }
            }
        }
        Err(USBError::NotFound)
    }

    fn is_skhynix_uas_alt(&self, interface: u8, alternate: u8) -> bool {
        let Ok(alt) = self.find_interface_alt(interface, alternate) else {
            return false;
        };
        self.desc.vendor_id == 0x152E
            && self.desc.product_id == 0x7001
            && matches!(self.port_speed, Speed::SuperSpeed | Speed::SuperSpeedPlus)
            && alt.class == 0x08
            && alt.subclass == 0x06
            && alt.protocol == 0x62
    }

    fn should_enable_skhynix_uas_streams(
        &self,
        interface: u8,
        alternate: u8,
        desc: &EndpointDescriptor,
    ) -> bool {
        self.is_skhynix_uas_alt(interface, alternate)
            && matches!(desc.transfer_type, EndpointType::Bulk)
            && matches!(desc.address, 0x81 | 0x83 | 0x02)
    }

    fn xhci_bulk_max_burst_size(
        &self,
        interface: u8,
        alternate: u8,
        desc: &EndpointDescriptor,
    ) -> u8 {
        if self.should_enable_skhynix_uas_streams(interface, alternate, desc) {
            15
        } else {
            0
        }
    }

    /// 根据 XHCI 规范计算端点的 interval 值
    /// 参考 xHCI 规范第 6.2.3.6 节
    fn calculate_xhci_interval(
        &self,
        binterval: u8,
        transfer_type: EndpointType,
        default: u8,
    ) -> u8 {
        match transfer_type {
            EndpointType::Isochronous => {
                match self.port_speed {
                    Speed::High | Speed::SuperSpeed | Speed::SuperSpeedPlus => {
                        // HighSpeed, SuperSpeed, SuperSpeedPlus ISO 端点
                        // Interval = max(1, min(16, bInterval))
                        let interval = binterval.clamp(1, 16);
                        info!(
                            "ISO endpoint HS/SS: bInterval={} -> XHCI interval={}",
                            binterval, interval
                        );
                        interval
                    }
                    _ => {
                        // FullSpeed/LowSpeed ISO 端点
                        // Interval = max(1, min(16, floor(log2(bInterval)) + 3))
                        if binterval == 0 {
                            1
                        } else {
                            // 计算 floor(log2(bInterval))
                            let log2_binterval = binterval.ilog2() as u8;
                            let interval = (log2_binterval + 3).clamp(1, 16);
                            info!(
                                "ISO endpoint FS/LS: bInterval={} -> log2={} -> XHCI interval={}",
                                binterval, log2_binterval, interval
                            );
                            interval
                        }
                    }
                }
            }
            EndpointType::Interrupt => {
                match self.port_speed {
                    Speed::High | Speed::SuperSpeed | Speed::SuperSpeedPlus => {
                        // HighSpeed, SuperSpeed, SuperSpeedPlus 中断端点
                        // Interval = max(1, min(16, bInterval))
                        let interval = binterval.clamp(1, 16);
                        info!(
                            "INT endpoint HS/SS: bInterval={} -> XHCI interval={}",
                            binterval, interval
                        );
                        interval
                    }
                    _ => {
                        // FullSpeed/LowSpeed 中断端点
                        // Interval = max(1, min(16, floor(log2(bInterval)) + 3))
                        if binterval == 0 {
                            1
                        } else {
                            // 计算 floor(log2(bInterval))
                            let log2_binterval = binterval.ilog2() as u8;
                            let interval = (log2_binterval + 3).clamp(1, 16);
                            info!(
                                "INT endpoint FS/LS: bInterval={} -> log2={} -> XHCI interval={}",
                                binterval, log2_binterval, interval
                            );
                            interval
                        }
                    }
                }
            }
            _ => {
                // 控制和批量端点不使用 interval
                default
            }
        }
    }

    async fn update_hub_inner(&mut self, params: HubParams) -> Result<()> {
        debug!(
            "Updating hub context for slot {}: ports={}, multi_tt={}, tt_time={}ns",
            self.id.as_u8(),
            params.num_ports,
            params.multi_tt,
            params.tt_think_time_ns,
        );

        self.ctx.perper_change();
        // 2. 设置 Slot Context Hub 参数
        self.ctx.with_input(|input| {
            let slot_ctx = input.device_mut().slot_mut();

            // 设置 Hub 标志
            slot_ctx.set_hub();

            // 设置 Multi-TT 标志（参考 U-Boot）
            // 如果 hub->tt.multi 为真，设置 MTT
            // 对于 Full Speed Hub，必须清除 MTT（xHCI 规范 6.2.2）
            if params.multi_tt {
                slot_ctx.set_multi_tt();
            } else if matches!(self.port_speed, Speed::Full) {
                slot_ctx.clear_multi_tt();
            }

            // 设置端口数量
            slot_ctx.set_number_of_ports(params.num_ports);

            // 设置 TT 思考时间（参考 U-Boot xhci_update_hub_device）
            // xHCI spec: TT_THINK_TIME (Bits[16:17] of DWORD 2)
            // 0 = 8 FS bit times, 1 = 16 FS bit times, 2 = 24 FS bit times, 3 = 32 FS bit times
            // 只对 High Speed Hub 设置 TT 思考时间
            if matches!(self.port_speed, Speed::High) {
                // params.tt_think_time_ns 已经是转换后的值 (0, 666, 1333, 1999)
                // 需要转换为 xHCI 寄存器值
                let think_time = if params.tt_think_time_ns > 0 {
                    (params.tt_think_time_ns / 666).saturating_sub(1) as u8
                } else {
                    0
                };
                slot_ctx.set_tt_think_time(think_time);
                debug!(
                    "Set TT think time: {} (tt_think_time_ns={}ns)",
                    think_time, params.tt_think_time_ns
                );
            }
        });

        if let Err(err) = self.evaluate().await {
            let lifecycle = self.lifecycle();
            self.abort_command_lifecycle(&lifecycle, CommandLifecycleStage::EvaluateContext, &err)
                .await;
            return Err(err.into());
        }
        Ok(())
    }

    async fn debug_reset_endpoint_inner(
        &mut self,
        endpoint_address: u8,
        preserve_transfer_state: bool,
    ) -> Result<()> {
        let lifecycle = self.lifecycle();
        let endpoint_id = usb_if::descriptor::EndpointDescriptor {
            address: endpoint_address,
            max_packet_size: 0,
            transfer_type: EndpointType::Bulk,
            direction: if (endpoint_address & 0x80) != 0 {
                usb_if::transfer::Direction::In
            } else {
                usb_if::transfer::Direction::Out
            },
            packets_per_microframe: 0,
            interval: 0,
        }
        .dci();
        lifecycle.begin(CommandLifecycleStage::ResetEndpoint, u32::from(endpoint_id));

        let mut cmd = command::ResetEndpoint::default();
        if preserve_transfer_state {
            cmd.set_transfer_state_preserve();
        } else {
            cmd.clear_transfer_state_preserve();
        }
        cmd.set_endpoint_id(endpoint_id).set_slot_id(self.id.into());

        if let Err(err) = self.cmd.cmd_request(command::Allowed::ResetEndpoint(cmd)).await {
            lifecycle.fail(CommandLifecycleStage::ResetEndpoint, &err);
            return Err(err.into());
        }

        if let Some(ep) = self.eps.get_mut(&endpoint_id.into()) {
            let ring_addr = ep.as_raw_mut::<Endpoint>().bus_addr().raw();
            lifecycle.begin(CommandLifecycleStage::SetTrDequeuePointer, ring_addr as u32);
            let mut set_deq = command::SetTrDequeuePointer::default();
            set_deq
                .set_dequeue_cycle_state()
                .set_new_tr_dequeue_pointer(ring_addr)
                .set_endpoint_id(endpoint_id)
                .set_slot_id(self.id.into());

            if let Err(err) = self
                .cmd
                .cmd_request(command::Allowed::SetTrDequeuePointer(set_deq))
                .await
            {
                lifecycle.fail(CommandLifecycleStage::SetTrDequeuePointer, &err);
                return Err(err.into());
            }
            lifecycle.ok(CommandLifecycleStage::SetTrDequeuePointer, ring_addr as u32);
        }
        lifecycle.ok(CommandLifecycleStage::ResetEndpoint, u32::from(endpoint_id));
        Ok(())
    }

    async fn debug_close_slot_inner(&mut self) -> Result<()> {
        let lifecycle = self.lifecycle();
        self.transfer_result_handler
            .unregister_slot(self.id.as_u8());
        self.eps.clear();
        self.ctrl_ep = None;

        let mut cmd = command::DisableSlot::default();
        cmd.set_slot_id(self.id.into());
        lifecycle.begin(CommandLifecycleStage::DisableSlot, self.id.as_u8() as u32);
        match self.cmd.cmd_request(command::Allowed::DisableSlot(cmd)).await {
            Ok(_) => lifecycle.ok(CommandLifecycleStage::DisableSlot, self.id.as_u8() as u32),
            Err(err) => {
                lifecycle.fail(CommandLifecycleStage::DisableSlot, &err);
                return Err(err.into());
            }
        }
        Ok(())
    }
}

impl DeviceOp for Device {
    fn id(&self) -> usize {
        self.id.as_usize()
    }

    fn backend_name(&self) -> &str {
        "xhci"
    }

    fn descriptor(&self) -> &DeviceDescriptor {
        &self.desc
    }
    fn claim_interface<'a>(
        &'a mut self,
        interface: u8,
        alternate: u8,
    ) -> BoxFuture<'a, Result<()>> {
        self._claim_interface(interface, alternate).boxed()
    }
    fn set_configuration<'a>(&'a mut self, configuration_value: u8) -> BoxFuture<'a, Result<()>> {
        self._set_configuration(configuration_value).boxed()
    }

    fn ep_ctrl(&mut self) -> &mut EndpointControl {
        self.ctrl_ep.as_mut().unwrap()
    }

    fn configuration_descriptors(&self) -> &[ConfigurationDescriptor] {
        &self.config_desc
    }

    fn get_endpoint(
        &mut self,
        desc: &usb_if::descriptor::EndpointDescriptor,
    ) -> Result<EndpointBase> {
        let ep = self.eps.remove(&desc.dci().into());
        ep.ok_or(USBError::NotFound)
    }

    fn debug_reset_endpoint<'a>(
        &'a mut self,
        endpoint_address: u8,
        preserve_transfer_state: bool,
    ) -> BoxFuture<'a, Result<()>> {
        self.debug_reset_endpoint_inner(endpoint_address, preserve_transfer_state)
            .boxed()
    }

    fn debug_close_slot<'a>(&'a mut self) -> BoxFuture<'a, Result<()>> {
        self.debug_close_slot_inner().boxed()
    }

    fn update_hub(&mut self, params: HubParams) -> BoxFuture<'_, Result<()>> {
        self.update_hub_inner(params).boxed()
    }
}
