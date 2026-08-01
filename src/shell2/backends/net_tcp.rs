use alloc::collections::VecDeque;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::net::adapter::NetHandle;
use crate::shell2::{ShellBackend2, ShellIo2, TerminalHandoffOwner};

pub(crate) use crate::r::net::ports::NET_SHELL_TCP_PORT;

pub(crate) struct NetTcpShellBackend;

pub(crate) static NET_TCP_SHELL_BACKEND: NetTcpShellBackend = NetTcpShellBackend;

static NET_TCP_LAST_WAS_CR: AtomicBool = AtomicBool::new(false);
pub(crate) static NET_SHELL_STARTED: AtomicBool = AtomicBool::new(false);
static NET_SHELL_DIRECT_OWNER: AtomicU32 = AtomicU32::new(0);
static NET_SHELL_DIRECT_RX_LAST_WAS_CR: AtomicBool = AtomicBool::new(false);
const NET_SHELL_RX_CAP: usize = 8 * 1024;
const NET_SHELL_FRONTEND_REPLAY_CAP: usize = 256 * 1024;
pub(crate) const NET_SHELL_FRONTEND_FLAG_DROPPED: u32 = 1 << 0;
pub(crate) const NET_SHELL_FRONTEND_FLAG_HANDOFF: u32 = 1 << 1;
// Direct terminal apps may stop before their userspace guard flushes its
// cleanup, and release_net_shell_direct intentionally drops queued app paint.
// Restore every terminal mode that shell2 relies on before repainting it.
const NET_SHELL_DIRECT_TERMINAL_RESET: &[u8] = b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1004l\x1b[?1006l\x1b[?1015l\x1b[?2004l\x1b[?1049l\x1b[0m\x1b[39;49m\x1b[r\x1b[?25h";

pub(crate) struct NetShellState {
    pub(crate) handle: Option<NetHandle>,
    pub(crate) rx: VecDeque<u8>,
    pub(crate) tx: VecDeque<u8>,
    frontend_owner: Option<u8>,
    frontend_epoch: u64,
    frontend_base_seq: u64,
    frontend_next_seq: u64,
    frontend_replay: VecDeque<u8>,
}

pub(crate) static NET_SHELL_STATE: spin::Mutex<NetShellState> = spin::Mutex::new(NetShellState {
    handle: None,
    rx: VecDeque::new(),
    tx: VecDeque::new(),
    frontend_owner: None,
    frontend_epoch: 0,
    frontend_base_seq: 0,
    frontend_next_seq: 0,
    frontend_replay: VecDeque::new(),
});

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    if net_shell_direct_active() {
        return None;
    }
    NET_SHELL_STATE.lock().rx.pop_front()
}

pub(crate) fn net_shell_readable_len() -> usize {
    if net_shell_direct_active() {
        return 0;
    }
    NET_SHELL_STATE.lock().rx.len()
}

pub(crate) fn net_shell_write_bytes(bytes: &[u8]) {
    const MAX_TX: usize = 2 * 1024 * 1024;
    let mut st = NET_SHELL_STATE.lock();
    append_frontend_replay(&mut st, bytes);
    let mut dropped = 0usize;
    for &b in bytes {
        if st.tx.len() >= MAX_TX {
            let _ = st.tx.pop_front();
            dropped = dropped.saturating_add(1);
        }
        st.tx.push_back(b);
    }
    if dropped != 0 {
        crate::log!("net-shell: tx buffer dropped {} bytes at cap={}\n", dropped, MAX_TX);
    }
}

pub(crate) fn net_shell_direct_reset_terminal() {
    net_shell_write_bytes(NET_SHELL_DIRECT_TERMINAL_RESET);
}

fn claim_net_shell_terminal(owner: TerminalHandoffOwner) -> bool {
    let owner = owner.raw();
    let previous = NET_SHELL_DIRECT_OWNER
        .compare_exchange(0, owner, Ordering::AcqRel, Ordering::Acquire)
        .unwrap_or_else(|current| current);
    if previous != 0 && previous != owner {
        return false;
    }

    let mut st = NET_SHELL_STATE.lock();
    st.rx.clear();
    st.tx.clear();
    reset_frontend_replay(&mut st);
    NET_TCP_LAST_WAS_CR.store(false, Ordering::Release);
    NET_SHELL_DIRECT_RX_LAST_WAS_CR.store(false, Ordering::Release);
    NET_SHELL_DIRECT_OWNER.store(owner, Ordering::Release);
    drop(st);
    net_shell_direct_reset_terminal();
    true
}

fn release_net_shell_terminal(owner: TerminalHandoffOwner) {
    if NET_SHELL_DIRECT_OWNER
        .compare_exchange(owner.raw(), 0, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        {
            let mut st = NET_SHELL_STATE.lock();
            st.rx.clear();
            st.tx.clear();
            reset_frontend_replay(&mut st);
        }
        NET_TCP_LAST_WAS_CR.store(false, Ordering::Release);
        NET_SHELL_DIRECT_RX_LAST_WAS_CR.store(false, Ordering::Release);
        net_shell_direct_reset_terminal();
        crate::shell2::repaint_backend_screen(&NET_TCP_SHELL_BACKEND);
    }
}

pub(crate) fn net_shell_direct_active() -> bool {
    NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) != 0
}

pub(crate) fn net_shell_direct_passthrough_active() -> bool {
    (NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) & TerminalHandoffOwner::STREAM_KIND) != 0
}

fn net_shell_terminal_owned_by(owner: TerminalHandoffOwner) -> bool {
    NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire) == owner.raw()
}

fn net_shell_terminal_read(owner: TerminalHandoffOwner, out: &mut [u8]) -> usize {
    if out.is_empty() || !net_shell_terminal_owned_by(owner) {
        return 0;
    }
    let mut st = NET_SHELL_STATE.lock();
    let read = out.len().min(st.rx.len());
    for byte in &mut out[..read] {
        *byte = st.rx.pop_front().unwrap_or_default();
    }
    read
}

fn net_shell_terminal_write(owner: TerminalHandoffOwner, bytes: &[u8]) -> bool {
    if !net_shell_terminal_owned_by(owner) {
        return false;
    }
    net_shell_write_bytes(bytes);
    true
}

pub(crate) fn claim_net_shell_direct(vm_id: u8) -> bool {
    claim_net_shell_terminal(TerminalHandoffOwner::blueprint(vm_id))
}

pub(crate) fn release_net_shell_direct(vm_id: u8) {
    release_net_shell_terminal(TerminalHandoffOwner::blueprint(vm_id));
}

pub(crate) fn net_shell_direct_owned_by(vm_id: u8) -> bool {
    net_shell_terminal_owned_by(TerminalHandoffOwner::blueprint(vm_id))
}

pub(crate) fn net_shell_direct_inject_input(vm_id: u8, bytes: &[u8]) -> bool {
    if !net_shell_direct_owned_by(vm_id) {
        return false;
    }
    NET_SHELL_STATE.lock().rx.extend(bytes.iter().copied());
    true
}

pub(crate) fn net_shell_direct_read(vm_id: u8, out: &mut [u8]) -> usize {
    if out.is_empty() || !net_shell_direct_owned_by(vm_id) {
        return 0;
    }
    let mut st = NET_SHELL_STATE.lock();
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
    NET_SHELL_DIRECT_RX_LAST_WAS_CR.store(last_was_cr, Ordering::Release);
    read
}

pub(crate) fn net_shell_direct_readable_len(vm_id: u8) -> usize {
    if !net_shell_direct_owned_by(vm_id) {
        return 0;
    }
    NET_SHELL_STATE.lock().rx.len()
}

impl ShellIo2 for NetTcpShellBackend {
    #[inline]
    fn raw_write_str(&self, s: &str) {
        crate::shell2::crlf::write_bytes_crlf(s.as_bytes(), &NET_TCP_LAST_WAS_CR, |chunk| {
            net_shell_write_bytes(chunk);
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
                        net_shell_write_bytes(chunk);
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
            net_shell_write_bytes(chunk);
        });
    }

    #[inline]
    fn raw_write_byte(&self, b: u8) {
        crate::shell2::crlf::write_bytes_crlf(&[b], &NET_TCP_LAST_WAS_CR, |chunk| {
            net_shell_write_bytes(chunk);
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

    fn release_terminal_handoff(&self, owner: TerminalHandoffOwner) {
        release_net_shell_terminal(owner);
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
