use crate::shell::uart1_com1;
use crate::shell::{ShellBackend, ShellIo};
use crate::shell2::{ShellBackend2, ShellIo2};
use core::sync::atomic::AtomicBool;

pub(crate) struct Uart1Com1Backend;

pub(crate) static UART1_COM1_BACKEND: Uart1Com1Backend = Uart1Com1Backend;

static UART_LAST_WAS_CR: AtomicBool = AtomicBool::new(false);

impl ShellIo for Uart1Com1Backend {
    #[inline]
    fn write_str(&self, s: &str) {
        crate::shell::crlf::write_bytes_crlf(s.as_bytes(), &UART_LAST_WAS_CR, |chunk| {
            uart1_com1::write_bytes(chunk);
        });
    }

    #[inline]
    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        use core::fmt::Write;

        struct Writer;

        impl Write for Writer {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                crate::shell::crlf::write_bytes_crlf(s.as_bytes(), &UART_LAST_WAS_CR, |chunk| {
                    uart1_com1::write_bytes(chunk);
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
        crate::shell::crlf::write_bytes_crlf(s.as_bytes(), &UART_LAST_WAS_CR, |chunk| {
            uart1_com1::write_bytes(chunk);
        });
    }

    #[inline]
    fn write_byte(&self, b: u8) {
        crate::shell::crlf::write_bytes_crlf(&[b], &UART_LAST_WAS_CR, |chunk| {
            uart1_com1::write_bytes(chunk);
        });
    }
}

impl ShellBackend for Uart1Com1Backend {
    #[inline]
    fn init(&self) {
        uart1_com1::init();
    }

    #[inline]
    fn read_byte(&self) -> Option<u8> {
        uart1_com1::read_byte()
    }
}

impl ShellIo2 for Uart1Com1Backend {
    #[inline]
    fn write_str(&self, s: &str) {
        <Self as ShellIo>::write_str(self, s);
    }

    #[inline]
    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        <Self as ShellIo>::write_fmt(self, args);
    }

    #[inline]
    fn write_char(&self, ch: char) {
        <Self as ShellIo>::write_char(self, ch);
    }

    #[inline]
    fn write_byte(&self, b: u8) {
        <Self as ShellIo>::write_byte(self, b);
    }
}

impl ShellBackend2 for Uart1Com1Backend {
    #[inline]
    fn init(&self) {
        <Self as ShellBackend>::init(self);
    }

    #[inline]
    fn read_byte(&self) -> Option<u8> {
        <Self as ShellBackend>::read_byte(self)
    }
}
