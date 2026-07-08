use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::net::adapter::{
    NetCommand, NetEvent, NetHandle, NetQueue, SocketKind, register_app_queues,
};
use crate::shell2::backends::net_tcp::{
    NET_SHELL_STARTED, NET_SHELL_STATE, NET_SHELL_TCP_PORT, net_shell_write_bytes,
};

const TERMINAL_SIZE_QUERY: &[u8] = b"\x1b[18t";
const INITIAL_REPAINT_WAIT_TICKS: u32 = 8;
const RESIZE_QUERY_TICKS: u32 = 100;

fn parse_terminal_size_report(data: &[u8]) -> Option<(usize, usize, usize, usize)> {
    let start = data.windows(2).position(|w| w == b"\x1b[")?;
    let mut params = [0usize; 3];
    let mut idx = 0usize;
    let mut saw_digit = false;

    for (offset, &b) in data[start + 2..].iter().enumerate() {
        match b {
            b'0'..=b'9' => {
                if idx >= params.len() {
                    return None;
                }
                params[idx] = params[idx]
                    .saturating_mul(10)
                    .saturating_add(usize::from(b - b'0'));
                saw_digit = true;
            }
            b';' => {
                if !saw_digit || idx + 1 >= params.len() {
                    return None;
                }
                idx += 1;
                saw_digit = false;
            }
            b't' => {
                if idx == 2 && saw_digit && params[0] == 8 && params[1] > 0 && params[2] > 0 {
                    return Some((params[2], params[1], start, start + 2 + offset + 1));
                }
                return None;
            }
            _ => return None,
        }
    }

    None
}

