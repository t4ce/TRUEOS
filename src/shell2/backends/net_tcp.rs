use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_sync::signal::Signal;

use crate::net::adapter::NetHandle;
use crate::shell2::{ShellBackend2, ShellIo2, TerminalHandoffOwner};

pub(crate) use crate::r::net::ports::NET_SHELL_TCP_PORT;

pub(crate) struct NetTcpShellBackend;

pub(crate) static NET_TCP_SHELL_BACKEND: NetTcpShellBackend = NetTcpShellBackend;

static NET_TCP_LAST_WAS_CR: AtomicBool = AtomicBool::new(false);
pub(crate) static NET_SHELL_STARTED: AtomicBool = AtomicBool::new(false);
static NET_SHELL_DIRECT_OWNER: AtomicU32 = AtomicU32::new(0);
static NET_SHELL_DIRECT_RX_LAST_WAS_CR: AtomicBool = AtomicBool::new(false);
static NET_SHELL_DIRECT_RX_TRACE_SEQ: AtomicU32 = AtomicU32::new(0);
static NET_SHELL_WORK_READY: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();
const NET_SHELL_RX_CAP: usize = 8 * 1024;
const NET_SHELL_FRONTEND_REPLAY_CAP: usize = 256 * 1024;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) const NET_SHELL_FRONTEND_FLAG_DROPPED: u32 = 1 << 0;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) const NET_SHELL_FRONTEND_FLAG_HANDOFF: u32 = 1 << 1;
// Direct terminal apps may stop before their userspace guard flushes its
// cleanup, and release_net_shell_direct intentionally drops queued app paint.
// Restore every terminal mode that shell2 relies on before repainting it.
const NET_SHELL_DIRECT_TERMINAL_RESET: &[u8] = b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1004l\x1b[?1006l\x1b[?1015l\x1b[?2004l\x1b[?1049l\x1b[?7h\x1b[0m\x1b[39;49m\x1b[r\x1b[?25h";

pub(crate) struct NetShellState {
    pub(crate) handle: Option<NetHandle>,
    pub(crate) established_handle: Option<NetHandle>,
    pub(crate) rx: VecDeque<u8>,
    rx_oldest_ns: u64,
    pub(crate) tx: VecDeque<u8>,
    tx_oldest_ns: u64,
    direct_control_tx: VecDeque<NetShellDirectControlToken>,
    frontend_owner: Option<u8>,
    frontend_epoch: u64,
    frontend_base_seq: u64,
    frontend_next_seq: u64,
    frontend_replay: VecDeque<u8>,
    handoff_epoch: u64,
    surface_generation: u64,
    surface_cols: u32,
    surface_rows: u32,
}

pub(crate) static NET_SHELL_STATE: spin::Mutex<NetShellState> = spin::Mutex::new(NetShellState {
    handle: None,
    established_handle: None,
    rx: VecDeque::new(),
    rx_oldest_ns: 0,
    tx: VecDeque::new(),
    tx_oldest_ns: 0,
    direct_control_tx: VecDeque::new(),
    frontend_owner: None,
    frontend_epoch: 0,
    frontend_base_seq: 0,
    frontend_next_seq: 0,
    frontend_replay: VecDeque::new(),
    handoff_epoch: 0,
    surface_generation: 0,
    surface_cols: 180,
    surface_rows: 51,
});

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NetShellSurfaceSnapshot {
    pub(crate) generation: u64,
    pub(crate) cols: u32,
    pub(crate) rows: u32,
}

/// Opaque identity for one direct-owner terminal-control sequence.  This is
/// carried beside the control bytes instead of recognising escape bytes later:
/// a user application is allowed to emit the same bytes as ordinary output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NetShellDirectControlToken {
    pub(crate) owner: u32,
    pub(crate) epoch: u64,
    pub(crate) surface_generation: u64,
}

impl NetShellDirectControlToken {
    pub(crate) const fn is_blueprint(self) -> bool {
        (self.owner & TerminalHandoffOwner::STREAM_KIND) == 0
    }

    pub(crate) fn blueprint_vm(self) -> Option<u32> {
        if self.is_blueprint() {
            Some(self.owner.saturating_sub(1))
        } else {
            None
        }
    }

    pub(crate) fn stream_session(self) -> Option<u32> {
        if self.is_blueprint() {
            None
        } else {
            Some(self.owner & !TerminalHandoffOwner::STREAM_KIND)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NetShellTxAdmission {
    pub(crate) handle: NetHandle,
    pub(crate) len: usize,
    pub(crate) admitted: bool,
    pub(crate) direct: bool,
    pub(crate) direct_control: Option<NetShellDirectControlToken>,
    pub(crate) queued_at_ns: u64,
}

/// Wake the TCP bridge when terminal work arrives. `Signal` is intentionally
/// level-like here: many stdout writes may collapse into one wake because the
/// bridge drains the shared queue until it reaches adapter backpressure.
pub(crate) fn notify_net_shell_work() {
    NET_SHELL_WORK_READY.signal(());
}

pub(crate) async fn wait_for_net_shell_work() {
    NET_SHELL_WORK_READY.wait().await;
}

/// A locked snapshot of the direct-owner boundary. The epoch prevents an
/// owner ABA (`shell -> app -> shell`) from admitting a packet parsed across
/// that transition into the later queue incarnation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NetShellOwnershipSnapshot {
    owner: u32,
    epoch: u64,
}

impl NetShellOwnershipSnapshot {
    pub(crate) const fn direct_active(self) -> bool {
        self.owner != 0
    }

    pub(crate) const fn direct_passthrough_active(self) -> bool {
        (self.owner & TerminalHandoffOwner::STREAM_KIND) != 0
    }

    fn blueprint_vm(self) -> Option<u8> {
        if self.owner == 0 || self.direct_passthrough_active() {
            return None;
        }
        u8::try_from(self.owner - 1).ok()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct NetShellFrontendRead {
    pub(crate) len: usize,
    pub(crate) next_seq: u64,
    pub(crate) epoch: u64,
    pub(crate) flags: u32,
}

fn reset_frontend_replay(st: &mut NetShellState) {
    st.frontend_epoch = st.frontend_epoch.wrapping_add(1).max(1);
    st.frontend_base_seq = 0;
    st.frontend_next_seq = 0;
    st.frontend_replay.clear();
}

fn advance_surface_generation(st: &mut NetShellState) {
    st.surface_generation = st.surface_generation.saturating_add(1).max(1);
}

fn append_frontend_replay(st: &mut NetShellState, bytes: &[u8]) {
    if st.frontend_owner.is_none() {
        return;
    }
    for &byte in bytes {
        if st.frontend_replay.len() >= NET_SHELL_FRONTEND_REPLAY_CAP {
            let _ = st.frontend_replay.pop_front();
            st.frontend_base_seq = st.frontend_base_seq.wrapping_add(1);
        }
        st.frontend_replay.push_back(byte);
        st.frontend_next_seq = st.frontend_next_seq.wrapping_add(1);
    }
}

pub(crate) fn net_shell_frontend_active() -> bool {
    NET_SHELL_STATE.lock().frontend_owner.is_some()
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn attach_net_shell_frontend(vm_id: u8, cols: usize, rows: usize) -> i32 {
    if cols == 0 || rows == 0 || cols > 4_096 || rows > 4_096 {
        return -1;
    }
    {
        let mut st = NET_SHELL_STATE.lock();
        if st.frontend_owner.is_some_and(|owner| owner != vm_id) {
            return -2;
        }
        if st.frontend_owner.is_none() {
            st.frontend_owner = Some(vm_id);
            reset_frontend_replay(&mut st);
        }
    }

    crate::shell2::activate_net_shell_frontend_view(cols, rows);
    0
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn read_net_shell_frontend(
    vm_id: u8,
    read_seq: u64,
    out: &mut [u8],
) -> Result<NetShellFrontendRead, i32> {
    let st = NET_SHELL_STATE.lock();
    if st.frontend_owner != Some(vm_id) {
        return Err(-2);
    }

    let mut flags = if net_shell_direct_active() {
        NET_SHELL_FRONTEND_FLAG_HANDOFF
    } else {
        0
    };
    let start_seq = if read_seq < st.frontend_base_seq || read_seq > st.frontend_next_seq {
        flags |= NET_SHELL_FRONTEND_FLAG_DROPPED;
        st.frontend_base_seq
    } else {
        read_seq
    };
    let offset = start_seq.saturating_sub(st.frontend_base_seq) as usize;
    let len = out
        .len()
        .min(st.frontend_replay.len().saturating_sub(offset));
    for (dst, byte) in out[..len]
        .iter_mut()
        .zip(st.frontend_replay.iter().skip(offset))
    {
        *dst = *byte;
    }

    Ok(NetShellFrontendRead {
        len,
        next_seq: start_seq.wrapping_add(len as u64),
        epoch: st.frontend_epoch,
        flags,
    })
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn submit_net_shell_frontend_input(vm_id: u8, bytes: &[u8]) -> Result<usize, i32> {
    if bytes.is_empty() {
        return Ok(0);
    }
    let mut st = NET_SHELL_STATE.lock();
    if st.frontend_owner != Some(vm_id) {
        return Err(-2);
    }

    // Preserve each frontend call as one queue operation. A normal key is one
    // UTF-8 scalar; a paste is one block. If a block exceeds the shared shell
    // queue, retain its newest bytes, matching the TCP producer's bounded policy.
    let accepted = bytes.len().min(NET_SHELL_RX_CAP);
    let bytes = &bytes[bytes.len() - accepted..];
    while st.rx.len().saturating_add(bytes.len()) > NET_SHELL_RX_CAP {
        let _ = st.rx.pop_front();
    }
    st.rx.extend(bytes.iter().copied());
    Ok(accepted)
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn release_net_shell_frontend(vm_id: u8) -> i32 {
    let mut st = NET_SHELL_STATE.lock();
    if st.frontend_owner != Some(vm_id) {
        return -2;
    }
    st.frontend_owner = None;
    reset_frontend_replay(&mut st);
    0
}

pub(crate) fn net_shell_read_byte() -> Option<u8> {
    let mut st = NET_SHELL_STATE.lock();
    if NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) == 0 {
        st.rx.pop_front()
    } else {
        None
    }
}

pub(crate) fn net_shell_readable_len() -> usize {
    let st = NET_SHELL_STATE.lock();
    if NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) == 0 {
        st.rx.len()
    } else {
        0
    }
}

/// Return the active socket only when Shell2 itself has no retained bytes or
/// terminal-app ownership to transfer. The adapter performs the complementary
/// TCP-buffer/ACK check after NIC DMA has been frozen.
pub(crate) fn net_shell_quiet_handle_for_warm_handoff() -> Option<NetHandle> {
    let st = NET_SHELL_STATE.lock();
    (NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) == 0
        && st.frontend_owner.is_none()
        && st.rx.is_empty()
        && st.tx.is_empty()
        && st.direct_control_tx.is_empty()
        && st.frontend_replay.is_empty())
    .then_some(st.established_handle?)
}

pub(crate) fn net_shell_ownership_snapshot() -> NetShellOwnershipSnapshot {
    let st = NET_SHELL_STATE.lock();
    NetShellOwnershipSnapshot {
        owner: NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire),
        epoch: st.handoff_epoch,
    }
}

/// Install a newly established TCP handle and snapshot terminal ownership in
/// one transport critical section. The caller uses this result to decide
/// whether its initial repaint belongs to Shell2 or a direct terminal app.
pub(crate) fn net_shell_begin_connection(handle: NetHandle) -> (bool, NetShellOwnershipSnapshot) {
    let mut st = NET_SHELL_STATE.lock();
    // `TcpData` is allowed to select `handle` before `TcpEstablished` is
    // drained, so transport identity needs its own established-handle field.
    // Clearing it on close also makes reuse of the same adapter handle a new
    // terminal surface incarnation.
    let is_new_connection = st.established_handle != Some(handle);
    st.handle = Some(handle);
    if is_new_connection {
        st.established_handle = Some(handle);
        st.rx.clear();
        st.rx_oldest_ns = 0;
        st.tx.clear();
        st.tx_oldest_ns = 0;
        // A command has not yet been admitted to this connection, so a reset
        // retained from an earlier surface is stale.  The TCP task observes
        // the direct owner below and queues one freshly tagged reset for this
        // connection after the state transition.
        st.direct_control_tx.clear();
        advance_surface_generation(&mut st);
    }
    (
        is_new_connection,
        NetShellOwnershipSnapshot {
            owner: NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire),
            epoch: st.handoff_epoch,
        },
    )
}

/// Record terminal geometry in the same state object as surface identity.
/// Geometry is compared independently by Crossterm; only ownership/reconnect
/// boundaries advance the generation, so a generation change also marks a
/// safe byte-parser reset point.
pub(crate) fn update_net_shell_surface_size(cols: usize, rows: usize) -> bool {
    if cols == 0 || rows == 0 {
        return false;
    }
    let cols = cols.min(u32::MAX as usize) as u32;
    let rows = rows.min(u32::MAX as usize) as u32;
    let mut st = NET_SHELL_STATE.lock();
    if st.surface_cols == cols && st.surface_rows == rows {
        return false;
    }
    st.surface_cols = cols;
    st.surface_rows = rows;
    let owner = NetShellOwnershipSnapshot {
        owner: NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire),
        epoch: st.handoff_epoch,
    };
    drop(st);
    if let Some(vm_id) = owner.blueprint_vm() {
        crate::hv::notify_blueprint_console_input(vm_id);
    }
    true
}

/// Return one transport-locked surface snapshot for the exact Blueprint owner.
pub(crate) fn net_shell_direct_surface_snapshot(vm_id: u8) -> Option<NetShellSurfaceSnapshot> {
    let st = NET_SHELL_STATE.lock();
    (NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) == TerminalHandoffOwner::blueprint(vm_id).raw())
        .then_some(NetShellSurfaceSnapshot {
            generation: st.surface_generation.max(1),
            cols: st.surface_cols.max(1),
            rows: st.surface_rows.max(1),
        })
}

/// Admit one parsed TCP input packet only if the transport ownership has not
/// changed since its parser took a snapshot. The handle test belongs in the
/// same critical section so reconnects cannot attach old bytes to a new queue.
pub(crate) fn enqueue_net_shell_rx_if_unchanged(
    snapshot: NetShellOwnershipSnapshot,
    handle: NetHandle,
    bytes: &[u8],
) -> bool {
    let mut st = NET_SHELL_STATE.lock();
    if st.handoff_epoch != snapshot.epoch
        || NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) != snapshot.owner
    {
        return false;
    }
    if st.handle.is_none() {
        st.handle = Some(handle);
    }
    if st.handle != Some(handle) {
        return false;
    }
    let vm_id = snapshot.blueprint_vm();
    if vm_id.is_some() && st.rx.is_empty() && !bytes.is_empty() {
        st.rx_oldest_ns = crate::chronos::monotonic_nanos();
    }
    for &byte in bytes {
        if st.rx.len() >= NET_SHELL_RX_CAP {
            let _ = st.rx.pop_front();
        }
        st.rx.push_back(byte);
    }
    let vm_id = (!bytes.is_empty()).then_some(vm_id).flatten();
    drop(st);
    if let Some(vm_id) = vm_id {
        crate::hv::notify_blueprint_console_input(vm_id);
    }
    true
}

fn enqueue_net_shell_bytes(st: &mut NetShellState, bytes: &[u8]) {
    const MAX_TX: usize = 2 * 1024 * 1024;
    if bytes.is_empty() {
        return;
    }
    if st.tx.is_empty() {
        st.tx_oldest_ns = crate::chronos::monotonic_nanos();
    }
    append_frontend_replay(st, bytes);
    for &b in bytes {
        if st.tx.len() >= MAX_TX {
            let _ = st.tx.pop_front();
        }
        st.tx.push_back(b);
    }
}

fn queue_direct_terminal_reset(st: &mut NetShellState, owner: u32) -> bool {
    if owner == 0 {
        return false;
    }
    let token = NetShellDirectControlToken {
        owner,
        epoch: st.handoff_epoch,
        surface_generation: st.surface_generation.max(1),
    };
    // A reset already waiting for this ownership epoch is sufficient.  This
    // preserves the reset-before-app-output order without turning repeated
    // reconnect observations into a control-byte storm.
    if !st.direct_control_tx.contains(&token) {
        append_frontend_replay(st, NET_SHELL_DIRECT_TERMINAL_RESET);
        st.direct_control_tx.push_back(token);
    }
    true
}

/// Admit the complete terminal output currently pending under the transport lock.
/// Direct terminal reset bytes live in a tagged queue, ahead of untagged app
/// output, so their lifecycle can be proven without scanning payload bytes. The
/// adapter queue is the only backpressure boundary; this layer does not pace an
/// immediate-mode terminal with a manually selected chunk or burst size.
pub(crate) fn admit_net_shell_tx<F>(submit: F) -> Option<NetShellTxAdmission>
where
    F: FnOnce(NetHandle, Vec<u8>) -> bool,
{
    let mut st = NET_SHELL_STATE.lock();
    let handle = st.handle?;
    let direct = NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) != 0;
    if let Some(token) = st.direct_control_tx.front().copied() {
        let data = NET_SHELL_DIRECT_TERMINAL_RESET.to_vec();
        let len = data.len();
        let admitted = submit(handle, data);
        if admitted {
            let _ = st.direct_control_tx.pop_front();
        }
        return Some(NetShellTxAdmission {
            handle,
            len,
            admitted,
            direct,
            direct_control: Some(token),
            queued_at_ns: 0,
        });
    }
    if st.tx.is_empty() {
        return None;
    }
    let queued_at_ns = st.tx_oldest_ns;
    let mut data = Vec::with_capacity(st.tx.len());
    data.extend(st.tx.iter().copied());
    let len = data.len();
    let admitted = submit(handle, data);
    if admitted {
        for _ in 0..len {
            let _ = st.tx.pop_front();
        }
        if st.tx.is_empty() {
            st.tx_oldest_ns = 0;
        }
    }
    Some(NetShellTxAdmission {
        handle,
        len,
        admitted,
        direct,
        direct_control: None,
        queued_at_ns,
    })
}

/// Shell-origin output is admitted only while Shell2 owns the transport. The
/// state lock closes the check/write race with a direct-owner claim.
pub(crate) fn net_shell_write_bytes(bytes: &[u8]) -> bool {
    {
        let mut st = NET_SHELL_STATE.lock();
        if NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) != 0 {
            return false;
        }
        enqueue_net_shell_bytes(&mut st, bytes);
    }
    notify_net_shell_work();
    true
}

/// Send the one terminal-control sequence that a direct owner needs after a
/// transport reconnect. This deliberately is not a generic privileged output
/// API: arbitrary Shell2 bytes must remain suppressed while an app owns the
/// TCP terminal.
pub(crate) fn net_shell_direct_reset_terminal() -> bool {
    let queued = {
        let mut st = NET_SHELL_STATE.lock();
        queue_direct_terminal_reset(&mut st, NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire))
    };
    if queued {
        notify_net_shell_work();
    }
    queued
}

/// Request terminal geometry for the current direct owner. The TCP reader
/// already recognizes and removes the matching report before the app's input
/// queue is exposed, so this control byte cannot leak into Crossterm input.
pub(crate) fn net_shell_terminal_size_query() -> bool {
    {
        let mut st = NET_SHELL_STATE.lock();
        if NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) == 0 {
            return false;
        }
        enqueue_net_shell_bytes(&mut st, b"\x1b[18t");
    }
    notify_net_shell_work();
    true
}

fn claim_net_shell_terminal(owner: TerminalHandoffOwner) -> bool {
    let owner = owner.raw();
    let committed = {
        let mut st = NET_SHELL_STATE.lock();
        let previous = NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire);
        if previous != 0 && previous != owner {
            return false;
        }

        // The owner token and its byte queues form one transaction. Publishing
        // only while this lock is held prevents Shell2 from consuming an app byte
        // (or an app from painting ahead of the reset) between the ownership
        // change and queue reset.
        st.rx.clear();
        st.rx_oldest_ns = 0;
        st.tx.clear();
        st.tx_oldest_ns = 0;
        st.direct_control_tx.clear();
        reset_frontend_replay(&mut st);
        st.handoff_epoch = st.handoff_epoch.wrapping_add(1).max(1);
        advance_surface_generation(&mut st);
        NET_TCP_LAST_WAS_CR.store(false, Ordering::Release);
        NET_SHELL_DIRECT_RX_LAST_WAS_CR.store(false, Ordering::Release);
        NET_SHELL_DIRECT_RX_TRACE_SEQ.store(0, Ordering::Release);
        let epoch = st.handoff_epoch;
        let handle = st.handle;
        let surface_generation = st.surface_generation;
        let _ = queue_direct_terminal_reset(&mut st, owner);
        NET_SHELL_DIRECT_OWNER.store(owner, Ordering::Release);
        (epoch, handle, surface_generation)
    };
    notify_net_shell_work();
    // This marker is deliberately after the state lock drops: a diagnostic
    // transport must never extend the handoff critical section.
    if (owner & TerminalHandoffOwner::STREAM_KIND) == 0 {
        crate::log_os::service_important_line(format_args!(
            "terminal-handoff probe=direct-claim-committed owner_kind=blueprint owner={} vm={} epoch={} handle={:?} surface_generation={}\n",
            owner,
            owner.saturating_sub(1),
            committed.0,
            committed.1,
            committed.2,
        ));
    } else {
        crate::log_os::service_important_line(format_args!(
            "terminal-handoff probe=direct-claim-committed owner_kind=stream owner={} session={} epoch={} handle={:?} surface_generation={}\n",
            owner,
            owner & !TerminalHandoffOwner::STREAM_KIND,
            committed.0,
            committed.1,
            committed.2,
        ));
    }
    true
}

fn release_net_shell_terminal(owner: TerminalHandoffOwner) -> bool {
    {
        let mut st = NET_SHELL_STATE.lock();
        if NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) != owner.raw() {
            return false;
        }
        st.rx.clear();
        st.rx_oldest_ns = 0;
        st.tx.clear();
        st.tx_oldest_ns = 0;
        st.direct_control_tx.clear();
        reset_frontend_replay(&mut st);
        st.handoff_epoch = st.handoff_epoch.wrapping_add(1).max(1);
        advance_surface_generation(&mut st);
        NET_TCP_LAST_WAS_CR.store(false, Ordering::Release);
        NET_SHELL_DIRECT_RX_LAST_WAS_CR.store(false, Ordering::Release);
        // Queue the reset before publishing Shell2 ownership. A shell writer
        // which acquires the lock next therefore always follows the reset.
        enqueue_net_shell_bytes(&mut st, NET_SHELL_DIRECT_TERMINAL_RESET);
        NET_SHELL_DIRECT_OWNER.store(0, Ordering::Release);
    }
    notify_net_shell_work();
    crate::shell2::repaint_backend_screen(&NET_TCP_SHELL_BACKEND);
    true
}

pub(crate) fn net_shell_direct_active() -> bool {
    NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) != 0
}

pub(crate) fn net_shell_direct_passthrough_active() -> bool {
    (NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) & TerminalHandoffOwner::STREAM_KIND) != 0
}

