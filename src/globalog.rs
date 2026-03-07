use core::fmt;

extern crate alloc;

#[macro_export]
macro_rules! log {
    ($($tt:tt)*) => {{
        $crate::globalog::log(format_args!($($tt)*));
    }};
}

pub fn log(args: fmt::Arguments<'_>) {
    crate::usb::truekey::push_fmt(args);
    debugcon::log(args);
    //let _ = crate::vga::log(args);
    placeholder::log(args);
}

pub fn bringup_log_tail(max_bytes: usize) -> alloc::vec::Vec<u8> {
    placeholder::snapshot_tail(max_bytes)
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

mod placeholder {
    use core::fmt;
    use heapless::Deque;
    use spin::Mutex;

    const BRINGUP_LOG_BYTES: usize = 256 * 1024;
    static BRINGUP_LOG: Mutex<Deque<u8, BRINGUP_LOG_BYTES>> = Mutex::new(Deque::new());

    pub(super) fn snapshot_tail(max_bytes: usize) -> alloc::vec::Vec<u8> {
        let q = BRINGUP_LOG.lock();
        if q.is_empty() || max_bytes == 0 {
            return alloc::vec::Vec::new();
        }

        let keep = core::cmp::min(max_bytes, q.len());
        let skip = q.len().saturating_sub(keep);
        let mut out = alloc::vec::Vec::with_capacity(keep);
        for (i, b) in q.iter().enumerate() {
            if i >= skip {
                out.push(*b);
            }
        }
        out
    }

    pub(super) fn log(args: fmt::Arguments<'_>) {
        struct Writer;

        impl fmt::Write for Writer {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                let mut q = BRINGUP_LOG.lock();
                for &b in s.as_bytes() {
                    if q.push_back(b).is_err() {
                        let _ = q.pop_front();
                        let _ = q.push_back(b);
                    }
                }
                Ok(())
            }
        }

        let _ = fmt::write(&mut Writer, args);
    }
}
