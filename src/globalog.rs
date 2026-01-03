use core::fmt;

#[macro_export]
macro_rules! log {
    ($($tt:tt)*) => {{
        $crate::globalog::log(format_args!($($tt)*));
    }};
}

pub fn log(args: fmt::Arguments<'_>) {
    crate::usb::truekey::push_fmt(args);
    debugcon::log(args);
    let _ = crate::vga::log_fmt(args);
    uart0::log(args);
    placeholder::log(args);
}

#[inline(always)]
pub(crate) fn debugcon_write_byte_raw(b: u8) {
    debugcon::write_byte_raw(b)
}

mod debugcon {
    use core::fmt;

    #[inline(always)]
    pub(super) fn write_byte_raw(b: u8) {
        unsafe { crate::portio::outb(0xE9, b) };
    }

    struct Writer;

    impl fmt::Write for Writer {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            for &b in s.as_bytes() {
                write_byte_raw(b);
            }
            Ok(())
        }
    }

    pub(super) fn log(args: fmt::Arguments<'_>) {
        let _ = fmt::write(&mut Writer, args);
    }
}

mod uart0 {
    use core::fmt;

    struct Writer;

    impl fmt::Write for Writer {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            for &b in s.as_bytes() {
                // let _ = crate::serial::COM1_BACKEND.try_write_byte(b); its because.. what is UART0?
            }
            Ok(())
        }
    }

    pub(super) fn log(args: fmt::Arguments<'_>) {
        let _ = fmt::write(&mut Writer, args);
    }
}

mod placeholder {
    use core::fmt;

    pub(super) fn log(_args: fmt::Arguments<'_>) {}
}