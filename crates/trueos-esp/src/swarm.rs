use alloc::collections::BTreeSet;
use core::future::Future;

use embassy_time::{Duration as EmbassyDuration, Timer};
use v::vnet as api;

pub const ESP_GATE_TCP_PORT: u16 = 2;
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
    ListenerBound(api::NetHandle),
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
}

pub struct SwarmService {
    listen_port: u16,
    listener: Option<api::NetHandle>,
    clients: BTreeSet<u32>,
}

impl Default for SwarmService {
    fn default() -> Self {
        Self::new(ESP_GATE_TCP_PORT)
    }
}

impl SwarmService {
    pub fn new(listen_port: u16) -> Self {
        Self {
            listen_port,
            listener: None,
            clients: BTreeSet::new(),
        }
    }

    pub fn listen_port(&self) -> u16 {
        self.listen_port
    }

    pub fn bootstrap_command(&self) -> api::Command {
        api::Command::OpenTcpListen {
            port: self.listen_port,
        }
    }

    pub fn has_client(&self, handle: api::NetHandle) -> bool {
        self.clients.contains(&handle.0)
    }

    pub fn on_event(&mut self, ev: api::Event) -> Option<SwarmSignal> {
        match self.step_for_event(ev) {
            SwarmStep::None | SwarmStep::Submit(_) => None,
            SwarmStep::Signal(signal) => Some(signal),
        }
    }

    pub async fn run_forever<V, F>(&mut self, vlayer: &V, mut on_signal: F) -> !
    where
        V: VLayer + Sync,
        F: FnMut(SwarmSignal),
    {
        let _ = vlayer.submit(self.bootstrap_command());

        loop {
            if let Some(ev) = vlayer.pop_event() {
                match self.step_for_event(ev) {
                    SwarmStep::None => {}
                    SwarmStep::Signal(signal) => on_signal(signal),
                    SwarmStep::Submit(cmd) => {
                        let _ = vlayer.submit(cmd);
                    }
                }
                continue;
            }

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
            api::Event::Opened { handle, kind } if kind == api::SocketKind::Tcp => {
                self.listener = Some(handle);
                SwarmStep::Signal(SwarmSignal::ListenerBound(handle))
            }
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
                let had_client = self.clients.remove(&handle.0);
                if self.listener == Some(handle) {
                    self.listener = None;
                    return SwarmStep::Submit(api::Command::OpenTcpListen {
                        port: self.listen_port,
                    });
                }

                if had_client {
                    SwarmStep::Signal(SwarmSignal::ClientClosed(handle))
                } else {
                    SwarmStep::None
                }
            }
            api::Event::Error { msg } => SwarmStep::Signal(SwarmSignal::Error(msg)),
            api::Event::UdpPacket { .. }
            | api::Event::UdpPacketV6 { .. }
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
                assert_eq!(notice.len, 15);
                assert_eq!(notice.preview.as_slice(), b"hello esp32-c3");
            }
            _ => panic!("expected rx signal"),
        }
    }

    #[test]
    fn keeps_port_open_after_listener_close() {
        let mut service = SwarmService::new(ESP_GATE_TCP_PORT);
        let _ = service.on_event(api::Event::Opened {
            handle: api::NetHandle(1),
            kind: api::SocketKind::Tcp,
        });

        assert!(matches!(
            service.step_for_event(api::Event::Closed {
                handle: api::NetHandle(1),
            }),
            SwarmStep::Submit(api::Command::OpenTcpListen { port: ESP_GATE_TCP_PORT })
        ));
    }
}