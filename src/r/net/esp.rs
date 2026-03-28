use alloc::{collections::VecDeque, format, string::String, vec::Vec};

use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::{VNet, ports};

const ESP_GATE_REGISTRY_MAX_DEVICES: usize = 64;

enum SwarmToGate {
    Connected(v::vnet::NetHandle),
    Activity(v::vnet::NetHandle),
    Closed(v::vnet::NetHandle),
}

enum GateToSwarm {
    NoteConnected(v::vnet::NetHandle),
    NoteClosed(v::vnet::NetHandle),
    CloseDevice(v::vnet::NetHandle),
}

static CHANNEL_A_TO_B: Mutex<VecDeque<SwarmToGate>> = Mutex::new(VecDeque::new());
static CHANNEL_B_TO_A: Mutex<VecDeque<GateToSwarm>> = Mutex::new(VecDeque::new());
static DEVICE_REGISTRY: Mutex<trueos_esp::gate::DeviceRegistry> = Mutex::new(
    trueos_esp::gate::DeviceRegistry::new(ESP_GATE_REGISTRY_MAX_DEVICES),
);

fn monotonic_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

fn channel_a_to_b_push(event: SwarmToGate) {
    CHANNEL_A_TO_B.lock().push_back(event);
}

fn channel_a_to_b_drain(max_events: usize) -> Vec<SwarmToGate> {
    let mut out = Vec::new();
    let mut queue = CHANNEL_A_TO_B.lock();
    for _ in 0..max_events {
        let Some(event) = queue.pop_front() else {
            break;
        };
        out.push(event);
    }
    out
}

fn channel_b_to_a_push(event: GateToSwarm) {
    CHANNEL_B_TO_A.lock().push_back(event);
}

fn channel_b_to_a_drain(max_events: usize) -> Vec<GateToSwarm> {
    let mut out = Vec::new();
    let mut queue = CHANNEL_B_TO_A.lock();
    for _ in 0..max_events {
        let Some(event) = queue.pop_front() else {
            break;
        };
        out.push(event);
    }
    out
}

fn process_gate_to_swarm(vnet: &VNet) {
    for event in channel_b_to_a_drain(32) {
        match event {
            GateToSwarm::NoteConnected(handle) => {
                crate::log!("esp-gate: gate-confirmed connected handle={}\n", handle.0);
            }
            GateToSwarm::NoteClosed(handle) => {
                crate::log!("esp-gate: gate-confirmed closed handle={}\n", handle.0);
            }
            GateToSwarm::CloseDevice(handle) => {
                let _ = vnet.submit(v::vnet::Command::Close { handle });
                crate::log!("esp-gate: close requested by gate handle={}\n", handle.0);
            }
        }
    }
}

#[allow(dead_code)]
pub fn remove_device(handle: v::vnet::NetHandle) -> bool {
    let removed = DEVICE_REGISTRY.lock().remove_device(handle);
    if removed {
        channel_b_to_a_push(GateToSwarm::CloseDevice(handle));
    }
    removed
}

#[allow(dead_code)]
pub fn device_snapshot() -> Vec<trueos_esp::gate::DeviceSnapshot> {
    DEVICE_REGISTRY.lock().snapshot()
}

fn device_label(snapshot: &trueos_esp::gate::DeviceSnapshot) -> String {
    match snapshot.ip {
        Some(trueos_esp::gate::DeviceIp::V4(addr)) => format!(
            "{}.{}.{}.{}:{}",
            addr[0], addr[1], addr[2], addr[3], snapshot.tcp_port
        ),
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
            snapshot.tcp_port
        ),
        None => format!("pending:{}", snapshot.tcp_port),
    }
}

fn device_html(snapshot: &trueos_esp::gate::DeviceSnapshot) -> String {
    let endpoint = device_label(snapshot);
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
            "<dt>connected_ms</dt><dd>{connected_at_ms}</dd>",
            "<dt>last_activity_ms</dt><dd>{last_activity_ms}</dd>",
            "</dl></div></body></html>"
        ),
        tag = snapshot.tag.as_str(),
        handle = snapshot.handle.0,
        endpoint = endpoint,
        connected_at_ms = snapshot.connected_at_ms,
        last_activity_ms = snapshot.last_activity_ms,
    )
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

