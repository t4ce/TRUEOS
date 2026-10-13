//! Host-owned advisory locks for the POSIX file compatibility namespace.
//!
//! TRUEOSFS has no hard links and its rename compatibility operation copies
//! then deletes. Use the canonical root/path namespace, including unpublished
//! O_CREAT files, so two processes cannot bypass coordination by opening a
//! file before its buffered creation has reached disk.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    pub realm: u64,
    pub description: u64,
}

#[derive(Default)]
pub(crate) struct Registry {
    files: BTreeMap<(u32, String), Vec<(Owner, bool)>>,
}

impl Registry {
    pub(crate) const fn new() -> Self {
        Self { files: BTreeMap::new() }
    }

    /// `None` unlocks; `Some(true)` is exclusive, `Some(false)` shared.
    pub(crate) fn try_set(&mut self, root: u32, path: &str, owner: Owner, mode: Option<bool>) -> bool {
        let key = (root, String::from(path));
        let Some(exclusive) = mode else {
            self.release(owner);
            return true;
        };
        let holders = self.files.entry(key).or_default();
        if holders.iter().any(|(other, held_exclusive)| {
            *other != owner && (exclusive || *held_exclusive)
        }) {
            return false;
        }
        if let Some((_, held_exclusive)) = holders.iter_mut().find(|(other, _)| *other == owner) {
            *held_exclusive = exclusive;
        } else {
            holders.push((owner, exclusive));
        }
        true
    }

    pub(crate) fn release(&mut self, owner: Owner) {
        self.files.retain(|_, holders| {
            holders.retain(|(other, _)| *other != owner);
            !holders.is_empty()
        });
    }

    pub(crate) fn release_realm(&mut self, realm: u64) {
        self.files.retain(|_, holders| {
            holders.retain(|(owner, _)| owner.realm != realm);
            !holders.is_empty()
        });
    }
}

pub(crate) static LOCKS: Mutex<Registry> = Mutex::new(Registry::new());
