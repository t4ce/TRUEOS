use alloc::vec::Vec;

use heapless::String;
use v::vnet as api;

pub const DEVICE_TAG_CAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceIp {
    V4([u8; 4]),
    V6([u8; 16]),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSnapshot {
    pub handle: api::NetHandle,
    pub tag: String<DEVICE_TAG_CAP>,
    pub ip: Option<DeviceIp>,
    pub tcp_port: u16,
    pub connected_at_ms: u64,
    pub last_activity_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceRecord {
    pub handle: api::NetHandle,
    pub ip: Option<DeviceIp>,
    pub tcp_port: u16,
    pub connected_at_ms: u64,
    pub last_activity_ms: u64,
}

pub struct DeviceRegistry {
    devices: Vec<DeviceRecord>,
    max_devices: usize,
}

impl DeviceRegistry {
    pub const fn new(max_devices: usize) -> Self {
        Self {
            devices: Vec::new(),
            max_devices,
        }
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    pub fn snapshot(&self) -> Vec<DeviceSnapshot> {
        self.devices
            .iter()
            .map(|entry| DeviceSnapshot {
                handle: entry.handle,
                tag: device_tag(),
                ip: entry.ip,
                tcp_port: entry.tcp_port,
                connected_at_ms: entry.connected_at_ms,
                last_activity_ms: entry.last_activity_ms,
            })
            .collect()
    }

    pub fn snapshot_for(&self, handle: api::NetHandle) -> Option<DeviceSnapshot> {
        self.devices
            .iter()
            .find(|entry| entry.handle == handle)
            .map(|entry| DeviceSnapshot {
                handle: entry.handle,
                tag: device_tag(),
                ip: entry.ip,
                tcp_port: entry.tcp_port,
                connected_at_ms: entry.connected_at_ms,
                last_activity_ms: entry.last_activity_ms,
            })
    }

    pub fn connect(&mut self, handle: api::NetHandle, tcp_port: u16, now_ms: u64) -> bool {
        if let Some(existing) = self.devices.iter_mut().find(|entry| entry.handle == handle) {
            existing.tcp_port = tcp_port;
            existing.last_activity_ms = now_ms;
            return false;
        }

        if self.max_devices != 0 && self.devices.len() >= self.max_devices {
            let _ = self.devices.remove(0);
        }

        self.devices.push(DeviceRecord {
            handle,
            ip: None,
            tcp_port,
            connected_at_ms: now_ms,
            last_activity_ms: now_ms,
        });
        true
    }

    pub fn set_ip_v4(&mut self, handle: api::NetHandle, addr: [u8; 4], port: u16) {
        if let Some(existing) = self.devices.iter_mut().find(|entry| entry.handle == handle) {
            existing.ip = Some(DeviceIp::V4(addr));
            existing.tcp_port = port;
        }
    }

    pub fn set_ip_v6(&mut self, handle: api::NetHandle, addr: [u8; 16], port: u16) {
        if let Some(existing) = self.devices.iter_mut().find(|entry| entry.handle == handle) {
            existing.ip = Some(DeviceIp::V6(addr));
            existing.tcp_port = port;
        }
    }

    pub fn touch(&mut self, handle: api::NetHandle, now_ms: u64) {
        if let Some(existing) = self.devices.iter_mut().find(|entry| entry.handle == handle) {
            existing.last_activity_ms = now_ms;
        }
    }

    #[allow(dead_code)]
    pub fn remove_device(&mut self, handle: api::NetHandle) -> bool {
        if let Some(idx) = self.devices.iter().position(|entry| entry.handle == handle) {
            self.devices.remove(idx);
            return true;
        }
        false
    }
}

fn device_tag() -> String<DEVICE_TAG_CAP> {
    let mut tag = String::new();
    let _ = tag.push_str("esp");
    tag
}