impl trueos_esp::swarm::VLayer for VNet {
    fn submit(&self, cmd: v::vnet::Command) -> Result<(), ()> {
        VNet::submit(self, cmd)
    }

    fn pop_event(&self) -> Option<v::vnet::Event> {
        VNet::pop_event(self)
    }
}

#[embassy_executor::task]
pub async fn esp_gate_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_CONFIGURED).await;

    loop {
        let Some(vnet) = VNet::open_primary() else {
            Timer::after(Duration::from_millis(100)).await;
            continue;
        };

        let mut swarm = trueos_esp::swarm::SwarmService::new(ports::ESP_GATE_TCP_PORT);
        crate::log!(
            "esp-gate: starting tcp listener on port {}\n",
            swarm.listen_port()
        );

        swarm
            .run_forever_with_idle(
                &vnet,
                |vnet, signal| {
                    process_gate_to_swarm(vnet);
                    match signal {
                        trueos_esp::swarm::SwarmSignal::ListenerBound(handle) => crate::log!(
                            "esp-gate: listening handle={} port={}\n",
                            handle.0,
                            ports::ESP_GATE_TCP_PORT
                        ),
                        trueos_esp::swarm::SwarmSignal::ClientConnected(handle) => {
                            channel_a_to_b_push(SwarmToGate::Connected(handle));
                            crate::log!("esp-gate: client connected handle={}\n", handle.0)
                        }
                        trueos_esp::swarm::SwarmSignal::ClientClosed(handle) => {
                            channel_a_to_b_push(SwarmToGate::Closed(handle));
                            crate::log!("esp-gate: client closed handle={}\n", handle.0)
                        }
                        trueos_esp::swarm::SwarmSignal::Received(notice) => {
                            channel_a_to_b_push(SwarmToGate::Activity(notice.handle));
                            crate::log!(
                                "esp-gate: rx handle={} len={} preview={:?}\n",
                                notice.handle.0,
                                notice.len,
                                notice.preview.as_slice()
                            )
                        }
                        trueos_esp::swarm::SwarmSignal::Error(msg) => {
                            crate::log!("esp-gate: error {}\n", msg)
                        }
                    }
                },
                |vnet| process_gate_to_swarm(vnet),
            )
            .await;
    }
}

#[embassy_executor::task]
pub async fn esp_gate_registry_task() {
    let mut heartbeat_tick = 0u32;

    loop {
        let now_ms = monotonic_ms();
        let events = channel_a_to_b_drain(64);

        if !events.is_empty() {
            for event in events {
                match event {
                    SwarmToGate::Connected(handle) => {
                        let snapshot = {
                            let mut registry = DEVICE_REGISTRY.lock();
                            let is_new = registry.connect(
                                handle,
                                trueos_esp::swarm::ESP_GATE_TCP_PORT,
                                now_ms,
                            );
                            if is_new {
                                registry.snapshot_for(handle)
                            } else {
                                None
                            }
                        };
                        channel_b_to_a_push(GateToSwarm::NoteConnected(handle));
                        if let Some(snapshot) = snapshot.as_ref() {
                            handoff_device_to_truesurfer(snapshot).await;
                        }
                    }
                    SwarmToGate::Activity(handle) => {
                        DEVICE_REGISTRY.lock().touch(handle, now_ms);
                    }
                    SwarmToGate::Closed(handle) => {
                        let _ = DEVICE_REGISTRY.lock().remove_device(handle);
                        channel_b_to_a_push(GateToSwarm::NoteClosed(handle));
                    }
                }
            }
        }

        heartbeat_tick = heartbeat_tick.wrapping_add(1);
        if heartbeat_tick >= 20 {
            heartbeat_tick = 0;
            let count = DEVICE_REGISTRY.lock().len();
            if count != 0 {
                crate::log!("esp-gate: registry active_devices={}\n", count);
            }
        }

        Timer::after(Duration::from_millis(250)).await;
    }
}
