use alloc::{boxed::Box, collections::btree_map::BTreeMap, format, vec::Vec};

use futures::{
    FutureExt,
    future::{BoxFuture, LocalBoxFuture},
};
use id_arena::{Arena, Id};
use usb_if::{
    descriptor::{ConfigurationDescriptor, DeviceDescriptor},
    err::USBError,
    host::hub::Speed,
};

use super::osal::Kernel;
use crate::{
    Device, DeviceAddressInfo,
    backend::{
        BackendOp,
        kmod::hub::{Hub, HubDevice, HubInfo, HubOp, PortChangeInfo},
        ty::{DeviceInfoOp, DeviceOp, EventHandlerOp, ProbedDeviceInfoOp},
    },
};

pub trait CoreOp: Send + 'static {
    /// 初始化后端
    fn init<'a>(&'a mut self) -> BoxFuture<'a, Result<(), USBError>>;

    fn root_hub(&mut self) -> Box<dyn HubOp>;

    fn new_addressed_device<'a>(
        &'a mut self,
        addr: DeviceAddressInfo,
    ) -> BoxFuture<'a, Result<Box<dyn DeviceOp>, USBError>>;

    fn create_event_handler(&mut self) -> Box<dyn EventHandlerOp>;

    fn kernel(&self) -> &Kernel;
}

pub struct Core {
    pub(crate) backend: Box<dyn CoreOp>,
    hubs: Arena<Hub>,
    root_hub: Option<Id<Hub>>,
    inited_devices: BTreeMap<usize, Box<dyn DeviceOp>>,
    deferred_initial_superspeed: bool,
    deferred_initial_keyboard: bool,
}

fn is_xhci_command_timeout(err: &USBError) -> bool {
    format!("{err:?}").contains("xHCI command timed out")
}

impl Core {
    pub(crate) fn new(backend: impl CoreOp) -> Self {
        Self {
            root_hub: None,
            backend: Box::new(backend),
            hubs: Arena::new(),
            inited_devices: BTreeMap::new(),
            deferred_initial_superspeed: false,
            deferred_initial_keyboard: false,
        }
    }

    fn hub_infos(&self) -> BTreeMap<Id<Hub>, HubInfo> {
        let mut out = BTreeMap::new();
        for (id, hub) in self.hubs.iter() {
            let info = hub.info.clone();
            out.insert(id, info);
        }
        out
    }

