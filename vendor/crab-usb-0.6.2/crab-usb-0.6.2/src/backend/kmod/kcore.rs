use alloc::{boxed::Box, collections::btree_map::BTreeMap, vec::Vec};

use futures::{
    FutureExt,
    future::{BoxFuture, LocalBoxFuture},
};
use id_arena::{Arena, Id};
use usb_if::{
    descriptor::{ConfigurationDescriptor, DeviceDescriptor},
    err::USBError,
};

use super::osal::Kernel;
use crate::{
    Device, DeviceAddressInfo,
    backend::{
        BackendOp, DeviceId,
        kmod::hub::{Hub, HubDevice, HubInfo, HubOp, PortChangeInfo},
        ty::{DeviceInfoOp, DeviceOp, EventHandlerOp},
    },
    device::{DeviceTopology, DeviceTopologyHop},
    topology::{DeviceLocation, DeviceNode, DeviceTree},
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
    discovered_devices: BTreeMap<usize, DeviceInfo>,
}

impl Core {
    pub(crate) fn new(backend: impl CoreOp) -> Self {
        Self {
            root_hub: None,
            backend: Box::new(backend),
            hubs: Arena::new(),
            inited_devices: BTreeMap::new(),
            discovered_devices: BTreeMap::new(),
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

    fn device_location(
        root_port_id: u8,
        parent_hub: Option<Id<Hub>>,
        port_id: u8,
        infos: &BTreeMap<Id<Hub>, HubInfo>,
    ) -> DeviceLocation {
        let mut hops = Vec::new();
        let mut parent = parent_hub;
        while let Some(id) = parent {
            let Some(info) = infos.get(&id) else {
                break;
            };
            if info.hub_depth >= 0 {
                hops.push(info.port_id);
            }
            parent = info.parent;
        }
        hops.reverse();

        let mut path = Vec::with_capacity(hops.len() + 2);
        path.push(root_port_id);
        path.extend(hops.iter().copied());
        if port_id != 0 {
            path.push(port_id);
        }

        let mut route_string = 0u32;
        for (idx, port) in hops
            .iter()
            .copied()
            .chain((port_id != 0).then_some(port_id))
            .take(5)
            .enumerate()
        {
            let nibble = u32::from(port.min(15));
            route_string |= nibble << (idx * 4);
        }

        DeviceLocation {
            root_port: root_port_id,
            route_string,
            path,
        }
    }

    fn build_topology(&self) -> DeviceTree {
        let infos = self.hub_infos();
        let mut nodes = Vec::new();

        for (hub_id, hub) in self.hubs.iter() {
            if self.root_hub == Some(hub_id) {
                continue;
            }

            let mut current = Some(hub_id);
            let mut root_port_id = 0u8;
            while let Some(id) = current {
                let Some(info) = infos.get(&id) else {
                    break;
                };
                if info.hub_depth == -1 {
                    root_port_id = info.port_id;
                    break;
                }
                current = info.parent;
            }
            if root_port_id == 0 {
                continue;
            }

            let location =
                Self::device_location(root_port_id, hub.info.parent, hub.info.port_id, &infos);
            let parent = hub.info.parent.and_then(|id| {
                infos.get(&id).and_then(|info| {
                    (info.hub_depth >= 0).then(|| {
                        let location =
                            Self::device_location(root_port_id, info.parent, info.port_id, &infos);
                        location.device_id()
                    })
                })
            });
            nodes.push(DeviceNode {
                id: location.device_id(),
                descriptor: hub
                    .backend
                    .descriptor()
                    .expect("non-root hub should expose a descriptor"),
                configurations: hub.backend.configuration_descriptors(),
                location,
                parent,
                port: hub.info.port_id,
                is_hub: true,
            });
        }

        for dev in self.discovered_devices.values() {
            let topology = dev.topology();
            let location = Self::device_location(
                topology.root_port_id,
                dev.addr_info.parent_hub,
                topology.port_id,
                &infos,
            );
            let parent = dev.addr_info.parent_hub.and_then(|id| {
                infos.get(&id).and_then(|info| {
                    (info.hub_depth >= 0).then(|| {
                        let parent_loc = Self::device_location(
                            topology.root_port_id,
                            info.parent,
                            info.port_id,
                            &infos,
                        );
                        parent_loc.device_id()
                    })
                })
            });
            nodes.push(DeviceNode {
                id: location.device_id(),
                descriptor: dev.desc.clone(),
                configurations: dev.config_desc.clone(),
                location,
                parent,
                port: topology.port_id,
                is_hub: false,
            });
        }

        nodes.sort_by_key(|node| {
            (
                node.location.root_port,
                node.location.path.len(),
                node.location.route_string,
                u8::from(node.is_hub),
            )
        });

        DeviceTree { nodes }
    }

    fn discovered_leaf_by_stable_id(&self, id: DeviceId) -> Option<DeviceInfo> {
        let infos = self.hub_infos();
        self.discovered_devices.values().find_map(|dev| {
            let topology = dev.topology();
            let location = Self::device_location(
                topology.root_port_id,
                dev.addr_info.parent_hub,
                topology.port_id,
                &infos,
            );
            (location.device_id() == id).then(|| dev.clone())
        })
    }

    async fn open_leaf_device(&mut self, dev_info: DeviceInfo) -> Result<Box<dyn DeviceOp>, USBError> {
        if let Some(device) = self.inited_devices.remove(&dev_info.id) {
            return Ok(device);
        }

        info!(
            "crabusb/kcore: reopening stable leaf device id=0x{:08x} runtime_id={} root_port={} port={} speed={:?}",
            dev_info.stable_id().raw(),
            dev_info.id,
            dev_info.addr_info.root_port_id,
            dev_info.addr_info.port_id,
            dev_info.addr_info.port_speed
        );
        self.backend
            .new_addressed_device(dev_info.addr_info.clone())
            .await
    }

    async fn _probe_devices(&mut self) -> Result<(bool, Vec<Box<dyn DeviceInfoOp>>), USBError> {
        let mut is_have_new_hub = false;
        let mut out = Vec::new();

        let hub_ids: Vec<Id<Hub>> = self.hubs.iter().map(|(id, _)| id).collect();
        info!("crabusb/kcore: _probe_devices begin hubs={}", hub_ids.len());

        for id in hub_ids {
            crate::debug_set_usb_probe_progress(1, 0, 0, 0, id.index() as u32);
            let addr_infos = self.hub_changed_ports(id).await?;
            let parent_hub_id = self.hubs.get(id).unwrap().backend.slot_id();
            info!(
                "crabusb/kcore: hub {:?} slot={} changed_ports={}",
                id,
                parent_hub_id,
                addr_infos.len()
            );
            for addr_info in addr_infos {
                info!(
                    "crabusb/kcore: hub {:?} addr_info root_port={} port={} speed={:?}",
                    id, addr_info.root_port_id, addr_info.port_id, addr_info.port_speed
                );
                let info = DeviceAddressInfo {
                    root_port_id: addr_info.root_port_id,
                    port_speed: addr_info.port_speed,
                    parent_hub: Some(id),
                    port_id: addr_info.port_id,
                    infos: self.hub_infos(),
                };
                crate::debug_set_usb_probe_progress(
                    2,
                    info.root_port_id,
                    info.port_id,
                    0,
                    u32::from(info.port_speed.to_xhci_slot_value()),
                );

                info!(
                    "crabusb/kcore: calling new_addressed_device root_port={} port={} speed={:?}",
                    info.root_port_id, info.port_id, info.port_speed
                );
                let reopen_info = info.clone();
                let device = match self.backend.new_addressed_device(info).await {
                    Ok(device) => device,
                    Err(err) => {
                        if let Some(hub) = self.hubs.get_mut(id) {
                            hub.backend.rearm_port(addr_info.port_id);
                        }
                        return Err(err);
                    }
                };
                let device_id = device.id();
                let desc = device.descriptor();
                info!(
                    "crabusb/kcore: new device id={} vid={:04x} pid={:04x} class={:02x} subclass={:02x} proto={:02x}",
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
                    info!(
                        "crabusb/kcore: device id={} classified as hub cfg={} if={} alt={}",
                        device_id,
                        hub_settings.config_value,
                        hub_settings.interface_number,
                        hub_settings.alt_setting
                    );
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

                    info!("Added new hub with id {:?}", hub_id);
                } else {
                    let desc = device.descriptor().clone();
                    let configs = device.configuration_descriptors().to_vec();

                    self.inited_devices.insert(device_id, device);

                    let device_info = DeviceInfo::new(device_id, desc, &configs, reopen_info);
                    self.discovered_devices
                        .insert(device_id, device_info.clone());
                    let device_info = Box::new(device_info) as Box<dyn DeviceInfoOp>;

                    info!("crabusb/kcore: device id={} kept as leaf device", device_id);
                    out.push(device_info);
                }
            }
        }

        info!(
            "crabusb/kcore: _probe_devices end new_hub={} leaf_devices={}",
            is_have_new_hub,
            out.len()
        );
        Ok((is_have_new_hub, out))
    }

    async fn hub_changed_ports(
        &mut self,
        hub_id: Id<Hub>,
    ) -> Result<Vec<PortChangeInfo>, USBError> {
        let hub = self.hubs.get_mut(hub_id).expect("Hub id should be valid");
        hub.backend.changed_ports().await
    }

    async fn probe_devices(&mut self) -> Result<Vec<Box<dyn DeviceInfoOp>>, USBError> {
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

    fn device_list<'a>(
        &'a mut self,
    ) -> BoxFuture<'a, Result<Vec<Box<dyn DeviceInfoOp>>, USBError>> {
        self.probe_devices().boxed()
    }

    fn topology<'a>(&'a mut self) -> BoxFuture<'a, Result<DeviceTree, USBError>> {
        async {
            self.probe_devices().await?;
            Ok(self.build_topology())
        }
        .boxed()
    }

    fn open_device<'a>(
        &'a mut self,
        dev: &'a dyn crate::backend::ty::DeviceInfoOp,
    ) -> LocalBoxFuture<'a, Result<Box<dyn DeviceOp>, USBError>> {
        async {
            if let Some(dev_info) = (dev as &dyn core::any::Any).downcast_ref::<DeviceInfo>() {
                return self.open_leaf_device(dev_info.clone()).await;
            }

            if let Some(device) = self.inited_devices.remove(&dev.id()) {
                return Ok(device);
            }

            Err(USBError::Other(anyhow!(
                "device {} not found in session cache",
                dev.id()
            )))
        }
        .boxed()
    }

    fn open_device_by_id<'a>(
        &'a mut self,
        id: DeviceId,
    ) -> LocalBoxFuture<'a, Result<Box<dyn DeviceOp>, USBError>> {
        async move {
            self.probe_devices().await?;
            let Some(dev_info) = self.discovered_leaf_by_stable_id(id) else {
                return Err(USBError::Other(anyhow!(
                    "device 0x{:08x} not found in topology",
                    id.raw()
                )));
            };
            self.open_leaf_device(dev_info).await
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
    addr_info: crate::backend::kmod::DeviceAddressInfo,
}

