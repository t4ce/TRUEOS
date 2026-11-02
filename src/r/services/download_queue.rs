//! Volatile, bounded work ownership for the independent Hull download VM.
extern crate alloc;
use alloc::{collections::VecDeque, vec::Vec};

pub const CAPACITY: usize = 128;
pub const MAX_FRAME: usize = 4096 + 1 + 512;
#[derive(Clone, Debug)]
pub struct Job {
    pub id: u64,
    pub payload: Vec<u8>,
}
#[derive(Debug, PartialEq)]
pub enum Next {
    Job(Vec<u8>),
    Wait,
    Drained,
}
pub struct Queue {
    pending: VecDeque<Job>,
    active: VecDeque<Job>,
    generation: u64,
    sequence: u64,
    running: bool,
    owner: Option<(u32, u64)>,
}
impl Queue {
    pub const fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            active: VecDeque::new(),
            generation: 0,
            sequence: 0,
            running: false,
            owner: None,
        }
    }
    pub fn submit(&mut self, payload: &[u8]) -> Result<u64, ()> {
        if !valid_submission(payload) || self.pending.len() + self.active.len() >= CAPACITY {
            return Err(());
        }
        self.sequence = self.sequence.checked_add(1).ok_or(())?;
        let id = self.sequence;
        self.pending.push_back(Job {
            id,
            payload: payload.to_vec(),
        });
        Ok(id)
    }
    pub fn cancel_pending(&mut self, id: u64) {
        self.pending.retain(|job| job.id != id);
    }
    pub fn begin(&mut self) -> Option<u64> {
        if self.running || self.pending.is_empty() {
            return None;
        }
        self.generation = self.generation.checked_add(1)?;
        self.running = true;
        self.owner = None;
        Some(self.generation)
    }
    pub fn started(&mut self, generation: u64, owner: u32, run: u64) {
        if self.running && generation == self.generation {
            self.owner = Some((owner, run));
        }
    }
    pub fn failed(&mut self, generation: u64) -> bool {
        if !self.running || generation != self.generation {
            return false;
        }
        while let Some(job) = self.active.pop_back() {
            self.pending.push_front(job);
        }
        self.running = false;
        self.owner = None;
        !self.pending.is_empty()
    }
    fn authorize(&self, generation: u64, owner: u32, run: u64) -> Result<bool, ()> {
        if !self.running || generation != self.generation {
            return Err(());
        }
        match self.owner {
            None => Ok(false),
            Some(expected) if expected == (owner, run) => Ok(true),
            _ => Err(()),
        }
    }
    pub fn next(&mut self, generation: u64, owner: u32, run: u64) -> Result<Next, ()> {
        if !self.authorize(generation, owner, run)? {
            return Ok(Next::Wait);
        }
        if self.active.len() >= 32 {
            return Ok(Next::Wait);
        }
        if let Some(job) = self.pending.pop_front() {
            let mut bytes = Vec::with_capacity(9 + job.payload.len());
            bytes.push(b'J');
            bytes.extend_from_slice(&job.id.to_le_bytes());
            bytes.extend_from_slice(&job.payload);
            self.active.push_back(job);
            return Ok(Next::Job(bytes));
        }
        if !self.active.is_empty() {
            return Ok(Next::Wait);
        }
        // Admission and this transition share the same lock: a late submit
        // either joins this generation or elects its successor.
        self.running = false;
        self.owner = None;
        Ok(Next::Drained)
    }
    pub fn done(&mut self, generation: u64, owner: u32, run: u64, id: u64) -> Result<(), ()> {
        if !self.authorize(generation, owner, run)? {
            return Err(());
        }
        let index = self.active.iter().position(|job| job.id == id).ok_or(())?;
        self.active.remove(index);
        Ok(())
    }
}
pub fn valid_submission(bytes: &[u8]) -> bool {
    if bytes.len() > MAX_FRAME {
        return false;
    }
    let Some(split) = bytes.iter().position(|&b| b == 0) else {
        return false;
    };
    if split == 0 || split > 4096 || bytes.len() - split - 1 > 512 {
        return false;
    }
    let Ok(url) = core::str::from_utf8(&bytes[..split]) else {
        return false;
    };
    let Some(authority) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    !authority.is_empty()
        && !authority.starts_with('/')
        && !url.chars().any(|ch| ch.is_control() || ch.is_whitespace())
        && core::str::from_utf8(&bytes[split + 1..])
            .is_ok_and(|hint| !hint.chars().any(char::is_control))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn payload(n: usize) -> Vec<u8> {
        alloc::format!("https://example.test/{n}\0name").into_bytes()
    }
    fn worker(q: &mut Queue) -> u64 {
        let generation = q.begin().unwrap();
        q.started(generation, 7, 9);
        generation
    }
    fn job_id(next: Next) -> u64 {
        let Next::Job(bytes) = next else {
            panic!("expected job")
        };
        u64::from_le_bytes(bytes[1..9].try_into().unwrap())
    }
    #[test]
    fn arrivals_join_active_generation_and_retire_only_after_ack() {
        let mut q = Queue::new();
        let a = q.submit(&payload(1)).unwrap();
        let g = worker(&mut q);
        assert_eq!(job_id(q.next(g, 7, 9).unwrap()), a);
        assert_eq!(q.next(g, 7, 9), Ok(Next::Wait));
        let b = q.submit(&payload(2)).unwrap();
        assert_eq!(q.begin(), None);
        assert_eq!(job_id(q.next(g, 7, 9).unwrap()), b);
        q.done(g, 7, 9, a).unwrap();
        assert_eq!(q.next(g, 7, 9), Ok(Next::Wait));
        q.done(g, 7, 9, b).unwrap();
        assert_eq!(q.next(g, 7, 9), Ok(Next::Drained));
        q.submit(&payload(3)).unwrap();
        let new = worker(&mut q);
        assert_ne!(g, new);
        assert!(q.next(g, 7, 9).is_err());
        assert!(!q.failed(g));
        assert!(matches!(q.next(new, 7, 9), Ok(Next::Job(_))));
    }
    #[test]
    fn crash_requeues_unfinished_work_and_rejects_old_run() {
        let mut q = Queue::new();
        let a = q.submit(&payload(1)).unwrap();
        let b = q.submit(&payload(2)).unwrap();
        let g = worker(&mut q);
        q.next(g, 7, 9).unwrap();
        q.next(g, 7, 9).unwrap();
        q.done(g, 7, 9, a).unwrap();
        assert!(q.failed(g));
        let new = worker(&mut q);
        assert!(q.done(g, 7, 9, b).is_err());
        assert!(q.next(new, 7, 10).is_err());
        assert_eq!(job_id(q.next(new, 7, 9).unwrap()), b);
        q.done(new, 7, 9, b).unwrap();
        assert!(q.done(new, 7, 9, b).is_err());
    }
    #[test]
    fn bounded_outstanding_and_active_work() {
        let mut q = Queue::new();
        for n in 0..CAPACITY {
            q.submit(&payload(n)).unwrap();
        }
        assert!(q.submit(&payload(CAPACITY)).is_err());
        let g = worker(&mut q);
        let first = job_id(q.next(g, 7, 9).unwrap());
        for _ in 1..32 {
            q.next(g, 7, 9).unwrap();
        }
        assert_eq!(q.next(g, 7, 9), Ok(Next::Wait));
        q.done(g, 7, 9, first).unwrap();
        q.submit(&payload(CAPACITY)).unwrap();
        assert!(matches!(q.next(g, 7, 9), Ok(Next::Job(_))));
    }
    #[test]
    fn submission_racing_retirement_never_strands_accepted_work() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Mutex};
        let queue = Arc::new(Mutex::new(Queue::new()));
        let finished = Arc::new(AtomicBool::new(false));
        let producer_queue = queue.clone();
        let producer_finished = finished.clone();
        let producer = std::thread::spawn(move || {
            for n in 0..1000 {
                loop {
                    if producer_queue.lock().unwrap().submit(&payload(n)).is_ok() {
                        break;
                    }
                    std::thread::yield_now();
                }
                std::thread::yield_now();
            }
            producer_finished.store(true, Ordering::Release);
        });
        let mut completed = alloc::collections::BTreeSet::new();
        loop {
            let mut q = queue.lock().unwrap();
            if let Some(g) = q.begin() {
                q.started(g, 7, 9);
            }
            if q.running {
                let g = q.generation;
                match q.next(g, 7, 9).unwrap() {
                    Next::Job(bytes) => {
                        let id = u64::from_le_bytes(bytes[1..9].try_into().unwrap());
                        assert!(completed.insert(id));
                        q.done(g, 7, 9, id).unwrap();
                    }
                    Next::Wait | Next::Drained => {}
                }
            } else if finished.load(Ordering::Acquire) && q.pending.is_empty() {
                break;
            }
            drop(q);
            std::thread::yield_now();
        }
        producer.join().unwrap();
        assert_eq!(completed.len(), 1000);
    }
    #[test]
    fn rejects_bad_frames_and_startup_claim_is_not_lost() {
        for bytes in [
            &b"file:///etc/passwd\0"[..],
            b"https://x\nfoo\0",
            b"https://x\0a\0b",
            b"https://\0",
            b"https://x",
        ] {
            assert!(!valid_submission(bytes));
        }
        let mut q = Queue::new();
        q.submit(&payload(1)).unwrap();
        let g = q.begin().unwrap();
        assert_eq!(q.next(g, 7, 9), Ok(Next::Wait));
        q.started(g, 7, 9);
        assert!(q.next(g, 8, 9).is_err());
        assert!(matches!(q.next(g, 7, 9), Ok(Next::Job(_))));
    }
}
