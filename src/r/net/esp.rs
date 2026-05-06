use alloc::{collections::VecDeque, format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::VNet;

const ESP_GATE_REGISTRY_MAX_DEVICES: usize = 64;
const ESP_STATUS_FETCH_TIMEOUT_MS: u32 = 3000;
const ESP_STATUS_FETCH_MAX_RX: usize = 1024;
const ESP_STATUS_POLL_MS: u64 = 1000;
const ESP_CONTROL_TIMEOUT_MS: u32 = 3000;
const ESP_CONTROL_MAX_RX: usize = 1024;
const TRUEOS_PEER_ADVERTISE_MS: u64 = 5000;
const TRUEOS_PEER_HELLO_MAX: usize = 128;
const TRUEOS_LUMEN_WORK_FRAME_MAX: usize = 256;
const TRUEOS_LUMEN_WORK_PROBE_TIMEOUT_MS: u64 = 3000;
const TRUEOS_LUMEN_WORK_PROBE_PREFIX: &str = "C0DEC0DE LUMEN_CAN_TAKE_WORK";
const TRUEOS_LUMEN_WORK_CAP_PREFIX: &str = "C0DEC0DE LUMEN_WORK_CAP";
const TRUEOS_SWARM_HOST_CAP: usize = crate::allcaps::net::TRUEOS_SWARM_HOST_CAP;
const TRUEOS_PEER_LINK_CAP: usize = crate::allcaps::net::TRUEOS_SWARM_PEER_LINK_CAP;
const TRUEOS_PEER_RX_BUF_BYTES: usize = crate::allcaps::net::TRUEOS_SWARM_PEER_RX_BUF_BYTES;

static DEVICE_REGISTRY: Mutex<trueos_esp::gate::DeviceRegistry> =
    Mutex::new(trueos_esp::gate::DeviceRegistry::with_trueos_host_limit(
        ESP_GATE_REGISTRY_MAX_DEVICES,
        TRUEOS_SWARM_HOST_CAP,
    ));
static STATUS_EVENTS: Mutex<VecDeque<trueos_esp::swarm::StatusChangeEvent>> =
    Mutex::new(VecDeque::new());
static REGISTRY_CHANGE_SEQ: AtomicU32 = AtomicU32::new(1);
static LUMEN_WORK_PROBE_SEQ: AtomicU32 = AtomicU32::new(1);

fn monotonic_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

#[allow(dead_code)]
pub fn remove_device(handle: v::vnet::NetHandle) -> bool {
    let removed = DEVICE_REGISTRY.lock().remove_device(handle);
    if removed {
        note_registry_change();
    }
    removed
}

#[allow(dead_code)]
pub fn device_snapshot() -> Vec<trueos_esp::gate::DeviceSnapshot> {
    DEVICE_REGISTRY.lock().snapshot()
}

#[allow(dead_code)]
pub fn device_status_snapshot(
    handle: v::vnet::NetHandle,
) -> Option<trueos_esp::swarm::DeviceStatusSnapshot> {
    DEVICE_REGISTRY
        .lock()
        .snapshot_for(handle)
        .and_then(|snapshot| snapshot.status)
}

#[allow(dead_code)]
pub fn drain_status_events(max_events: usize) -> Vec<trueos_esp::swarm::StatusChangeEvent> {
    let mut out = Vec::new();
    let mut queue = STATUS_EVENTS.lock();
    for _ in 0..max_events {
        let Some(event) = queue.pop_front() else {
            break;
        };
        out.push(event);
    }
    out
}

#[allow(dead_code)]
pub fn registry_change_seq() -> u32 {
    REGISTRY_CHANGE_SEQ.load(Ordering::Acquire)
}

pub(crate) fn request_lumen_work_capacity_probe() {
    let seq = LUMEN_WORK_PROBE_SEQ
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    crate::log!(
        "esp-gate: lumen work capacity probe requested seq={} timeout_ms={}\n",
        seq,
        TRUEOS_LUMEN_WORK_PROBE_TIMEOUT_MS
    );
}

fn note_registry_change() {
    REGISTRY_CHANGE_SEQ.fetch_add(1, Ordering::AcqRel);
}

fn snapshot_for_handle(handle: v::vnet::NetHandle) -> Option<trueos_esp::gate::DeviceSnapshot> {
    DEVICE_REGISTRY.lock().snapshot_for(handle)
}

fn trueos_node_id(vnet: &VNet) -> u64 {
    if let Some(v::vnet::MacAddr(mac)) = vnet.mac_address() {
        let mut bytes = [0u8; 8];
        bytes[0] = 0xC0;
        bytes[1] = 0xDE;
        bytes[2..].copy_from_slice(&mac);
        return u64::from_be_bytes(bytes);
    }
    0
}

fn trueos_peer_hello(node_id: u64) -> String {
    format!(
        "{} v=1 node=0x{:016X} tcp={} caps=registry,status,lumen-work\n",
        trueos_esp::gate::TRUEOS_SWARM_MAGIC_TEXT,
        node_id,
        trueos_esp::gate::TRUEOS_PEER_TCP_PORT
    )
}

fn submit_trueos_peer_hello(vnet: &VNet, handle: v::vnet::NetHandle, node_id: u64) {
    let hello = trueos_peer_hello(node_id);
    let bytes = hello.as_bytes();
    let len = bytes.len().min(TRUEOS_PEER_HELLO_MAX);
    let _ = vnet.submit(v::vnet::Command::SendTcp {
        handle,
        data: v::vnet::ByteBuf::from_slice_trunc(&bytes[..len]),
    });
}

fn trueos_lumen_work_probe_received(data: &[u8]) -> bool {
    core::str::from_utf8(data)
        .map(|text| {
            text.lines().any(|line| {
                line.trim_start()
                    .starts_with(TRUEOS_LUMEN_WORK_PROBE_PREFIX)
            })
        })
        .unwrap_or(false)
}

#[derive(Copy, Clone, Debug, Default)]
struct LumenWorkCapacity {
    lanes: u32,
    protocol_version: u16,
    caps: u32,
    workers: u32,
    pending: u32,
    min_rows: u32,
}

fn parse_u32_field(part: &str, name: &str) -> Option<u32> {
    let value = part.strip_prefix(name)?;
    if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).ok()
    } else {
        value.parse::<u32>().ok()
    }
}

