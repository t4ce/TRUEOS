use embassy_time::{Duration, Timer};

use super::VNet;

#[embassy_executor::task]
pub async fn lan_discovery_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

    loop {
        let Some(vnet) = VNet::open_primary() else {
            Timer::after(Duration::from_millis(100)).await;
            continue;
        };

        let mut udp_handle: Option<v::vnet::NetHandle> = None;
        let _ = vnet.submit(v::vnet::Command::OpenUdp {
            port: crate::r::net::ports::TRUEOS_DISCOVERY_UDP_PORT,
        });
        crate::log!(
            "lan-discovery: starting udp listener port={}\n",
            crate::r::net::ports::TRUEOS_DISCOVERY_UDP_PORT
        );

        loop {
            if let Some(ev) = vnet.pop_event() {
                match ev {
                    v::vnet::Event::Opened { handle, kind } if kind == v::vnet::SocketKind::Udp => {
                        udp_handle = Some(handle);
                        crate::log!(
                            "lan-discovery: udp listener bound handle={} port={}\n",
                            handle.0,
                            crate::r::net::ports::TRUEOS_DISCOVERY_UDP_PORT
                        );
                    }
                    v::vnet::Event::UdpPacket { handle, from, data }
                        if udp_handle == Some(handle) =>
                    {
                        if data.as_slice() == trueos_esp::gate::ESP_SWARM_HEARTBEAT {
                            crate::r::net::esp::publish_swarm_heartbeat_v4(from);
                        } else if let Some(advertisement) =
                            crate::r::net::trueos_peer::parse_peer_advertisement(
                                from,
                                data.as_slice(),
                            )
                        {
                            crate::r::net::trueos_peer::publish_host_advertisement(advertisement);
                        }
                    }
                    v::vnet::Event::Closed { handle } if udp_handle == Some(handle) => {
                        udp_handle = None;
                        crate::log!("lan-discovery: udp listener closed, reopening\n");
                        let _ = vnet.submit(v::vnet::Command::OpenUdp {
                            port: crate::r::net::ports::TRUEOS_DISCOVERY_UDP_PORT,
                        });
                    }
                    v::vnet::Event::Error { msg } => {
                        crate::log!("lan-discovery: error {}\n", msg);
                    }
                    v::vnet::Event::UdpPacket { .. }
                    | v::vnet::Event::UdpPacketV6 { .. }
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

            if let Some(handle) = udp_handle {
                for advertisement in crate::r::net::trueos_peer::take_peer_advertisements() {
                    let _ = vnet.submit(v::vnet::Command::SendUdp {
                        handle,
                        remote: advertisement.remote,
                        data: v::vnet::ByteBuf::from_slice_trunc(advertisement.data.as_slice()),
                    });
                }
            }

            Timer::after(Duration::from_millis(10)).await;
        }
    }
}