fn net_shell_terminal_read(owner: TerminalHandoffOwner, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    let mut st = NET_SHELL_STATE.lock();
    if NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) != owner.raw() {
        return 0;
    }
    let read = out.len().min(st.rx.len());
    for byte in &mut out[..read] {
        *byte = st.rx.pop_front().unwrap_or_default();
    }
    read
}

fn net_shell_terminal_write(owner: TerminalHandoffOwner, bytes: &[u8]) -> bool {
    {
        let mut st = NET_SHELL_STATE.lock();
        if NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) != owner.raw() {
            return false;
        }
        enqueue_net_shell_bytes(&mut st, bytes);
    }
    notify_net_shell_work();
    true
}

pub(crate) fn claim_net_shell_direct(vm_id: u8) -> bool {
    claim_net_shell_terminal(TerminalHandoffOwner::blueprint(vm_id))
}

pub(crate) fn release_net_shell_direct(vm_id: u8) -> bool {
    release_net_shell_terminal(TerminalHandoffOwner::blueprint(vm_id))
}

pub(crate) fn net_shell_direct_write(vm_id: u8, bytes: &[u8]) -> bool {
    net_shell_terminal_write(TerminalHandoffOwner::blueprint(vm_id), bytes)
}

pub(crate) fn net_shell_direct_read(vm_id: u8, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    let mut st = NET_SHELL_STATE.lock();
    if NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire)
        != TerminalHandoffOwner::blueprint(vm_id).raw()
    {
        return 0;
    }
    let queued_at_ns = st.rx_oldest_ns;
    let mut read = 0usize;
    let mut last_was_cr = NET_SHELL_DIRECT_RX_LAST_WAS_CR.load(Ordering::Acquire);
    while read < out.len() {
        let Some(byte) = st.rx.pop_front() else {
            break;
        };
        match byte {
            b'\n' if last_was_cr => {
                last_was_cr = false;
            }
            b'\n' => {
                out[read] = b'\r';
                read += 1;
                last_was_cr = false;
            }
            b'\r' => {
                out[read] = byte;
                read += 1;
                last_was_cr = true;
            }
            _ => {
                out[read] = byte;
                read += 1;
                last_was_cr = false;
            }
        }
    }
    if st.rx.is_empty() {
        st.rx_oldest_ns = 0;
    }
    NET_SHELL_DIRECT_RX_LAST_WAS_CR.store(last_was_cr, Ordering::Release);
    drop(st);
    if read != 0 {
        let trace_seq = NET_SHELL_DIRECT_RX_TRACE_SEQ.fetch_add(1, Ordering::Relaxed);
        if trace_seq < 32 {
            let queue_wait_us =
                crate::chronos::monotonic_nanos().saturating_sub(queued_at_ns) / 1_000;
            crate::log!(
                "net-shell: direct rx read vm={} len={} queue_wait_us={} seq={}\n",
                vm_id,
                read,
                queue_wait_us,
                trace_seq,
            );
        }
    }
    read
}

