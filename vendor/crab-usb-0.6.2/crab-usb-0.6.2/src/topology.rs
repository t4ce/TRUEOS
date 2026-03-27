use alloc::vec::Vec;

use usb_if::descriptor::{ConfigurationDescriptor, DeviceDescriptor};

use crate::backend::DeviceId;

#[derive(Clone, Debug)]
pub struct DeviceLocation {
    pub root_port: u8,
    pub route_string: u32,
    pub path: Vec<u8>,
}

impl DeviceLocation {
    pub fn device_id(&self) -> DeviceId {
        DeviceId((((self.root_port as u32) & 0xFF) << 24) | (self.route_string & 0x00FF_FFFF))
    }
}

#[derive(Clone, Debug)]
pub struct DeviceNode {
    pub id: DeviceId,
    pub descriptor: DeviceDescriptor,
    pub configurations: Vec<ConfigurationDescriptor>,
    pub location: DeviceLocation,
    pub parent: Option<DeviceId>,
    pub port: u8,
    pub is_hub: bool,
}

#[derive(Clone, Debug)]
pub struct DeviceHandle {
    node: DeviceNode,
}

impl DeviceHandle {
    pub fn id(&self) -> DeviceId {
        self.node.id
    }

    pub fn descriptor(&self) -> &DeviceDescriptor {
        &self.node.descriptor
    }

    pub fn configurations(&self) -> &[ConfigurationDescriptor] {
        &self.node.configurations
    }

    pub fn location(&self) -> &DeviceLocation {
        &self.node.location
    }

    pub fn parent(&self) -> Option<DeviceId> {
        self.node.parent
    }

    pub fn port(&self) -> u8 {
        self.node.port
    }

    pub fn is_hub(&self) -> bool {
        self.node.is_hub
    }

    pub fn node(&self) -> &DeviceNode {
        &self.node
    }
}

impl From<DeviceNode> for DeviceHandle {
    fn from(node: DeviceNode) -> Self {
        Self { node }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DeviceTree {
    pub nodes: Vec<DeviceNode>,
}

impl DeviceTree {
    pub fn iter(&self) -> impl Iterator<Item = &DeviceNode> {
        self.nodes.iter()
    }

    pub fn get(&self, id: DeviceId) -> Option<&DeviceNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn device(&self, id: DeviceId) -> Option<DeviceHandle> {
        self.get(id).cloned().map(DeviceHandle::from)
    }
}
