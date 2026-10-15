//! Passive bounded async-FS timing. Samples never acquire filesystem locks.
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
const CAP: usize = 64;
const RESERVED: u64 = u64::MAX;
struct Slot {
    identity: AtomicU64,
    class: AtomicU32,
    phase: AtomicU32,
    admitted: AtomicU64,
    begun: AtomicU64,
    done: AtomicU64,
    observed: AtomicU64,
}
impl Slot {
    const fn new() -> Self {
        Self {
            identity: AtomicU64::new(0),
            class: AtomicU32::new(0),
            phase: AtomicU32::new(0),
            admitted: AtomicU64::new(0),
            begun: AtomicU64::new(0),
            done: AtomicU64::new(0),
            observed: AtomicU64::new(0),
        }
    }
}
pub(super) struct Tracker {
    slots: [Slot; CAP],
    pub(super) missed: AtomicU64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub owner: u32,
    pub id: u32,
    pub class: u32,
    pub phase: u32,
    pub admitted: u64,
    pub begun: u64,
    pub done: u64,
    pub observed: u64,
}
impl Tracker {
    pub(super) const fn new() -> Self {
        Self {
            slots: [const { Slot::new() }; CAP],
            missed: AtomicU64::new(0),
        }
    }
    fn identity(owner: u32, id: u32) -> u64 {
        (u64::from(owner) << 32) | u64::from(id)
    }
    fn find(&self, owner: u32, id: u32) -> Option<&Slot> {
        let identity = Self::identity(owner, id);
        (0..CAP)
            .map(|offset| &self.slots[(id as usize + offset) % CAP])
            .find(|slot| slot.identity.load(Ordering::Acquire) == identity)
    }
    pub(super) fn admit(&self, owner: u32, id: u32, class: u32, now: u64) {
        let identity = Self::identity(owner, id);
        if identity == 0 || identity == RESERVED {
            self.missed.fetch_add(1, Ordering::Relaxed);
            return;
        }
        for offset in 0..CAP {
            let slot = &self.slots[(id as usize + offset) % CAP];
            if slot
                .identity
                .compare_exchange(0, RESERVED, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }
            slot.class.store(class, Ordering::Relaxed);
            slot.admitted.store(now, Ordering::Relaxed);
            slot.begun.store(0, Ordering::Relaxed);
            slot.done.store(0, Ordering::Relaxed);
            slot.observed.store(0, Ordering::Relaxed);
            slot.phase.store(1, Ordering::Relaxed);
            slot.identity.store(identity, Ordering::Release);
            return;
        }
        self.missed.fetch_add(1, Ordering::Relaxed);
    }
    pub(super) fn begin(&self, owner: u32, id: u32, now: u64) {
        if let Some(slot) = self.find(owner, id) {
            slot.begun.store(now, Ordering::Relaxed);
            slot.phase.store(2, Ordering::Release);
        }
    }
    pub(super) fn done(&self, owner: u32, id: u32, now: u64) -> Option<Snapshot> {
        if let Some(slot) = self.find(owner, id) {
            slot.done.store(now, Ordering::Relaxed);
            slot.phase.store(3, Ordering::Release);
            return self.read(slot);
        }
        None
    }
    pub(super) fn observe(&self, owner: u32, id: u32, now: u64) -> Option<Snapshot> {
        let slot = self.find(owner, id)?;
        if slot
            .phase
            .compare_exchange(3, 5, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return None;
        }
        slot.observed.store(now, Ordering::Relaxed);
        slot.phase.store(4, Ordering::Release);
        self.read(slot)
    }
    pub(super) fn release(&self, owner: u32, id: u32) {
        if let Some(slot) = self.find(owner, id) {
            slot.identity.store(0, Ordering::Release);
        }
    }
    fn read(&self, slot: &Slot) -> Option<Snapshot> {
        let identity = slot.identity.load(Ordering::Acquire);
        if identity == 0 || identity == RESERVED {
            return None;
        }
        let snapshot = Snapshot {
            owner: (identity >> 32) as u32,
            id: identity as u32,
            class: slot.class.load(Ordering::Relaxed),
            phase: slot.phase.load(Ordering::Acquire),
            admitted: slot.admitted.load(Ordering::Relaxed),
            begun: slot.begun.load(Ordering::Relaxed),
            done: slot.done.load(Ordering::Relaxed),
            observed: slot.observed.load(Ordering::Acquire),
        };
        (snapshot.phase != 5 && identity == slot.identity.load(Ordering::Acquire))
            .then_some(snapshot)
    }
    pub(super) fn snapshots(&self) -> impl Iterator<Item = Snapshot> + '_ {
        self.slots.iter().filter_map(|slot| self.read(slot))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unfinished_queue_service_and_consumer_delay_visible() {
        let t = Tracker::new();
        t.admit(7, 9, 6, 100);
        assert_eq!(t.snapshots().next().unwrap().phase, 1);
        t.begin(7, 9, 350);
        t.done(7, 9, 700);
        let s = t.observe(7, 9, 1000).unwrap();
        assert_eq!((s.begun - s.admitted, s.done - s.begun, s.observed - s.done), (250, 350, 300));
        assert!(t.observe(7, 9, 2000).is_none());
        t.release(8, 9);
        assert_eq!(t.snapshots().count(), 1);
        t.release(7, 9);
        assert_eq!(t.snapshots().count(), 0);
    }
    #[test]
    fn hash_collisions_capacity_and_reuse_are_bounded() {
        let t = Tracker::new();
        for i in 0..64 {
            t.admit(i, 1, 1, 10);
        }
        assert_eq!(t.snapshots().count(), 64);
        t.admit(80, 1, 1, 10);
        assert_eq!(t.missed.load(Ordering::Relaxed), 1);
        t.release(12, 1);
        t.admit(80, 1, 2, 20);
        assert_eq!(t.snapshots().find(|s| s.owner == 80).unwrap().class, 2);
    }
    #[test]
    fn concurrent_owners_and_sampler_never_need_a_filesystem_lock() {
        let tracker = std::sync::Arc::new(Tracker::new());
        let mut workers = Vec::new();
        for owner in 0..6 {
            let tracker = tracker.clone();
            workers.push(std::thread::spawn(move || {
                for id in 1..500 {
                    tracker.admit(owner, id, 7, 10);
                    tracker.begin(owner, id, 20);
                    tracker.done(owner, id, 30);
                    assert!(tracker.observe(owner, id, 40).is_some());
                    tracker.release(owner, id);
                }
            }));
        }
        while workers.iter().any(|worker| !worker.is_finished()) {
            let snapshots: Vec<_> = tracker.snapshots().collect();
            assert!(snapshots.len() <= 64);
            for snapshot in snapshots {
                assert!(snapshot.owner < 6 && snapshot.id > 0 && snapshot.class == 7);
                assert!((1..=4).contains(&snapshot.phase));
            }
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(tracker.snapshots().count(), 0);
        assert_eq!(tracker.missed.load(Ordering::Relaxed), 0);
    }
}
