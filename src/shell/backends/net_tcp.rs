use core::fmt::Write;

use crate::shell::{ShellBackend, ShellIo};

pub(crate) struct NetTcpShellBackend;

pub(crate) static NET_TCP_SHELL_BACKEND: NetTcpShellBackend = NetTcpShellBackend;

impl ShellIo for NetTcpShellBackend {
    #[inline]
    fn write_str(&self, s: &str) {
        crate::net::adapter::net_shell_write_bytes(s.as_bytes());
    }

    #[inline]
    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        struct Writer;

        impl Write for Writer {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                crate::net::adapter::net_shell_write_bytes(s.as_bytes());
                Ok(())
            }
        }

        let _ = Writer.write_fmt(args);
    }

    #[inline]
    fn write_char(&self, ch: char) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        crate::net::adapter::net_shell_write_bytes(s.as_bytes());
    }

    #[inline]
    fn write_byte(&self, b: u8) {
        crate::net::adapter::net_shell_write_byte(b);
    }
}

impl ShellBackend for NetTcpShellBackend {
    #[inline]
    fn read_byte(&self) -> Option<u8> {
        crate::net::adapter::net_shell_read_byte()
    }
}
