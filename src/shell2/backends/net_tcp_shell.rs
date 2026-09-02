use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use trueos_executor::task;
use trueos_time::{Duration as EmbassyDuration, Instant, with_timeout};

use crate::net::adapter::{
    NetCommand, NetEvent, NetHandle, NetQueue, SocketKind, register_app_queues,
};
use crate::shell2::backends::net_tcp::{
    NET_SHELL_STARTED, NET_SHELL_STATE, NET_SHELL_TCP_PORT, NetShellDirectControlToken,
    NetShellOwnershipSnapshot, admit_net_shell_tx, enqueue_net_shell_rx_if_unchanged,
    net_shell_begin_connection, net_shell_direct_reset_terminal, net_shell_ownership_snapshot,
    net_shell_terminal_size_query, net_shell_write_bytes, update_net_shell_surface_size,
    wait_for_net_shell_work,
};

const TERMINAL_SIZE_QUERY: &[u8] = b"\x1b[18t";
const INITIAL_REPAINT_WAIT_MS: u64 = 80;
const RESIZE_QUERY_INTERVAL_MS: u64 = 1_000;
// `CSI 8;<rows>;<cols>t` is small even with maximum decimal values.  Keeping
// an incomplete candidate bounded prevents an unrelated, unterminated CSI
// sequence from retaining TCP input indefinitely.
const TERMINAL_SIZE_REPORT_MAX_LEN: usize = 32;
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

enum TerminalSizeReport {
    Complete {
        cols: usize,
        rows: usize,
        len: usize,
    },
    Incomplete,
    NotSizeReport,
}

/// Classify a possible `CSI 8;<rows>;<cols>t` at the start of `data`.
///
/// TCP is a byte stream, so a terminal can split one reply anywhere or batch
/// many replies with normal key sequences.  The caller must retain only an
/// incomplete *size-report* prefix; all other bytes remain application input.
fn parse_terminal_size_report_prefix(data: &[u8]) -> TerminalSizeReport {
    debug_assert_eq!(data.first(), Some(&0x1b));

    if data.len() == 1 {
        return TerminalSizeReport::Incomplete;
    }
    if data[1] != b'[' {
        return TerminalSizeReport::NotSizeReport;
    }
    if data.len() == 2 {
        return TerminalSizeReport::Incomplete;
    }
    if data[2] != b'8' || data.len() > TERMINAL_SIZE_REPORT_MAX_LEN {
        return TerminalSizeReport::NotSizeReport;
    }

    let mut cursor = 3usize;
    if cursor == data.len() {
        return TerminalSizeReport::Incomplete;
    }
    if data[cursor] != b';' {
        return TerminalSizeReport::NotSizeReport;
    }
    cursor += 1;

    let mut values = [0usize; 2];
    for (index, value) in values.iter_mut().enumerate() {
        let begin = cursor;
        while cursor < data.len() && data[cursor].is_ascii_digit() {
            *value = value
                .saturating_mul(10)
                .saturating_add(usize::from(data[cursor] - b'0'));
            cursor += 1;
        }
        if cursor == begin {
            return if cursor == data.len() {
                TerminalSizeReport::Incomplete
            } else {
                TerminalSizeReport::NotSizeReport
            };
        }
        if cursor == data.len() {
            return TerminalSizeReport::Incomplete;
        }
        let terminator = if index == 0 { b';' } else { b't' };
        if data[cursor] != terminator {
            return TerminalSizeReport::NotSizeReport;
        }
        cursor += 1;
    }

    if values[0] == 0 || values[1] == 0 {
        return TerminalSizeReport::NotSizeReport;
    }
    TerminalSizeReport::Complete {
        cols: values[1],
        rows: values[0],
        len: cursor,
    }
}

#[derive(Default)]
struct TerminalSizeReportFilter {
    pending: Vec<u8>,
}

struct FilteredTerminalInput {
    bytes: Vec<u8>,
    latest_size: Option<(usize, usize)>,
}

impl TerminalSizeReportFilter {
    fn clear(&mut self) {
        self.pending.clear();
    }

