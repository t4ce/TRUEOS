//! Keep a large slot-0 painter plan pinned while bounded BCS batches retire.

use super::{Ui4AsyncCompositionError, Ui4AsyncCompositionPoll};
use crate::intel::{
    GucBcs0CopyCompletion, GucBcs0CopySubmission, GucBcs0CopySubmitError, GucBcs0RgbaCopy,
    GucBcs0RgbaSurface,
};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// The shared BCS lane accepts at most 160 rectangles in one batch.
const COPIES_PER_BATCH: usize = 160;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static PENDING: Mutex<Option<PendingCopies>> = Mutex::new(None);

struct PendingCopies {
    sequence: u64,
    destination: GucBcs0RgbaSurface,
    copies: Vec<GucBcs0RgbaCopy>,
    scanout_cache: Vec<bool>,
    next: usize,
    active: Option<GucBcs0CopySubmission>,
    ownership_uncertain: bool,
}

impl PendingCopies {
    fn submit_next(&mut self) -> Result<(), GucBcs0CopySubmitError> {
        let end = self
            .next
            .saturating_add(COPIES_PER_BATCH)
            .min(self.copies.len());
        let submission = crate::r::services::vcpy_service::queue_rgba_copies_with_scanout_cache(
            self.destination,
            &self.copies[self.next..end],
            &self.scanout_cache[self.next..end],
        )?;
        self.next = end;
        self.active = Some(submission);
        Ok(())
    }

    fn retain_uncertain(&mut self, reason: &'static str) {
        if !self.ownership_uncertain {
            crate::log_error!(target: "ui4";
                "ui4/slot0-bcs ownership=uncertain reason={} sequence={} submitted_copies={} total_copies={} action=retain-plan-and-frame-leases-until-reboot\n",
                reason, self.sequence, self.next, self.copies.len(),
            );
            self.ownership_uncertain = true;
        }
    }
}

pub(super) fn queue(
    destination: GucBcs0RgbaSurface,
    copies: Vec<GucBcs0RgbaCopy>,
    scanout_cache: Vec<bool>,
) -> Result<u64, Ui4AsyncCompositionError> {
    if copies.is_empty() || copies.len() != scanout_cache.len() {
        return Err(Ui4AsyncCompositionError::Unavailable);
    }
    let mut pending = PENDING.lock();
    if pending.is_some() {
        return Err(Ui4AsyncCompositionError::Busy);
    }
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    let mut plan = PendingCopies {
        sequence,
        destination,
        copies,
        scanout_cache,
        next: 0,
        active: None,
        ownership_uncertain: false,
    };
    match plan.submit_next() {
        Ok(()) => {}
        // An ambiguous first submit still needs a compositor token: returning
        // an error here would let the caller release its source read leases.
        Err(GucBcs0CopySubmitError::SubmitFailed) => plan.retain_uncertain("first-submit"),
        Err(GucBcs0CopySubmitError::Busy) => return Err(Ui4AsyncCompositionError::Busy),
        Err(GucBcs0CopySubmitError::Unavailable) => {
            return Err(Ui4AsyncCompositionError::Unavailable);
        }
        Err(GucBcs0CopySubmitError::InvalidRequest) => {
            return Err(Ui4AsyncCompositionError::Failed);
        }
    }
    *pending = Some(plan);
    Ok(sequence)
}

pub(super) fn poll(sequence: u64) -> Ui4AsyncCompositionPoll {
    let mut pending = PENDING.lock();
    let Some(plan) = pending.as_mut().filter(|plan| plan.sequence == sequence) else {
        return Ui4AsyncCompositionPoll::Failed;
    };
    if plan.ownership_uncertain {
        return Ui4AsyncCompositionPoll::Pending;
    }
    if let Some(active) = plan.active {
        match crate::r::services::vcpy_service::poll_rgba_copies(active) {
            GucBcs0CopyCompletion::Pending => return Ui4AsyncCompositionPoll::Pending,
            GucBcs0CopyCompletion::Complete => plan.active = None,
            GucBcs0CopyCompletion::Failed | GucBcs0CopyCompletion::InvalidSubmission => {
                plan.retain_uncertain("completion");
                return Ui4AsyncCompositionPoll::Pending;
            }
        }
    }
    if plan.next == plan.copies.len() {
        *pending = None;
        return Ui4AsyncCompositionPoll::Ready;
    }
    match plan.submit_next() {
        Ok(()) | Err(GucBcs0CopySubmitError::Busy) => Ui4AsyncCompositionPoll::Pending,
        Err(GucBcs0CopySubmitError::SubmitFailed) => {
            plan.retain_uncertain("next-submit");
            Ui4AsyncCompositionPoll::Pending
        }
        // No batch is active and all preceding chunks retired exactly. A
        // rejected request cannot touch these allocations, so release the plan.
        Err(GucBcs0CopySubmitError::Unavailable | GucBcs0CopySubmitError::InvalidRequest) => {
            *pending = None;
            Ui4AsyncCompositionPoll::Failed
        }
    }
}
