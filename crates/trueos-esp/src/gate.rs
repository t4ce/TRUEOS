use alloc::vec::Vec;

use heapless::String;
use v::vnet as api;

use crate::swarm::{DeviceStatusSnapshot, StatusChangeEvent};

pub const DEVICE_TAG_CAP: usize = 16;
pub const ESP_HTTP_UPLOAD_PORT: u16 = 8080;
pub const ESP_UDP_BROADCAST_PORT: u16 = 32343;
pub const ESP_SWARM_HEARTBEAT: &[u8; 5] = b"swarm";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DefaultAppState {
    Pending,
    SkippedExistingApp,
    Uploaded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateSignal {
    UdpBound(api::NetHandle),
    EspDiscovered(api::EndpointV4),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateAction {
    None,
    Signal(GateSignal),
    Submit(api::Command),
}

pub struct GateDiscovery {
    udp_handle: Option<api::NetHandle>,
}

impl Default for GateDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl GateDiscovery {
    pub const fn new() -> Self {
        Self { udp_handle: None }
    }

    pub const fn bootstrap_command(&self) -> api::Command {
        api::Command::OpenUdp {
            port: ESP_UDP_BROADCAST_PORT,
        }
    }

    pub fn on_event(&mut self, ev: api::Event) -> GateAction {
        match ev {
            api::Event::Opened { handle, kind } if kind == api::SocketKind::Udp => {
                self.udp_handle = Some(handle);
                GateAction::Signal(GateSignal::UdpBound(handle))
            }
            api::Event::UdpPacket { handle, from, data }
                if self.udp_handle == Some(handle) && data.as_slice() == ESP_SWARM_HEARTBEAT =>
            {
                GateAction::Signal(GateSignal::EspDiscovered(from))
            }
            api::Event::Closed { handle } if self.udp_handle == Some(handle) => {
                self.udp_handle = None;
                GateAction::Submit(api::Command::OpenUdp {
                    port: ESP_UDP_BROADCAST_PORT,
                })
            }
            api::Event::UdpPacket { .. }
            | api::Event::UdpPacketV6 { .. }
            | api::Event::TcpEstablished { .. }
            | api::Event::TcpData { .. }
            | api::Event::TcpSent { .. }
            | api::Event::IcmpReply { .. }
            | api::Event::IcmpReplyV6 { .. }
            | api::Event::Error { .. }
            | api::Event::Opened { .. }
            | api::Event::Closed { .. } => GateAction::None,
        }
    }
}

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
    pub service_port: u16,
    pub connected_at_ms: u64,
    pub last_activity_ms: u64,
    pub status: Option<DeviceStatusSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceRecord {
    pub handle: api::NetHandle,
    pub ip: Option<DeviceIp>,
    pub service_port: u16,
    pub connected_at_ms: u64,
    pub last_activity_ms: u64,
    pub status: Option<DeviceStatusSnapshot>,
    default_app_state: DefaultAppState,
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
                service_port: entry.service_port,
                connected_at_ms: entry.connected_at_ms,
                last_activity_ms: entry.last_activity_ms,
                status: entry.status.clone(),
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
                service_port: entry.service_port,
                connected_at_ms: entry.connected_at_ms,
                last_activity_ms: entry.last_activity_ms,
                status: entry.status.clone(),
            })
    }

    pub fn upsert_heartbeat_v4(&mut self, addr: [u8; 4], service_port: u16, now_ms: u64) -> bool {
        let handle = device_handle_v4(addr);
        if let Some(existing) = self.devices.iter_mut().find(|entry| entry.handle == handle) {
            existing.ip = Some(DeviceIp::V4(addr));
            existing.service_port = service_port;
            existing.last_activity_ms = now_ms;
            return false;
        }

        if self.max_devices != 0 && self.devices.len() >= self.max_devices {
            let _ = self.devices.remove(0);
        }

        self.devices.push(DeviceRecord {
            handle,
            ip: Some(DeviceIp::V4(addr)),
            service_port,
            connected_at_ms: now_ms,
            last_activity_ms: now_ms,
            status: None,
            default_app_state: DefaultAppState::Pending,
        });
        true
    }

    pub fn should_upload_default_app(&mut self, handle: api::NetHandle, app_exists: bool) -> bool {
        let Some(existing) = self.devices.iter_mut().find(|entry| entry.handle == handle) else {
            return false;
        };
        if app_exists {
            existing.default_app_state = DefaultAppState::SkippedExistingApp;
            return false;
        }

        matches!(existing.default_app_state, DefaultAppState::Pending)
    }

    pub fn set_default_app_upload_result(
        &mut self,
        handle: api::NetHandle,
        uploaded: bool,
        now_ms: u64,
    ) -> bool {
        let Some(existing) = self.devices.iter_mut().find(|entry| entry.handle == handle) else {
            return false;
        };
        if uploaded {
            existing.default_app_state = DefaultAppState::Uploaded;
        }
        existing.last_activity_ms = now_ms;
        true
    }

    pub fn update_status(
        &mut self,
        handle: api::NetHandle,
        status: DeviceStatusSnapshot,
        now_ms: u64,
    ) -> Option<StatusChangeEvent> {
        let existing = self
            .devices
            .iter_mut()
            .find(|entry| entry.handle == handle)?;
        existing.last_activity_ms = now_ms;

        if existing.status.as_ref() == Some(&status) {
            return None;
        }

        let previous = existing.status.replace(status.clone());
        Some(StatusChangeEvent {
            handle,
            previous,
            current: status,
        })
    }

    pub fn set_ip_v4(&mut self, handle: api::NetHandle, addr: [u8; 4], port: u16) {
        if let Some(existing) = self.devices.iter_mut().find(|entry| entry.handle == handle) {
            existing.ip = Some(DeviceIp::V4(addr));
            existing.service_port = port;
        }
    }

    pub fn set_ip_v6(&mut self, handle: api::NetHandle, addr: [u8; 16], port: u16) {
        if let Some(existing) = self.devices.iter_mut().find(|entry| entry.handle == handle) {
            existing.ip = Some(DeviceIp::V6(addr));
            existing.service_port = port;
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

pub const fn device_handle_v4(addr: [u8; 4]) -> api::NetHandle {
    api::NetHandle(u32::from_be_bytes(addr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swarm_heartbeat_triggers_ws_connect() {
        let mut gate = GateDiscovery::new();
        let _ = gate.on_event(api::Event::Opened {
            handle: api::NetHandle(1),
            kind: api::SocketKind::Udp,
        });

        let from = api::EndpointV4::new([192, 168, 1, 42], ESP_UDP_BROADCAST_PORT);
        let step = gate.on_event(api::Event::UdpPacket {
            handle: api::NetHandle(1),
            from,
            data: api::ByteBuf::from_slice_trunc(ESP_SWARM_HEARTBEAT),
        });

        assert_eq!(step, GateAction::Signal(GateSignal::EspDiscovered(from)));
    }

    #[test]
    fn ignores_non_swarm_udp_payloads() {
        let mut gate = GateDiscovery::new();
        let _ = gate.on_event(api::Event::Opened {
            handle: api::NetHandle(3),
            kind: api::SocketKind::Udp,
        });

        let step = gate.on_event(api::Event::UdpPacket {
            handle: api::NetHandle(3),
            from: api::EndpointV4::new([10, 0, 0, 7], ESP_UDP_BROADCAST_PORT),
            data: api::ByteBuf::from_slice_trunc(b"noise"),
        });

        assert_eq!(step, GateAction::None);
    }

    #[test]
    fn reopens_udp_listener_after_close() {
        let mut gate = GateDiscovery::new();
        let _ = gate.on_event(api::Event::Opened {
            handle: api::NetHandle(9),
            kind: api::SocketKind::Udp,
        });

        assert_eq!(
            gate.on_event(api::Event::Closed {
                handle: api::NetHandle(9),
            }),
            GateAction::Submit(api::Command::OpenUdp {
                port: ESP_UDP_BROADCAST_PORT,
            })
        );
    }

    #[test]
    fn heartbeat_registry_dedupes_by_ipv4_handle() {
        let mut registry = DeviceRegistry::new(8);

        assert!(registry.upsert_heartbeat_v4([192, 168, 1, 42], ESP_HTTP_UPLOAD_PORT, 100));
        assert!(!registry.upsert_heartbeat_v4([192, 168, 1, 42], ESP_HTTP_UPLOAD_PORT, 200));
        assert_eq!(registry.len(), 1);

        let snapshot = registry
            .snapshot_for(device_handle_v4([192, 168, 1, 42]))
            .expect("device snapshot");
        assert_eq!(snapshot.service_port, ESP_HTTP_UPLOAD_PORT);
        assert_eq!(snapshot.connected_at_ms, 100);
        assert_eq!(snapshot.last_activity_ms, 200);
        assert_eq!(snapshot.status, None);
    }

    #[test]
    fn emits_status_change_event() {
        let mut registry = DeviceRegistry::new(8);
        let addr = [192, 168, 1, 42];
        let handle = device_handle_v4(addr);
        assert!(registry.upsert_heartbeat_v4(addr, ESP_HTTP_UPLOAD_PORT, 100));

        let mut last_status = String::new();
        let _ = last_status.push_str("running");
        let mut last_error = String::new();
        let _ = last_error.push_str("none");
        let status = DeviceStatusSnapshot {
            threading_available: true,
            app_exists: true,
            running: true,
            last_status,
            last_error,
            last_started_ms: Some(123),
            last_finished_ms: None,
        };

        let event = registry
            .update_status(handle, status.clone(), 200)
            .expect("status change event");
        assert_eq!(event.handle, handle);
        assert_eq!(event.previous, None);
        assert_eq!(event.current, status);
    }

    #[test]
    fn default_upload_is_decided_once() {
        let mut registry = DeviceRegistry::new(8);
        let addr = [192, 168, 1, 55];
        let handle = device_handle_v4(addr);
        assert!(registry.upsert_heartbeat_v4(addr, ESP_HTTP_UPLOAD_PORT, 100));

        assert!(registry.should_upload_default_app(handle, false));
        assert!(registry.should_upload_default_app(handle, false));
        assert!(registry.set_default_app_upload_result(handle, true, 200));
        assert!(!registry.should_upload_default_app(handle, false));
    }

    #[test]
    fn existing_app_skips_default_upload() {
        let mut registry = DeviceRegistry::new(8);
        let addr = [192, 168, 1, 56];
        let handle = device_handle_v4(addr);
        assert!(registry.upsert_heartbeat_v4(addr, ESP_HTTP_UPLOAD_PORT, 100));

        assert!(!registry.should_upload_default_app(handle, true));
        assert!(!registry.should_upload_default_app(handle, false));
    }
}
