#!/usr/bin/env python3
"""Exercise production network queue waits and targeted service wakeups on host."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]


def production_wait_queue():
    """Share the real wait registration with fixtures extracting NetQueue."""
    source = (ROOT / "src/wait.rs").read_text()
    definitions = "\n".join(item(source, signature) for signature in (
        "pub struct WaitQueue", "struct QueuedWaker", "struct WaitRegistration<'a>",
        "impl<'a> WaitRegistration<'a>", "impl Drop for WaitRegistration<'_>",
    ))
    implementation = "\n".join(item(source, signature) for signature in (
        "pub const fn new()", "pub fn observe(&self)",
        "pub async fn wait_after(&self", "pub fn notify_all(&self)",
    ))
    return r'''
mod allocators {
    pub fn with_host_alloc_domain_strong<T>(f: impl FnOnce() -> T) -> T { f() }
}
mod wait {
    use alloc::vec::Vec;
    use core::{sync::atomic::{AtomicU32, Ordering}, task::{Context, Poll, Waker}};
    use spin::Mutex;
    @DEFINITIONS@
    impl WaitQueue { @IMPLEMENTATION@ }
}
'''.replace("@DEFINITIONS@", definitions).replace("@IMPLEMENTATION@", implementation)


HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
use alloc::{boxed::Box, collections::VecDeque, vec::Vec};
use core::{future::Future, sync::atomic::{AtomicU32, AtomicUsize, Ordering}, task::{Context, Poll}};
use std::{sync::Arc, task::{Wake, Waker}};
@WAIT@
@QUEUE@
const MAX_NET_DEVICES: usize = 3;
const NET_SERVICE_SLEEP_US: u64 = 100;
pub enum NetCommand { Data }
pub enum NetEvent { Ready }
@APP_QUEUES@
static APP_QUEUES: spin::Mutex<Vec<AppQueues>> = spin::Mutex::new(Vec::new());
@REGISTER@
@SERVICE_WAKE@
@WAKE_SERVICE@
@SERVICE_WAIT@
@OWNER_DEVICE@
mod net {
    use super::*;
    pub static PRIMARY: AtomicUsize = AtomicUsize::new(0);
    pub fn primary_device_index() -> usize { PRIMARY.load(Ordering::SeqCst) }
    pub fn device_index_from_owner(owner: &str) -> Option<usize> {
        owner.rsplit_once('@').and_then(|(_, suffix)| suffix.parse().ok())
    }
}
struct EmbassyDuration;
impl EmbassyDuration { fn from_micros(_: u64) -> Self { Self } }
static MAINTENANCE_DUE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
struct Timer;
impl Timer { fn after(_: EmbassyDuration) -> Self { Self } }
impl Future for Timer {
    type Output = ();
    fn poll(self: core::pin::Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if MAINTENANCE_DUE.load(Ordering::SeqCst) { Poll::Ready(()) } else { Poll::Pending }
    }
}
fn poll<F: Future>(future: core::pin::Pin<&mut F>, waker: &Waker) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(waker))
}
struct QueueWake { queue: &'static NetQueue<u32>, calls: AtomicUsize }
impl Wake for QueueWake {
    fn wake(self: Arc<Self>) {
        assert!(self.queue.inner.try_lock().is_some(), "queue lock held during wake");
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}
fn counting(queue: &'static NetQueue<u32>) -> (Arc<QueueWake>, Waker) {
    let count = Arc::new(QueueWake { queue, calls: AtomicUsize::new(0) });
    let waker = Waker::from(count.clone());
    (count, waker)
}
#[test]
fn consumer_cannot_miss_publication_between_empty_check_and_registration() {
    let queue = NetQueue::new_leaked("consumer-race", 1);
    let observed = queue.change_generation();
    assert!(queue.is_empty());
    queue.push(7).unwrap();
    let mut waiting = std::pin::pin!(queue.wait_change_after(observed));
    assert!(poll(waiting.as_mut(), Waker::noop()).is_ready());
    assert_eq!(queue.pop(), Some(7));
}
#[test]
fn registered_consumer_wakes_after_queue_lock_is_released() {
    let queue = NetQueue::new_leaked("consumer", 1);
    let (count, waker) = counting(queue);
    let observed = queue.change_generation();
    let mut waiting = std::pin::pin!(queue.wait_change_after(observed));
    assert!(poll(waiting.as_mut(), &waker).is_pending());
    queue.push(8).unwrap();
    assert_eq!(count.calls.load(Ordering::SeqCst), 1);
    assert!(poll(waiting.as_mut(), &waker).is_ready());
    assert_eq!(queue.pop(), Some(8));
}
#[test]
fn full_queue_returns_owned_item_and_pop_before_wait_registration_releases_producer() {
    let queue = NetQueue::new_leaked("producer-race", 1);
    queue.push(Box::new(10)).unwrap();
    let item = Box::new(11);
    let address = (&*item) as *const i32;
    let observed = queue.change_generation();
    let item = queue.try_push(item).unwrap_err();
    assert_eq!((&*item) as *const i32, address);
    assert_eq!(queue.pop().map(|value| *value), Some(10));
    let mut waiting = std::pin::pin!(queue.wait_change_after(observed));
    assert!(poll(waiting.as_mut(), Waker::noop()).is_ready());
    queue.try_push(item).unwrap();
    assert_eq!(queue.pop().map(|value| *value), Some(11));
}
#[test]
fn drain_releases_registered_producer_without_waking_for_failed_or_empty_operations() {
    let queue = NetQueue::new_leaked("producer", 2);
    queue.push(1).unwrap(); queue.push(2).unwrap();
    let (count, waker) = counting(queue);
    let observed = queue.change_generation();
    let mut waiting = std::pin::pin!(queue.wait_change_after(observed));
    assert!(poll(waiting.as_mut(), &waker).is_pending());
    assert_eq!(queue.try_push(3), Err(3));
    assert!(queue.drain(0).is_empty());
    assert_eq!(queue.change_generation(), observed);
    assert_eq!(count.calls.load(Ordering::SeqCst), 0);
    assert_eq!(queue.drain(2), vec![1, 2]);
    assert_eq!(count.calls.load(Ordering::SeqCst), 1);
    assert!(poll(waiting.as_mut(), &waker).is_ready());
    let empty = queue.change_generation();
    assert_eq!(queue.pop(), None);
    assert!(queue.drain(4).is_empty());
    assert_eq!(queue.change_generation(), empty);
}
#[test]
fn cancelling_an_old_wait_keeps_a_new_wait_from_the_same_task_registered() {
    let queue = NetQueue::new_leaked("cancelled", 1);
    let (count, waker) = counting(queue);
    let observed = queue.change_generation();
    let mut old = Box::pin(queue.wait_change_after(observed));
    let mut current = Box::pin(queue.wait_change_after(observed));
    assert!(poll(old.as_mut(), &waker).is_pending());
    assert!(poll(current.as_mut(), &waker).is_pending());
    drop(old);
    queue.push(9).unwrap();
    assert_eq!(count.calls.load(Ordering::SeqCst), 1);
    assert!(poll(current.as_mut(), &waker).is_ready());
}
#[test]
fn registered_commands_wake_only_the_current_target_service_and_interrupt_maintenance_wait() {
    APP_QUEUES.lock().clear();
    MAINTENANCE_DUE.store(false, Ordering::SeqCst);
    net::PRIMARY.store(0, Ordering::SeqCst);
    let commands = NetQueue::new_leaked("commands", 2);
    let events = NetQueue::new_leaked("events", 2);
    register_app_queues("queue-wake-test", commands, events);
    let before = NET_SERVICE_WAKE.each_ref().map(|wake| wake.observe());
    let mut waiting = std::pin::pin!(wait_for_service_work(0, before[0]));
    assert!(poll(waiting.as_mut(), Waker::noop()).is_pending());
    events.push(NetEvent::Ready).ok().unwrap();
    assert_eq!(NET_SERVICE_WAKE.each_ref().map(|wake| wake.observe()), before);
    commands.push(NetCommand::Data).ok().unwrap();
    assert!(poll(waiting.as_mut(), Waker::noop()).is_ready(), "waited for maintenance timer");
    let after = NET_SERVICE_WAKE.each_ref().map(|wake| wake.observe());
    assert_ne!(after[0], before[0]);
    assert_eq!(&after[1..], &before[1..]);
    commands.pop();
    assert_eq!(NET_SERVICE_WAKE.each_ref().map(|wake| wake.observe()), after);
    net::PRIMARY.store(2, Ordering::SeqCst);
    commands.push(NetCommand::Data).ok().unwrap();
    let moved = NET_SERVICE_WAKE.each_ref().map(|wake| wake.observe());
    assert_eq!(&moved[..2], &after[..2]);
    assert_ne!(moved[2], after[2]);
    APP_QUEUES.lock().clear();
}
#[test]
fn registration_catches_existing_commands_and_explicit_nic_ignores_primary_changes() {
    APP_QUEUES.lock().clear();
    net::PRIMARY.store(2, Ordering::SeqCst);
    let commands = NetQueue::new_leaked("preexisting", 1);
    let events = NetQueue::new_leaked("preexisting-events", 1);
    commands.push(NetCommand::Data).ok().unwrap();
    let before = NET_SERVICE_WAKE.each_ref().map(|wake| wake.observe());
    register_app_queues("queue-wake-explicit@1", commands, events);
    let after = NET_SERVICE_WAKE.each_ref().map(|wake| wake.observe());
    assert_ne!(after[1], before[1]);
    assert_eq!((after[0], after[2]), (before[0], before[2]));
    commands.pop();
    net::PRIMARY.store(0, Ordering::SeqCst);
    commands.push(NetCommand::Data).ok().unwrap();
    let republished = NET_SERVICE_WAKE.each_ref().map(|wake| wake.observe());
    assert_ne!(republished[1], after[1]);
    assert_eq!((republished[0], republished[2]), (after[0], after[2]));
    APP_QUEUES.lock().clear();
}
#[test]
fn service_wait_still_runs_maintenance_without_new_commands() {
    MAINTENANCE_DUE.store(false, Ordering::SeqCst);
    let mut waiting = std::pin::pin!(wait_for_service_work(1, NET_SERVICE_WAKE[1].observe()));
    assert!(poll(waiting.as_mut(), Waker::noop()).is_pending());
    MAINTENANCE_DUE.store(true, Ordering::SeqCst);
    assert!(poll(waiting.as_mut(), Waker::noop()).is_ready());
    MAINTENANCE_DUE.store(false, Ordering::SeqCst);
}
'''


def main():
    source = (ROOT / "src/net/adapter.rs").read_text()
    substitutions = {
        "WAIT": production_wait_queue(),
        "QUEUE": "\n".join(item(source, signature) for signature in (
            "pub struct NetQueue<T>", "impl<T> NetQueue<T>",
        )),
        "APP_QUEUES": item(source, "struct AppQueues"),
        "REGISTER": item(source, "pub fn register_app_queues("),
        "SERVICE_WAKE": re.search(
            r"static NET_SERVICE_WAKE:[\s\S]*?;\n", source)[0],
        "WAKE_SERVICE": item(source, "fn wake_service_for_owner("),
        "SERVICE_WAIT": item(source, "async fn wait_for_service_work("),
        "OWNER_DEVICE": item(source, "fn owner_device_index("),
    }
    harness = HARNESS
    for key, value in substitutions.items():
        harness = harness.replace("@" + key + "@", value)
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-net-queue-wake-") as directory:
        directory = Path(directory)
        (directory / "lib.rs").write_text(harness)
        (directory / "Cargo.toml").write_text(
            '[package]\nname="net-queue-wake-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nspin="0.10"\n')
        subprocess.run([
            "cargo", "+" + channel, "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
            "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(ROOT / "bld/test-cache/net-queue-wake"),
            "--", "--test-threads=1",
        ], cwd=directory, check=True, timeout=120)


if __name__ == "__main__":
    main()