    async fn _probe_devices(&mut self) -> Result<(bool, Vec<ProbedDeviceInfoOp>), USBError> {
        let mut is_have_new_hub = false;
        let mut out = Vec::new();

        let hub_ids: Vec<Id<Hub>> = self.hubs.iter().map(|(id, _)| id).collect();
        debug!("kcore: probe begin hubs={}", hub_ids.len());

        for id in hub_ids {
            debug!("kcore: hub {:?} changed_ports begin", id);
            let mut addr_infos = self.hub_changed_ports(id).await?;
            let parent_hub_id = self.hubs.get(id).unwrap().backend.slot_id();
            if addr_infos.is_empty() {
                debug!(
                    "kcore: hub {:?} changed_ports done count=0 parent_slot={}",
                    id, parent_hub_id
                );
            } else {
                info!(
                    "kcore: hub {:?} changed_ports done count={} parent_slot={}",
                    id,
                    addr_infos.len(),
                    parent_hub_id
                );
            }
            if addr_infos.iter().any(|addr| addr.root_port_id == 4) {
                info!("kcore: temporary prioritize known mouse root_port=4 before other ports");
                addr_infos.sort_by_key(|addr| if addr.root_port_id == 4 { 0 } else { 1 });
            }
            let defer_superspeed = !self.deferred_initial_superspeed
                && addr_infos.iter().any(|addr| {
                    !matches!(
                        addr.port_speed,
                        Speed::SuperSpeed | Speed::SuperSpeedPlus
                    )
                })
                && addr_infos.iter().any(|addr| {
                    matches!(addr.port_speed, Speed::SuperSpeed | Speed::SuperSpeedPlus)
                });
            for addr_info in addr_infos {
                if !self.deferred_initial_keyboard && addr_info.root_port_id == 3 {
                    info!(
                        "kcore: deferred initial known keyboard address root_port={} port={} speed={:?}",
                        addr_info.root_port_id,
                        addr_info.port_id,
                        addr_info.port_speed
                    );
                    self.deferred_initial_keyboard = true;
                    if let Some(hub) = self.hubs.get_mut(id) {
                        hub.backend.rearm_port(addr_info.port_id);
                    }
                    continue;
                }

                if addr_info.root_port_id == 11 {
                    info!(
                        "kcore: temporary skip known LED controller before address root_port={} port={} speed={:?}",
                        addr_info.root_port_id,
                        addr_info.port_id,
                        addr_info.port_speed
                    );
                    continue;
                }

                if defer_superspeed
                    && matches!(
                        addr_info.port_speed,
                        Speed::SuperSpeed | Speed::SuperSpeedPlus
                    )
                {
                    info!(
                        "kcore: deferred initial superspeed address root_port={} port={} speed={:?}",
                        addr_info.root_port_id,
                        addr_info.port_id,
                        addr_info.port_speed
                    );
                    self.deferred_initial_superspeed = true;
                    if let Some(hub) = self.hubs.get_mut(id) {
                        hub.backend.rearm_port(addr_info.port_id);
                    }
                    continue;
                }
                info!(
                    "kcore: address begin root_port={} port={} speed={:?}",
                    addr_info.root_port_id,
                    addr_info.port_id,
                    addr_info.port_speed
                );
                let info = DeviceAddressInfo {
                    root_port_id: addr_info.root_port_id,
                    port_speed: addr_info.port_speed,
                    parent_hub: Some(id),
                    port_id: addr_info.port_id,
                    infos: self.hub_infos(),
                };

                let device = match self.backend.new_addressed_device(info).await {
                    Ok(device) => device,
                    Err(err) => {
                        let xhci_command_timeout = is_xhci_command_timeout(&err);
                        warn!(
                            "kcore: address failed root_port={} port={} speed={:?} err={:?}",
                            addr_info.root_port_id,
                            addr_info.port_id,
                            addr_info.port_speed,
                            err
                        );
                        if xhci_command_timeout {
                            warn!(
                                "kcore: address failed on xhci command timeout; suppressing rearm root_port={} port={}",
                                addr_info.root_port_id,
                                addr_info.port_id
                            );
                            continue;
                        }
                        if let Some(hub) = self.hubs.get_mut(id) {
                            hub.backend.rearm_port(addr_info.port_id);
                        }
                        continue;
                    }
                };

                let device_id = device.id();
                let desc = device.descriptor();
                info!(
                    "kcore: address done id={} vid={:04x} pid={:04x} class={:02x}/{:02x}/{:02x}",
                    device_id,
                    desc.vendor_id,
                    desc.product_id,
                    desc.class,
                    desc.subclass,
                    desc.protocol
                );

                if let Some(hub_settings) =
                    HubDevice::is_hub(device.descriptor(), device.configuration_descriptors())
                {
                    let desc = device.descriptor().clone();
                    let configs = device.configuration_descriptors().to_vec();
                    let device_inner: Device = device.into();

                    let hub_device = HubDevice::new(
                        device_inner,
                        hub_settings,
                        addr_info.root_port_id,
                        parent_hub_id,
                        self.backend.kernel(),
                    )
                    .await?;
                    let mut hub = Hub::new(
                        Box::new(hub_device),
                        &self.hub_infos(),
                        addr_info.port_id,
                        Some(id),
                    );
                    let info = hub.backend.init(hub.info.clone()).await?;
                    hub.info = info;

                    let hub_id = self.hubs.alloc(hub);
                    is_have_new_hub = true;

                    let hub_info = Box::new(DeviceInfo::new(
                        device_id,
                        desc,
                        &configs,
                        addr_info.root_port_id,
                        addr_info.port_id,
                    )) as Box<dyn DeviceInfoOp>;
                    out.push(ProbedDeviceInfoOp::Hub(hub_info));

                    info!("Added new hub with id {:?}", hub_id);
                } else {
                    let desc = device.descriptor().clone();
                    let configs = device.configuration_descriptors().to_vec();

                    self.inited_devices.insert(device_id, device);

                    let device_info = Box::new(DeviceInfo::new(
                        device_id,
                        desc,
                        &configs,
                        addr_info.root_port_id,
                        addr_info.port_id,
                    )) as Box<dyn DeviceInfoOp>;

                    out.push(ProbedDeviceInfoOp::Device(device_info));
                }
            }
        }

        Ok((is_have_new_hub, out))
    }

