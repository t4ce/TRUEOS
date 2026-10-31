//! Bounded startup-only observation of owned native filesystem requests.
//! No filesystem/VM locks, allocation, paths, payloads, or cancellation policy.
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence};

pub(super) const OWNER_CAP: usize = 64;
const CAP: usize = 16;
pub(super) const OUTPUT_CAP: usize = 4;
const BUDGET: u64 = 128;
const WINDOW_MS: u64 = 180_000;
const RESERVED: u64 = u64::MAX;

pub(super) const QUEUED: u32 = 1;
pub(super) const SERVICE: u32 = 2;
pub(super) const FILESYSTEM: u32 = 3;
pub(super) const RESULT: u32 = 4;
pub(super) const PUBLISH: u32 = 5;
pub(super) const PUBLISHED: u32 = 6;
pub(super) const RETIRE: u32 = 7;
pub(super) const FINISHED: u32 = 8;
// Read-range subphases retain SERVICE's begun timestamp and the same deadline.
pub(super) const RANGE_SCRATCH: u32 = 9;
pub(super) const RANGE_PLACEMENT: u32 = 10;
pub(super) const RANGE_RECORD_CACHE: u32 = 11;
pub(super) const RANGE_INDEX: u32 = 12;
pub(super) const RANGE_READ: u32 = 13;

struct Window {
    arming: AtomicBool,
    // High bits identify this exact arm; low byte is the remaining budget.
    budget: AtomicU64,
    generation: AtomicU64,
    launch: AtomicU64,
    until: AtomicU64,
}
impl Window {
    const fn new() -> Self {
        Self { arming: AtomicBool::new(false), budget: AtomicU64::new(0),
            generation: AtomicU64::new(0), launch: AtomicU64::new(0), until: AtomicU64::new(0) }
    }
}
struct Slot {
    id: AtomicU64,
    version: AtomicU64,
    owner: AtomicU32,
    generation: AtomicU64,
    launch: AtomicU64,
    operation: AtomicU32,
    phase: AtomicU32,
    status: AtomicU32,
    queued: AtomicU64,
    begun: AtomicU64,
    changed: AtomicU64,
    until: AtomicU64,
}
impl Slot {
    const fn new() -> Self {
        Self { id: AtomicU64::new(0), version: AtomicU64::new(0), owner: AtomicU32::new(0),
            generation: AtomicU64::new(0), launch: AtomicU64::new(0), operation: AtomicU32::new(0),
            phase: AtomicU32::new(0), status: AtomicU32::new(0), queued: AtomicU64::new(0),
            begun: AtomicU64::new(0), changed: AtomicU64::new(0), until: AtomicU64::new(0) }
    }
    fn publish(&self, id: u64) {
        self.version.fetch_add(1, Ordering::Release);
        self.id.store(id, Ordering::Release);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub owner: u32,
    pub generation: u64,
    pub launch: u64,
    pub id: u64,
    pub operation: u32,
    pub phase: u32,
    pub status: u32,
    pub queued: u64,
    pub begun: u64,
    pub changed: u64,
    pub until: u64,
}
pub(super) struct Tracker {
    windows: [Window; OWNER_CAP],
    slots: [Slot; CAP],
    next_epoch: AtomicU64,
    pub full: AtomicU64,
    pub missed: AtomicU64,
    pub expired: AtomicU64,
    pub released: AtomicU64,
}
pub(super) struct Ticket<'a> {
    tracker: &'a Tracker,
    slot: Option<usize>,
    id: u64,
}
impl Tracker {
    pub(super) const fn new() -> Self {
        Self { windows: [const { Window::new() }; OWNER_CAP], slots: [const { Slot::new() }; CAP],
            next_epoch: AtomicU64::new(1), full: AtomicU64::new(0), missed: AtomicU64::new(0), expired: AtomicU64::new(0),
            released: AtomicU64::new(0) }
    }
    pub(super) fn inactive(&self) -> Ticket<'_> { Ticket { tracker: self, slot: None, id: 0 } }

