use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use crate::net::adapter::{
    NetCommand, NetEvent, NetHandle, NetQueue, SocketKind, register_app_queues,
};
use crate::shell2::backends::net_tcp::{
    NET_SHELL_STARTED, NET_SHELL_STATE, NET_SHELL_TCP_PORT, NetShellDirectControlToken,
    NetShellOwnershipSnapshot, admit_net_shell_tx_chunk, enqueue_net_shell_rx_if_unchanged,
    net_shell_begin_connection, net_shell_direct_reset_terminal, net_shell_ownership_snapshot,
    net_shell_terminal_size_query, net_shell_write_bytes, update_net_shell_surface_size,
};

const TERMINAL_SIZE_QUERY: &[u8] = b"\x1b[18t";
const INITIAL_REPAINT_WAIT_TICKS: u32 = 8;
const RESIZE_QUERY_TICKS: u32 = 100;
const TX_CHUNK_BYTES: usize = 8 * 1024;

/// One command accepted by the adapter queue.  `TcpSent` reports byte counts,
/// not command IDs, so this FIFO retains the explicit control token until the
/// corresponding bytes are reported as flushed from the adapter/socket.
struct PendingTcpWrite {
    handle: NetHandle,
    remaining: usize,
    len: usize,
    direct_control: Option<NetShellDirectControlToken>,
}

fn account_tcp_sent(
    pending: &mut VecDeque<PendingTcpWrite>,
    handle: NetHandle,
    mut sent: usize,
    mut control_complete: impl FnMut(NetShellDirectControlToken, usize),
) {
    while sent > 0 {
        // Events from separate TCP handles can interleave.  Keep FIFO order
        // *within* one handle, but never let an old socket's pending entry
        // block accounting for a new socket before its Closed event is seen.
        let Some(index) = pending.iter().position(|entry| entry.handle == handle) else {
            break;
        };
        let complete = {
            let Some(entry) = pending.get_mut(index) else {
                break;
            };
            let consumed = sent.min(entry.remaining);
            entry.remaining -= consumed;
            sent -= consumed;
            entry.remaining == 0
        };
        if !complete {
            break;
        }
        if let Some(entry) = pending.remove(index)
            && let Some(token) = entry.direct_control
        {
            control_complete(token, entry.len);
        }
    }
}

fn forget_tcp_handle(pending: &mut VecDeque<PendingTcpWrite>, handle: NetHandle) {
    pending.retain(|entry| entry.handle != handle);
}

