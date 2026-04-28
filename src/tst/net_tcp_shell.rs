use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::net::adapter::{
    NetCommand, NetEvent, NetHandle, NetQueue, SocketKind, register_app_queues,
};
use crate::shell2::backends::net_tcp::{NET_SHELL_STARTED, NET_SHELL_STATE, NET_SHELL_TCP_PORT};

/// TCP-backed shell I/O bridge.
///
/// - Listens on `NET_SHELL_TCP_PORT`.
/// - Buffers RX bytes into `net_shell_read_byte()`.
/// - Buffers shell output from `net_shell_write_bytes()` and flushes it over TCP.
#[task]
pub async fn net_shell_task() {
    async move {
        if NET_SHELL_STARTED.swap(true, Ordering::SeqCst) {
            return;
        }

        crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

        // Route the shell over a NIC that is actually usable.
        // Historically this was pinned to dev0, but on real hardware dev0 is often the
        // physically-unplugged port. Prefer the current primary, but fall back to any
        // link-up NIC to keep the shell reachable whenever the network works.
        let mut dev_idx = crate::net::primary_device_index();
        let primary_up = crate::net::link_state_at(dev_idx)
            .map(|ls| ls.up)
            .unwrap_or(false);
        if !primary_up {
            for idx in 0..crate::net::device_count() {
                if crate::net::link_state_at(idx)
                    .map(|ls| ls.up)
                    .unwrap_or(false)
                {
                    dev_idx = idx;
                    break;
                }
            }
        }

        // Keep owner unsuffixed so command routing follows the current primary NIC
        // instead of a one-time pre-readiness device snapshot.
        let owner: &'static str = "net-shell";

        let ip = crate::net::adapter::ipv4_at(dev_idx);
        let name = crate::net::device_name_at(dev_idx).unwrap_or("?");
        match ip {
            Some([a, b, c, d]) => {
                crate::log!(
                    "net-shell: routing dev={} {} owner={} ip={}.{}.{}.{}\n",
                    dev_idx,
                    name,
                    owner,
                    a,
                    b,
                    c,
                    d
                )
            }
            None => {
                crate::log!("net-shell: routing dev={} {} owner={} ip=none\n", dev_idx, name, owner)
            }
        }

        let cmds = NetQueue::new_leaked("net-shell-cmd", 256);
        let events = NetQueue::new_leaked("net-shell-evt", 256);
        register_app_queues(owner, cmds, events);

        let _ = cmds.push(NetCommand::OpenTcpListen {
            port: NET_SHELL_TCP_PORT,
        });
        crate::log!("net-shell: listening on tcp {} owner={}\n", NET_SHELL_TCP_PORT, owner);

        let mut ticks: u32 = 0;
        let mut logged_first_rx: bool = false;
        let mut pending: Option<Vec<u8>> = None;
        let mut pending_handle: Option<NetHandle> = None;
        let mut pending_ticks: u32 = 0;
        let mut pending_len: usize = 0;
        let mut tx_log_budget: u32 = 16;
        let mut tcp_handle: Option<NetHandle> = None;

        loop {
            for ev in events.drain(32) {
                match ev {
                    NetEvent::Opened { handle, kind } => {
                        if kind == SocketKind::Tcp {
                            tcp_handle = Some(handle);
                            crate::log!("net-shell: opened tcp handle={}\n", handle.0);
                        }
                    }
                    NetEvent::TcpEstablished { handle } => {
                        let mut repaint_screen = false;
                        {
                            let mut st = NET_SHELL_STATE.lock();
                            let is_new_conn = st.handle != Some(handle);
                            st.handle = Some(handle);
                            if is_new_conn {
                                st.rx.clear();
                                st.tx.clear();
                                repaint_screen = true;
                            }
                        }
                        if repaint_screen {
                            crate::shell2::repaint_backend_screen(
                                &crate::shell2::NET_TCP_SHELL_BACKEND,
                            );
                        }
                        pending = None;
                        pending_handle = Some(handle);
                        pending_ticks = 0;
                        pending_len = 0;
                        logged_first_rx = false;
                        tx_log_budget = 16;
                        crate::log!("net-shell: tcp established handle={}\n", handle.0);
                    }
                    NetEvent::TcpData { handle, data } => {
                        // Only accept bytes from the active connection.
                        // NOTE: Data can arrive before we process `TcpEstablished` (event ordering),
                        // so treat the first inbound bytes as selecting the active handle.
                        {
                            let mut st = NET_SHELL_STATE.lock();
                            if st.handle.is_none() {
                                st.handle = Some(handle);
                            }
                            if st.handle != Some(handle) {
                                continue;
                            }

                            if !logged_first_rx {
                                logged_first_rx = true;
                                crate::log!(
                                    "net-shell: first rx {} bytes (including {:?})\n",
                                    data.len(),
                                    data.first().copied()
                                );
                            }

                            const MAX_RX: usize = 8 * 1024;
                            for b in data {
                                if st.rx.len() >= MAX_RX {
                                    let _ = st.rx.pop_front();
                                }
                                st.rx.push_back(b);
                            }
                        }
                    }
                    NetEvent::TcpSent { handle, len } => {
                        if pending_handle != Some(handle) {
                            continue;
                        }

                        if tx_log_budget > 0 {
                            tx_log_budget -= 1;
                            crate::log!(
                                "net-shell: tx accepted handle={} len={} (pending_len={})\n",
                                handle.0,
                                len,
                                pending_len
                            );
                        }

                        // Drop the bytes we now know were accepted by smoltcp.
                        // NOTE: smoltcp may accept only a prefix of the buffer; keep the rest queued.
                        let mut st = NET_SHELL_STATE.lock();
                        for _ in 0..len {
                            let _ = st.tx.pop_front();
                        }
                        pending = None;
                        pending_ticks = 0;
                        pending_len = 0;
                    }
                    NetEvent::Closed { handle } => {
                        let mut st = NET_SHELL_STATE.lock();
                        if st.handle == Some(handle) {
                            st.handle = None;
                            st.rx.clear();
                            pending = None;
                            pending_handle = None;
                            pending_ticks = 0;
                            pending_len = 0;
                        }

                        if tcp_handle == Some(handle) {
                            tcp_handle = None;
                            crate::log!("net-shell: tcp closed handle={} (relisten)\n", handle.0);
                            let _ = cmds.push(NetCommand::OpenTcpListen {
                                port: NET_SHELL_TCP_PORT,
                            });
                        }
                    }
                    NetEvent::Error { msg } => {
                        // These are useful during bring-up; keep them visible but not too spammy.
                        if ticks.is_multiple_of(100) {
                            crate::log!("net-shell: error {}\n", msg);
                        }
                    }
                    NetEvent::UdpPacket { .. } => {}
                    NetEvent::UdpPacketV6 { .. } => {}
                    NetEvent::IcmpReply { .. } => {}
                    NetEvent::IcmpReplyV6 { .. } => {}
                }
            }

            // Flush buffered TX to the active TCP connection.
            // Use an explicit ack event (`TcpSent`) so we only pop on success.
            if pending.is_none() {
                let (handle, chunk) = {
                    let st = NET_SHELL_STATE.lock();
                    match st.handle {
                        None => (None, Vec::new()),
                        Some(handle) => {
                            if st.tx.is_empty() {
                                (Some(handle), Vec::new())
                            } else {
                                let mut v = Vec::with_capacity(512);
                                for &b in st.tx.iter().take(512) {
                                    v.push(b);
                                }
                                (Some(handle), v)
                            }
                        }
                    }
                };

                if let Some(handle) = handle
                    && !chunk.is_empty()
                {
                    pending_handle = Some(handle);
                    pending = Some(chunk.clone());
                    pending_ticks = 0;
                    pending_len = chunk.len();

                    if tx_log_budget > 0 {
                        tx_log_budget -= 1;
                        crate::log!(
                            "net-shell: tx queue handle={} len={}\n",
                            handle.0,
                            pending_len
                        );
                    }

                    if cmds
                        .push(NetCommand::SendTcp {
                            handle,
                            data: chunk,
                        })
                        .is_err()
                    {
                        // If the command queue is full, don't stall forever waiting for an event.
                        pending = None;
                        pending_ticks = 0;
                        pending_len = 0;
                        crate::log!("net-shell: tx queue full (dropping pending)\n");
                    }
                }
            }

            // Safety: if we somehow miss the `TcpSent` event (or the socket is briefly not-ready),
            // don't wedge TX forever. We'll retry by clearing `pending` after a short timeout.
            if pending.is_some() {
                pending_ticks = pending_ticks.wrapping_add(1);
                if pending_ticks == 250 {
                    crate::log!("net-shell: tx stalled (pending_len={}), retrying\n", pending_len);
                    pending = None;
                    pending_ticks = 0;
                    pending_len = 0;
                }
            }

            ticks = ticks.wrapping_add(1);
            Timer::after(EmbassyDuration::from_millis(10)).await;
            let _ = ticks;
        }
    }
    .await;
}