    async fn hub_changed_ports(
        &mut self,
        hub_id: Id<Hub>,
    ) -> Result<Vec<PortChangeInfo>, USBError> {
        let hub = self.hubs.get_mut(hub_id).expect("Hub id should be valid");
        hub.backend.changed_ports().await
    }

    async fn probe_devices(&mut self) -> Result<Vec<ProbedDeviceInfoOp>, USBError> {
        let mut result = Vec::new();

        loop {
            let (is_have_new_hub, mut devices) = self._probe_devices().await?;
            result.append(&mut devices);
            if !is_have_new_hub {
                break;
            }
        }
        Ok(result)
    }
}

impl BackendOp for Core {
    fn init<'a>(&'a mut self) -> BoxFuture<'a, Result<(), USBError>> {
        async {
            self.backend.init().await?;
            let mut root_hub = Hub::new(self.backend.root_hub(), &self.hub_infos(), 0, None);
            let info = root_hub.backend.init(root_hub.info.clone()).await?;
            root_hub.info = info;

            let id = self.hubs.alloc(root_hub);
            self.root_hub = Some(id);
            Ok(())
        }
        .boxed()
    }

    fn device_list<'a>(&'a mut self) -> BoxFuture<'a, Result<Vec<ProbedDeviceInfoOp>, USBError>> {
        self.probe_devices().boxed()
    }

    fn open_device<'a>(
        &'a mut self,
        dev: &'a dyn crate::backend::ty::DeviceInfoOp,
    ) -> LocalBoxFuture<'a, Result<Box<dyn DeviceOp>, USBError>> {
        async {
            let device = self.inited_devices.remove(&dev.id()).unwrap_or_else(|| {
                panic!("Device id {} not found in inited_devices", dev.id());
            });

            Ok(device)
        }
        .boxed()
    }

    fn create_event_handler(&mut self) -> Box<dyn EventHandlerOp> {
        self.backend.create_event_handler()
    }
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    id: usize,
    desc: DeviceDescriptor,
    config_desc: Vec<ConfigurationDescriptor>,
    root_port_id: u8,
    port_id: u8,
}

impl DeviceInfo {
    pub fn new(
        id: usize,
        desc: DeviceDescriptor,
        config_desc: &[ConfigurationDescriptor],
        root_port_id: u8,
        port_id: u8,
    ) -> Self {
        Self {
            id,
            desc,
            config_desc: config_desc.to_vec(),
            root_port_id,
            port_id,
        }
    }
}

impl DeviceInfoOp for DeviceInfo {
    fn id(&self) -> usize {
        self.id
    }

    fn backend_name(&self) -> &str {
        "kernel"
    }

    fn descriptor(&self) -> &DeviceDescriptor {
        &self.desc
    }

    fn configuration_descriptors(&self) -> &[ConfigurationDescriptor] {
        &self.config_desc
    }

    fn root_port_id(&self) -> Option<u8> {
        Some(self.root_port_id)
    }

    fn port_id(&self) -> Option<u8> {
        Some(self.port_id)
    }
}
