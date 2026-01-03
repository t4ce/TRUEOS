use core::fmt::{self, Write};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use heapless::Vec as HVec;
use log::{LevelFilter, Log, Metadata, Record};
use spin::{Mutex, RwLock};

#[inline(always)]
pub(crate) fn debugcon_write_byte_raw(b: u8) {
    unsafe { crate::portio::outb(0xE9, b) };
}

#[inline(always)]
fn try_write_byte(b: u8) -> bool {
    debugcon_write_byte_raw(b);
    true
}

#[inline(always)]
pub(crate) fn debugcon_write_str(s: &str) {
    for &b in s.as_bytes() {
        let _ = try_write_byte(b);
    }
}

pub(crate) struct DebugCon;

impl Write for DebugCon {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        debugcon_write_str(s);
        Ok(())
    }
}

#[macro_export]
macro_rules! debugconf {
    ($($tt:tt)*) => {{
        let _ = core::fmt::write(&mut $crate::truelog::DebugCon, format_args!($($tt)*));
        let _ = $crate::vga::log_fmt(format_args!($($tt)*));
    }};
}

const PREHEAP_BOOTLOG_CAP: usize = 1024 * 1024;
static BOOTLOG: Mutex<BootLog> = Mutex::new(BootLog::new());

struct BootLog {
    preheap: [u8; PREHEAP_BOOTLOG_CAP],
    cap: usize,
    head: usize,
    len: usize,
    flushed: bool,
}

impl BootLog {
    const fn new() -> Self {
        Self {
            preheap: [0u8; PREHEAP_BOOTLOG_CAP],
            cap: PREHEAP_BOOTLOG_CAP,
            head: 0,
            len: 0,
            flushed: false,
        }
    }
    fn record(&mut self, b: u8) {
        if self.flushed {
            return;
        }
        if self.cap == 0 {
            return;
        }
        self.preheap[self.head] = b;
        self.head += 1;
        if self.head >= self.cap {
            self.head = 0;
        }
        if self.len < self.cap {
            self.len += 1;
        }
    }
}

pub trait SerialBackend: Sync {
    fn name(&self) -> &'static str;
    fn try_write_byte(&self, byte: u8) -> bool;

    fn try_write(&self, bytes: &[u8]) -> usize {
        let mut written = 0usize;
        for &b in bytes {
            if self.try_write_byte(b) {
                written += 1;
            } else {
                break;
            }
        }
        written
    }

    fn try_read_byte(&self) -> Option<u8> {
        None
    }

    fn apply_baud(&self, _baud: u32) -> bool {
        false
    }
}

#[derive(Debug)]
pub enum BackendError {
    TableFull,
}

#[inline(always)]
pub(crate) fn write_str(s: &str) {
    for &b in s.as_bytes() {
        let _ = try_write_byte(b);
    }
}

struct TruelogFmtWriter;

impl fmt::Write for TruelogFmtWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::truelog::write_str(s);
        Ok(())
    }
}

struct TrueLog;

impl Log for TrueLog {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level().to_level_filter() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let mut writer = TruelogFmtWriter;
        if record.target().is_empty() {
            let _ = write!(&mut writer, "[{}] ", record.level());
        } else {
            let _ = write!(&mut writer, "[{}:{}] ", record.level(), record.target());
        }
        let _ = writer.write_fmt(*record.args());
        let _ = writer.write_str("\n");
    }

    fn flush(&self) {}
}

static LOG: TrueLog = TrueLog;

const DEFAULT_LOG_LEVEL: LevelFilter = LevelFilter::Trace;

pub(crate) fn init_log_shim() {
    if log::set_logger(&LOG).is_ok() {
        log::set_max_level(DEFAULT_LOG_LEVEL);
    }
}