pub(crate) fn net_shell_direct_readable_len(vm_id: u8) -> usize {
    let st = NET_SHELL_STATE.lock();
    if NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire)
        == TerminalHandoffOwner::blueprint(vm_id).raw()
    {
        st.rx.len()
    } else {
        0
    }
}

impl ShellIo2 for NetTcpShellBackend {
    fn output_mask(&self) -> crate::shell2::OutputMask {
        crate::shell2::OUTPUT_NET_TCP_MASK
    }

    fn transport_scope(&self) -> u8 {
        crate::shell2::TRANSPORT_NET_TCP_SCOPE
    }

    #[inline]
    fn raw_write_str(&self, s: &str) {
        crate::shell2::crlf::write_bytes_crlf(s.as_bytes(), &NET_TCP_LAST_WAS_CR, |chunk| {
            let _ = net_shell_write_bytes(chunk);
        });
    }

    #[inline]
    fn raw_write_fmt(&self, args: core::fmt::Arguments<'_>) {
        struct Writer;

        impl Write for Writer {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                crate::shell2::crlf::write_bytes_crlf(
                    s.as_bytes(),
                    &NET_TCP_LAST_WAS_CR,
                    |chunk| {
                        let _ = net_shell_write_bytes(chunk);
                    },
                );
                Ok(())
            }
        }

