//! Small producer-facing 2D print queue.
//!
//! Blueprint producers submit compact document state here.  The BSP printer
//! service is the sole consumer and owns rendering, transport, and remote job
//! monitoring.

extern crate alloc;

use alloc::{
    collections::VecDeque,
    string::String,
};
use spin::Mutex;






pub const ERROR_INVALID_DOCUMENT: i64 = -1;

pub const ERROR_NOT_OWNER: i64 = -3;
pub const ERROR_UNKNOWN_JOB: i32 = -4;
pub const ERROR_TRANSPORT: i64 = -5;




#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum PrintJobState {
    
    WaitingForPrinter = 2,
    
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
            
            Self::WaitingForPrinter => "waiting-for-printer",
            
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

    #[cfg(test)]
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled | Self::OutcomeUnknown)
    }
}



pub(crate) struct PrintJob {
    pub id: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub owner: PrintOwner,
    
    pub printer_uri: Option<String>,
}

#[derive(Clone, Copy)]
struct JobRecord {
    id: u32,
    owner: PrintOwner,
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

    

    
}


static PRINT_QUEUE: Mutex<PrintQueue> = Mutex::new(PrintQueue::new());















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
    if record.owner != PrintOwner::Blueprint(owner) {
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

pub(crate) fn submit_for_owner(_owner: u8, _document_kind: u32, _subject: u64, _raw: &[u8]) -> i64 {
    ERROR_INVALID_DOCUMENT
}
