//! Kernel-owned provenance of guest-created LINK buckets. Bucket labels and
//! persona metadata never grant permission to mutate another caller's bucket.
extern crate alloc;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Guest {
    pub vm: u8,
    pub run: u64,
}

pub(super) struct Ownership {
    entries: Vec<(u32, Guest)>,
}

impl Ownership {
    pub const fn new() -> Self { Self { entries: Vec::new() } }

    pub fn remember(&mut self, bucket: u32, guest: Guest) -> bool {
        if bucket == 0 || guest.run == 0 || self.entries.iter().any(|entry| entry.0 == bucket)
            || self.entries.len() >= 32 || self.entries.try_reserve(1).is_err() {
            return false;
        }
        self.entries.push((bucket, guest));
        true
    }

    pub fn owns(&self, bucket: u32, guest: Guest) -> bool {
        self.entries.iter().any(|entry| *entry == (bucket, guest))
    }

    pub fn forget(&mut self, bucket: u32) {
        self.entries.retain(|entry| entry.0 != bucket);
    }

    pub fn retain_live(&mut self, mut live: impl FnMut(u32) -> bool) {
        self.entries.retain(|entry| live(entry.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_authority_is_exact_run_and_does_not_follow_vm_reuse() {
        let mut policy = Ownership::new();
        let alice = Guest { vm: 1, run: 7 };
        assert!(policy.remember(13, alice));
        assert!(policy.owns(13, alice));
        assert!(!policy.owns(13, Guest { vm: 2, run: 7 }));
        assert!(!policy.owns(13, Guest { vm: 1, run: 8 }));
        assert!(!policy.remember(13, Guest { vm: 1, run: 8 }));
        policy.forget(13);
        assert!(!policy.owns(13, alice));
        assert!(policy.remember(13, Guest { vm: 1, run: 8 }));
        assert!(!policy.owns(13, alice));
    }

    #[test]
    fn removed_buckets_release_bounded_provenance_slots() {
        let mut policy = Ownership::new();
        let guest = Guest { vm: 1, run: 2 };
        assert!(!policy.remember(0, guest));
        assert!(!policy.remember(1, Guest { vm: 1, run: 0 }));
        for id in 1..=32 { assert!(policy.remember(id, guest)); }
        assert!(!policy.remember(33, guest));
        policy.retain_live(|id| id != 7);
        assert!(!policy.owns(7, guest));
        assert!(policy.remember(33, guest));
    }
}
