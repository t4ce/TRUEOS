//! Small producer-facing 2D print queue.
//!
//! Blueprint producers submit compact document state here.  The BSP printer
//! service is the sole consumer and owns rendering, transport, and remote job
//! monitoring.

extern crate alloc;

use alloc::{
    collections::VecDeque,
    string::{String, ToString},
    vec::Vec,
};
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

pub const DOCUMENT_GRIDPAPER_A4: u32 = 1;
pub const DOCUMENT_GRIDPAPER_REQUEST: u32 = 2;

pub const ERROR_INVALID_DOCUMENT: i64 = -1;
pub const ERROR_QUEUE_FULL: i64 = -2;
pub const ERROR_NOT_OWNER: i64 = -3;
pub const ERROR_UNKNOWN_JOB: i32 = -4;
pub const ERROR_TRANSPORT: i64 = -5;

const QUEUE_CAPACITY: usize = 8;
const STATUS_CAPACITY: usize = 64;

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
        matches!(self, Self::Completed | Self::Failed | Self::Canceled | Self::OutcomeUnknown)
    }
}

pub(crate) enum PrintDocument {
    GridPaperA4 {
        generation: u64,
        size: crate::r::gridpaper_service::GridSize,
        raw: Vec<u8>,
    },
}

pub(crate) struct PrintJob {
    pub id: u32,
    pub owner: u8,
    pub document: PrintDocument,
    pub printer_uri: Option<String>,
}

#[derive(Clone, Copy)]
struct JobRecord {
    id: u32,
    owner: u8,
    state: PrintJobState,
}

struct PrintQueue {
    pending: VecDeque<PrintJob>,
    records: VecDeque<JobRecord>,
}

impl PrintQueue {
    const fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            records: VecDeque::new(),
        }
    }

    fn retain_status_capacity(&mut self) {
        while self.records.len() >= STATUS_CAPACITY {
            let removable = self
                .records
                .iter()
                .position(|record| record.state.terminal())
                .unwrap_or(0);
            let _ = self.records.remove(removable);
        }
    }

    fn has_job_capacity(&self) -> bool {
        self.records
            .iter()
            .filter(|record| !record.state.terminal())
            .count()
            < QUEUE_CAPACITY
    }
}

static NEXT_JOB_ID: AtomicU32 = AtomicU32::new(1);
static PRINT_QUEUE: Mutex<PrintQueue> = Mutex::new(PrintQueue::new());

fn next_job_id() -> u32 {
    loop {
        let id = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
    }
}

fn enqueue(owner: u8, document: PrintDocument, printer_uri: Option<String>) -> Result<u32, i64> {
    let id = next_job_id();
    {
        let mut queue = PRINT_QUEUE.lock();
        if !queue.has_job_capacity() {
            return Err(ERROR_QUEUE_FULL);
        }
        queue.retain_status_capacity();
        queue.records.push_back(JobRecord {
            id,
            owner,
            state: PrintJobState::Queued,
        });
        queue.pending.push_back(PrintJob {
            id,
            owner,
            document,
            printer_uri,
        });
    }
    crate::log_os::print2d_job_state(id, PrintJobState::Queued.name(), "accepted");
    Ok(id)
}

fn enqueue_gridpaper_request(owner: u8, token: u32) -> Result<u32, i64> {
    let id = {
        // Keep queue capacity and request consumption atomic from the caller's
        // perspective. A full print queue leaves the Print Screen token
        // available for the Blueprint to retry on its next cooperative poll.
        let mut queue = PRINT_QUEUE.lock();
        if !queue.has_job_capacity() {
            return Err(ERROR_QUEUE_FULL);
        }
        let Some((generation, size, raw)) =
            crate::r::gridpaper_service::consume_print_request(owner, token)
        else {
            return Err(ERROR_NOT_OWNER);
        };
        let id = next_job_id();
        queue.retain_status_capacity();
        queue.records.push_back(JobRecord {
            id,
            owner,
            state: PrintJobState::Queued,
        });
        queue.pending.push_back(PrintJob {
            id,
            owner,
            document: PrintDocument::GridPaperA4 {
                generation,
                size,
                raw,
            },
            printer_uri: None,
        });
        id
    };
    crate::log_os::print2d_job_state(id, PrintJobState::Queued.name(), "accepted-print-screen");
    Ok(id)
}

pub(crate) fn submit_for_owner(owner: u8, document_kind: u32, subject: u64, raw: &[u8]) -> i64 {
    let document = match document_kind {
        DOCUMENT_GRIDPAPER_A4 => {
            if !crate::r::gridpaper_service::valid_print_snapshot(raw) {
                return ERROR_INVALID_DOCUMENT;
            }
            PrintDocument::GridPaperA4 {
                generation: subject,
                size: crate::r::gridpaper_service::GridSize::FULL,
                raw: raw.to_vec(),
            }
        }
        DOCUMENT_GRIDPAPER_REQUEST => {
            if !raw.is_empty() || subject == 0 || subject > u32::MAX as u64 {
                return ERROR_INVALID_DOCUMENT;
            }
            return match enqueue_gridpaper_request(owner, subject as u32) {
                Ok(id) => i64::from(id),
                Err(error) => error,
            };
        }
        _ => return ERROR_INVALID_DOCUMENT,
    };

    match enqueue(owner, document, None) {
        Ok(id) => i64::from(id),
        Err(error) => error,
    }
}

pub(crate) fn submit_gridpaper_to_printer(
    owner: u8,
    generation: u64,
    size: crate::r::gridpaper_service::GridSize,
    raw: Vec<u8>,
    printer_uri: &str,
) -> Result<u32, i64> {
    if printer_uri.is_empty() || !crate::r::gridpaper_service::valid_print_snapshot(&raw) {
        return Err(ERROR_INVALID_DOCUMENT);
    }
    enqueue(
        owner,
        PrintDocument::GridPaperA4 {
            generation,
            size,
            raw,
        },
        Some(printer_uri.to_string()),
    )
}

pub(crate) fn take_next_job() -> Option<PrintJob> {
    PRINT_QUEUE.lock().pending.pop_front()
}

pub(crate) fn transition(job_id: u32, state: PrintJobState, detail: &'static str) {
    let changed = {
        let mut queue = PRINT_QUEUE.lock();
        let Some(record) = queue.records.iter_mut().find(|record| record.id == job_id) else {
            return;
        };
        if record.state == state {
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
    let queue = PRINT_QUEUE.lock();
    let Some(record) = queue.records.iter().find(|record| record.id == job_id) else {
        return ERROR_UNKNOWN_JOB;
    };
    if record.owner != owner {
        return ERROR_NOT_OWNER as i32;
    }
    record.state as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_states_are_explicit() {
        assert!(PrintJobState::Completed.terminal());
        assert!(PrintJobState::Failed.terminal());
        assert!(!PrintJobState::Submitted.terminal());
    }
}
