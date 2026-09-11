//! Serialize log-head mutations for each root, including the whole lifetime
//! of an open streamed Put. Read handles remain usable while a writer yields.

use super::block;
use alloc::vec::Vec;
use spin::Mutex;
use trueos_time::{Duration, Timer};

static WRITERS: Mutex<Vec<block::DiscId>> = Mutex::new(Vec::new());

pub(super) struct RootWriteLease {
    disk_id: block::DiscId,
}

impl RootWriteLease {
    pub(super) fn try_acquire(disk_id: block::DiscId) -> Option<Self> {
        let mut writers = WRITERS.lock();
        if writers.contains(&disk_id) {
            return None;
        }
        writers.push(disk_id);
        Some(Self { disk_id })
    }

    pub(super) async fn acquire(disk_id: block::DiscId) -> Self {
        loop {
            if let Some(lease) = Self::try_acquire(disk_id) {
                return lease;
            }
            Timer::after(Duration::from_millis(1)).await;
        }
    }
}

impl Drop for RootWriteLease {
    fn drop(&mut self) {
        let mut writers = WRITERS.lock();
        if let Some(index) = writers.iter().position(|id| *id == self.disk_id) {
            writers.swap_remove(index);
        }
    }
}
