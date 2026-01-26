use crate::shell::{ShellBackend, ShellIo};
use core::sync::atomic::AtomicBool;

pub(crate) struct UsbCdcShellBackend;

pub(crate) static USB_CDC_SHELL_BACKEND: UsbCdcShellBackend = UsbCdcShellBackend;

static USB_CDC_LAST_WAS_CR: AtomicBool = AtomicBool::new(false);

impl ShellIo for UsbCdcShellBackend {
    #[inline]
    fn write_str(&self, s: &str) {
        crate::shell::crlf::write_bytes_crlf(s.as_bytes(), &USB_CDC_LAST_WAS_CR, |chunk| {
            let _ = crate::usb::cdc_shell::write(chunk);
        });
    }

    #[inline]
    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        use core::fmt::Write;

        struct Writer;

        impl Write for Writer {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                crate::shell::crlf::write_bytes_crlf(
                    s.as_bytes(),
                    &USB_CDC_LAST_WAS_CR,
                    |chunk| {
                        let _ = crate::usb::cdc_shell::write(chunk);
                    },
                );
                Ok(())
            }
        }

        let _ = Writer.write_fmt(args);
    }

    #[inline]
    fn write_char(&self, ch: char) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        crate::shell::crlf::write_bytes_crlf(s.as_bytes(), &USB_CDC_LAST_WAS_CR, |chunk| {
            let _ = crate::usb::cdc_shell::write(chunk);
        });
    }

    #[inline]
    fn write_byte(&self, b: u8) {
        crate::shell::crlf::write_bytes_crlf(&[b], &USB_CDC_LAST_WAS_CR, |chunk| {
            let _ = crate::usb::cdc_shell::write(chunk);
        });
    }
}

impl ShellBackend for UsbCdcShellBackend {
    #[inline]
    fn read_byte(&self) -> Option<u8> {
        crate::usb::cdc_shell::read_byte()
    }
}
