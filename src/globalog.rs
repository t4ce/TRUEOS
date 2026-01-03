use core::fmt;

#[macro_export]
macro_rules! debugconf {
    ($($tt:tt)*) => {{
        $crate::globalog::log(format_args!($($tt)*));
    }};
}

/// Global log fan-out.
///
/// Order:
/// A) QEMU debugcon (0xE9)
/// B) VGA
/// C) UART0 (COM1)
/// D) Placeholder
pub fn log(args: fmt::Arguments<'_>) {
    cache::log(args);
    debugcon::log(args);
    let _ = crate::vga::log_fmt(args);
    uart0::log(args);
    placeholder::log(args);
}

mod cache {
    use core::fmt;

    const ROWS: usize = 512;
    const COLS: usize = 1024;
    static mut BUF: [[u8; COLS]; ROWS] = [[0; COLS]; ROWS];
    static mut ROW: usize = 0;
    static mut COL: usize = 0;

    struct Writer;
    impl fmt::Write for Writer {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            unsafe {
                for &b in s.as_bytes() {
                    if b == b'\n' {
                        ROW = (ROW + 1) % ROWS;
                        COL = 0;
                        continue;
                    }
                    if COL < COLS {
                        BUF[ROW][COL] = b;
                        COL += 1;
                    }
                }
            }
            Ok(())
        }
    }

    #[inline(always)]
    pub(super) fn log(args: fmt::Arguments<'_>) {
        let _ = fmt::write(&mut Writer, args);
    }
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
                let _ = crate::serial::COM1_BACKEND.try_write_byte(b);
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