use embassy_time::{Duration, Timer};

use super::VNet;

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

        let mut swarm = trueos_esp::swarm::SwarmService::default();
        crate::log!(
            "esp-gate: starting tcp listener on port {}\n",
            swarm.listen_port()
        );

        swarm
            .run_forever(&vnet, |signal| match signal {
                trueos_esp::swarm::SwarmSignal::ListenerBound(handle) => crate::log!(
                    "esp-gate: listening handle={} port={}\n",
                    handle.0,
                    trueos_esp::swarm::ESP_GATE_TCP_PORT
                ),
                trueos_esp::swarm::SwarmSignal::ClientConnected(handle) => {
                    crate::log!("esp-gate: client connected handle={}\n", handle.0)
                }
                trueos_esp::swarm::SwarmSignal::ClientClosed(handle) => {
                    crate::log!("esp-gate: client closed handle={}\n", handle.0)
                }
                trueos_esp::swarm::SwarmSignal::Received(notice) => crate::log!(
                    "esp-gate: rx handle={} len={} preview={:?}\n",
                    notice.handle.0,
                    notice.len,
                    notice.preview.as_slice()
                ),
                trueos_esp::swarm::SwarmSignal::Error(msg) => {
                    crate::log!("esp-gate: error {}\n", msg)
                }
            })
            .await;
    }
}
