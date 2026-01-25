use crate::shell::{ShellBackend, ShellIo};

pub(crate) struct UsbCdcShellBackend;

pub(crate) static USB_CDC_SHELL_BACKEND: UsbCdcShellBackend = UsbCdcShellBackend;

impl ShellIo for UsbCdcShellBackend {
    #[inline]
    fn write_str(&self, s: &str) {
        let _ = crate::usb::cdc_shell::write(s.as_bytes());
    }

    #[inline]
    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        use core::fmt::Write;

        struct Writer;

        impl Write for Writer {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                let _ = crate::usb::cdc_shell::write(s.as_bytes());
                Ok(())
            }
        }

        let _ = Writer.write_fmt(args);
    }

    #[inline]
    fn write_char(&self, ch: char) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        let _ = crate::usb::cdc_shell::write(s.as_bytes());
    }

    #[inline]
    fn write_byte(&self, b: u8) {
        let _ = crate::usb::cdc_shell::write(&[b]);
    }
}

impl ShellBackend for UsbCdcShellBackend {
    #[inline]
    fn read_byte(&self) -> Option<u8> {
        crate::usb::cdc_shell::read_byte()
    }
}