impl DeviceInfo {
    pub fn new(
        id: usize,
        desc: DeviceDescriptor,
        config_desc: &[ConfigurationDescriptor],
        addr_info: crate::backend::kmod::DeviceAddressInfo,
    ) -> Self {
        Self {
            id,
            desc,
            config_desc: config_desc.to_vec(),
            addr_info,
        }
    }

    fn stable_id(&self) -> DeviceId {
        let topology = self.topology();
        let location = Core::device_location(
            topology.root_port_id,
            self.addr_info.parent_hub,
            topology.port_id,
            &self.addr_info.infos,
        );
        location.device_id()
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

    fn topology(&self) -> DeviceTopology {
        let mut path = Vec::new();
        let mut parent = self.addr_info.parent_hub;
        while let Some(id) = parent {
            let Some(info) = self.addr_info.infos.get(&id) else {
                break;
            };
            if info.hub_depth >= 0 {
                path.push(DeviceTopologyHop {
                    slot_id: info.slot_id,
                    port_id: info.port_id,
                    hub_depth: info.hub_depth as u8,
                    speed: info.speed,
                });
            }
            parent = info.parent;
        }
        path.reverse();

        DeviceTopology {
            root_port_id: self.addr_info.root_port_id,
            port_id: self.addr_info.port_id,
            port_speed: self.addr_info.port_speed,
            parent_hub_slot_id: path.last().map(|hop| hop.slot_id),
            path,
        }
    }
}
