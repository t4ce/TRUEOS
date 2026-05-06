use alloc::vec::Vec;

use heapless::String;
use v::vnet as api;

use crate::swarm::{DeviceStatusSnapshot, StatusChangeEvent};

pub const DEVICE_TAG_CAP: usize = 16;
pub const ESP_HTTP_UPLOAD_PORT: u16 = 8080;
pub const ESP_UDP_BROADCAST_PORT: u16 = 32343;
pub const ESP_SWARM_HEARTBEAT: &[u8; 5] = b"swarm";
pub const TRUEOS_SWARM_MAGIC_TEXT: &str = "C0DEC0DE";
pub const TRUEOS_PEER_TCP_PORT: u16 = 32344;

pub mod device_caps {
    pub const REGISTRY: u32 = 1 << 0;
    pub const STATUS: u32 = 1 << 1;
    pub const FS: u32 = 1 << 2;
    pub const RPC: u32 = 1 << 3;
    pub const LUMEN_WORK: u32 = 1 << 4;

    pub const TRUEOS_HOST_DEFAULT: u32 = REGISTRY | STATUS | LUMEN_WORK;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceClass {
    EspUploader,
    TrueOsHost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrueOsHostAdvertisement {
    pub from: api::EndpointV4,
    pub peer_tcp_port: u16,
    pub node_id: u64,
    pub caps: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateSignal {
    UdpBound(api::NetHandle),
    EspDiscovered(api::EndpointV4),
    TrueOsHostDiscovered(TrueOsHostAdvertisement),
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
            api::Event::UdpPacket { handle, from, data } if self.udp_handle == Some(handle) => {
                match parse_trueos_host_advertisement(from, data.as_slice()) {
                    Some(advertisement) => {
                        GateAction::Signal(GateSignal::TrueOsHostDiscovered(advertisement))
                    }
                    None => GateAction::None,
                }
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

pub fn parse_trueos_host_advertisement(
    from: api::EndpointV4,
    data: &[u8],
) -> Option<TrueOsHostAdvertisement> {
    let text = core::str::from_utf8(data).ok()?.trim();
    let rest = text.strip_prefix(TRUEOS_SWARM_MAGIC_TEXT)?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }

    let mut peer_tcp_port = TRUEOS_PEER_TCP_PORT;
    let mut node_id = 0;
    let mut caps = device_caps::TRUEOS_HOST_DEFAULT;
    let mut saw_advertisement_field = false;

    for token in rest.split_whitespace() {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        match key {
            "tcp" | "port" => {
                if let Ok(port) = value.parse::<u16>() {
                    peer_tcp_port = port;
                    saw_advertisement_field = true;
                }
            }
            "node" | "node_id" => {
                if let Some(parsed) = parse_u64_token(value) {
                    node_id = parsed;
                    saw_advertisement_field = true;
                }
            }
            "caps" => {
                caps = parse_caps_token(value);
                saw_advertisement_field = true;
            }
            _ => {}
        }
    }

    if !saw_advertisement_field {
        return None;
    }

    Some(TrueOsHostAdvertisement {
        from,
        peer_tcp_port,
        node_id,
        caps,
    })
}

fn parse_u64_token(value: &str) -> Option<u64> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).ok()
    } else {
        value
            .parse::<u64>()
            .ok()
            .or_else(|| u64::from_str_radix(value, 16).ok())
    }
}

fn parse_caps_token(value: &str) -> u32 {
    if let Some(caps) = parse_u64_token(value) {
        return caps as u32;
    }

    let mut caps = 0;
    for cap in value.split(',') {
        match cap {
            "registry" => caps |= device_caps::REGISTRY,
            "status" => caps |= device_caps::STATUS,
            "fs" => caps |= device_caps::FS,
            "rpc" => caps |= device_caps::RPC,
            "lumen" | "lumen-work" | "lumen_work" => caps |= device_caps::LUMEN_WORK,
            _ => {}
        }
    }
    caps
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceIp {
    V4([u8; 4]),
    V6([u8; 16]),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSnapshot {
    pub handle: api::NetHandle,
    pub class: DeviceClass,
    pub tag: String<DEVICE_TAG_CAP>,
    pub ip: Option<DeviceIp>,
    pub service_port: u16,
    pub node_id: u64,
    pub caps: u32,
    pub connected_at_ms: u64,
    pub last_activity_ms: u64,
    pub status: Option<DeviceStatusSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceRecord {
    pub handle: api::NetHandle,
    pub class: DeviceClass,
    pub ip: Option<DeviceIp>,
    pub service_port: u16,
    pub node_id: u64,
    pub caps: u32,
    pub connected_at_ms: u64,
    pub last_activity_ms: u64,
    pub status: Option<DeviceStatusSnapshot>,
}

pub struct DeviceRegistry {
    devices: Vec<DeviceRecord>,
    max_devices: usize,
    max_trueos_hosts: usize,
}

impl DeviceRegistry {
    pub const fn new(max_devices: usize) -> Self {
        Self::with_trueos_host_limit(max_devices, max_devices)
    }

    pub const fn with_trueos_host_limit(max_devices: usize, max_trueos_hosts: usize) -> Self {
        Self {
            devices: Vec::new(),
            max_devices,
            max_trueos_hosts,
        }
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    pub fn trueos_host_len(&self) -> usize {
        self.devices
            .iter()
            .filter(|entry| entry.class == DeviceClass::TrueOsHost)
            .count()
    }

    pub fn snapshot(&self) -> Vec<DeviceSnapshot> {
        self.devices
            .iter()
            .map(|entry| DeviceSnapshot {
                handle: entry.handle,
                class: entry.class,
                tag: device_tag(entry.class),
                ip: entry.ip,
                service_port: entry.service_port,
                node_id: entry.node_id,
                caps: entry.caps,
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
                class: entry.class,
                tag: device_tag(entry.class),
                ip: entry.ip,
                service_port: entry.service_port,
                node_id: entry.node_id,
                caps: entry.caps,
                connected_at_ms: entry.connected_at_ms,
                last_activity_ms: entry.last_activity_ms,
                status: entry.status.clone(),
            })
    }

    pub fn upsert_heartbeat_v4(&mut self, addr: [u8; 4], service_port: u16, now_ms: u64) -> bool {
        self.upsert_device_v4(
            DeviceClass::EspUploader,
            addr,
            service_port,
            0,
            0,
            now_ms,
        )
    }

    pub fn upsert_trueos_host_v4(
        &mut self,
        addr: [u8; 4],
        peer_tcp_port: u16,
        node_id: u64,
        caps: u32,
        now_ms: u64,
    ) -> bool {
        self.upsert_device_v4(
            DeviceClass::TrueOsHost,
            addr,
            peer_tcp_port,
            node_id,
            caps,
            now_ms,
        )
    }

    fn upsert_device_v4(
        &mut self,
        class: DeviceClass,
        addr: [u8; 4],
        service_port: u16,
        node_id: u64,
        caps: u32,
        now_ms: u64,
    ) -> bool {
        let handle = device_handle_for_v4(class, addr, node_id);
        if let Some(existing) = self.devices.iter_mut().find(|entry| entry.handle == handle) {
            existing.class = class;
            existing.ip = Some(DeviceIp::V4(addr));
            existing.service_port = service_port;
            existing.node_id = node_id;
            existing.caps = caps;
            existing.last_activity_ms = now_ms;
            return false;
        }

        if class == DeviceClass::TrueOsHost
            && self.max_trueos_hosts != 0
            && self.trueos_host_len() >= self.max_trueos_hosts
        {
            if let Some(idx) = self
                .devices
                .iter()
                .position(|entry| entry.class == DeviceClass::TrueOsHost)
            {
                let _ = self.devices.remove(idx);
            }
        }

        if self.max_devices != 0 && self.devices.len() >= self.max_devices {
            let _ = self.devices.remove(0);
        }

        self.devices.push(DeviceRecord {
            handle,
            class,
            ip: Some(DeviceIp::V4(addr)),
            service_port,
            node_id,
            caps,
            connected_at_ms: now_ms,
            last_activity_ms: now_ms,
            status: None,
        });
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

fn device_tag(class: DeviceClass) -> String<DEVICE_TAG_CAP> {
    let mut tag = String::new();
    let _ = tag.push_str(match class {
        DeviceClass::EspUploader => "esp",
        DeviceClass::TrueOsHost => "trueos",
    });
    tag
}

pub const fn device_handle_v4(addr: [u8; 4]) -> api::NetHandle {
    api::NetHandle(u32::from_be_bytes(addr))
}

pub fn trueos_host_handle_v4(addr: [u8; 4], node_id: u64) -> api::NetHandle {
    let folded = ((node_id >> 32) as u32) ^ (node_id as u32);
    let seed = if folded == 0 {
        u32::from_be_bytes(addr)
    } else {
        folded
    };
    api::NetHandle(0xC000_0000 ^ seed)
}

fn device_handle_for_v4(class: DeviceClass, addr: [u8; 4], node_id: u64) -> api::NetHandle {
    match class {
        DeviceClass::EspUploader => device_handle_v4(addr),
        DeviceClass::TrueOsHost => trueos_host_handle_v4(addr, node_id),
    }
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
    fn c0dec0de_discovers_trueos_host() {
        let mut gate = GateDiscovery::new();
        let _ = gate.on_event(api::Event::Opened {
            handle: api::NetHandle(7),
            kind: api::SocketKind::Udp,
        });

        let from = api::EndpointV4::new([10, 0, 0, 23], ESP_UDP_BROADCAST_PORT);
        let step = gate.on_event(api::Event::UdpPacket {
            handle: api::NetHandle(7),
            from,
            data: api::ByteBuf::from_slice_trunc(
                b"C0DEC0DE v=1 node=0x1234 tcp=32344 caps=registry,status,fs",
            ),
        });

        assert_eq!(
            step,
            GateAction::Signal(GateSignal::TrueOsHostDiscovered(
                TrueOsHostAdvertisement {
                    from,
                    peer_tcp_port: TRUEOS_PEER_TCP_PORT,
                    node_id: 0x1234,
                    caps: device_caps::REGISTRY | device_caps::STATUS | device_caps::FS,
                },
            ))
        );
    }

    #[test]
    fn c0dec0de_work_frame_is_not_host_advertisement() {
        let from = api::EndpointV4::new([10, 0, 0, 23], ESP_UDP_BROADCAST_PORT);
        assert_eq!(
            parse_trueos_host_advertisement(from, b"C0DEC0DE LUMEN_CAN_TAKE_WORK v=1"),
            None
        );
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
        assert_eq!(snapshot.class, DeviceClass::EspUploader);
        assert_eq!(snapshot.tag.as_str(), "esp");
        assert_eq!(snapshot.service_port, ESP_HTTP_UPLOAD_PORT);
        assert_eq!(snapshot.connected_at_ms, 100);
        assert_eq!(snapshot.last_activity_ms, 200);
        assert_eq!(snapshot.status, None);
    }

    #[test]
    fn trueos_host_registry_uses_trueos_class() {
        let mut registry = DeviceRegistry::new(8);
        let addr = [10, 0, 0, 23];
        let node_id = 0xC0DE;

        assert!(registry.upsert_trueos_host_v4(
            addr,
            TRUEOS_PEER_TCP_PORT,
            node_id,
            device_caps::TRUEOS_HOST_DEFAULT,
            100
        ));
        assert!(!registry.upsert_trueos_host_v4(
            addr,
            TRUEOS_PEER_TCP_PORT,
            node_id,
            device_caps::TRUEOS_HOST_DEFAULT,
            200
        ));

        let snapshot = registry
            .snapshot_for(trueos_host_handle_v4(addr, node_id))
            .expect("trueos device snapshot");
        assert_eq!(snapshot.class, DeviceClass::TrueOsHost);
        assert_eq!(snapshot.tag.as_str(), "trueos");
        assert_eq!(snapshot.service_port, TRUEOS_PEER_TCP_PORT);
        assert_eq!(snapshot.node_id, node_id);
        assert_eq!(snapshot.caps, device_caps::TRUEOS_HOST_DEFAULT);
        assert_eq!(snapshot.connected_at_ms, 100);
        assert_eq!(snapshot.last_activity_ms, 200);
    }

    #[test]
    fn trueos_host_registry_obeys_class_limit() {
        let mut registry = DeviceRegistry::with_trueos_host_limit(8, 2);

        assert!(registry.upsert_trueos_host_v4(
            [10, 0, 0, 1],
            TRUEOS_PEER_TCP_PORT,
            1,
            device_caps::TRUEOS_HOST_DEFAULT,
            100
        ));
        assert!(registry.upsert_trueos_host_v4(
            [10, 0, 0, 2],
            TRUEOS_PEER_TCP_PORT,
            2,
            device_caps::TRUEOS_HOST_DEFAULT,
            200
        ));
        assert!(registry.upsert_trueos_host_v4(
            [10, 0, 0, 3],
            TRUEOS_PEER_TCP_PORT,
            3,
            device_caps::TRUEOS_HOST_DEFAULT,
            300
        ));

        assert_eq!(registry.trueos_host_len(), 2);
        assert!(registry
            .snapshot_for(trueos_host_handle_v4([10, 0, 0, 1], 1))
            .is_none());
        assert!(registry
            .snapshot_for(trueos_host_handle_v4([10, 0, 0, 2], 2))
            .is_some());
        assert!(registry
            .snapshot_for(trueos_host_handle_v4([10, 0, 0, 3], 3))
            .is_some());
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
            last_heartbeat_ms: Some(150),
            heartbeat_count: 1,
        };

        let event = registry
            .update_status(handle, status.clone(), 200)
            .expect("status change event");
        assert_eq!(event.handle, handle);
        assert_eq!(event.previous, None);
        assert_eq!(event.current, status);
    }
}