fn parse_trueos_lumen_work_capacity(data: &[u8]) -> Option<LumenWorkCapacity> {
    let text = core::str::from_utf8(data).ok()?;
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with(TRUEOS_LUMEN_WORK_CAP_PREFIX))?;
    let mut out = LumenWorkCapacity::default();
    for part in line.split_ascii_whitespace() {
        if let Some(value) = parse_u32_field(part, "n=") {
            out.lanes = value;
        } else if let Some(value) = parse_u32_field(part, "proto=") {
            out.protocol_version = value.min(u16::MAX as u32) as u16;
        } else if let Some(value) = parse_u32_field(part, "caps=") {
            out.caps = value;
        } else if let Some(value) = parse_u32_field(part, "workers=") {
            out.workers = value;
        } else if let Some(value) = parse_u32_field(part, "pending=") {
            out.pending = value;
        } else if let Some(value) = parse_u32_field(part, "min_rows=") {
            out.min_rows = value;
        }
    }
    Some(out)
}

fn submit_trueos_lumen_work_capacity(vnet: &VNet, handle: v::vnet::NetHandle) {
    let capacity = crate::r::lumen_service::remote_work_capacity();
    let telemetry = crate::lumen_net::backend_telemetry(capacity);
    let reply = format!(
        "{} LUMEN_WORK_CAP v=1 n={} proto={} caps=0x{:08X} workers={} pending={} min_rows={}\n",
        trueos_esp::gate::TRUEOS_SWARM_MAGIC_TEXT,
        telemetry.capacity_lanes,
        telemetry.protocol_version,
        telemetry.caps,
        telemetry.local_workers,
        telemetry.pending_bf16_matvecs,
        telemetry.min_remote_rows
    );
    let bytes = reply.as_bytes();
    let len = bytes.len().min(TRUEOS_LUMEN_WORK_FRAME_MAX);
    let _ = vnet.submit(v::vnet::Command::SendTcp {
        handle,
        data: v::vnet::ByteBuf::from_slice_trunc(&bytes[..len]),
    });
    crate::log!(
        "esp-gate: lumen work capacity reply handle={} n={} proto={} caps=0x{:08X} workers={} pending={} min_rows={} online={} running={}\n",
        handle.0,
        telemetry.capacity_lanes,
        telemetry.protocol_version,
        telemetry.caps,
        telemetry.local_workers,
        telemetry.pending_bf16_matvecs,
        telemetry.min_remote_rows,
        crate::r::lumen_service::is_online(),
        crate::r::lumen_service::is_prompt_running()
    );
}

