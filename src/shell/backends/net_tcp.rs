use core::fmt::Write;
use core::sync::atomic::AtomicBool;

use crate::shell::{ShellBackend, ShellIo};

pub(crate) struct NetTcpShellBackend;

pub(crate) static NET_TCP_SHELL_BACKEND: NetTcpShellBackend = NetTcpShellBackend;

static NET_TCP_LAST_WAS_CR: AtomicBool = AtomicBool::new(false);

impl ShellIo for NetTcpShellBackend {
    #[inline]
    fn write_str(&self, s: &str) {
        crate::shell::crlf::write_bytes_crlf(s.as_bytes(), &NET_TCP_LAST_WAS_CR, |chunk| {
            crate::v::net::net_shell_write_bytes(chunk);
        });
    }

    #[inline]
    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        struct Writer;

        impl Write for Writer {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                crate::shell::crlf::write_bytes_crlf(s.as_bytes(), &NET_TCP_LAST_WAS_CR, |chunk| {
                    crate::v::net::net_shell_write_bytes(chunk);
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
        crate::shell::crlf::write_bytes_crlf(s.as_bytes(), &NET_TCP_LAST_WAS_CR, |chunk| {
            crate::v::net::net_shell_write_bytes(chunk);
        });
    }

    #[inline]
    fn write_byte(&self, b: u8) {
        crate::shell::crlf::write_bytes_crlf(&[b], &NET_TCP_LAST_WAS_CR, |chunk| {
            crate::v::net::net_shell_write_bytes(chunk);
        });
    }
}

impl ShellBackend for NetTcpShellBackend {
    #[inline]
    fn read_byte(&self) -> Option<u8> {
        crate::v::net::net_shell_read_byte()
    }
}
