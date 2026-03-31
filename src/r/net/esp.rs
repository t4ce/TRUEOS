use alloc::{collections::VecDeque, format, string::String, vec::Vec};

use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::VNet;

const ESP_GATE_REGISTRY_MAX_DEVICES: usize = 64;
const ESP_STATUS_FETCH_TIMEOUT_MS: u32 = 750;
const ESP_STATUS_FETCH_MAX_RX: usize = 1024;
const ESP_STATUS_POLL_MS: u64 = 1000;

static DEVICE_REGISTRY: Mutex<trueos_esp::gate::DeviceRegistry> =
    Mutex::new(trueos_esp::gate::DeviceRegistry::new(ESP_GATE_REGISTRY_MAX_DEVICES));
static STATUS_EVENTS: Mutex<VecDeque<trueos_esp::swarm::StatusChangeEvent>> =
    Mutex::new(VecDeque::new());

fn monotonic_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

#[allow(dead_code)]
pub fn remove_device(handle: v::vnet::NetHandle) -> bool {
    DEVICE_REGISTRY.lock().remove_device(handle)
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

fn device_label(snapshot: &trueos_esp::gate::DeviceSnapshot) -> String {
    match snapshot.ip {
        Some(trueos_esp::gate::DeviceIp::V4(addr)) => {
            format!("{}.{}.{}.{}:{}", addr[0], addr[1], addr[2], addr[3], snapshot.service_port)
        }
        Some(trueos_esp::gate::DeviceIp::V6(addr)) => format!(
            "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{}",
            u16::from_be_bytes([addr[0], addr[1]]),
            u16::from_be_bytes([addr[2], addr[3]]),
            u16::from_be_bytes([addr[4], addr[5]]),
            u16::from_be_bytes([addr[6], addr[7]]),
            u16::from_be_bytes([addr[8], addr[9]]),
            u16::from_be_bytes([addr[10], addr[11]]),
            u16::from_be_bytes([addr[12], addr[13]]),
            u16::from_be_bytes([addr[14], addr[15]]),
            snapshot.service_port
        ),
        None => format!("pending:{}", snapshot.service_port),
    }
}

fn device_html(snapshot: &trueos_esp::gate::DeviceSnapshot) -> String {
    let endpoint = device_label(snapshot);
    let status_rows = if let Some(status) = snapshot.status.as_ref() {
        format!(
            concat!(
                "<dt>threading</dt><dd>{threading}</dd>",
                "<dt>app_exists</dt><dd>{app_exists}</dd>",
                "<dt>running</dt><dd>{running}</dd>",
                "<dt>last_status</dt><dd>{last_status}</dd>",
                "<dt>last_error</dt><dd>{last_error}</dd>",
                "<dt>last_started_ms</dt><dd>{last_started_ms}</dd>",
                "<dt>last_finished_ms</dt><dd>{last_finished_ms}</dd>"
            ),
            threading = if status.threading_available {
                "true"
            } else {
                "false"
            },
            app_exists = if status.app_exists { "true" } else { "false" },
            running = if status.running { "true" } else { "false" },
            last_status = status.last_status.as_str(),
            last_error = status.last_error.as_str(),
            last_started_ms = status
                .last_started_ms
                .map(|value| format!("{}", value))
                .unwrap_or_else(|| String::from("None")),
            last_finished_ms = status
                .last_finished_ms
                .map(|value| format!("{}", value))
                .unwrap_or_else(|| String::from("None")),
        )
    } else {
        String::from("<dt>status</dt><dd>pending</dd>")
    };
    format!(
        concat!(
            "<!doctype html><html><head><meta charset=\"utf-8\">",
            "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">",
            "<title>ESP Device {handle}</title>",
            "<style>",
            "body{{font-family:monospace;background:#f4f1e8;color:#171412;padding:24px;}}",
            ".card{{max-width:560px;background:#fffaf0;border:1px solid #171412;padding:18px;}}",
            "h1{{margin:0 0 14px;font-size:24px;}}",
            "dl{{display:grid;grid-template-columns:120px 1fr;gap:8px 12px;margin:0;}}",
            "dt{{font-weight:700;}}dd{{margin:0;word-break:break-word;}}",
            "</style></head><body><div class=\"card\">",
            "<h1>ESP Device</h1><dl>",
            "<dt>tag</dt><dd>{tag}</dd>",
            "<dt>handle</dt><dd>{handle}</dd>",
            "<dt>endpoint</dt><dd>{endpoint}</dd>",
            "<dt>upload</dt><dd>POST /upload, POST /run</dd>",
            "<dt>connected_ms</dt><dd>{connected_at_ms}</dd>",
            "<dt>last_activity_ms</dt><dd>{last_activity_ms}</dd>",
            "{status_rows}",
            "</dl></div></body></html>"
        ),
        tag = snapshot.tag.as_str(),
        handle = snapshot.handle.0,
        endpoint = endpoint,
        connected_at_ms = snapshot.connected_at_ms,
        last_activity_ms = snapshot.last_activity_ms,
        status_rows = status_rows,
    )
}

async fn poll_device_status(snapshot: &trueos_esp::gate::DeviceSnapshot) {
    let iface = trueos_esp::swarm::DeviceInterface::from_snapshot(snapshot);
    let Some(url) = iface.status_url() else {
        return;
    };

    let Ok(body) = crate::r::net::cli::http::fetch_http_body(
        url.as_str(),
        ESP_STATUS_FETCH_TIMEOUT_MS,
        ESP_STATUS_FETCH_MAX_RX,
    )
    .await
    else {
        return;
    };

    let Some(status) = trueos_esp::swarm::parse_status_snapshot(body.as_slice()) else {
        crate::log!(
            "esp-gate: status parse failed handle={} url={} bytes={}\n",
            snapshot.handle.0,
            url.as_str(),
            body.len()
        );
        return;
    };

    let now_ms = monotonic_ms();
    let event = DEVICE_REGISTRY
        .lock()
        .update_status(snapshot.handle, status, now_ms);
    if let Some(event) = event {
        crate::log!(
            "esp-gate: status changed handle={} running={} last_status={} last_error={}\n",
            event.handle.0,
            if event.current.running { 1 } else { 0 },
            event.current.last_status.as_str(),
            event.current.last_error.as_str()
        );
        STATUS_EVENTS.lock().push_back(event);
    }
}

async fn handoff_device_to_truesurfer(snapshot: &trueos_esp::gate::DeviceSnapshot) {
    let Some(browser_instance_id) = crate::r::spawn_service::spawn_truesurfer_tab_with_html()
    else {
        crate::log!(
            "esp-gate: browser handoff skipped handle={} reason=spawn_failed\n",
            snapshot.handle.0
        );
        return;
    };

    let url = format!("html://esp-device/{}", snapshot.handle.0);
    let handed_off = trueos_qjs::browser_task::queue_set_html_with_url_for_browser(
        browser_instance_id,
        device_html(snapshot),
        Some(url),
    )
    .await;
    crate::log!(
        "esp-gate: browser handoff handle={} browser={} ok={}\n",
        snapshot.handle.0,
        browser_instance_id,
        if handed_off { 1 } else { 0 }
    );
}

#[embassy_executor::task]
pub async fn esp_gate_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_CONFIGURED).await;

    loop {
        let Some(vnet) = VNet::open_primary() else {
            Timer::after(Duration::from_millis(100)).await;
            continue;
        };

        let mut gate = trueos_esp::gate::GateDiscovery::new();
        let _ = vnet.submit(gate.bootstrap_command());
        crate::log!(
            "esp-gate: starting udp swarm listener on port {} payload=swarm\n",
            trueos_esp::gate::ESP_UDP_BROADCAST_PORT
        );

        loop {
            if let Some(ev) = vnet.pop_event() {
                if let v::vnet::Event::Error { msg } = ev {
                    crate::log!("esp-gate: error {}\n", msg);
                }

                match gate.on_event(ev) {
                    trueos_esp::gate::GateAction::None => {}
                    trueos_esp::gate::GateAction::Signal(signal) => match signal {
                        trueos_esp::gate::GateSignal::UdpBound(handle) => crate::log!(
                            "esp-gate: udp listener bound handle={} port={}\n",
                            handle.0,
                            trueos_esp::gate::ESP_UDP_BROADCAST_PORT
                        ),
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
                            let snapshot = {
                                let mut registry = DEVICE_REGISTRY.lock();
                                let is_new = registry.upsert_heartbeat_v4(
                                    from.addr,
                                    trueos_esp::gate::ESP_HTTP_UPLOAD_PORT,
                                    now_ms,
                                );
                                if is_new {
                                    registry
                                        .snapshot_for(trueos_esp::gate::device_handle_v4(from.addr))
                                } else {
                                    None
                                }
                            };
                            if let Some(snapshot) = snapshot.as_ref() {
                                handoff_device_to_truesurfer(snapshot).await;
                            }
                        }
                    },
                    trueos_esp::gate::GateAction::Submit(cmd) => {
                        let _ = vnet.submit(cmd);
                    }
                }

                continue;
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