    pub(super) fn arm(&self, owner: u8, generation: u64, launch: u64, now: u64) {
        let Some(window) = self.windows.get(owner as usize) else { return; };
        if launch == 0 || launch == RESERVED { return; }
        if window.arming.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            self.missed.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let epoch = self.next_epoch.fetch_add(1, Ordering::Relaxed);
        window.budget.store(0, Ordering::Release);
        if epoch != 0 && epoch <= u64::MAX >> 8 {
            window.generation.store(generation, Ordering::Relaxed);
            window.launch.store(launch, Ordering::Relaxed);
            window.until.store(now.saturating_add(WINDOW_MS), Ordering::Relaxed);
            window.budget.store((epoch << 8) | BUDGET, Ordering::Release);
        }
        window.arming.store(false, Ordering::Release);
    }

    pub(super) fn selected(&self, owner: u8) -> bool {
        self.windows.get(owner as usize).is_some_and(|window| window.budget.load(Ordering::Acquire) & 255 != 0)
    }
    pub(super) fn has_samples(&self) -> bool {
        self.windows.iter().any(|w| w.budget.load(Ordering::Acquire) & 255 != 0)
            || self.slots.iter().any(|slot| {
                let id = slot.id.load(Ordering::Acquire);
                // Never reclaim a paused publisher. Also do not keep the
                // observer clock running forever solely for that claim.
                id != 0 && id != RESERVED
            })
    }

    pub(super) fn admit(&self, owner: u8, generation: u64, id: u64, operation: u32,
        clock: impl FnOnce() -> u64) -> Ticket<'_>
    {
        if id == 0 || id == RESERVED || operation == 0 { return self.inactive(); }
        let Some(window) = self.windows.get(owner as usize) else { return self.inactive(); };
        let budget = window.budget.load(Ordering::Acquire);
        if budget & 255 == 0 { return self.inactive(); }
        let run = window.generation.load(Ordering::Relaxed);
        let launch = window.launch.load(Ordering::Relaxed);
        let until = window.until.load(Ordering::Relaxed);
        fence(Ordering::Acquire);
        if run != generation || window.budget.load(Ordering::Acquire) >> 8 != budget >> 8 {
            return self.inactive();
        }
        // Reserve only from this exact launch budget. A re-arm cannot spend a
        // later incarnation's budget; simultaneous admissions may fail closed.
        if window.budget.compare_exchange(budget, budget - 1, Ordering::AcqRel, Ordering::Relaxed).is_err() {
            self.missed.fetch_add(1, Ordering::Relaxed);
            return self.inactive();
        }
        for offset in 0..CAP {
            let index = (id as usize).wrapping_add(offset) % CAP;
            let slot = &self.slots[index];
            if slot.id.compare_exchange(0, RESERVED, Ordering::Acquire, Ordering::Relaxed).is_err() { continue; }
            let now = clock();
            if now >= until {
                // Expiry affects observation only. It never cancels disk work.
                let _ = window.budget.compare_exchange(budget - 1, 0, Ordering::AcqRel, Ordering::Relaxed);
                slot.id.store(0, Ordering::Release);
                self.expired.fetch_add(1, Ordering::Relaxed);
                return self.inactive();
            }
            slot.version.fetch_add(1, Ordering::AcqRel);
            // Pair with the reader's acquire fence even when its relaxed
            // field read, rather than its initial id read, sees this write.
            fence(Ordering::Release);
            slot.owner.store(u32::from(owner), Ordering::Relaxed);
            slot.generation.store(generation, Ordering::Relaxed);
            slot.launch.store(launch, Ordering::Relaxed);
            slot.operation.store(operation, Ordering::Relaxed);
            slot.phase.store(QUEUED, Ordering::Relaxed);
            slot.status.store(0, Ordering::Relaxed);
            slot.queued.store(now, Ordering::Relaxed);
            slot.begun.store(0, Ordering::Relaxed);
            slot.changed.store(now, Ordering::Relaxed);
            slot.until.store(until, Ordering::Relaxed);
            slot.publish(id);
            return Ticket { tracker: self, slot: Some(index), id };
        }
        self.full.fetch_add(1, Ordering::Relaxed);
        self.missed.fetch_add(1, Ordering::Relaxed);
        self.inactive()
    }

