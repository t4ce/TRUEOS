//! Fixed launch phases on the shared monotonic clock. No guest contents,
//! filenames, arguments, SQL or per-allocation records enter this stream.
use core::{fmt::{self, Write}, sync::atomic::{AtomicU64, Ordering}};

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug)]
pub(crate) struct Trace {
    pub request: u64,
    pub vm: u8,
    pub generation: u64,
}

#[derive(Clone, Copy)]
pub(crate) enum Status { Ok, Error, Cancelled }

impl Trace {
    pub(crate) const DISABLED: Self = Self { request: 0, vm: u8::MAX, generation: 0 };

    pub(crate) fn for_archive(archive: &str) -> Self {
        let name = archive.rsplit('/').next().unwrap_or(archive);
        // SQLite is the bounded localhost dependency launched on a cold Codex
        // connection by the SQLx adapter; give its loader its own request ID.
        if !["codex", "codex.bp", "codex-check", "codex-check.bp", "sqlite", "sqlite.bp"].iter()
            .any(|candidate| name.eq_ignore_ascii_case(candidate)) { return Self::DISABLED; }
        Self { request: NEXT_REQUEST.fetch_add(1, Ordering::Relaxed), ..Self::DISABLED }
    }

    pub(crate) fn bind(self, vm: u8, generation: u64) -> Self { Self { vm, generation, ..self } }

    pub(crate) fn span(self, phase: &'static str) -> Span {
        Span { pending: self.begin(phase), finished: false }
    }

    /// Explicitly transported queue span. Dropping an unpolled queued task can
    /// leave its begin unmatched; replay reports it as pending, never zero ms.
    pub(crate) fn begin(self, phase: &'static str) -> Pending {
        let started = if self.request == 0 { 0 } else { now_ms() };
        let pending = Pending { trace: self, phase, started };
        pending.emit("begin", "pending", started);
        pending
    }

    pub(crate) fn measure<T, E>(self, phase: &'static str, run: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        let mut span = self.span(phase);
        let result = run();
        span.finish(if result.is_ok() { Status::Ok } else { Status::Error });
        result
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Pending {
    trace: Trace,
    phase: &'static str,
    started: u64,
}

impl Pending {
    pub(crate) const DISABLED: Self = Self { trace: Trace::DISABLED, phase: "disabled", started: 0 };

    pub(crate) fn finish(self, status: Status) {
        let status = match status { Status::Ok => "ok", Status::Error => "error", Status::Cancelled => "cancelled" };
        if self.trace.request != 0 { self.emit("end", status, now_ms()); }
    }

    fn emit(self, event: &str, status: &str, now: u64) {
        if self.trace.request == 0 { return; }
        // Stack storage avoids recursive allocator/logging work in the loader.
        let mut line = Line { bytes: [0; 256], len: 0 };
        if write!(line,
            "trueos-startup-v1 request={} vm={} gen={} phase={} event={} status={} mono_ms={} elapsed_ms={}",
            self.trace.request, self.trace.vm, self.trace.generation, self.phase,
            event, status, now, now.saturating_sub(self.started)).is_err() { return; }
        const TARGET: &[u8] = b"blueprint-loader";
        unsafe {
            // This existing boundary routes Hull records to the resident
            // logger rather than the Hull's private copy of logger globals.
            crate::r::io::cabi::trueos_cabi_log(trueos_vm::vmcall::BP_LOG_LEVEL_IMPORTANT,
                TARGET.as_ptr(), TARGET.len(), line.bytes.as_ptr(), line.len);
        }
    }
}

pub(crate) struct Span { pending: Pending, finished: bool }
impl Span {
    pub(crate) fn complete(&mut self) { self.finish(Status::Ok); }
    pub(crate) fn error(&mut self) { self.finish(Status::Error); }
    pub(crate) fn finish(&mut self, status: Status) {
        if !self.finished {
            self.finished = true;
            self.pending.finish(status);
        }
    }
}
impl Drop for Span {
    fn drop(&mut self) { self.finish(Status::Cancelled); }
}

fn now_ms() -> u64 { crate::r::platform::trueos_platform_monotonic_nanos() / 1_000_000 }

struct Line { bytes: [u8; 256], len: usize }
impl fmt::Write for Line {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let end = self.len.checked_add(value.len()).filter(|end| *end <= self.bytes.len()).ok_or(fmt::Error)?;
        self.bytes[self.len..end].copy_from_slice(value.as_bytes());
        self.len = end;
        Ok(())
    }
}
