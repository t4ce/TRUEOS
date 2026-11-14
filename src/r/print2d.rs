//! Bounded A4 print-job admission for native app producers.
//!
//! Apps own page layout and PWG encoding. This queue admits a complete document
//! before the existing printer worker can send it; partial uploads never print.

extern crate alloc;

use alloc::{collections::VecDeque, string::String, vec::Vec};
use spin::Mutex;

const PWG_BEGIN: u32 = 3;
const PWG_APPEND: u32 = 4;
const PWG_COMMIT: u32 = 5;
const PWG_ABORT: u32 = 6;
const QUEUE_CAPACITY: usize = 4;
const STATUS_CAPACITY: usize = 64;
const UPLOAD_IDLE_TIMEOUT_MS: u64 = 60_000;

pub const ERROR_INVALID_DOCUMENT: i64 = -1;
pub const ERROR_QUEUE_FULL: i64 = -2;
pub const ERROR_NOT_OWNER: i64 = -3;
pub const ERROR_UNKNOWN_JOB: i32 = -4;
pub const ERROR_TRANSPORT: i64 = -5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum PrintJobState {
    Queued = 1,
    WaitingForPrinter = 2,
    Rendering = 3,
    Connecting = 4,
    Sending = 5,
    Submitted = 6,
    Printing = 7,
    Completed = 8,
    Failed = 9,
    Canceled = 10,
    OutcomeUnknown = 11,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrintOwner {
    Blueprint(u8),
}

impl PrintJobState {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::WaitingForPrinter => "waiting-for-printer",
            Self::Rendering => "rendering",
            Self::Connecting => "connecting",
            Self::Sending => "sending",
            Self::Submitted => "submitted",
            Self::Printing => "printing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::OutcomeUnknown => "outcome-unknown",
        }
    }

    pub const fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Canceled | Self::OutcomeUnknown
        )
    }
}

pub(crate) struct PrintJob {
    pub id: u32,
    pub owner: PrintOwner,
    pub document: Vec<u8>,
    pub printer_uri: Option<String>,
}

#[derive(Clone, Copy)]
struct JobRecord {
    id: u32,
    owner: PrintOwner,
    state: PrintJobState,
}
struct Upload {
    id: u32,
    expected: usize,
    bytes: Vec<u8>,
    touched_ms: u64,
}
struct PrintQueue {
    next_id: u32,
    pending: VecDeque<PrintJob>,
    uploads: Vec<Upload>,
    records: VecDeque<JobRecord>,
}

impl PrintQueue {
    const fn new() -> Self {
        Self {
            next_id: 1,
            pending: VecDeque::new(),
            uploads: Vec::new(),
            records: VecDeque::new(),
        }
    }

    fn owned_record(&self, owner: u8, id: u32) -> Result<usize, i64> {
        let index = self
            .records
            .iter()
            .position(|record| record.id == id)
            .ok_or(i64::from(ERROR_UNKNOWN_JOB))?;
        if self.records[index].owner != PrintOwner::Blueprint(owner) {
            return Err(ERROR_NOT_OWNER);
        }
        Ok(index)
    }

    fn expire_uploads(&mut self, now_ms: u64) {
        let records = &mut self.records;
        self.uploads.retain(|upload| {
            if now_ms.saturating_sub(upload.touched_ms) < UPLOAD_IDLE_TIMEOUT_MS {
                return true;
            }
            if let Some(record) = records.iter_mut().find(|record| record.id == upload.id) {
                record.state = PrintJobState::Canceled;
            }
            false
        });
    }

    fn begin(&mut self, owner: u8, expected: usize, now_ms: u64) -> Result<u32, i64> {
        self.expire_uploads(now_ms);
        if expected < super::net::pwg_raster::MIN_DOCUMENT_BYTES
            || expected > super::net::pwg_raster::MAX_DOCUMENT_BYTES
        {
            return Err(ERROR_INVALID_DOCUMENT);
        }
        if self
            .records
            .iter()
            .filter(|record| !record.state.terminal())
            .count()
            >= QUEUE_CAPACITY
        {
            return Err(ERROR_QUEUE_FULL);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(expected)
            .map_err(|_| ERROR_QUEUE_FULL)?;
        while self.records.len() >= STATUS_CAPACITY {
            let index = self
                .records
                .iter()
                .position(|record| record.state.terminal())
                .ok_or(ERROR_QUEUE_FULL)?;
            self.records.remove(index);
        }
        let id = loop {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1).max(1);
            if id != 0 && !self.records.iter().any(|record| record.id == id) {
                break id;
            }
        };
        self.records.push_back(JobRecord {
            id,
            owner: PrintOwner::Blueprint(owner),
            state: PrintJobState::Rendering,
        });
        self.uploads.push(Upload {
            id,
            expected,
            bytes,
            touched_ms: now_ms,
        });
        Ok(id)
    }

