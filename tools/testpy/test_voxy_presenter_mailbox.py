#!/usr/bin/env python3
"""Exercise the production presenter mailbox without linking GPU/client code.

Mailbox, Producer teardown and activity counters come from Voxy. Only the job
payload is substituted, so tests can observe resource ownership and destruction.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
VOXY = ROOT.parent / "voxy"
BLUEPRINTS = ROOT.parent / "TRUEOS-Blueprints"

TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::atomic::AtomicUsize, time::Instant};

    fn job(revision: u64) -> Job { Job { revision, on_drop: None } }

    #[test]
    fn pending_frames_coalesce_to_the_latest_revision() {
        let mailbox = Mailbox::default();
        for revision in 1..=10_000 { mailbox.submit(job(revision)); }
        assert_eq!(mailbox.take().unwrap().revision, 10_000);
        assert!(mailbox.take().is_none());
        assert_eq!(mailbox.counters.take().queued_replacements, 9_999);
    }

    #[test]
    fn a_claimed_frame_keeps_its_resources_while_pending_frames_are_replaced() {
        let mailbox = Mailbox::default();
        let dropped = Arc::new(AtomicUsize::new(0));
        let observed = dropped.clone();
        mailbox.submit(Job {
            revision: 1,
            on_drop: Some(Box::new(move || { observed.fetch_add(1, Ordering::Relaxed); })),
        });
        let current = mailbox.take().unwrap();
        mailbox.submit(job(2));
        mailbox.submit(job(3));
        assert_eq!(current.revision, 1);
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
        assert_eq!(mailbox.take().unwrap().revision, 3);
        drop(current);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn displaced_resources_can_reenter_the_mailbox() {
        let mailbox = Arc::new(Mailbox::default());
        let weak = Arc::downgrade(&mailbox);
        mailbox.submit(Job {
            revision: 1,
            on_drop: Some(Box::new(move || { weak.upgrade().unwrap().submit(job(3)); })),
        });
        mailbox.submit(job(2));
        assert_eq!(mailbox.take().unwrap().revision, 3);
        assert_eq!(mailbox.counters.take().queued_replacements, 2);
    }

    #[test]
    fn concurrent_submission_and_consumption_preserve_order_and_drop_every_job() {
        const JOBS: usize = 50_000;
        let mailbox = Arc::new(Mailbox::default());
        let done = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicUsize::new(0));
        let worker_mailbox = mailbox.clone();
        let worker_done = done.clone();
        let consumer = thread::spawn(move || {
            let mut revisions = Vec::new();
            loop {
                if let Some(current) = worker_mailbox.take() {
                    revisions.push(current.revision);
                } else if worker_done.load(Ordering::Acquire) {
                    // A submission can precede the done flag after an empty pop.
                    if let Some(current) = worker_mailbox.take() {
                        revisions.push(current.revision);
                    }
                    break;
                } else {
                    thread::yield_now();
                }
            }
            revisions
        });
        for revision in 1..=JOBS {
            let observed = dropped.clone();
            mailbox.submit(Job {
                revision: revision as u64,
                on_drop: Some(Box::new(move || { observed.fetch_add(1, Ordering::Relaxed); })),
            });
        }
        done.store(true, Ordering::Release);
        let revisions = consumer.join().unwrap();
        assert_eq!(revisions.last(), Some(&(JOBS as u64)));
        assert!(revisions.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(dropped.load(Ordering::Relaxed), JOBS);
        assert_eq!(revisions.len() as u64 + mailbox.counters.take().queued_replacements, JOBS as u64);
    }

    #[test]
    fn producer_drop_stops_and_joins_an_idle_worker_without_notification() {
        let mailbox = Arc::new(Mailbox::default());
        let worker_mailbox = mailbox.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (exit_tx, exit_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            while !worker_mailbox.stopped.load(Ordering::Acquire) { worker_mailbox.wait(); }
            exit_tx.send(()).unwrap();
        });
        let producer = Producer { mailbox: mailbox.clone(), thread: Some(worker) };
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        // Confirm the thread can leave its timed sleep with no mailbox wake.
        let teardown = thread::spawn(move || drop(producer));
        exit_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        teardown.join().unwrap();
        let start = Instant::now();
        mailbox.wait();
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
'''


def main():
    source = (VOXY / "src/ui/ice/renderer/presenter.rs").read_text()
    mailbox_and_producer = source[source.index("struct Mailbox {"):
                                  source.index("pub(crate) struct LayeredPresenter {")]
    with tempfile.TemporaryDirectory(prefix="voxy-mailbox-") as directory:
        folder = Path(directory)
        (folder / "src").mkdir()
        (folder / "Cargo.toml").write_text(
            '[package]\nname="voxy-mailbox-tests"\nversion="0.1.0"\nedition="2024"\n'
            '[dependencies]\n'
            f'crossbeam-queue={{path="{VOXY / "vendor/crossbeam-queue-0.3.12"}"}}\n'
            '[patch.crates-io]\n'
            f'crossbeam-utils={{path="{BLUEPRINTS / "vendor/crossbeam-utils-0.8.21"}"}}\n'
            '[workspace]\n'
        )
        (folder / "src/lib.rs").write_text(
            '#![allow(dead_code, unused_imports)]\nuse crossbeam_queue::ArrayQueue;\n'
            'use std::{sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}, mpsc}, '
            'thread::{self, JoinHandle}, time::Duration};\n'
            f'#[path="{VOXY / "src/ui/ice/renderer/activity.rs"}"] mod activity;\n'
            'use activity::ProducerCounters;\n'
            'struct Job { revision: u64, on_drop: Option<Box<dyn FnOnce() + Send>> }\n'
            'impl Drop for Job { fn drop(&mut self) { '
            'if let Some(action) = self.on_drop.take() { action(); } } }\n'
            + mailbox_and_producer + TESTS
        )
        subprocess.run([
            "cargo", "test", "--offline", "--target", "x86_64-unknown-linux-gnu",
            "--target-dir", str(ROOT / "bld/voxy-mailbox-host-tests"),
        ], cwd=folder, check=True, timeout=120)


if __name__ == "__main__":
    main()
