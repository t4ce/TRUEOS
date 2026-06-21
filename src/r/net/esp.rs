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
static DEVICE_REGISTRY: Mutex<trueos_esp::gate::DeviceRegistry> =
    Mutex::new(trueos_esp::gate::DeviceRegistry::new(ESP_GATE_REGISTRY_MAX_DEVICES));
static STATUS_EVENTS: Mutex<VecDeque<trueos_esp::swarm::StatusChangeEvent>> =
    Mutex::new(VecDeque::new());
static REGISTRY_CHANGE_SEQ: AtomicU32 = AtomicU32::new(1);

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

fn note_registry_change() {
    REGISTRY_CHANGE_SEQ.fetch_add(1, Ordering::AcqRel);
}

fn snapshot_for_handle(handle: v::vnet::NetHandle) -> Option<trueos_esp::gate::DeviceSnapshot> {
    DEVICE_REGISTRY.lock().snapshot_for(handle)
}

pub(crate) fn publish_swarm_heartbeat_v4(from: v::vnet::EndpointV4) {
    crate::globalog::log_with_level(
        log::Level::Trace,
        format_args!(
            "esp-gate: heartbeat=swarm from {}.{}.{}.{} upload_port={}\n",
            from.addr[0],
            from.addr[1],
            from.addr[2],
            from.addr[3],
            trueos_esp::gate::ESP_HTTP_UPLOAD_PORT
        ),
    );

    let now_ms = monotonic_ms();
    let is_new = {
        let mut registry = DEVICE_REGISTRY.lock();
        registry.upsert_heartbeat_v4(from.addr, trueos_esp::gate::ESP_HTTP_UPLOAD_PORT, now_ms)
    };
    if is_new {
        note_registry_change();
    }
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
    crate::surfer::html_shack::post_bytes_via_pool(
        upload_url.as_str(),
        "application/octet-stream",
        &[("X-Filename", target_name)],
        body,
        ESP_CONTROL_TIMEOUT_MS as u64,
        ESP_CONTROL_MAX_RX,
    )
    .await
    .map_err(|_| EspControlError::UploadFailed)?;

    let Some(run_url) = iface.run_url() else {
        return Err(EspControlError::DeviceUnreachable);
    };
    crate::surfer::html_shack::post_bytes_via_pool(
        run_url.as_str(),
        "",
        &[],
        &[],
        ESP_CONTROL_TIMEOUT_MS as u64,
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
    let restart_requested = crate::surfer::html_shack::post_bytes_via_pool(
        url.as_str(),
        "",
        &[],
        &[],
        ESP_CONTROL_TIMEOUT_MS as u64,
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

    match crate::surfer::html_shack::fetch_bytes_via_pool(
        url.as_str(),
        ESP_STATUS_FETCH_TIMEOUT_MS as u64,
        ESP_STATUS_FETCH_MAX_RX,
    )
    .await
    {
        Ok(fetch) => {
            if let Some(status) = trueos_esp::swarm::parse_status_snapshot(fetch.bytes.as_slice()) {
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
                    fetch.bytes.len()
                );
            }
        }
        Err(err) => {
            crate::log!(
                "esp-gate: status fetch failed handle={} url={} timeout_ms={} err={}\n",
                snapshot.handle.0,
                url.as_str(),
                ESP_STATUS_FETCH_TIMEOUT_MS,
                err
            );
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
                            let kind = match event.kind {
                                trueos_esp::piano::PianoNoteEventKind::Down => "down",
                                trueos_esp::piano::PianoNoteEventKind::Up => "up",
                            };
                            crate::log!(
                                "esp-piano: note {} key={} note={} velocity={} delta={} from={}.{}.{}.{}\n",
                                kind,
                                event.key_index,
                                event.note,
                                event.velocity,
                                event.delta,
                                from.addr[0],
                                from.addr[1],
                                from.addr[2],
                                from.addr[3]
                            );

                            match event.kind {
                                trueos_esp::piano::PianoNoteEventKind::Down => {
                                    crate::aud::live_piano::note_on(event.note, event.velocity);
                                }
                                trueos_esp::piano::PianoNoteEventKind::Up => {
                                    crate::aud::live_piano::note_off(event.note);
                                }
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
