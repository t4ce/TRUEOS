#!/usr/bin/env python3
"""Exercise the production timer driver and compare optimized poll IR."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = r'''
#![allow(dead_code, unused_variables)]
extern crate self as trueos_executor;
pub mod raw { pub fn timer_slack_ticks_for_waker(_: &core::task::Waker) -> u64 { 0 } }
#[macro_export] macro_rules! log { ($($arg:tt)*) => {}; }
mod log_os { pub mod flags { pub const BOOT_INFO_LOGS: bool = false; } }
mod limine { pub fn boot_timestamp_secs() -> Option<u64> { None } }
mod efi { pub mod acpi { pub mod hpet {
    pub struct Hpet;
    pub fn ensure() -> Option<&'static Hpet> { None }
    impl Hpet {
        pub fn frequency_hz(&self) -> u64 { 0 }
        pub fn main_counter(&self) -> u64 { 0 }
        pub fn counter_delta(&self, a: u64, b: u64) -> u64 { b.wrapping_sub(a) }
    }
} } }
mod driver {
SOURCE
#[cfg(test)] mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex, atomic::{AtomicUsize, Ordering}};
    use std::task::Wake;
    struct Probe { id: usize, events: Arc<StdMutex<std::vec::Vec<usize>>>, drops: Arc<AtomicUsize>, reentrant: bool }
    impl Drop for Probe { fn drop(&mut self) { self.drops.fetch_add(1, Ordering::SeqCst); } }
    impl Wake for Probe {
        fn wake(self: Arc<Self>) {
            self.events.lock().unwrap().push(self.id);
            if self.reentrant {
                // Reacquiring QUEUE proves wake is outside its lock. The new
                // future entry must survive this poll's drain.
                let w = Waker::from(self.clone());
                DRIVER.schedule_wake(DRIVER.now() + 1_000_000, &w);
            }
        }
    }
    #[test] fn actual_driver_due_fifo_retained_reentrant_and_exact_drop() {
        let events = Arc::new(StdMutex::new(std::vec::Vec::new()));
        let drops = Arc::new(AtomicUsize::new(0));
        let make = |id, reentrant| Arc::new(Probe { id, reentrant, events: events.clone(), drops: drops.clone() });
        let a = make(1, false); let b = make(2, true); let retained = make(3, false);
        let wa = Waker::from(a.clone()); let wb = Waker::from(b.clone()); let wr = Waker::from(retained.clone());
        let future = DRIVER.now() + 1_000_000;
        DRIVER.schedule_wake(future, &wa); DRIVER.schedule_wake(future, &wb); DRIVER.schedule_wake(future + 100, &wr);
        assert_eq!(QUEUE.lock().len(), 3);
        { let mut queue = QUEUE.lock(); queue[0].at = 0; queue[1].at = 0; }
        poll();
        assert_eq!(*events.lock().unwrap(), [1, 2]);
        assert_eq!(QUEUE.lock().len(), 2);
        assert_eq!(Arc::strong_count(&a), 2, "due waker consumed exactly once");
        assert_eq!(Arc::strong_count(&b), 3, "reentrant schedule retained once");
        assert_eq!(Arc::strong_count(&retained), 3);
        poll();
        assert_eq!(*events.lock().unwrap(), [1, 2], "future entries not prematurely woken");
        { let mut queue = QUEUE.lock(); queue.clear(); }
        assert_eq!(Arc::strong_count(&b), 2);
        assert_eq!(Arc::strong_count(&retained), 2);
        drop((wa, wb, wr, a, b, retained));
        assert_eq!(drops.load(Ordering::SeqCst), 3);
        assert!(QUEUE.lock().is_empty());
    }
    #[test] fn same_task_keeps_earliest_deadline_and_one_owned_waker() {
        QUEUE.lock().clear();
        let events = Arc::new(StdMutex::new(std::vec::Vec::new()));
        let drops = Arc::new(AtomicUsize::new(0));
        let a = Arc::new(Probe { id: 31, events: events.clone(), drops: drops.clone(), reentrant: false });
        let b = Arc::new(Probe { id: 32, events: events.clone(), drops: drops.clone(), reentrant: false });
        let aw = Waker::from(a.clone()); let bw = Waker::from(b.clone());
        let base = DRIVER.now() + 60 * TICK_HZ;
        DRIVER.schedule_wake(base + 300, &aw);
        DRIVER.schedule_wake(base + 200, &bw);
        for _ in 0..128 { DRIVER.schedule_wake(base + 400, &aw); }
        assert_eq!(QUEUE.lock().len(), 2);
        assert_eq!(Arc::strong_count(&a), 3, "one queue-owned waker regardless of polls");
        DRIVER.schedule_wake(base + 100, &aw);
        { let queue = QUEUE.lock();
          assert_eq!(queue[0].at, base + 100, "earlier deadline moves ahead of independent timer");
          assert_eq!(queue[1].at, base + 200);
          assert!(queue[0].waker.will_wake(&aw));
        }
        DRIVER.schedule_wake(base + 500, &aw);
        assert_eq!(QUEUE.lock()[0].at, base + 100, "later request cannot extend earliest wake");
        for entry in QUEUE.lock().iter_mut() { entry.at = 0; }
        poll();
        assert_eq!(*events.lock().unwrap(), [31, 32], "dedup preserves chronological ordering");
        assert_eq!(Arc::strong_count(&a), 2);
        assert_eq!(Arc::strong_count(&b), 2);
        drop((aw, bw, a, b));
        assert_eq!(drops.load(Ordering::Relaxed), 2);
    }
    #[test] fn concurrent_independent_tasks_keep_one_registration_each() {
        QUEUE.lock().clear();
        let events = Arc::new(StdMutex::new(std::vec::Vec::new()));
        let drops = Arc::new(AtomicUsize::new(0));
        let wakers: std::vec::Vec<_> = (0..8).map(|id| Waker::from(Arc::new(Probe {
            id, events: events.clone(), drops: drops.clone(), reentrant: false,
        }))).collect();
        let base = DRIVER.now() + 60 * TICK_HZ;
        std::thread::scope(|scope| {
            for (id, waker) in wakers.iter().enumerate() {
                let waker = waker.clone();
                scope.spawn(move || {
                    for _ in 0..256 { DRIVER.schedule_wake(base + id as u64, &waker); }
                });
            }
        });
        assert_eq!(QUEUE.lock().len(), 8, "independent tasks retain independent slots");
        for entry in QUEUE.lock().iter_mut() { entry.at = 0; }
        poll();
        assert_eq!(*events.lock().unwrap(), (0..8).collect::<std::vec::Vec<_>>());
        drop(wakers);
        assert_eq!(drops.load(Ordering::Relaxed), 8, "all queue ownership released");
    }
    #[test] fn repeated_poll_of_one_timer_cannot_evict_an_independent_timer() {
        QUEUE.lock().clear();
        let events = Arc::new(StdMutex::new(std::vec::Vec::new()));
        let drops = Arc::new(AtomicUsize::new(0));
        let first = Arc::new(Probe { id: 10, events: events.clone(), drops: drops.clone(), reentrant: false });
        let later = Arc::new(Probe { id: 20, events: events.clone(), drops: drops.clone(), reentrant: false });
        let first_waker = Waker::from(first);
        let later_waker = Waker::from(later);
        let deadline = DRIVER.now() + 60 * TICK_HZ;
        // A pending Timer may be polled repeatedly before its deadline. One
        // logical registration must not consume the entire bounded queue.
        for _ in 0..MAX_WAKEUPS + 1 { DRIVER.schedule_wake(deadline, &first_waker); }
        DRIVER.schedule_wake(deadline + 1, &later_waker);
        for entry in QUEUE.lock().iter_mut() { entry.at = 0; }
        poll();
        let later_wakes = events.lock().unwrap().iter().filter(|&&id| id == 20).count();
        assert_eq!(events.lock().unwrap().len(), 2, "one wake per task is sufficient to repoll its timers");
        assert_eq!(later_wakes, 1, "duplicate registrations evicted an independent timer wake");
    }
}
}
#[unsafe(no_mangle)] pub fn production_timer_poll() { driver::poll(); }
'''

def run(*args, cwd):
    subprocess.run(args, cwd=cwd, check=True, timeout=180)


def main():
    source = (ROOT / 'src/r/time.rs').read_text()
    assert 'for w in to_wake.drain(..)' in source
    with tempfile.TemporaryDirectory(prefix='trueos-timer-wake-') as directory:
        work = Path(directory)
        (work / 'Cargo.toml').write_text('''[package]
name="timer-wake-contract"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
spin="0.10"
heapless="=0.9.3"
raw-cpuid="11"
embassy-time-driver={version="0.2.2",features=["tick-hz-1_000"]}
''')
        (work / 'src').mkdir()
        lib = work / 'src/lib.rs'
        lib.write_text(FIXTURE.replace('SOURCE', source))
        run('cargo', 'test', '--offline', '--release', '--', '--test-threads=1', cwd=work)
        copies = {}
        for variant in ['before', 'after']:
            code = source if variant == 'after' else source.replace('for w in to_wake.drain(..)', 'for w in to_wake')
            lib.write_text(FIXTURE.replace('SOURCE', code))
            run('cargo', 'rustc', '--offline', '--release', '--lib', '--', '--emit=llvm-ir', cwd=work)
            ir = next((work / 'target/release/deps').glob('timer_wake_contract-*.ll')).read_text()
            # Capacity-sized memcpy cannot be mistaken for smaller waker moves.
            copies[variant] = bool(re.search(r'@llvm\.memcpy[^\n]*i64 (?:noundef )?144008', ir))
        assert copies == {'before': True, 'after': False}, copies
        print('optimized IR: capacity-sized memcpy present before, absent after')


if __name__ == '__main__':
    main()
