use alloc::collections::BTreeSet;
use core::future::Future;

use embassy_time::{Duration as EmbassyDuration, Timer};
use v::vnet as api;

/// Port each ESP32 WebSocket (WebREPL) server listens on.
pub const ESP_WEBREPL_PORT: u16 = 21232;
/// UDP port on which ESP32s broadcast their presence to the kernel.
pub const ESP_UDP_BROADCAST_PORT: u16 = 32343;

pub const RX_PREVIEW_BYTES: usize = 64;
const IDLE_POLL_MS: u64 = 10;

pub trait VLayer {
    fn submit(&self, cmd: api::Command) -> Result<(), ()>;
    fn pop_event(&self) -> Option<api::Event>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RxNotice {
    pub handle: api::NetHandle,
    pub len: usize,
    pub preview: api::ByteBuf<RX_PREVIEW_BYTES>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwarmSignal {
    /// UDP broadcast listener socket is open and ready.
    UdpBound(api::NetHandle),
    /// A UDP broadcast arrived from an ESP32; an outbound WS connection is being opened.
    EspDiscovered(api::EndpointV4),
    ClientConnected(api::NetHandle),
    ClientClosed(api::NetHandle),
    Received(RxNotice),
    Error(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwarmStep {
    None,
    Signal(SwarmSignal),
    Submit(api::Command),
    /// Emit a signal to the layer above AND submit a command to the vnet.
    Both(SwarmSignal, api::Command),
}

pub struct SwarmService {
    udp_handle: Option<api::NetHandle>,
    clients: BTreeSet<u32>,
}

impl Default for SwarmService {
    fn default() -> Self {
        Self::new()
    }
}

impl SwarmService {
    pub fn new() -> Self {
        Self {
            udp_handle: None,
            clients: BTreeSet::new(),
        }
    }

    pub fn bootstrap_command(&self) -> api::Command {
        api::Command::OpenUdp {
            port: ESP_UDP_BROADCAST_PORT,
        }
    }

    pub fn has_client(&self, handle: api::NetHandle) -> bool {
        self.clients.contains(&handle.0)
    }

    pub fn on_event(&mut self, ev: api::Event) -> Option<SwarmSignal> {
        match self.step_for_event(ev) {
            SwarmStep::None | SwarmStep::Submit(_) => None,
            SwarmStep::Signal(signal) | SwarmStep::Both(signal, _) => Some(signal),
        }
    }

    pub async fn run_forever<V, F>(&mut self, vlayer: &V, mut on_signal: F) -> !
    where
        V: VLayer + Sync,
        F: FnMut(SwarmSignal),
    {
        self.run_forever_with_idle(vlayer, move |_, signal| on_signal(signal), |_| {})
            .await
    }

    pub async fn run_forever_with_idle<V, F, I>(
        &mut self,
        vlayer: &V,
        mut on_signal: F,
        mut on_idle: I,
    ) -> !
    where
        V: VLayer + Sync,
        F: FnMut(&V, SwarmSignal),
        I: FnMut(&V),
    {
        let _ = vlayer.submit(self.bootstrap_command());

        loop {
            if let Some(ev) = vlayer.pop_event() {
                match self.step_for_event(ev) {
                    SwarmStep::None => {}
                    SwarmStep::Signal(signal) => on_signal(vlayer, signal),
                    SwarmStep::Submit(cmd) => {
                        let _ = vlayer.submit(cmd);
                    }
                    SwarmStep::Both(signal, cmd) => {
                        on_signal(vlayer, signal);
                        let _ = vlayer.submit(cmd);
                    }
                }
                continue;
            }

            on_idle(vlayer);
            Timer::after(EmbassyDuration::from_millis(IDLE_POLL_MS)).await;
        }
    }

    pub async fn run_forever_after<W, V, F>(
        &mut self,
        wait_ready: W,
        vlayer: &V,
        on_signal: F,
    ) -> !
    where
        W: Future<Output = ()>,
        V: VLayer + Sync,
        F: FnMut(SwarmSignal),
    {
        wait_ready.await;
        self.run_forever(vlayer, on_signal).await
    }

    fn step_for_event(&mut self, ev: api::Event) -> SwarmStep {
        match ev {
            // UDP socket opened — broadcast listener is ready.
            api::Event::Opened { handle, kind } if kind == api::SocketKind::Udp => {
                self.udp_handle = Some(handle);
                SwarmStep::Signal(SwarmSignal::UdpBound(handle))
            }
            // UDP broadcast received from an ESP32 — initiate outbound WebSocket connection.
            api::Event::UdpPacket { from, .. } => {
                let remote = api::EndpointV4 {
                    addr: from.addr,
                    port: ESP_WEBREPL_PORT,
                };
                SwarmStep::Both(
                    SwarmSignal::EspDiscovered(from),
                    api::Command::OpenTcpConnect { remote },
                )
            }
            // Outbound TCP connection established — WebSocket session can begin.
            api::Event::TcpEstablished { handle } => {
                self.clients.insert(handle.0);
                SwarmStep::Signal(SwarmSignal::ClientConnected(handle))
            }
            api::Event::TcpData { handle, data } => {
                if !self.clients.contains(&handle.0) {
                    self.clients.insert(handle.0);
                }

                SwarmStep::Signal(SwarmSignal::Received(RxNotice {
                    handle,
                    len: data.len(),
                    preview: api::ByteBuf::from_slice_trunc(data.as_slice()),
                }))
            }
            api::Event::Closed { handle } => {
                // If the UDP listen socket closed, reopen it.
                if self.udp_handle == Some(handle) {
                    self.udp_handle = None;
                    return SwarmStep::Submit(api::Command::OpenUdp {
                        port: ESP_UDP_BROADCAST_PORT,
                    });
                }

                let had_client = self.clients.remove(&handle.0);
                if had_client {
                    SwarmStep::Signal(SwarmSignal::ClientClosed(handle))
                } else {
                    SwarmStep::None
                }
            }
            api::Event::Error { msg } => SwarmStep::Signal(SwarmSignal::Error(msg)),
            api::Event::UdpPacketV6 { .. }
            | api::Event::TcpSent { .. }
            | api::Event::IcmpReply { .. }
            | api::Event::IcmpReplyV6 { .. }
            | api::Event::Opened { .. } => SwarmStep::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_rx_preview() {
        let mut service = SwarmService::default();
        let signal = service.on_event(api::Event::TcpData {
            handle: api::NetHandle(7),
            data: api::ByteBuf::from_slice_trunc(b"hello esp32-c3"),
        });

        match signal {
            Some(SwarmSignal::Received(notice)) => {
                assert_eq!(notice.handle, api::NetHandle(7));
                assert_eq!(notice.len, 14);
                assert_eq!(notice.preview.as_slice(), b"hello esp32-c3");
            }
            _ => panic!("expected rx signal"),
        }
    }

    #[test]
    fn keeps_udp_open_after_close() {
        let mut service = SwarmService::new();
        // Simulate the UDP broadcast socket opening.
        let _ = service.on_event(api::Event::Opened {
            handle: api::NetHandle(1),
            kind: api::SocketKind::Udp,
        });

        // When it closes unexpectedly, the service should reopen it.
        assert!(matches!(
            service.step_for_event(api::Event::Closed {
                handle: api::NetHandle(1),
            }),
            SwarmStep::Submit(api::Command::OpenUdp {
                port: ESP_UDP_BROADCAST_PORT
            })
        ));
    }

    #[test]
    fn udp_broadcast_triggers_ws_connect() {
        let mut service = SwarmService::new();
        let from = api::EndpointV4::new([192, 168, 1, 42], 32343);
        let step = service.step_for_event(api::Event::UdpPacket {
            handle: api::NetHandle(2),
            from,
            data: api::ByteBuf::new(),
        });

        match step {
            SwarmStep::Both(SwarmSignal::EspDiscovered(ep), api::Command::OpenTcpConnect { remote }) => {
                assert_eq!(ep.addr, [192, 168, 1, 42]);
                assert_eq!(remote.addr, [192, 168, 1, 42]);
                assert_eq!(remote.port, ESP_WEBREPL_PORT);
            }
            _ => panic!("expected Both(EspDiscovered, OpenTcpConnect)"),
        }
    }
}