        let _ = Writer.write_fmt(args);
    }

    #[inline]
    fn raw_write_char(&self, ch: char) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        crate::shell2::crlf::write_bytes_crlf(s.as_bytes(), &NET_TCP_LAST_WAS_CR, |chunk| {
            let _ = net_shell_write_bytes(chunk);
        });
    }

    #[inline]
    fn raw_write_byte(&self, b: u8) {
        crate::shell2::crlf::write_bytes_crlf(&[b], &NET_TCP_LAST_WAS_CR, |chunk| {
            let _ = net_shell_write_bytes(chunk);
        });
    }
}

impl ShellBackend2 for NetTcpShellBackend {
    #[inline]
    fn init(&self) {}

    #[inline]
    fn read_byte(&self) -> Option<u8> {
        net_shell_read_byte()
    }

    fn claim_terminal_handoff(&self, owner: TerminalHandoffOwner) -> bool {
        claim_net_shell_terminal(owner)
    }

    fn release_terminal_handoff(&self, owner: TerminalHandoffOwner) -> bool {
        release_net_shell_terminal(owner)
    }

    fn terminal_handoff_active(&self) -> bool {
        net_shell_direct_active()
    }

    fn supports_terminal_handoff(&self) -> bool {
        true
    }

    fn terminal_handoff_read(&self, owner: TerminalHandoffOwner, out: &mut [u8]) -> usize {
        net_shell_terminal_read(owner, out)
    }

    fn terminal_handoff_write(&self, owner: TerminalHandoffOwner, bytes: &[u8]) -> bool {
        net_shell_terminal_write(owner, bytes)
    }
}
