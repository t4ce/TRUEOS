use crate::shell::uart1_com1;
use crate::shell::{ShellBackend, ShellIo};

pub(crate) struct Uart1Com1Backend;

pub(crate) static UART1_COM1_BACKEND: Uart1Com1Backend = Uart1Com1Backend;

impl ShellIo for Uart1Com1Backend {
    #[inline]
    fn write_str(&self, s: &str) {
        uart1_com1::write_str(s);
    }

    #[inline]
    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        uart1_com1::write_fmt(args);
    }

    #[inline]
    fn write_char(&self, ch: char) {
        uart1_com1::write_char(ch);
    }

    #[inline]
    fn write_byte(&self, b: u8) {
        uart1_com1::write_byte(b);
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