    fn read(&self, index: usize) -> Option<Snapshot> {
        let slot = &self.slots[index];
        // One attempt, no spinning: a concurrent writer is simply not sampled.
        let version = slot.version.load(Ordering::Acquire);
        if version & 1 != 0 { return None; }
        let id = slot.id.load(Ordering::Acquire);
        if id == 0 || id == RESERVED { return None; }
        let snapshot = Snapshot { owner: slot.owner.load(Ordering::Relaxed),
            generation: slot.generation.load(Ordering::Relaxed), launch: slot.launch.load(Ordering::Relaxed),
            id, operation: slot.operation.load(Ordering::Relaxed), phase: slot.phase.load(Ordering::Relaxed),
            status: slot.status.load(Ordering::Relaxed), queued: slot.queued.load(Ordering::Relaxed),
            begun: slot.begun.load(Ordering::Relaxed), changed: slot.changed.load(Ordering::Relaxed),
            until: slot.until.load(Ordering::Relaxed) };
        fence(Ordering::Acquire);
        (version == slot.version.load(Ordering::Acquire) && id == slot.id.load(Ordering::Acquire)).then_some(snapshot)
    }

    pub(super) fn sample(&self, now: u64, cursor: &mut usize, limit: usize) -> [Option<Snapshot>; OUTPUT_CAP] {
        let mut rows = [None; OUTPUT_CAP];
        let limit = limit.min(OUTPUT_CAP);
        if limit == 0 { return rows; }
        for window in &self.windows {
            let budget = window.budget.load(Ordering::Acquire);
            if budget & 255 != 0 && now >= window.until.load(Ordering::Relaxed) {
                let _ = window.budget.compare_exchange(budget, 0, Ordering::AcqRel, Ordering::Relaxed);
            }
        }
        let start = *cursor % CAP;
        let mut count = 0;
        for offset in 0..CAP {
            let index = (start + offset) % CAP;
            let Some(snapshot) = self.read(index) else { continue; };
            if now >= snapshot.until {
                if self.slots[index].id.compare_exchange(snapshot.id, 0, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                    self.expired.fetch_add(1, Ordering::Relaxed);
                }
                continue;
            }
            rows[count] = Some(snapshot);
            count += 1;
            *cursor = (index + 1) % CAP;
            if count == limit { break; }
        }
        rows
    }
}
impl Ticket<'_> {
    pub(super) fn advance(&mut self, phase: u32, status: u32, clock: impl FnOnce() -> u64) {
        let Some(index) = self.slot else { return; };
        let slot = &self.tracker.slots[index];
        if slot.id.load(Ordering::Acquire) != self.id { self.slot = None; return; }
        let now = clock();
        if now >= slot.until.load(Ordering::Relaxed) {
            if slot.id.compare_exchange(self.id, 0, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                self.tracker.expired.fetch_add(1, Ordering::Relaxed);
            }
            self.slot = None;
            return;
        }
        // Claim exclusive publication before writing any fields. Expiry/reuse
        // cannot race an old token's stores into a new request's slot.
        if slot.id.compare_exchange(self.id, RESERVED, Ordering::AcqRel, Ordering::Relaxed).is_err() {
            self.slot = None;
            return;
        }
        slot.version.fetch_add(1, Ordering::AcqRel);
        fence(Ordering::Release);
        if phase == SERVICE { slot.begun.store(now, Ordering::Relaxed); }
        slot.changed.store(now, Ordering::Relaxed);
        slot.status.store(status, Ordering::Relaxed);
        slot.phase.store(phase, Ordering::Relaxed);
        slot.publish(self.id);
    }
}
impl Drop for Ticket<'_> {
    fn drop(&mut self) {
        if let Some(index) = self.slot {
            if self.tracker.slots[index].id.compare_exchange(self.id, 0, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                self.tracker.released.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_service_result_publish_and_retirement_are_distinct() {
        let tracker = Tracker::new(); tracker.arm(0, 10, 6, 100);
        let mut ticket = tracker.admit(0, 10, 19, 2, || 110);
        let row = tracker.sample(115, &mut 0, 4).into_iter().flatten().next().unwrap();
        assert_eq!((row.phase, row.queued, row.begun, row.launch), (QUEUED, 110, 0, 6));
        for (phase, at) in [(SERVICE,120),(FILESYSTEM,130),(RESULT,200),(PUBLISH,201),(PUBLISHED,202),(RETIRE,203)] {
            ticket.advance(phase, u32::from(phase >= RESULT), || at);
            let row = tracker.sample(at, &mut 0, 4).into_iter().flatten().next().unwrap();
            assert_eq!((row.phase, row.begun, row.changed), (phase,120,at));
        }
        drop(ticket); assert!(tracker.sample(300, &mut 0, 4).iter().all(Option::is_none));
    }
    #[test]
    fn no_clock_for_unselected_invalid_full_or_exhausted_requests() {
        let tracker = Tracker::new();
        drop(tracker.admit(0, 1, 1, 1, || panic!("unselected clock")));
        tracker.arm(0, 1, 1, 10);
        drop(tracker.admit(0, 2, 1, 1, || panic!("wrong generation clock")));
        drop(tracker.admit(0, 1, 0, 1, || panic!("invalid id clock")));
        let tickets: Vec<_> = (1..=CAP as u64).map(|id| tracker.admit(0,1,id,1,||20)).collect();
        drop(tracker.admit(0,1,99,1,||panic!("full clock")));
        assert_eq!(tracker.missed.load(Ordering::Relaxed),1);
        assert_eq!(tracker.full.load(Ordering::Relaxed),1);
        drop(tickets);
        for id in 100..(100+BUDGET) { drop(tracker.admit(0,1,id,1,||20)); }
        drop(tracker.admit(0,1,999,1,||panic!("exhausted clock")));
    }
    #[test]
    fn sampling_bounds_round_robin_and_zero_limit() {
        let tracker=Tracker::new(); tracker.arm(0,1,1,0);
        let _tickets: Vec<_>=(1..=16).map(|id| tracker.admit(0,1,id,1,||1)).collect();
        let mut cursor=0; let mut seen=std::collections::BTreeSet::new();
        assert!(tracker.sample(2,&mut cursor,0).iter().all(Option::is_none)); assert_eq!(cursor,0);
        for _ in 0..4 { for row in tracker.sample(2,&mut cursor,usize::MAX).into_iter().flatten() { seen.insert(row.id); } }
        assert_eq!(seen.len(),16);
    }
    #[test]
    fn abandoned_consumer_does_not_own_or_cancel_pending_work() {
        let tracker=Tracker::new(); tracker.arm(0,1,1,0);
        let consumer=std::sync::Arc::new(()); let service=consumer.clone();
        let ticket=tracker.admit(0,1,7,1,||1);
        drop(consumer); assert_eq!(std::sync::Arc::strong_count(&service),1);
        assert!(tracker.sample(10,&mut 0,4).iter().any(Option::is_some));
        drop(ticket); assert_eq!(tracker.released.load(Ordering::Relaxed),1);
    }
    #[test]
    fn expiry_reuse_and_old_token_cannot_mutate_new_identity() {
        let tracker=Tracker::new(); tracker.arm(0,1,1,0);
        let mut old=tracker.admit(0,1,1,1,||1);
        assert!(tracker.sample(WINDOW_MS,&mut 0,4).iter().all(Option::is_none));
        assert_eq!(tracker.expired.load(Ordering::Relaxed),1);
        tracker.arm(0,2,2,WINDOW_MS);
        let _new=tracker.admit(0,2,17,9,||WINDOW_MS+1);
        old.advance(RESULT,99,||panic!("stale token clock")); drop(old);
        let row=tracker.sample(WINDOW_MS+2,&mut 0,4).into_iter().flatten().next().unwrap();
        assert_eq!((row.id,row.generation,row.operation,row.phase,row.status),(17,2,9,QUEUED,0));
    }
    #[test]
    fn expiry_and_reuse_between_old_identity_check_and_publish_are_safe() {
        let tracker=Tracker::new(); tracker.arm(0,1,1,0);
        let mut old=tracker.admit(0,1,1,1,||1);
        let mut replacement=None;
        old.advance(RESULT,99,|| {
            // advance has already checked the old id, but owns no write claim.
            tracker.sample(WINDOW_MS,&mut 0,4);
            tracker.arm(0,2,2,WINDOW_MS);
            replacement=Some(tracker.admit(0,2,17,9,||WINDOW_MS+1));
            WINDOW_MS+2
        });
        drop(old);
        let row=tracker.sample(WINDOW_MS+3,&mut 0,4).into_iter().flatten().next().unwrap();
        assert_eq!((row.id,row.generation,row.launch,row.operation,row.phase,row.status),(17,2,2,9,QUEUED,0));
        drop(replacement);
    }
    #[test]
    fn rearm_changes_launch_epoch_without_spending_new_or_wrong_owner_budget() {
        let tracker=Tracker::new(); tracker.arm(0,1,6,0);
        assert!(tracker.selected(0)); assert!(!tracker.selected(1));
        for id in 1..=BUDGET { drop(tracker.admit(0,1,id,1,||1)); }
        assert!(!tracker.selected(0));
        drop(tracker.admit(0,1,1000,1,||panic!("exhausted clock")));
        tracker.arm(0,2,7,2);
        drop(tracker.admit(0,1,1001,1,||panic!("old run clock")));
        let _ticket=tracker.admit(0,2,1002,1,||3);
        let row=tracker.sample(4,&mut 0,4).into_iter().flatten().next().unwrap();
        assert_eq!((row.generation,row.launch,row.id),(2,7,1002));
        assert_eq!(tracker.windows[0].budget.load(Ordering::Acquire)&255,BUDGET-1);
    }
    #[test]
    fn writer_claim_prevents_expiry_reuse_until_publication() {
        let tracker=Tracker::new(); tracker.arm(0,1,1,0);
        let mut ticket=tracker.admit(0,1,1,1,||1);
        let index=ticket.slot.unwrap(); let slot=&tracker.slots[index];
        assert!(slot.id.compare_exchange(1,RESERVED,Ordering::AcqRel,Ordering::Relaxed).is_ok());
        slot.version.fetch_add(1,Ordering::AcqRel);
        assert!(tracker.sample(WINDOW_MS,&mut 0,4).iter().all(Option::is_none));
        assert_eq!(slot.id.load(Ordering::Acquire),RESERVED);
        assert_eq!(tracker.expired.load(Ordering::Relaxed),0);
        assert!(!tracker.has_samples());
        slot.phase.store(FILESYSTEM,Ordering::Relaxed); slot.publish(1);
        ticket.advance(RESULT,1,||WINDOW_MS+1);
        assert_eq!(slot.id.load(Ordering::Acquire),0);
    }
    #[test]
    fn bounded_concurrent_sampler_never_mixes_reused_request_fields() {
        let tracker=std::sync::Arc::new(Tracker::new());
        for owner in 0..4 { tracker.arm(owner,100+u64::from(owner),200+u64::from(owner),0); }
        let mut workers=Vec::new();
        for owner in 0..4u8 {
            let t=tracker.clone(); workers.push(std::thread::spawn(move || {
                for n in 1..=100u64 {
                    let id=u64::from(owner)*1000+n;
                    let mut ticket=t.admit(owner,100+u64::from(owner),id,1+u32::from(owner),||10);
                    ticket.advance(SERVICE,0,||20); ticket.advance(RESULT,1,||30);
                }
            }));
        }
        let mut cursor=0;
        while workers.iter().any(|worker| !worker.is_finished()) {
            for row in tracker.sample(40,&mut cursor,4).into_iter().flatten() {
                assert_eq!(row.generation,100+u64::from(row.owner));
                assert_eq!(row.launch,200+u64::from(row.owner));
                assert_eq!(row.operation,1+row.owner);
                assert_eq!(row.id/1000,u64::from(row.owner));
                match row.phase { QUEUED=>assert_eq!((row.begun,row.changed,row.status),(0,10,0)),
                    SERVICE=>assert_eq!((row.begun,row.changed,row.status),(20,20,0)),
                    RESULT=>assert_eq!((row.begun,row.changed,row.status),(20,30,1)), _=>panic!("mixed phase") }
            }
        }
        for worker in workers { worker.join().unwrap(); }
        assert!(tracker.sample(40,&mut cursor,4).iter().all(Option::is_none));
    }
}