    /// Remove every complete terminal-size reply while preserving ordinary
    /// terminal input byte-for-byte.  A suffix that might be a split reply is
    /// retained and completed by the next TCP receive event.
    fn filter(&mut self, data: &[u8]) -> FilteredTerminalInput {
        let mut stream = core::mem::take(&mut self.pending);
        stream.extend_from_slice(data);

        let mut bytes = Vec::with_capacity(stream.len());
        let mut latest_size = None;
        let mut cursor = 0usize;
        while cursor < stream.len() {
            if stream[cursor] != 0x1b {
                bytes.push(stream[cursor]);
                cursor += 1;
                continue;
            }

            match parse_terminal_size_report_prefix(&stream[cursor..]) {
                TerminalSizeReport::Complete { cols, rows, len } => {
                    latest_size = Some((cols, rows));
                    cursor += len;
                }
                TerminalSizeReport::Incomplete => {
                    self.pending.extend_from_slice(&stream[cursor..]);
                    break;
                }
                TerminalSizeReport::NotSizeReport => {
                    bytes.push(stream[cursor]);
                    cursor += 1;
                }
            }
        }

        FilteredTerminalInput { bytes, latest_size }
    }
}

fn record_terminal_size(cols: usize, rows: usize, direct_mode: bool) -> bool {
    let _ = update_net_shell_surface_size(cols, rows);
    !crate::shell2::backends::net_tcp::net_shell_frontend_active()
        && crate::shell2::apply_reported_terminal_size_for_backend(
            &crate::shell2::NET_TCP_SHELL_BACKEND,
            cols,
            rows,
        )
        && !direct_mode
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

        let mut error_events: u64 = 0;
        let mut logged_first_rx: bool = false;
        let mut logged_first_wire_rx: bool = false;
        let mut tx_admit_log_budget: u32 = 16;
        let mut direct_tx_admit_log_budget: u32 = 16;
        let mut tx_flush_log_budget: u32 = 16;
        let mut direct_tx_flush_log_budget: u32 = 16;
        let mut pending_tcp_writes: VecDeque<PendingTcpWrite> = VecDeque::new();
        let mut direct_control_admitted: Option<NetShellDirectControlToken> = None;
        let mut direct_control_sent: Option<NetShellDirectControlToken> = None;
        let mut tcp_handle: Option<NetHandle> = None;
        let mut initial_repaint_handle: Option<NetHandle> = None;
        let mut initial_repaint_deadline: Option<Instant> = None;
        let mut initial_rx_probe: Vec<u8> = Vec::new();
        let mut initial_rx_owner: Option<NetShellOwnershipSnapshot> = None;
        let mut next_resize_query: Option<Instant> = None;
        let mut terminal_size_filter = TerminalSizeReportFilter::default();
        let mut terminal_size_filter_owner: Option<NetShellOwnershipSnapshot> = None;

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
                            initial_repaint_deadline = None;
                            initial_rx_probe.clear();
                            initial_rx_owner = None;
                            terminal_size_filter.clear();
                            terminal_size_filter_owner = None;
                            next_resize_query = None;
                            if ownership.direct_active() {
                                let _ = net_shell_direct_reset_terminal();
                                // A stream owns its protocol byte-for-byte, so it
                                // must never receive Shell2's geometry reply.
                                if !ownership.direct_passthrough_active() {
                                    let _ = net_shell_terminal_size_query();
                                }
                                initial_repaint_handle = None;
                            } else if net_shell_write_bytes(TERMINAL_SIZE_QUERY) {
                                initial_repaint_handle = Some(handle);
                                initial_repaint_deadline = Some(
                                    Instant::now()
                                        + EmbassyDuration::from_millis(INITIAL_REPAINT_WAIT_MS),
                                );
                            } else {
                                // A direct claim won after the locked snapshot;
                                // its claim reset is authoritative, and this
                                // narrowly scoped query refreshes its geometry.
                                if !crate::shell2::backends::net_tcp::net_shell_direct_passthrough_active() {
                                    let _ = net_shell_terminal_size_query();
                                }
                                initial_repaint_handle = None;
                            }
                        }
                        logged_first_rx = false;
                        logged_first_wire_rx = false;
                        tx_admit_log_budget = 16;
                        direct_tx_admit_log_budget = 16;
                        tx_flush_log_budget = 16;
                        direct_tx_flush_log_budget = 16;
                        crate::log!(
                            "net-shell: tcp established handle={} ms={}\n",
                            handle.0,
                            Instant::now().as_millis()
                        );
                        if let Some(notice) = crate::live_update::take_shell_notice()
                            && !net_shell_write_bytes(notice)
                        {
                            crate::live_update::rearm_shell_notice();
                        }
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
                        let ownership = net_shell_ownership_snapshot();
                        let direct_mode = ownership.direct_active();
                        let direct_passthrough = ownership.direct_passthrough_active();
                        if initial_rx_owner.is_some_and(|owner| owner != ownership) {
                            initial_rx_probe.clear();
                            initial_rx_owner = None;
                        }
                        if terminal_size_filter_owner.is_some_and(|owner| owner != ownership) {
                            terminal_size_filter.clear();
                        }
                        let rx_data = if direct_passthrough {
                            initial_repaint_handle = None;
                            initial_repaint_deadline = None;
                            initial_rx_probe.clear();
                            initial_rx_owner = None;
                            terminal_size_filter.clear();
                            terminal_size_filter_owner = None;
                            data
                        } else {
                            terminal_size_filter_owner = Some(ownership);
                            let filtered = terminal_size_filter.filter(data.as_slice());

                            if initial_repaint_handle == Some(handle) && !direct_mode {
                                if initial_rx_probe.is_empty() {
                                    initial_rx_owner = Some(ownership);
                                }
                                initial_rx_probe.extend_from_slice(&filtered.bytes);
                                if let Some((cols, rows)) = filtered.latest_size {
                                    let _ = record_terminal_size(cols, rows, direct_mode);
                                    // A newly connected shell must paint even when the
                                    // reported dimensions equal the previous surface.
                                    crate::shell2::repaint_backend_screen(
                                        &crate::shell2::NET_TCP_SHELL_BACKEND,
                                    );
                                    initial_repaint_handle = None;
                                    initial_repaint_deadline = None;
                                    initial_rx_owner = None;
                                    core::mem::take(&mut initial_rx_probe)
                                } else if initial_rx_probe.len() <= TERMINAL_SIZE_REPORT_MAX_LEN {
                                    continue;
                                } else {
                                    initial_rx_owner = None;
                                    core::mem::take(&mut initial_rx_probe)
                                }
                            } else {
                                if let Some((cols, rows)) = filtered.latest_size
                                    && record_terminal_size(cols, rows, direct_mode)
                                {
                                    crate::shell2::repaint_backend_screen(
                                        &crate::shell2::NET_TCP_SHELL_BACKEND,
                                    );
                                }
                                filtered.bytes
                            }
                        };
                        if !enqueue_net_shell_rx_if_unchanged(ownership, handle, &rx_data) {
                            // A claim/release occurred while this packet was
                            // being parsed. Never feed its bytes (or a partial
                            // terminal-size report) to the next owner.
                            initial_rx_probe.clear();
                            initial_rx_owner = None;
                            terminal_size_filter.clear();
                            terminal_size_filter_owner = None;
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
                        let direct = crate::shell2::backends::net_tcp::net_shell_direct_active();
                        let log_flush = if direct {
                            let log = direct_tx_flush_log_budget > 0;
                            direct_tx_flush_log_budget =
                                direct_tx_flush_log_budget.saturating_sub(1);
                            log
                        } else {
                            let log = tx_flush_log_budget > 0;
                            tx_flush_log_budget = tx_flush_log_budget.saturating_sub(1);
                            log
                        };
                        if log_flush {
                            crate::log!(
                                "net-shell: tx flushed handle={} len={} direct={} ms={}\n",
                                handle.0,
                                len,
                                direct as u8,
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
                        if closed_active {
                            if initial_repaint_handle == Some(handle) {
                                initial_repaint_handle = None;
                                initial_repaint_deadline = None;
                            }
                            next_resize_query = None;
                            initial_rx_probe.clear();
                            initial_rx_owner = None;
                            terminal_size_filter.clear();
                            terminal_size_filter_owner = None;
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
                        error_events = error_events.saturating_add(1);
                        if error_events <= 2 || error_events.is_power_of_two() {
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

            // Drain terminal output to the adapter's real backpressure boundary.
            // Once a command is accepted, the adapter owns those bytes. Keeping
            // snapshot, admission, and dequeue under the transport lock prevents
            // a direct-owner claim from putting stale pre-claim paint on the wire.
            loop {
                let Some(admission) = admit_net_shell_tx(|handle, data| {
                    cmds.push(NetCommand::SendTcp { handle, data }).is_ok()
                }) else {
                    break;
                };

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
                let log_admission = if admission.direct {
                    let log = direct_tx_admit_log_budget > 0;
                    direct_tx_admit_log_budget = direct_tx_admit_log_budget.saturating_sub(1);
                    log
                } else {
                    let log = tx_admit_log_budget > 0;
                    tx_admit_log_budget = tx_admit_log_budget.saturating_sub(1);
                    log
                };
                if log_admission {
                    let queue_wait_us = if admission.queued_at_ns == 0 {
                        0
                    } else {
                        crate::chronos::monotonic_nanos().saturating_sub(admission.queued_at_ns)
                            / 1_000
                    };
                    crate::log!(
                        "net-shell: tx admit handle={} len={} direct={} queue_wait_us={} ms={}\n",
                        admission.handle.0,
                        admission.len,
                        admission.direct as u8,
                        queue_wait_us,
                        Instant::now().as_millis(),
                    );
                }
                if !admission.admitted {
                    // Bytes remain in NET_SHELL_STATE and will be retried.
                    crate::log!("net-shell: tx queue full (will retry)\n");
                    // Retrying in this wake cannot make command-queue capacity
                    // available and would only starve the rest of the loop.
                    break;
                }
            }

            if let Some(handle) = initial_repaint_handle {
                if crate::shell2::backends::net_tcp::net_shell_direct_active() {
                    initial_repaint_handle = None;
                    initial_repaint_deadline = None;
                    initial_rx_probe.clear();
                    initial_rx_owner = None;
                    terminal_size_filter.clear();
                    terminal_size_filter_owner = None;
                    let _ = handle;
                } else if initial_repaint_deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
                {
                    crate::shell2::repaint_backend_screen(&crate::shell2::NET_TCP_SHELL_BACKEND);
                    initial_repaint_handle = None;
                    initial_repaint_deadline = None;
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

            if crate::shell2::backends::net_tcp::net_shell_direct_passthrough_active() {
                next_resize_query = None;
                terminal_size_filter.clear();
                terminal_size_filter_owner = None;
            } else if initial_repaint_handle.is_none() {
                let active_handle = {
                    let st = NET_SHELL_STATE.lock();
                    st.handle
                };
                if active_handle.is_some() {
                    let now = Instant::now();
                    if next_resize_query.is_none() {
                        next_resize_query =
                            Some(now + EmbassyDuration::from_millis(RESIZE_QUERY_INTERVAL_MS));
                    } else if next_resize_query.is_some_and(|deadline| now >= deadline) {
                        next_resize_query =
                            Some(now + EmbassyDuration::from_millis(RESIZE_QUERY_INTERVAL_MS));
                        if crate::shell2::backends::net_tcp::net_shell_direct_active() {
                            let _ = net_shell_terminal_size_query();
                        } else {
                            let _ = net_shell_write_bytes(TERMINAL_SIZE_QUERY);
                        }
                    }
                } else {
                    next_resize_query = None;
                }
            } else {
                next_resize_query = None;
            }

            // Work wakes this task immediately. When idle, sleep directly until
            // the next protocol deadline instead of imposing a paint cadence.
            let maintenance_deadline = match (initial_repaint_deadline, next_resize_query) {
                (Some(initial), Some(resize)) => Some(initial.min(resize)),
                (Some(initial), None) => Some(initial),
                (None, Some(resize)) => Some(resize),
                (None, None) => None,
            };
            if let Some(deadline) = maintenance_deadline {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let _ = with_timeout(remaining, wait_for_net_shell_work()).await;
            } else {
                wait_for_net_shell_work().await;
            }
        }
    }
    .await;
}

#[cfg(test)]
mod tests {
    use super::TerminalSizeReportFilter;

    #[test]
    fn filters_every_coalesced_terminal_size_reply() {
        let mut filter = TerminalSizeReportFilter::default();
        let result = filter.filter(b"a\x1b[8;30;118t\x1b[8;31;120tb");

        assert_eq!(result.bytes, b"ab");
        assert_eq!(result.latest_size, Some((120, 31)));
    }

    #[test]
    fn preserves_another_csi_sequence_before_the_size_reply() {
        let mut filter = TerminalSizeReportFilter::default();
        let result = filter.filter(b"\x1b[A\x1b[8;30;118t");

        assert_eq!(result.bytes, b"\x1b[A");
        assert_eq!(result.latest_size, Some((118, 30)));
    }

    #[test]
    fn joins_a_size_reply_split_across_tcp_reads() {
        let mut filter = TerminalSizeReportFilter::default();
        let first = filter.filter(b"a\x1b[8;30;");
        let second = filter.filter(b"118tb");

        assert_eq!(first.bytes, b"a");
        assert_eq!(first.latest_size, None);
        assert_eq!(second.bytes, b"b");
        assert_eq!(second.latest_size, Some((118, 30)));
    }
}
