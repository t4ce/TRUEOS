//! Small run queue extracted from the TrustOS scheduler shape.

extern crate alloc;

use alloc::collections::VecDeque;
use spin::Mutex;

use super::types::ThreadId;

pub struct RunQueue {
    cpu_id: usize,
    queue: Mutex<VecDeque<ThreadId>>,
}

impl RunQueue {
    pub const fn new(cpu_id: usize) -> Self {
        Self {
            cpu_id,
            queue: Mutex::new(VecDeque::new()),
        }
    }

    pub fn cpu_id(&self) -> usize {
        self.cpu_id
    }

    pub fn push(&self, thread_id: ThreadId) {
        self.queue.lock().push_back(thread_id);
    }

    pub fn pop(&self) -> Option<ThreadId> {
        self.queue.lock().pop_front()
    }

    pub fn steal(&self) -> Option<ThreadId> {
        self.queue.lock().pop_back()
    }

    pub fn len(&self) -> usize {
        self.queue.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.lock().is_empty()
    }

    pub fn try_push(&self, thread_id: ThreadId) -> bool {
        if let Some(mut queue) = self.queue.try_lock() {
            queue.push_back(thread_id);
            true
        } else {
            false
        }
    }

    pub fn try_pop(&self) -> Option<ThreadId> {
        self.queue.try_lock()?.pop_front()
    }

    pub fn try_steal(&self) -> Option<ThreadId> {
        self.queue.try_lock()?.pop_back()
    }
}
