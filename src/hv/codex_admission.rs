//! One Codex Hull across launchers, including the preparation interval.
//! This limits VMs, not agents or threads inside the admitted Codex process.
use core::sync::atomic::{AtomicU64, Ordering};

pub(super) struct Limit {
    owner: AtomicU64,
    sequence: AtomicU64,
}

impl Limit {
    pub(super) const fn new() -> Self {
        Self {
            owner: AtomicU64::new(0),
            sequence: AtomicU64::new(1),
        }
    }

    pub(super) fn claim(&self, vm_id: u8) -> Result<Permit<'_>, ()> {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let ticket = (sequence << 8) | (u64::from(vm_id) + 1);
        self.owner
            .compare_exchange(0, ticket, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| ())?;
        Ok(Permit {
            limit: self,
            ticket,
            committed: false,
        })
    }

    /// Called at final offline publication, after guest jobs and carrier drain.
    pub(super) fn release_vm(&self, vm_id: u8) {
        let ticket = self.owner.load(Ordering::Acquire);
        if ticket & 0xff == u64::from(vm_id) + 1 {
            let _ = self
                .owner
                .compare_exchange(ticket, 0, Ordering::AcqRel, Ordering::Acquire);
        }
    }
}

pub(super) struct Permit<'a> {
    limit: &'a Limit,
    ticket: u64,
    committed: bool,
}

impl Permit<'_> {
    /// Successful launches hold the slot until final offline publication.
    pub(super) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.limit.owner.compare_exchange(
                self.ticket,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
}