fn submit_trueos_lumen_work_probe(vnet: &VNet, handle: v::vnet::NetHandle) {
    let probe = format!(
        "{} LUMEN_CAN_TAKE_WORK v=1 timeout_ms={}\n",
        trueos_esp::gate::TRUEOS_SWARM_MAGIC_TEXT,
        TRUEOS_LUMEN_WORK_PROBE_TIMEOUT_MS
    );
    let bytes = probe.as_bytes();
    let len = bytes.len().min(TRUEOS_LUMEN_WORK_FRAME_MAX);
    let _ = vnet.submit(v::vnet::Command::SendTcp {
        handle,
        data: v::vnet::ByteBuf::from_slice_trunc(&bytes[..len]),
    });
}

fn submit_trueos_lumen_work_probe_to_all(vnet: &VNet, links: &[TrueOsPeerLink]) -> usize {
    let mut sent = 0usize;
    for link in links.iter().filter(|link| link.handle.is_some()) {
        if let Some(handle) = link.handle {
            submit_trueos_lumen_work_probe(vnet, handle);
            sent = sent.saturating_add(1);
        }
    }
    sent
}

fn advertise_trueos_peer(vnet: &VNet, udp_handle: v::vnet::NetHandle, node_id: u64) {
    let hello = trueos_peer_hello(node_id);
    let bytes = hello.as_bytes();
    let len = bytes.len().min(TRUEOS_PEER_HELLO_MAX);
    let _ = vnet.submit(v::vnet::Command::SendUdp {
        handle: udp_handle,
        remote: v::vnet::EndpointV4::new(
            [255, 255, 255, 255],
            trueos_esp::gate::ESP_UDP_BROADCAST_PORT,
        ),
        data: v::vnet::ByteBuf::from_slice_trunc(&bytes[..len]),
    });
}

struct TrueOsPeerLink {
    handle: Option<v::vnet::NetHandle>,
    node_id: u64,
    rx: Vec<u8>,
}

fn allocate_trueos_peer_links() -> Vec<TrueOsPeerLink> {
    let mut links = Vec::with_capacity(TRUEOS_PEER_LINK_CAP);
    for _ in 0..TRUEOS_PEER_LINK_CAP {
        links.push(TrueOsPeerLink {
            handle: None,
            node_id: 0,
            rx: Vec::with_capacity(TRUEOS_PEER_RX_BUF_BYTES),
        });
    }
    links
}

fn trueos_peer_link_count(links: &[TrueOsPeerLink]) -> usize {
    links.iter().filter(|link| link.handle.is_some()).count()
}

fn trueos_peer_link_known(links: &[TrueOsPeerLink], node_id: u64) -> bool {
    node_id != 0
        && links
            .iter()
            .any(|link| link.handle.is_some() && link.node_id == node_id)
}

fn trueos_peer_link_has_room_or_known(links: &[TrueOsPeerLink], node_id: u64) -> bool {
    trueos_peer_link_known(links, node_id) || trueos_peer_link_count(links) < TRUEOS_PEER_LINK_CAP
}

fn ensure_trueos_peer_link(links: &mut [TrueOsPeerLink], handle: v::vnet::NetHandle) -> bool {
    if links.iter().any(|link| link.handle == Some(handle)) {
        return true;
    }

    let Some(link) = links.iter_mut().find(|link| link.handle.is_none()) else {
        return false;
    };

    link.handle = Some(handle);
    link.node_id = 0;
    link.rx.clear();
    true
}

fn remove_trueos_peer_link(links: &mut [TrueOsPeerLink], handle: v::vnet::NetHandle) -> bool {
    let Some(link) = links.iter_mut().find(|link| link.handle == Some(handle)) else {
        return false;
    };

    link.handle = None;
    link.node_id = 0;
    link.rx.clear();
    true
}

