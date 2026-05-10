#![allow(dead_code)]

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use v::vnet;

use crate::r::net::VNet;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum VNetUdpPacket {
    V4 {
        from: vnet::EndpointV4,
        data: vnet::ByteBuf<{ vnet::MAX_MSG }>,
    },
    V6 {
        from: vnet::EndpointV6,
        data: vnet::ByteBuf<{ vnet::MAX_MSG }>,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum VNetUdpEvent {
    Packet(VNetUdpPacket),
    Closed,
}

pub struct VNetUdpEndpoint<'a> {
    net: &'a VNet,
    handle: vnet::NetHandle,
    closed: bool,
}

impl<'a> VNetUdpEndpoint<'a> {
    pub async fn bind(net: &'a VNet, port: u16, timeout: EmbassyDuration) -> Option<Self> {
        let _ = net.submit(vnet::Command::OpenUdp { port });

        let deadline = Instant::now() + timeout;
        loop {
            for _ in 0..64 {
                let Some(ev) = net.pop_event() else {
                    break;
                };
                if let vnet::Event::Opened { handle, kind } = ev
                    && kind == vnet::SocketKind::Udp
                {
                    return Some(Self {
                        net,
                        handle,
                        closed: false,
                    });
                }
            }

            if Instant::now() >= deadline {
                return None;
            }

            Timer::after(EmbassyDuration::from_millis(5)).await;
        }
    }

    #[inline]
    pub const fn handle(&self) -> vnet::NetHandle {
        self.handle
    }

    pub fn close(&mut self) {
        if self.closed {
            return;
        }

        let _ = self.net.submit(vnet::Command::Close {
            handle: self.handle,
        });
        self.closed = true;
    }

    pub fn send_v4(&self, remote: vnet::EndpointV4, data: &[u8]) -> Result<(), ()> {
        if self.closed {
            return Err(());
        }

        self.net.submit(vnet::Command::SendUdp {
            handle: self.handle,
            remote,
            data: vnet::ByteBuf::from_slice_trunc(data),
        })
    }

    pub fn send_v6(&self, remote: vnet::EndpointV6, data: &[u8]) -> Result<(), ()> {
        if self.closed {
            return Err(());
        }

        self.net.submit(vnet::Command::SendUdpV6 {
            handle: self.handle,
            remote,
            data: vnet::ByteBuf::from_slice_trunc(data),
        })
    }

    pub fn poll_event(&mut self) -> Option<VNetUdpEvent> {
        loop {
            let ev = self.net.pop_event()?;
            match ev {
                vnet::Event::UdpPacket { handle, from, data } if handle == self.handle => {
                    return Some(VNetUdpEvent::Packet(VNetUdpPacket::V4 { from, data }));
                }
                vnet::Event::UdpPacketV6 { handle, from, data } if handle == self.handle => {
                    return Some(VNetUdpEvent::Packet(VNetUdpPacket::V6 { from, data }));
                }
                vnet::Event::Closed { handle } if handle == self.handle => {
                    self.closed = true;
                    return Some(VNetUdpEvent::Closed);
                }
                _ => {}
            }
        }
    }

    pub async fn next_event(
        &mut self,
        timeout: EmbassyDuration,
        idle_poll: EmbassyDuration,
    ) -> Option<VNetUdpEvent> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(ev) = self.poll_event() {
                return Some(ev);
            }

            if Instant::now() >= deadline {
                return None;
            }

            Timer::after(idle_poll).await;
        }
    }
}

impl Drop for VNetUdpEndpoint<'_> {
    fn drop(&mut self) {
        self.close();
    }
}
