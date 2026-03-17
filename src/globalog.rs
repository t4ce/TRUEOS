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
    let _ = crate::vga::log(args);
    placeholder::log(args);
}

#[inline(always)]
pub(crate) fn debugcon_write_byte_raw(b: u8) {
    debugcon::write_byte_raw(b)
}

#[embassy_executor::task]
pub async fn persist_once_task() {
    use embassy_time::{Duration as EmbassyDuration, Timer};

    const RETRY_MS: u64 = 1_000;

    Timer::after(EmbassyDuration::from_secs(10)).await;

    let snapshot = placeholder::snapshot();
    if snapshot.is_empty() {
        return;
    }

    loop {
        let Some(disk) = crate::v::fs::trueosfs::primary_root_handle() else {
            Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
            continue;
        };

        if let Err(e) = persist_snapshot(disk, snapshot.as_slice()).await {
            crate::log!("globalog: persist failed: {:?}\n", e);
            return;
        }

        crate::log!("[globalog] the log has been written\n");
        return;
    }
}

async fn persist_snapshot(
    disk: crate::disc::block::DeviceHandle,
    bytes: &[u8],
) -> Result<(), crate::disc::block::Error> {
    const PATH: &str = "logs/globalog.txt";
    const CHUNK_BYTES: usize = 64 * 1024;

    let Some(handle) =
        crate::v::fs::trueosfs::file_write_begin_async(disk, PATH, bytes.len() as u64).await?
    else {
        return Err(crate::disc::block::Error::Io);
    };

    for chunk in bytes.chunks(CHUNK_BYTES) {
        if let Err(e) = crate::v::fs::trueosfs::file_write_chunk_async(handle, chunk).await {
            let _ = crate::v::fs::trueosfs::file_write_abort_async(handle).await;
            return Err(e);
        }
    }

    crate::v::fs::trueosfs::file_write_finish_async(handle).await
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
    use alloc::vec::Vec;
    use core::{cmp::min, fmt};
    use spin::Mutex;

    const BRINGUP_LOG_BYTES: usize = 2 * 1024 * 1024;

    struct BringupLog {
        buf: [u8; BRINGUP_LOG_BYTES],
        head: usize,
        len: usize,
    }

    impl BringupLog {
        const fn new() -> Self {
            Self {
                buf: [0; BRINGUP_LOG_BYTES],
                head: 0,
                len: 0,
            }
        }

        #[inline]
        fn write_bytes(&mut self, bytes: &[u8]) {
            if bytes.is_empty() {
                return;
            }

            if bytes.len() >= BRINGUP_LOG_BYTES {
                let keep = &bytes[bytes.len() - BRINGUP_LOG_BYTES..];
                self.buf.copy_from_slice(keep);
                self.head = 0;
                self.len = BRINGUP_LOG_BYTES;
                return;
            }

            let first = min(bytes.len(), BRINGUP_LOG_BYTES - self.head);
            self.buf[self.head..self.head + first].copy_from_slice(&bytes[..first]);

            let rest = bytes.len() - first;
            if rest != 0 {
                self.buf[..rest].copy_from_slice(&bytes[first..]);
            }

            self.head = (self.head + bytes.len()) % BRINGUP_LOG_BYTES;
            self.len = min(self.len + bytes.len(), BRINGUP_LOG_BYTES);
        }

        #[inline]
        fn oldest_index(&self) -> usize {
            (self.head + BRINGUP_LOG_BYTES - self.len) % BRINGUP_LOG_BYTES
        }

        fn snapshot(&self) -> Vec<u8> {
            if self.len == 0 {
                return Vec::new();
            }

            let start = self.oldest_index();
            let first = min(self.len, BRINGUP_LOG_BYTES - start);
            let mut out = Vec::with_capacity(self.len);
            out.extend_from_slice(&self.buf[start..start + first]);
            if self.len > first {
                out.extend_from_slice(&self.buf[..self.len - first]);
            }
            out
        }
    }

    static BRINGUP_LOG: Mutex<BringupLog> = Mutex::new(BringupLog::new());

    pub(super) fn log(args: fmt::Arguments<'_>) {
        struct Writer;

        impl fmt::Write for Writer {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                BRINGUP_LOG.lock().write_bytes(s.as_bytes());
                Ok(())
            }
        }

        let _ = fmt::write(&mut Writer, args);
    }

    pub(super) fn snapshot() -> Vec<u8> {
        BRINGUP_LOG.lock().snapshot()
    }
}