fn record_trueos_peer_data(
    links: &mut [TrueOsPeerLink],
    handle: v::vnet::NetHandle,
    data: &[u8],
) -> Option<trueos_esp::gate::TrueOsHostAdvertisement> {
    let link = links.iter_mut().find(|link| link.handle == Some(handle))?;

    if link.rx.len().saturating_add(data.len()) > TRUEOS_PEER_RX_BUF_BYTES {
        link.rx.clear();
    }
    let room = TRUEOS_PEER_RX_BUF_BYTES.saturating_sub(link.rx.len());
    link.rx.extend_from_slice(&data[..data.len().min(room)]);

    let advertisement = trueos_esp::gate::parse_trueos_host_advertisement(
        v::vnet::EndpointV4::new([0, 0, 0, 0], 0),
        link.rx.as_slice(),
    )?;
    link.node_id = advertisement.node_id;
    Some(advertisement)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EspControlError {
    DeviceMissing,
    DeviceUnreachable,
    UploadFailed,
    RunFailed,
    RestartFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EspRestartResult {
    pub restart_requested: bool,
    pub removed_from_registry: bool,
}

fn manual_endpoint_url(snapshot: &trueos_esp::gate::DeviceSnapshot, path: &str) -> Option<String> {
    if snapshot.class != trueos_esp::gate::DeviceClass::EspUploader {
        return None;
    }

    let path = path.strip_prefix('/').unwrap_or(path);
    match snapshot.ip {
        Some(trueos_esp::gate::DeviceIp::V4(addr)) => Some(format!(
            "http://{}.{}.{}.{}:{}/{}",
            addr[0], addr[1], addr[2], addr[3], snapshot.service_port, path
        )),
        Some(trueos_esp::gate::DeviceIp::V6(_)) | None => None,
    }
}

#[allow(dead_code)]
pub async fn upload_app_to_device(
    handle: v::vnet::NetHandle,
    source_name: &str,
    body: &[u8],
    target_name: &str,
) -> Result<(), EspControlError> {
    let Some(snapshot) = snapshot_for_handle(handle) else {
        return Err(EspControlError::DeviceMissing);
    };
    let iface = trueos_esp::swarm::DeviceInterface::from_snapshot(&snapshot);
    let Some(upload_url) = iface.upload_url() else {
        return Err(EspControlError::DeviceUnreachable);
    };
    crate::log!(
        "esp-gate: manual upload handle={} source={} target={} bytes={}\n",
        handle.0,
        source_name,
        target_name,
        body.len()
    );
    crate::t::net::http::post_http_body(
        upload_url.as_str(),
        &[("X-Filename", target_name)],
        body,
        ESP_CONTROL_TIMEOUT_MS,
        ESP_CONTROL_MAX_RX,
    )
    .await
    .map_err(|_| EspControlError::UploadFailed)?;

    let Some(run_url) = iface.run_url() else {
        return Err(EspControlError::DeviceUnreachable);
    };
    crate::t::net::http::post_http_body(
        run_url.as_str(),
        &[],
        &[],
        ESP_CONTROL_TIMEOUT_MS,
        ESP_CONTROL_MAX_RX,
    )
    .await
    .map_err(|_| EspControlError::RunFailed)?;

    Ok(())
}

#[allow(dead_code)]
pub async fn restart_device(
    handle: v::vnet::NetHandle,
) -> Result<EspRestartResult, EspControlError> {
    let Some(snapshot) = snapshot_for_handle(handle) else {
        return Err(EspControlError::DeviceMissing);
    };

    let Some(url) = manual_endpoint_url(&snapshot, trueos_esp::swarm::ESP_RESTART_PATH) else {
        return Err(EspControlError::DeviceUnreachable);
    };
    let restart_requested = crate::t::net::http::post_http_body(
        url.as_str(),
        &[],
        &[],
        ESP_CONTROL_TIMEOUT_MS,
        ESP_CONTROL_MAX_RX,
    )
    .await
    .is_ok();

    let removed_from_registry = remove_device(handle);
    if restart_requested {
        Ok(EspRestartResult {
            restart_requested,
            removed_from_registry,
        })
    } else if removed_from_registry {
        Ok(EspRestartResult {
            restart_requested,
            removed_from_registry,
        })
    } else {
        Err(EspControlError::RestartFailed)
    }
}

async fn poll_device_status(snapshot: &trueos_esp::gate::DeviceSnapshot) {
    let iface = trueos_esp::swarm::DeviceInterface::from_snapshot(snapshot);
    let Some(url) = iface.status_url() else {
        return;
    };

    match crate::t::net::http::fetch_http_body(
        url.as_str(),
        ESP_STATUS_FETCH_TIMEOUT_MS,
        ESP_STATUS_FETCH_MAX_RX,
    )
    .await
    {
        Ok(body) => {
            if let Some(status) = trueos_esp::swarm::parse_status_snapshot(body.as_slice()) {
                let now_ms = monotonic_ms();
                let event =
                    DEVICE_REGISTRY
                        .lock()
                        .update_status(snapshot.handle, status.clone(), now_ms);
                if let Some(event) = event {
                    crate::log!(
                        "esp-gate: status changed handle={} running={} last_status={} last_error={}\n",
                        event.handle.0,
                        if event.current.running { 1 } else { 0 },
                        event.current.last_status.as_str(),
                        event.current.last_error.as_str()
                    );
                    note_registry_change();
                    STATUS_EVENTS.lock().push_back(event);
                }
            } else {
                crate::log!(
                    "esp-gate: status parse failed handle={} url={} bytes={}\n",
                    snapshot.handle.0,
                    url.as_str(),
                    body.len()
                );
            }
        }
        Err(err) => {
            crate::log!(
                "esp-gate: status fetch failed handle={} url={} timeout_ms={} err={:?}\n",
                snapshot.handle.0,
                url.as_str(),
                ESP_STATUS_FETCH_TIMEOUT_MS,
                err
            );
        }
    }
}

#[embassy_executor::task]
pub async fn esp_gate_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

    loop {
        let Some(vnet) = VNet::open_primary() else {
            Timer::after(Duration::from_millis(100)).await;
            continue;
        };

        let mut gate = trueos_esp::gate::GateDiscovery::new();
        let local_node_id = trueos_node_id(&vnet);
        let mut udp_handle: Option<v::vnet::NetHandle> = None;
        let mut peer_listener: Option<v::vnet::NetHandle> = None;
        let mut peer_links = allocate_trueos_peer_links();
        let mut next_peer_advertise_ms = 0u64;
        let mut seen_lumen_work_probe_seq = LUMEN_WORK_PROBE_SEQ.load(Ordering::Acquire);
        let mut lumen_work_probe_seq = seen_lumen_work_probe_seq;
        let mut lumen_work_probe_deadline_ms = 0u64;
        let mut lumen_work_probe_sent = 0usize;
        let mut lumen_work_probe_replies = 0usize;
        let mut lumen_work_probe_best = 0u32;
        let _ = vnet.submit(gate.bootstrap_command());
        let _ = vnet.submit(v::vnet::Command::OpenTcpListen {
            port: trueos_esp::gate::TRUEOS_PEER_TCP_PORT,
        });
        crate::log!(
            "esp-gate: starting udp swarm listener on port {} payload=swarm trueos_magic={} peer_tcp={} node=0x{:016X} peer_slots={} rx_buf_bytes={}\n",
            trueos_esp::gate::ESP_UDP_BROADCAST_PORT,
            trueos_esp::gate::TRUEOS_SWARM_MAGIC_TEXT,
            trueos_esp::gate::TRUEOS_PEER_TCP_PORT,
            local_node_id,
            TRUEOS_PEER_LINK_CAP,
            TRUEOS_PEER_RX_BUF_BYTES
        );

        loop {
            if let Some(ev) = vnet.pop_event() {
                if let v::vnet::Event::Error { msg } = ev {
                    crate::log!("esp-gate: error {}\n", msg);
                }

                match ev {
                    v::vnet::Event::Opened {
                        handle,
                        kind: v::vnet::SocketKind::Tcp,
                    } if peer_listener.is_none() => {
                        peer_listener = Some(handle);
                        crate::log!(
                            "esp-gate: trueos peer tcp listener bound handle={} port={}\n",
                            handle.0,
                            trueos_esp::gate::TRUEOS_PEER_TCP_PORT
                        );
                    }
                    v::vnet::Event::TcpEstablished { handle } => {
                        if !ensure_trueos_peer_link(peer_links.as_mut_slice(), handle) {
                            crate::log!(
                                "esp-gate: trueos peer tcp rejected handle={} reason=peer-slot-cap cap={}\n",
                                handle.0,
                                TRUEOS_PEER_LINK_CAP
                            );
                            let _ = vnet.submit(v::vnet::Command::Close { handle });
                            continue;
                        }

                        crate::log!(
                            "esp-gate: trueos peer tcp established handle={} active_links={} sending hello\n",
                            handle.0,
                            trueos_peer_link_count(peer_links.as_slice())
                        );
                        submit_trueos_peer_hello(&vnet, handle, local_node_id);
                    }
                    v::vnet::Event::TcpData { handle, data } => {
                        if trueos_lumen_work_probe_received(data.as_slice()) {
                            submit_trueos_lumen_work_capacity(&vnet, handle);
                        }
                        if let Some(capacity) = parse_trueos_lumen_work_capacity(data.as_slice()) {
                            let now_ms = monotonic_ms();
                            if lumen_work_probe_deadline_ms != 0
                                && now_ms <= lumen_work_probe_deadline_ms
                            {
                                lumen_work_probe_replies =
                                    lumen_work_probe_replies.saturating_add(1);
                                lumen_work_probe_best = lumen_work_probe_best.max(capacity.lanes);
                            }
                            crate::log!(
                                "esp-gate: lumen work capacity received handle={} n={} proto={} caps=0x{:08X} workers={} pending={} min_rows={}\n",
                                handle.0,
                                capacity.lanes,
                                capacity.protocol_version,
                                capacity.caps,
                                capacity.workers,
                                capacity.pending,
                                capacity.min_rows
                            );
                        }
                        if let Some(advertisement) = record_trueos_peer_data(
                            peer_links.as_mut_slice(),
                            handle,
                            data.as_slice(),
                        ) {
                            crate::log!(
                                "esp-gate: trueos peer hello received handle={} node=0x{:016X} bytes={}\n",
                                handle.0,
                                advertisement.node_id,
                                data.len()
                            );
                        }
                    }
                    v::vnet::Event::Closed { handle } if peer_listener == Some(handle) => {
                        peer_listener = None;
                        let _ = remove_trueos_peer_link(peer_links.as_mut_slice(), handle);
                        crate::log!(
                            "esp-gate: trueos peer tcp listener closed, reopening port={}\n",
                            trueos_esp::gate::TRUEOS_PEER_TCP_PORT
                        );
                        let _ = vnet.submit(v::vnet::Command::OpenTcpListen {
                            port: trueos_esp::gate::TRUEOS_PEER_TCP_PORT,
                        });
                    }
                    v::vnet::Event::Closed { handle } => {
                        if remove_trueos_peer_link(peer_links.as_mut_slice(), handle) {
                            crate::log!(
                                "esp-gate: trueos peer tcp closed handle={} active_links={}\n",
                                handle.0,
                                trueos_peer_link_count(peer_links.as_slice())
                            );
                        }
                    }
                    _ => {}
                }

                match gate.on_event(ev) {
                    trueos_esp::gate::GateAction::None => {}
                    trueos_esp::gate::GateAction::Signal(signal) => match signal {
                        trueos_esp::gate::GateSignal::UdpBound(handle) => {
                            udp_handle = Some(handle);
                            next_peer_advertise_ms = 0;
                            crate::log!(
                                "esp-gate: udp listener bound handle={} port={}\n",
                                handle.0,
                                trueos_esp::gate::ESP_UDP_BROADCAST_PORT
                            );
                        }
                        trueos_esp::gate::GateSignal::EspDiscovered(from) => {
                            crate::log!(
                                "esp-gate: heartbeat=swarm from {}.{}.{}.{} upload_port={}\n",
                                from.addr[0],
                                from.addr[1],
                                from.addr[2],
                                from.addr[3],
                                trueos_esp::gate::ESP_HTTP_UPLOAD_PORT
                            );

                            let now_ms = monotonic_ms();
                            let is_new = {
                                let mut registry = DEVICE_REGISTRY.lock();
                                registry.upsert_heartbeat_v4(
                                    from.addr,
                                    trueos_esp::gate::ESP_HTTP_UPLOAD_PORT,
                                    now_ms,
                                )
                            };
                            if is_new {
                                note_registry_change();
                            }
                        }
                        trueos_esp::gate::GateSignal::TrueOsHostDiscovered(advertisement) => {
                            if advertisement.node_id != 0 && advertisement.node_id == local_node_id
                            {
                                continue;
                            }

                            crate::log!(
                                "esp-gate: heartbeat={} from {}.{}.{}.{} peer_tcp={} node=0x{:016X} caps=0x{:08X}\n",
                                trueos_esp::gate::TRUEOS_SWARM_MAGIC_TEXT,
                                advertisement.from.addr[0],
                                advertisement.from.addr[1],
                                advertisement.from.addr[2],
                                advertisement.from.addr[3],
                                advertisement.peer_tcp_port,
                                advertisement.node_id,
                                advertisement.caps
                            );

                            let now_ms = monotonic_ms();
                            let is_new = {
                                let mut registry = DEVICE_REGISTRY.lock();
                                registry.upsert_trueos_host_v4(
                                    advertisement.from.addr,
                                    advertisement.peer_tcp_port,
                                    advertisement.node_id,
                                    advertisement.caps,
                                    now_ms,
                                )
                            };
                            if is_new {
                                note_registry_change();
                                if trueos_peer_link_has_room_or_known(
                                    peer_links.as_slice(),
                                    advertisement.node_id,
                                ) {
                                    let _ = vnet.submit(v::vnet::Command::OpenTcpConnect {
                                        remote: v::vnet::EndpointV4::new(
                                            advertisement.from.addr,
                                            advertisement.peer_tcp_port,
                                        ),
                                    });
                                } else {
                                    crate::log!(
                                        "esp-gate: trueos peer dial skipped node=0x{:016X} reason=peer-slot-cap cap={}\n",
                                        advertisement.node_id,
                                        TRUEOS_PEER_LINK_CAP
                                    );
                                }
                            }
                        }
                    },
                    trueos_esp::gate::GateAction::Submit(cmd) => {
                        let _ = vnet.submit(cmd);
                    }
                }

                continue;
            }

            let now_ms = monotonic_ms();
            if let Some(handle) = udp_handle
                && now_ms >= next_peer_advertise_ms
            {
                advertise_trueos_peer(&vnet, handle, local_node_id);
                next_peer_advertise_ms = now_ms.saturating_add(TRUEOS_PEER_ADVERTISE_MS);
            }

            let requested_lumen_work_probe_seq = LUMEN_WORK_PROBE_SEQ.load(Ordering::Acquire);
            if requested_lumen_work_probe_seq != seen_lumen_work_probe_seq {
                seen_lumen_work_probe_seq = requested_lumen_work_probe_seq;
                lumen_work_probe_seq = requested_lumen_work_probe_seq;
                lumen_work_probe_replies = 0;
                lumen_work_probe_best = 0;
                lumen_work_probe_sent =
                    submit_trueos_lumen_work_probe_to_all(&vnet, peer_links.as_slice());
                if lumen_work_probe_sent == 0 {
                    lumen_work_probe_deadline_ms = 0;
                    crate::log!(
                        "esp-gate: lumen work capacity probe seq={} skipped reason=no-peers\n",
                        lumen_work_probe_seq
                    );
                } else {
                    lumen_work_probe_deadline_ms =
                        now_ms.saturating_add(TRUEOS_LUMEN_WORK_PROBE_TIMEOUT_MS);
                    crate::log!(
                        "esp-gate: lumen work capacity probe seq={} sent={} timeout_ms={}\n",
                        lumen_work_probe_seq,
                        lumen_work_probe_sent,
                        TRUEOS_LUMEN_WORK_PROBE_TIMEOUT_MS
                    );
                }
            }

            if lumen_work_probe_deadline_ms != 0 && now_ms >= lumen_work_probe_deadline_ms {
                crate::log!(
                    "esp-gate: lumen work capacity probe seq={} complete sent={} replies={} best={}\n",
                    lumen_work_probe_seq,
                    lumen_work_probe_sent,
                    lumen_work_probe_replies,
                    lumen_work_probe_best
                );
                lumen_work_probe_deadline_ms = 0;
            }

            Timer::after(Duration::from_millis(10)).await;
        }
    }
}

#[embassy_executor::task]
pub async fn esp_gate_registry_task() {
    let mut heartbeat_tick = 0u32;
    let mut status_poll_index = 0usize;

    loop {
        let snapshot_to_poll = {
            let snapshots = DEVICE_REGISTRY.lock().snapshot();
            if snapshots.is_empty() {
                None
            } else {
                let idx = status_poll_index % snapshots.len();
                status_poll_index = status_poll_index.wrapping_add(1);
                Some(snapshots[idx].clone())
            }
        };

        if let Some(snapshot) = snapshot_to_poll.as_ref() {
            poll_device_status(snapshot).await;
        }

        heartbeat_tick = heartbeat_tick.wrapping_add(1);
        if heartbeat_tick >= 20 {
            heartbeat_tick = 0;
            let count = DEVICE_REGISTRY.lock().len();
            if count != 0 {
                crate::log!("esp-gate: registry active_devices={}\n", count);
            }
        }

        Timer::after(Duration::from_millis(ESP_STATUS_POLL_MS)).await;
    }
}

#[embassy_executor::task]
pub async fn esp_piano_udp_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

    loop {
        let Some(vnet) = VNet::open_primary() else {
            Timer::after(Duration::from_millis(100)).await;
            continue;
        };

        let mut piano = trueos_esp::piano::PianoUdpReceiver::new();
        let _ = vnet.submit(piano.bootstrap_command());
        crate::log!(
            "esp-piano: starting udp listener port={} keys={}\n",
            trueos_esp::piano::TRUEOS_PIANO_UDP_PORT,
            trueos_esp::piano::PIANO_KEY_COUNT
        );

        loop {
            if let Some(ev) = vnet.pop_event() {
                match ev {
                    v::vnet::Event::Opened { handle, kind } if kind == v::vnet::SocketKind::Udp => {
                        piano.bind(handle);
                        crate::log!(
                            "esp-piano: udp listener bound handle={} port={}\n",
                            handle.0,
                            trueos_esp::piano::TRUEOS_PIANO_UDP_PORT
                        );
                    }
                    v::vnet::Event::Closed { handle } if piano.unbind(handle) => {
                        crate::log!("esp-piano: udp listener closed, reopening\n");
                        let _ = vnet.submit(piano.bootstrap_command());
                    }
                    v::vnet::Event::UdpPacket { handle, from, data } => {
                        let handled = piano.on_packet(handle, data.as_slice(), |event| {
                            let duration_ms = 45 + (u32::from(event.velocity) * 140 / 127);
                            crate::log!(
                                "esp-piano: note key={} note={} velocity={} delta={} from={}.{}.{}.{}\n",
                                event.key_index,
                                event.note,
                                event.velocity,
                                event.delta,
                                from.addr[0],
                                from.addr[1],
                                from.addr[2],
                                from.addr[3]
                            );
                            if let Err(err) =
                                crate::aud::play_midi_note(event.note, event.velocity, duration_ms)
                            {
                                crate::log!("esp-piano: note play err={}\n", err);
                            }
                        });
                        if !handled {
                            crate::log!(
                                "esp-piano: ignored udp bytes={} from={}.{}.{}.{}:{}\n",
                                data.len(),
                                from.addr[0],
                                from.addr[1],
                                from.addr[2],
                                from.addr[3],
                                from.port
                            );
                        }
                    }
                    v::vnet::Event::Error { msg } => {
                        crate::log!("esp-piano: error {}\n", msg);
                    }
                    v::vnet::Event::UdpPacketV6 { .. }
                    | v::vnet::Event::TcpEstablished { .. }
                    | v::vnet::Event::TcpData { .. }
                    | v::vnet::Event::TcpSent { .. }
                    | v::vnet::Event::IcmpReply { .. }
                    | v::vnet::Event::IcmpReplyV6 { .. }
                    | v::vnet::Event::Opened { .. }
                    | v::vnet::Event::Closed { .. } => {}
                }

                continue;
            }

            Timer::after(Duration::from_millis(5)).await;
        }
    }
}
