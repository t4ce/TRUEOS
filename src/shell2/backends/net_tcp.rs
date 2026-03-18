use alloc::{boxed::Box, collections::VecDeque, format, vec::Vec};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::net::adapter::{
    NetCommand, NetEvent, NetHandle, NetQueue, SocketKind, register_app_queues,
};
use crate::shell2::{ShellBackend2, ShellIo2};

pub(crate) const NET_SHELL_TCP_PORT: u16 = 4245;

pub(crate) struct NetTcpShellBackend;

pub(crate) static NET_TCP_SHELL_BACKEND: NetTcpShellBackend = NetTcpShellBackend;

static NET_TCP_LAST_WAS_CR: AtomicBool = AtomicBool::new(false);
pub(crate) static NET_SHELL_STARTED: AtomicBool = AtomicBool::new(false);

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
    NET_SHELL_STATE.lock().rx.pop_front()
}

pub(crate) fn net_shell_write_bytes(bytes: &[u8]) {
    const MAX_TX: usize = 32 * 1024;
    let mut st = NET_SHELL_STATE.lock();
    for &b in bytes {
        if st.tx.len() >= MAX_TX {
            let _ = st.tx.pop_front();
        }
        st.tx.push_back(b);
    }
}

impl ShellIo2 for NetTcpShellBackend {
    #[inline]
    fn write_str(&self, s: &str) {
        crate::shell2::crlf::write_bytes_crlf(s.as_bytes(), &NET_TCP_LAST_WAS_CR, |chunk| {
            net_shell_write_bytes(chunk);
        });
    }

    #[inline]
    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        struct Writer;

        impl Write for Writer {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                crate::shell2::crlf::write_bytes_crlf(s.as_bytes(), &NET_TCP_LAST_WAS_CR, |chunk| {
                    net_shell_write_bytes(chunk);
                });
                Ok(())
            }
        }

        let _ = Writer.write_fmt(args);
    }

    #[inline]
    fn write_char(&self, ch: char) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        crate::shell2::crlf::write_bytes_crlf(s.as_bytes(), &NET_TCP_LAST_WAS_CR, |chunk| {
            net_shell_write_bytes(chunk);
        });
    }

    #[inline]
    fn write_byte(&self, b: u8) {
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
