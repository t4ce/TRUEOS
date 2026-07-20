use alloc::collections::VecDeque;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::net::adapter::NetHandle;
use crate::shell2::{ShellBackend2, ShellIo2};

pub(crate) use crate::r::net::ports::NET_SHELL_TCP_PORT;

pub(crate) struct NetTcpShellBackend;

pub(crate) static NET_TCP_SHELL_BACKEND: NetTcpShellBackend = NetTcpShellBackend;

static NET_TCP_LAST_WAS_CR: AtomicBool = AtomicBool::new(false);
pub(crate) static NET_SHELL_STARTED: AtomicBool = AtomicBool::new(false);
static NET_SHELL_DIRECT_VM: AtomicU8 = AtomicU8::new(0);
static NET_SHELL_DIRECT_RX_LAST_WAS_CR: AtomicBool = AtomicBool::new(false);
// Direct terminal apps may stop before their userspace guard flushes its
// cleanup, and release_net_shell_direct intentionally drops queued app paint.
// Restore every terminal mode that shell2 relies on before repainting it.
const NET_SHELL_DIRECT_TERMINAL_RESET: &[u8] = b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1004l\x1b[?1006l\x1b[?1015l\x1b[?2004l\x1b[?1049l\x1b[0m\x1b[39;49m\x1b[r\x1b[?25h";

pub(crate) struct NetShellState {
    pub(crate) handle: Option<NetHandle>,
    pub(crate) rx: VecDeque<u8>,
    pub(crate) tx: VecDeque<u8>,
}

pub(crate) static NET_SHELL_STATE: spin::Mutex<NetShellState> = spin::Mutex::new(NetShellState {
    handle: None,
    rx: VecDeque::new(),
    tx: VecDeque::new(),
});

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

pub(crate) fn claim_net_shell_direct(vm_id: u8) -> bool {
    let owner = vm_id.saturating_add(1);
    let previous = NET_SHELL_DIRECT_VM
        .compare_exchange(0, owner, Ordering::AcqRel, Ordering::Acquire)
        .unwrap_or_else(|current| current);
    if previous != 0 && previous != owner {
        return false;
    }

    let mut st = NET_SHELL_STATE.lock();
    st.rx.clear();
    st.tx.clear();
    NET_TCP_LAST_WAS_CR.store(false, Ordering::Release);
    NET_SHELL_DIRECT_RX_LAST_WAS_CR.store(false, Ordering::Release);
    NET_SHELL_DIRECT_VM.store(owner, Ordering::Release);
    drop(st);
    net_shell_direct_reset_terminal();
    true
}

pub(crate) fn release_net_shell_direct(vm_id: u8) {
    let owner = vm_id.saturating_add(1);
    if NET_SHELL_DIRECT_VM
        .compare_exchange(owner, 0, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        {
            let mut st = NET_SHELL_STATE.lock();
            st.rx.clear();
            st.tx.clear();
        }
        NET_TCP_LAST_WAS_CR.store(false, Ordering::Release);
        NET_SHELL_DIRECT_RX_LAST_WAS_CR.store(false, Ordering::Release);
        net_shell_direct_reset_terminal();
        crate::shell2::repaint_backend_screen(&NET_TCP_SHELL_BACKEND);
    }
}

pub(crate) fn net_shell_direct_active() -> bool {
    NET_SHELL_DIRECT_VM.load(Ordering::Acquire) != 0
}

pub(crate) fn net_shell_direct_owned_by(vm_id: u8) -> bool {
    NET_SHELL_DIRECT_VM.load(Ordering::Acquire) == vm_id.saturating_add(1)
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
}