    fn append(
        &mut self,
        owner: u8,
        id: u32,
        offset: usize,
        bytes: &[u8],
        now_ms: u64,
    ) -> Result<u32, i64> {
        self.expire_uploads(now_ms);
        self.owned_record(owner, id)?;
        let upload = self
            .uploads
            .iter_mut()
            .find(|upload| upload.id == id)
            .ok_or(ERROR_INVALID_DOCUMENT)?;
        if bytes.is_empty()
            || offset != upload.bytes.len()
            || bytes.len() > upload.expected.saturating_sub(upload.bytes.len())
        {
            return Err(ERROR_INVALID_DOCUMENT);
        }
        upload.bytes.extend_from_slice(bytes);
        upload.touched_ms = now_ms;
        Ok(id)
    }

    fn commit(&mut self, owner: u8, id: u32, now_ms: u64) -> Result<u32, i64> {
        self.expire_uploads(now_ms);
        let record = self.owned_record(owner, id)?;
        let upload = self
            .uploads
            .iter()
            .position(|upload| upload.id == id)
            .ok_or(ERROR_INVALID_DOCUMENT)?;
        if self.uploads[upload].bytes.len() != self.uploads[upload].expected {
            return Err(ERROR_INVALID_DOCUMENT);
        }
        let document = self.uploads.remove(upload).bytes;
        if super::net::pwg_raster::validate_a4(&document).is_err() {
            self.records[record].state = PrintJobState::Failed;
            return Err(ERROR_INVALID_DOCUMENT);
        }
        self.records[record].state = PrintJobState::Queued;
        self.pending.push_back(PrintJob {
            id,
            owner: PrintOwner::Blueprint(owner),
            document,
            printer_uri: None,
        });
        Ok(id)
    }

    fn abort(&mut self, owner: u8, id: u32) -> Result<u32, i64> {
        let record = self.owned_record(owner, id)?;
        if self.records[record].state == PrintJobState::Canceled {
            return Ok(id);
        }
        let index = self
            .uploads
            .iter()
            .position(|upload| upload.id == id)
            .ok_or(ERROR_INVALID_DOCUMENT)?;
        self.uploads.remove(index);
        self.records[record].state = PrintJobState::Canceled;
        Ok(id)
    }
}

static PRINT_QUEUE: Mutex<PrintQueue> = Mutex::new(PrintQueue::new());

pub(crate) fn take_next_job() -> Option<PrintJob> {
    let mut queue = PRINT_QUEUE.lock();
    queue.expire_uploads(trueos_time::Instant::now().as_millis());
    queue.pending.pop_front()
}

pub(crate) fn transition(job_id: u32, state: PrintJobState, detail: &'static str) {
    let changed = {
        let mut queue = PRINT_QUEUE.lock();
        let Some(record) = queue.records.iter_mut().find(|record| record.id == job_id) else {
            return;
        };
        // A late network response cannot revive a terminal job or trigger a resend.
        if record.state == state || record.state.terminal() {
            false
        } else {
            record.state = state;
            true
        }
    };
    if changed {
        crate::log_os::print2d_job_state(job_id, state.name(), detail);
    }
}

pub(crate) fn status_for_owner(owner: u8, job_id: u32) -> i32 {
    let mut queue = PRINT_QUEUE.lock();
    queue.expire_uploads(trueos_time::Instant::now().as_millis());
    match queue.owned_record(owner, job_id) {
        Ok(index) => queue.records[index].state as i32,
        Err(error) => error as i32,
    }
}

pub(crate) fn submit_for_owner(owner: u8, document_kind: u32, subject: u64, raw: &[u8]) -> i64 {
    let now_ms = trueos_time::Instant::now().as_millis();
    let result = {
        let mut queue = PRINT_QUEUE.lock();
        match document_kind {
            PWG_BEGIN if subject == 0 && raw.len() == 8 && raw[..4] == 1u32.to_le_bytes() => {
                let expected = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
                queue.begin(owner, expected, now_ms)
            }
            PWG_APPEND if subject != 0 && subject <= u64::from(u32::MAX) && raw.len() > 4 => {
                let offset = u32::from_le_bytes(raw[..4].try_into().unwrap()) as usize;
                queue.append(owner, subject as u32, offset, &raw[4..], now_ms)
            }
            PWG_COMMIT if subject != 0 && subject <= u64::from(u32::MAX) && raw.is_empty() => {
                queue.commit(owner, subject as u32, now_ms)
            }
            PWG_ABORT if subject != 0 && subject <= u64::from(u32::MAX) && raw.is_empty() => {
                queue.abort(owner, subject as u32)
            }
            _ => Err(ERROR_INVALID_DOCUMENT),
        }
    };
    if document_kind == PWG_COMMIT
        && let Ok(id) = result
    {
        crate::log_os::print2d_job_state(id, PrintJobState::Queued.name(), "accepted-a4-document");
    }
    result.map_or_else(|error| error, i64::from)
}

#[cfg(test)]
#[path = "print2d_tests.rs"]
mod tests;