fn looks_like_incomplete_terminal_size_report(data: &[u8]) -> bool {
    let Some(start) = data.windows(2).position(|w| w == b"\x1b[") else {
        return false;
    };
    let Some(&first) = data.get(start + 2) else {
        return true;
    };
    if first != b'8' {
        return false;
    }
    data[start + 3..]
        .iter()
        .all(|&b| b.is_ascii_digit() || b == b';')
}

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
        let ip_mode = match crate::net::adapter::dhcp_has_lease_at(dev_idx) {
            Some(true) => "dhcp",
            Some(false) => "fallback",
            None => "unknown",
        };
        let name = crate::net::device_name_at(dev_idx).unwrap_or("?");
        match ip {
            Some([a, b, c, d]) => {
                crate::log!(
                    "net-shell: routing dev={} {} owner={} ip={}.{}.{}.{} mode={}\n",
                    dev_idx,
                    name,
                    owner,
                    a,
                    b,
                    c,
                    d,
                    ip_mode
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
        let mut initial_repaint_handle: Option<NetHandle> = None;
        let mut initial_repaint_ticks: u32 = 0;
        let mut initial_rx_probe: Vec<u8> = Vec::new();
        let mut resize_query_ticks: u32 = 0;
        let mut resize_rx_probe: Vec<u8> = Vec::new();

        loop {
            for ev in events.drain(32) {
                match ev {
                    NetEvent::Opened { handle, kind } => {
                        if kind == SocketKind::Tcp {
                            tcp_handle = Some(handle);
                            crate::log!("net-shell: opened tcp handle={}\n", handle.0);
                        }
                    }
                    NetEvent::TcpEstablished { handle, .. } => {
                        let mut schedule_initial_repaint = false;
                        {
                            let mut st = NET_SHELL_STATE.lock();
                            let is_new_conn = st.handle != Some(handle);
                            st.handle = Some(handle);
                            if is_new_conn {
                                st.rx.clear();
                                st.tx.clear();
                                schedule_initial_repaint = true;
                            }
                        }
                        if schedule_initial_repaint {
                            net_shell_write_bytes(TERMINAL_SIZE_QUERY);
                            initial_repaint_handle = Some(handle);
                            initial_repaint_ticks = 0;
                            initial_rx_probe.clear();
                            resize_rx_probe.clear();
                            resize_query_ticks = 0;
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
                        let mut rx_data = data;
                        if initial_repaint_handle == Some(handle) {
                            initial_rx_probe.extend_from_slice(&rx_data);
                            if let Some((cols, rows, start, end)) =
                                parse_terminal_size_report(&initial_rx_probe)
                            {
                                crate::shell2::apply_reported_terminal_size_for_backend(
                                    &crate::shell2::NET_TCP_SHELL_BACKEND,
                                    cols,
                                    rows,
                                );
                                crate::shell2::repaint_backend_screen(
                                    &crate::shell2::NET_TCP_SHELL_BACKEND,
                                );
                                initial_repaint_handle = None;
                                initial_repaint_ticks = 0;
                                let mut filtered = Vec::new();
                                filtered.extend_from_slice(&initial_rx_probe[..start]);
                                filtered.extend_from_slice(&initial_rx_probe[end..]);
                                rx_data = filtered;
                                initial_rx_probe.clear();
                            } else if initial_rx_probe.len() <= 32 {
                                continue;
                            } else {
                                rx_data = core::mem::take(&mut initial_rx_probe);
                            }
                        } else {
                            if !resize_rx_probe.is_empty() {
                                resize_rx_probe.extend_from_slice(&rx_data);
                                if let Some((cols, rows, start, end)) =
                                    parse_terminal_size_report(&resize_rx_probe)
                                {
                                    if crate::shell2::apply_reported_terminal_size_for_backend(
                                        &crate::shell2::NET_TCP_SHELL_BACKEND,
                                        cols,
                                        rows,
                                    ) {
                                        crate::shell2::repaint_backend_screen(
                                            &crate::shell2::NET_TCP_SHELL_BACKEND,
                                        );
                                    }
                                    let mut filtered = Vec::new();
                                    filtered.extend_from_slice(&resize_rx_probe[..start]);
                                    filtered.extend_from_slice(&resize_rx_probe[end..]);
                                    rx_data = filtered;
                                    resize_rx_probe.clear();
                                } else if resize_rx_probe.len() <= 32
                                    && looks_like_incomplete_terminal_size_report(&resize_rx_probe)
                                {
                                    continue;
                                } else {
                                    rx_data = core::mem::take(&mut resize_rx_probe);
                                }
                            } else if let Some((cols, rows, start, end)) =
                                parse_terminal_size_report(&rx_data)
                            {
                                if crate::shell2::apply_reported_terminal_size_for_backend(
                                    &crate::shell2::NET_TCP_SHELL_BACKEND,
                                    cols,
                                    rows,
                                ) {
                                    crate::shell2::repaint_backend_screen(
                                        &crate::shell2::NET_TCP_SHELL_BACKEND,
                                    );
                                }
                                rx_data.drain(start..end);
                            } else if rx_data.len() <= 32
                                && looks_like_incomplete_terminal_size_report(&rx_data)
                            {
                                resize_rx_probe.extend_from_slice(&rx_data);
                                continue;
                            }
                        }
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
                                    rx_data.len(),
                                    rx_data.first().copied()
                                );
                            }

                            const MAX_RX: usize = 8 * 1024;
                            for b in rx_data {
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
                            if initial_repaint_handle == Some(handle) {
                                initial_repaint_handle = None;
                                initial_repaint_ticks = 0;
                                initial_rx_probe.clear();
                                resize_rx_probe.clear();
                            }
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

            if let Some(handle) = initial_repaint_handle {
                initial_repaint_ticks = initial_repaint_ticks.wrapping_add(1);
                if initial_repaint_ticks >= INITIAL_REPAINT_WAIT_TICKS {
                    crate::shell2::repaint_backend_screen(&crate::shell2::NET_TCP_SHELL_BACKEND);
                    initial_repaint_handle = None;
                    initial_repaint_ticks = 0;
                    if !initial_rx_probe.is_empty() {
                        let mut st = NET_SHELL_STATE.lock();
                        const MAX_RX: usize = 8 * 1024;
                        for b in initial_rx_probe.drain(..) {
                            if st.rx.len() >= MAX_RX {
                                let _ = st.rx.pop_front();
                            }
                            st.rx.push_back(b);
                        }
                    }
                    let _ = handle;
                }
            }

            if initial_repaint_handle.is_none() {
                let active_handle = {
                    let st = NET_SHELL_STATE.lock();
                    st.handle
                };
                if active_handle.is_some() {
                    resize_query_ticks = resize_query_ticks.wrapping_add(1);
                    if resize_query_ticks >= RESIZE_QUERY_TICKS {
                        resize_query_ticks = 0;
                        net_shell_write_bytes(TERMINAL_SIZE_QUERY);
                    }
                } else {
                    resize_query_ticks = 0;
                }
            }

            ticks = ticks.wrapping_add(1);
            Timer::after(EmbassyDuration::from_millis(10)).await;
            let _ = ticks;
        }
    }
    .await;
}
