use core::fmt::{self, Write};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use heapless::Vec as HVec;
use log::{LevelFilter, Log, Metadata, Record};
use spin::{Mutex, RwLock};

const DEFAULT_BAUD: u32 = 921_600;
const MAX_SERIAL_MIRRORS: usize = 4;
const PREHEAP_BOOTLOG_CAP: usize = 1024 * 1024;
static DESIRED_BAUD: AtomicU32 = AtomicU32::new(DEFAULT_BAUD);
static BOOTLOG_ENABLED: AtomicBool = AtomicBool::new(true);
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

    fn flush_to(&mut self, backend: &'static dyn SerialBackend) {
        if self.flushed {
            return;
        }

        // Once we start flushing, stop recording new bytes into the bootlog.
        // New logs will still be routed directly to active backends.
        BOOTLOG_ENABLED.store(false, Ordering::Release);

        if self.len == 0 || self.cap == 0 {
            self.flushed = true;
            return;
        }

        let mut idx = if self.head >= self.len {
            self.head - self.len
        } else {
            self.cap - (self.len - self.head)
        };

        let mut consumed = 0usize;
        for _ in 0..self.len {
            if !backend.try_write_byte(self.preheap[idx]) {
                break;
            }
            consumed += 1;
            idx += 1;
            if idx >= self.cap {
                idx = 0;
            }
        }

        // Keep remaining bytes for a later retry if the backend applied backpressure.
        self.len = self.len.saturating_sub(consumed);
        if self.len == 0 {
            self.flushed = true;
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

#[derive(Copy, Clone, Debug)]
pub enum BackendRole {
    Preferred,
    Mirror,
}

#[derive(Debug)]
pub enum BackendError {
    TableFull,
}

struct RoutingTable {
    primary: &'static dyn SerialBackend,
    mirrors: HVec<&'static dyn SerialBackend, MAX_SERIAL_MIRRORS>,
}

impl RoutingTable {
    const fn new() -> Self {
        Self {
            primary: &crate::serial::COM1_BACKEND,
            mirrors: HVec::new(),
        }
    }

    fn contains(&self, backend: &'static dyn SerialBackend) -> bool {
        ptr::eq(self.primary, backend)
            || self
                .mirrors
                .iter()
                .any(|registered| ptr::eq(*registered, backend))
    }

    fn add_mirror(&mut self, backend: &'static dyn SerialBackend) -> Result<(), BackendError> {
        if self.mirrors.iter().any(|b| ptr::eq(*b, backend)) {
            return Ok(());
        }
        self.mirrors
            .push(backend)
            .map_err(|_| BackendError::TableFull)
    }

    fn drop_mirror(&mut self, backend: &'static dyn SerialBackend) -> bool {
        let mut idx = 0usize;
        while idx < self.mirrors.len() {
            if ptr::eq(self.mirrors[idx], backend) {
                let _ = self.mirrors.remove(idx);
                return true;
            }
            idx += 1;
        }
        false
    }

    fn promote(&mut self, backend: &'static dyn SerialBackend) -> Result<(), BackendError> {
        if ptr::eq(self.primary, backend) {
            return Ok(());
        }
        let previous = self.primary;
        let _ = self.drop_mirror(backend);
        self.primary = backend;
        if !ptr::eq(previous, backend) {
            self.add_mirror(previous)?;
        }
        Ok(())
    }

    fn remove(&mut self, backend: &'static dyn SerialBackend) -> bool {
        if ptr::eq(self.primary, backend) {
            self.primary = &crate::serial::COM1_BACKEND;
            return true;
        }
        self.drop_mirror(backend)
    }
}

static ROUTING: RwLock<RoutingTable> = RwLock::new(RoutingTable::new());

#[inline(always)]
pub(crate) fn try_write_byte(b: u8) -> bool {
    // Lock ordering: ROUTING -> BOOTLOG (promotion paths take ROUTING write then BOOTLOG).
    // This avoids ROUTING/BOOTLOG lock inversion under SMP.
    let guard = ROUTING.read();
    if BOOTLOG_ENABLED.load(Ordering::Acquire) {
        BOOTLOG.lock().record(b);
    }
    let mut ok = guard.primary.try_write_byte(b);
    for backend in guard.mirrors.iter() {
        ok |= backend.try_write_byte(b);
    }
    ok
}

#[inline(always)]
pub(crate) fn write_str(s: &str) {
    for &b in s.as_bytes() {
        let _ = try_write_byte(b);
    }
}

#[inline(always)]
pub(crate) fn try_read_byte() -> Option<u8> {
    let guard = ROUTING.read();
    if let Some(byte) = guard.primary.try_read_byte() {
        return Some(byte);
    }
    for backend in guard.mirrors.iter() {
        if let Some(byte) = backend.try_read_byte() {
            return Some(byte);
        }
    }
    None
}

pub(crate) fn register_backend(
    backend: &'static dyn SerialBackend,
    role: BackendRole,
) -> Result<(), BackendError> {
    let mut guard = ROUTING.write();
    if guard.contains(backend) {
        if matches!(role, BackendRole::Preferred) {
            guard.promote(backend)?;
        }
        return Ok(());
    }
    let result = match role {
        BackendRole::Preferred => guard.promote(backend),
        BackendRole::Mirror => guard.add_mirror(backend),
    };
    if result.is_ok() {
        let baud = desired_baud();
        let _ = backend.apply_baud(baud);
    }
    result
}

pub(crate) fn unregister_backend(backend: &'static dyn SerialBackend) -> bool {
    ROUTING.write().remove(backend)
}

pub(crate) fn promote_backend(backend: &'static dyn SerialBackend) -> Result<(), BackendError> {
    let mut guard = ROUTING.write();
    if !guard.contains(backend) {
        guard.add_mirror(backend)?;
    }
    guard.promote(backend)?;

    // Flush buffered boot logs while holding the write lock so they appear
    // before any new logs routed to the promoted backend.
    BOOTLOG.lock().flush_to(backend);
    Ok(())
}

pub(crate) fn promote_backend_exclusive(
    backend: &'static dyn SerialBackend,
) -> Result<(), BackendError> {
    let mut guard = ROUTING.write();
    guard.drop_mirror(backend);
    guard.primary = backend;
    guard.mirrors.clear();

    let baud = desired_baud();
    let _ = backend.apply_baud(baud);

    // Flush buffered boot logs while holding the write lock so they appear
    // before any new logs routed to the promoted backend.
    BOOTLOG.lock().flush_to(backend);
    Ok(())
}

/// Attempt to continue draining any buffered bootlog bytes to the current primary.
///
/// This is non-blocking: it stops when the backend applies backpressure.
pub(crate) fn poll_bootlog_flush() {
    // Lock ordering: ROUTING -> BOOTLOG (same as `try_write_byte`).
    let guard = ROUTING.read();
    if BOOTLOG_ENABLED.load(Ordering::Acquire) {
        return;
    }
    BOOTLOG.lock().flush_to(guard.primary);
}

pub(crate) fn desired_baud() -> u32 {
    DESIRED_BAUD.load(Ordering::Relaxed)
}

pub(crate) fn set_desired_baud(baud: u32) {
    let clamped = baud.max(1);
    DESIRED_BAUD.store(clamped, Ordering::Relaxed);
    let guard = ROUTING.read();
    let _ = guard.primary.apply_baud(clamped);
    for backend in guard.mirrors.iter() {
        let _ = backend.apply_baud(clamped);
    }
}

struct TruelogFmtWriter;

impl fmt::Write for TruelogFmtWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::truelog::write_str(s);
        Ok(())
    }
}

struct TrueLogger;

impl Log for TrueLogger {
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

static LOGGER: TrueLogger = TrueLogger;
const DEFAULT_LOG_LEVEL: LevelFilter = LevelFilter::Trace;

pub(crate) fn init_log_shim() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(DEFAULT_LOG_LEVEL);
    }
}