fn log_direct_control_probe(
    probe: &str,
    token: NetShellDirectControlToken,
    handle: NetHandle,
    bytes: usize,
) {
    if let Some(vm) = token.blueprint_vm() {
        crate::log_os::service_important_line(format_args!(
            "terminal-handoff probe={} owner_kind=blueprint owner={} vm={} epoch={} handle={} surface_generation={} bytes={}\n",
            probe, token.owner, vm, token.epoch, handle.0, token.surface_generation, bytes,
        ));
    } else {
        crate::log_os::service_important_line(format_args!(
            "terminal-handoff probe={} owner_kind=stream owner={} session={} epoch={} handle={} surface_generation={} bytes={}\n",
            probe,
            token.owner,
            token.stream_session().unwrap_or_default(),
            token.epoch,
            handle.0,
            token.surface_generation,
            bytes,
        ));
    }
}

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

        // Deliberate BSP recovery privilege: net-shell must be reachable before the
        // ordinary network readiness gates open. `NetService::new` installs a
        // per-NIC static IPv4 fallback before starting DHCP, so a TCP listener can
        // be opened as soon as the service core and a physical link are available.
        // Keep this exception local to the recovery shell; other network consumers
        // continue to wait for NET_*_CONFIGURED in the service registry.
        crate::log!(
            "net-shell: early-network privilege active; bypassing NET_ANY_CONFIGURED ms={}\n",
            Instant::now().as_millis()
        );

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
                    "net-shell: routing dev={} {} owner={} ip={}.{}.{}.{} mode={} ms={}\n",
                    dev_idx,
                    name,
                    owner,
                    a,
                    b,
                    c,
                    d,
                    ip_mode,
                    Instant::now().as_millis()
                )
            }
            None => {
                crate::log!(
                    "net-shell: routing dev={} {} owner={} ip=none ms={}\n",
                    dev_idx,
                    name,
                    owner,
                    Instant::now().as_millis()
                )
            }
        }

        let cmds = NetQueue::new_leaked("net-shell-cmd", 256);
        let events = NetQueue::new_leaked("net-shell-evt", 256);
        register_app_queues(owner, cmds, events);

        let _ = cmds.push(NetCommand::OpenTcpListen {
            port: NET_SHELL_TCP_PORT,
        });
        crate::log!(
            "net-shell: listening on tcp {} owner={} ms={}\n",
            NET_SHELL_TCP_PORT,
            owner,
            Instant::now().as_millis()
        );

        let mut ticks: u32 = 0;
        let mut logged_first_rx: bool = false;
        let mut logged_first_wire_rx: bool = false;
        let mut tx_log_budget: u32 = 16;
        let mut pending_tcp_writes: VecDeque<PendingTcpWrite> = VecDeque::new();
        let mut direct_control_admitted: Option<NetShellDirectControlToken> = None;
        let mut direct_control_sent: Option<NetShellDirectControlToken> = None;
        let mut tcp_handle: Option<NetHandle> = None;
        let mut initial_repaint_handle: Option<NetHandle> = None;
        let mut initial_repaint_ticks: u32 = 0;
        let mut initial_rx_probe: Vec<u8> = Vec::new();
        let mut initial_rx_owner: Option<NetShellOwnershipSnapshot> = None;
        let mut resize_query_ticks: u32 = 0;
        let mut resize_rx_probe: Vec<u8> = Vec::new();
        let mut resize_rx_owner: Option<NetShellOwnershipSnapshot> = None;

        loop {
            for ev in events.drain(32) {
                match ev {
                    NetEvent::Opened { handle, kind } => {
                        if kind == SocketKind::Tcp {
                            tcp_handle = Some(handle);
                            crate::log!(
                                "net-shell: opened tcp handle={} ms={}\n",
                                handle.0,
                                Instant::now().as_millis()
                            );
                        }
                    }
                    NetEvent::TcpEstablished { handle, .. } => {
                        let (schedule_initial_repaint, ownership) =
                            net_shell_begin_connection(handle);
                        if schedule_initial_repaint {
                            initial_repaint_ticks = 0;
                            initial_rx_probe.clear();
                            initial_rx_owner = None;
                            resize_rx_probe.clear();
                            resize_rx_owner = None;
                            resize_query_ticks = 0;
                            if ownership.direct_active() {
                                let _ = net_shell_direct_reset_terminal();
                                let _ = net_shell_terminal_size_query();
                                initial_repaint_handle = None;
                            } else if net_shell_write_bytes(TERMINAL_SIZE_QUERY) {
                                initial_repaint_handle = Some(handle);
                            } else {
                                // A direct claim won after the locked snapshot;
                                // its claim reset is authoritative, and this
                                // narrowly scoped query refreshes its geometry.
                                let _ = net_shell_terminal_size_query();
                                initial_repaint_handle = None;
                            }
                        }
                        logged_first_rx = false;
                        logged_first_wire_rx = false;
                        tx_log_budget = 16;
                        crate::log!(
                            "net-shell: tcp established handle={} ms={}\n",
                            handle.0,
                            Instant::now().as_millis()
                        );
                    }
                    NetEvent::TcpData { handle, data } => {
                        // Only accept bytes from the active connection.
                        // NOTE: Data can arrive before we process `TcpEstablished` (event ordering),
                        // so treat the first inbound bytes as selecting the active handle.
                        if !logged_first_wire_rx {
                            logged_first_wire_rx = true;
                            crate::log!(
                                "net-shell: first wire rx handle={} bytes={} first={:?} ms={}\n",
                                handle.0,
                                data.len(),
                                data.first().copied(),
                                Instant::now().as_millis()
                            );
                        }
                        let mut rx_data = data;
                        let ownership = net_shell_ownership_snapshot();
                        let direct_mode = ownership.direct_active();
                        let direct_passthrough = ownership.direct_passthrough_active();
                        if initial_rx_owner.is_some_and(|owner| owner != ownership) {
                            initial_rx_probe.clear();
                            initial_rx_owner = None;
                        }
                        if resize_rx_owner.is_some_and(|owner| owner != ownership) {
                            resize_rx_probe.clear();
                            resize_rx_owner = None;
                        }
                        if direct_passthrough {
                            initial_repaint_handle = None;
                            initial_repaint_ticks = 0;
                            initial_rx_probe.clear();
                            initial_rx_owner = None;
                            resize_rx_probe.clear();
                            resize_rx_owner = None;
                        } else if initial_repaint_handle == Some(handle) && !direct_mode {
                            if initial_rx_probe.is_empty() {
                                initial_rx_owner = Some(ownership);
                            }
                            initial_rx_probe.extend_from_slice(&rx_data);
                            if let Some((cols, rows, start, end)) =
                                parse_terminal_size_report(&initial_rx_probe)
                            {
                                let _ = update_net_shell_surface_size(cols, rows);
                                if !crate::shell2::backends::net_tcp::net_shell_frontend_active() {
                                    crate::shell2::apply_reported_terminal_size_for_backend(
                                        &crate::shell2::NET_TCP_SHELL_BACKEND,
                                        cols,
                                        rows,
                                    );
                                }
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
                                initial_rx_owner = None;
                            } else if initial_rx_probe.len() <= 32 {
                                continue;
                            } else {
                                rx_data = core::mem::take(&mut initial_rx_probe);
                                initial_rx_owner = None;
                            }
                        } else {
                            if !resize_rx_probe.is_empty() {
                                resize_rx_probe.extend_from_slice(&rx_data);
                                if let Some((cols, rows, start, end)) =
                                    parse_terminal_size_report(&resize_rx_probe)
                                {
                                    let _ = update_net_shell_surface_size(cols, rows);
                                    if !crate::shell2::backends::net_tcp::net_shell_frontend_active(
                                    ) && crate::shell2::apply_reported_terminal_size_for_backend(
                                        &crate::shell2::NET_TCP_SHELL_BACKEND,
                                        cols,
                                        rows,
                                    ) && !direct_mode
                                    {
                                        crate::shell2::repaint_backend_screen(
                                            &crate::shell2::NET_TCP_SHELL_BACKEND,
                                        );
                                    }
                                    let mut filtered = Vec::new();
                                    filtered.extend_from_slice(&resize_rx_probe[..start]);
                                    filtered.extend_from_slice(&resize_rx_probe[end..]);
                                    rx_data = filtered;
                                    resize_rx_probe.clear();
                                    resize_rx_owner = None;
                                } else if resize_rx_probe.len() <= 32
                                    && looks_like_incomplete_terminal_size_report(&resize_rx_probe)
                                {
                                    continue;
                                } else {
                                    rx_data = core::mem::take(&mut resize_rx_probe);
                                    resize_rx_owner = None;
                                }
                            } else if let Some((cols, rows, start, end)) =
                                parse_terminal_size_report(&rx_data)
                            {
                                let _ = update_net_shell_surface_size(cols, rows);
                                if !crate::shell2::backends::net_tcp::net_shell_frontend_active()
                                    && crate::shell2::apply_reported_terminal_size_for_backend(
                                        &crate::shell2::NET_TCP_SHELL_BACKEND,
                                        cols,
                                        rows,
                                    )
                                    && !direct_mode
                                {
                                    crate::shell2::repaint_backend_screen(
                                        &crate::shell2::NET_TCP_SHELL_BACKEND,
                                    );
                                }
                                rx_data.drain(start..end);
                            } else if rx_data.len() <= 32
                                && looks_like_incomplete_terminal_size_report(&rx_data)
                            {
                                resize_rx_owner = Some(ownership);
                                resize_rx_probe.extend_from_slice(&rx_data);
                                continue;
                            }
                        }
                        if !enqueue_net_shell_rx_if_unchanged(ownership, handle, &rx_data) {
                            // A claim/release occurred while this packet was
                            // being parsed. Never feed its bytes (or a partial
                            // terminal-size report) to the next owner.
                            initial_rx_probe.clear();
                            initial_rx_owner = None;
                            resize_rx_probe.clear();
                            resize_rx_owner = None;
                            continue;
                        }

                        if !logged_first_rx {
                            logged_first_rx = true;
                            crate::log!(
                                "net-shell: first rx {} bytes (including {:?}) ms={}\n",
                                rx_data.len(),
                                rx_data.first().copied(),
                                Instant::now().as_millis()
                            );
                        }
                    }
                    NetEvent::TcpSent { handle, len } => {
                        account_tcp_sent(&mut pending_tcp_writes, handle, len, |token, bytes| {
                            if direct_control_sent != Some(token) {
                                direct_control_sent = Some(token);
                                // TcpSent is an adapter/socket flush, not a peer ACK or
                                // physical-wire receipt.  The tag makes this precise
                                // boundary attributable without inspecting payload bytes.
                                log_direct_control_probe(
                                    "direct-control-socket-flushed",
                                    token,
                                    handle,
                                    bytes,
                                );
                            }
                        });
                        if tx_log_budget > 0 {
                            tx_log_budget -= 1;
                            crate::log!(
                                "net-shell: tx flushed handle={} len={} ms={}\n",
                                handle.0,
                                len,
                                Instant::now().as_millis()
                            );
                        }
                    }
                    NetEvent::Closed { handle } => {
                        forget_tcp_handle(&mut pending_tcp_writes, handle);
                        let closed_active = {
                            let mut st = NET_SHELL_STATE.lock();
                            if st.handle == Some(handle) {
                                st.handle = None;
                                st.established_handle = None;
                                st.rx.clear();
                                true
                            } else {
                                false
                            }
                        };
                        if closed_active && initial_repaint_handle == Some(handle) {
                            initial_repaint_handle = None;
                            initial_repaint_ticks = 0;
                            initial_rx_probe.clear();
                            initial_rx_owner = None;
                            resize_rx_probe.clear();
                            resize_rx_owner = None;
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

                        // The privileged task can run before a NIC reports link-up. An
                        // early OpenTcpListen then fails with `link down`; retry it instead
                        // of waiting for an IP-readiness transition that this task is
                        // intentionally allowed to bypass.
                        if tcp_handle.is_none() {
                            let _ = cmds.push(NetCommand::OpenTcpListen {
                                port: NET_SHELL_TCP_PORT,
                            });
                        }
                    }
                    NetEvent::UdpPacket { .. } => {}
                    NetEvent::UdpPacketV6 { .. } => {}
                    NetEvent::IpPacket { .. } => {}
                    NetEvent::IcmpReply { .. } => {}
                    NetEvent::IcmpReplyV6 { .. } => {}
                }
            }

            // Flush buffered TX to the active TCP connection. Once the command is
            // queued successfully, the adapter owns those bytes. Keep the
            // snapshot, admission, and dequeue under the transport lock: a
            // direct-owner claim clears this queue and must not allow a stale
            // pre-claim chunk to reach the wire afterwards.
            let tx_admission = admit_net_shell_tx_chunk(TX_CHUNK_BYTES, |handle, data| {
                cmds.push(NetCommand::SendTcp { handle, data }).is_ok()
            });

            if let Some(admission) = tx_admission {
                if admission.admitted {
                    pending_tcp_writes.push_back(PendingTcpWrite {
                        handle: admission.handle,
                        remaining: admission.len,
                        len: admission.len,
                        direct_control: admission.direct_control,
                    });
                    if let Some(token) = admission.direct_control
                        && direct_control_admitted != Some(token)
                    {
                        direct_control_admitted = Some(token);
                        log_direct_control_probe(
                            "direct-control-admitted",
                            token,
                            admission.handle,
                            admission.len,
                        );
                    }
                }
                if tx_log_budget > 0 {
                    tx_log_budget -= 1;
                    crate::log!(
                        "net-shell: tx queue handle={} len={}\n",
                        admission.handle.0,
                        admission.len
                    );
                }
                if !admission.admitted {
                    // Bytes remain in NET_SHELL_STATE and will be retried.
                    crate::log!("net-shell: tx queue full (will retry)\n");
                }
            }

            if let Some(handle) = initial_repaint_handle {
                if crate::shell2::backends::net_tcp::net_shell_direct_active() {
                    initial_repaint_handle = None;
                    initial_repaint_ticks = 0;
                    initial_rx_probe.clear();
                    initial_rx_owner = None;
                    resize_rx_probe.clear();
                    resize_rx_owner = None;
                    let _ = handle;
                } else {
                    initial_repaint_ticks = initial_repaint_ticks.wrapping_add(1);
                    if initial_repaint_ticks >= INITIAL_REPAINT_WAIT_TICKS {
                        crate::shell2::repaint_backend_screen(
                            &crate::shell2::NET_TCP_SHELL_BACKEND,
                        );
                        initial_repaint_handle = None;
                        initial_repaint_ticks = 0;
                        if !initial_rx_probe.is_empty() {
                            let bytes = core::mem::take(&mut initial_rx_probe);
                            if let Some(snapshot) = initial_rx_owner.take() {
                                let _ = enqueue_net_shell_rx_if_unchanged(
                                    snapshot,
                                    handle,
                                    bytes.as_slice(),
                                );
                            }
                        }
                        let _ = handle;
                    }
                }
            }

            if crate::shell2::backends::net_tcp::net_shell_direct_passthrough_active() {
                resize_query_ticks = 0;
                resize_rx_probe.clear();
                resize_rx_owner = None;
            } else if initial_repaint_handle.is_none() {
                let active_handle = {
                    let st = NET_SHELL_STATE.lock();
                    st.handle
                };
                if active_handle.is_some() {
                    resize_query_ticks = resize_query_ticks.wrapping_add(1);
                    if resize_query_ticks >= RESIZE_QUERY_TICKS {
                        resize_query_ticks = 0;
                        if crate::shell2::backends::net_tcp::net_shell_direct_active() {
                            let _ = net_shell_terminal_size_query();
                        } else {
                            let _ = net_shell_write_bytes(TERMINAL_SIZE_QUERY);
                        }
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
