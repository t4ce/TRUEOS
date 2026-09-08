#!/usr/bin/env python3
"""Exercise the actual video cold-index wait without giving playback disk I/O."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_clip_position3_uv_texture import ROOT

source = (ROOT / 'src/intel/media/hw_vid.rs').read_text()
wait = re.search(r'^async fn h264_wait_for_fs_index\(.*?^}\n', source, re.M | re.S)
assert wait, 'production index wait missing'
harness = r'''
#![allow(dead_code)]
use std::{future::Future, sync::atomic::{AtomicBool, AtomicUsize, Ordering}, task::{Context, Poll, Waker}};
static CANCELLED: AtomicBool = AtomicBool::new(false);
static READY: AtomicBool = AtomicBool::new(false);
static PRESENT: AtomicBool = AtomicBool::new(true);
static WARM_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static TICKS: AtomicUsize = AtomicUsize::new(0);
mod disc { pub mod block {
    #[derive(Copy, Clone)] pub struct DeviceHandle;
    impl DeviceHandle { pub fn id(self) -> u8 { 1 } }
}}
mod ui4 {
    #[derive(Copy, Clone)] pub struct VideoPlaybackSession;
    impl VideoPlaybackSession { pub fn is_cancelled(self) -> bool { super::CANCELLED.load(super::Ordering::Relaxed) } }
}
mod r { pub mod fs { pub mod trueosfs {
    pub struct Root { pub disk_id: u8, pub index_ready: bool }
    pub fn request_warm_index(_: u8) { crate::WARM_REQUESTS.fetch_add(1, crate::Ordering::Relaxed); }
    pub fn list_roots() -> Vec<Root> {
        if crate::PRESENT.load(crate::Ordering::Relaxed) {
            vec![Root { disk_id: 1, index_ready: crate::READY.load(crate::Ordering::Relaxed) }]
        } else { vec![] }
    }
}}}
struct Timer;
impl Timer {
    async fn after_millis(ms: u64) {
        assert_eq!(ms, 5);
        TICKS.fetch_add(1, Ordering::Relaxed);
        CANCELLED.store(true, Ordering::Relaxed); // User closes while the root is still cold.
    }
}
fn run<T>(future: impl Future<Output=T>) -> T {
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value, Poll::Pending => panic!("unexpected unbounded wait"),
    }
}
fn reset() {
    CANCELLED.store(false, Ordering::Relaxed); READY.store(false, Ordering::Relaxed);
    PRESENT.store(true, Ordering::Relaxed); WARM_REQUESTS.store(0, Ordering::Relaxed);
    TICKS.store(0, Ordering::Relaxed);
}
#[test] fn cold_root_can_cancel_without_waiting_for_index_replay() {
    reset();
    assert_eq!(run(h264_wait_for_fs_index(ui4::VideoPlaybackSession, disc::block::DeviceHandle)), Err("playback cancelled"));
    assert!(!READY.load(Ordering::Relaxed));
    assert_eq!(WARM_REQUESTS.load(Ordering::Relaxed), 1);
    assert_eq!(TICKS.load(Ordering::Relaxed), 1);
}
#[test] fn ready_root_admits_immediately_and_removed_root_fails() {
    reset(); READY.store(true, Ordering::Relaxed);
    assert_eq!(run(h264_wait_for_fs_index(ui4::VideoPlaybackSession, disc::block::DeviceHandle)), Ok(()));
    assert_eq!(TICKS.load(Ordering::Relaxed), 0);
    PRESENT.store(false, Ordering::Relaxed);
    assert_eq!(run(h264_wait_for_fs_index(ui4::VideoPlaybackSession, disc::block::DeviceHandle)), Err("TRUEOSFS video root disappeared"));
}
'''
with tempfile.TemporaryDirectory(prefix='trueos-video-loading-') as tmp:
    src = Path(tmp) / 'test.rs'; binary = Path(tmp) / 'test'
    src.write_text(harness + wait.group())
    subprocess.run(['rustc', '--edition=2024', '--test', str(src), '-o', str(binary)], check=True)
    subprocess.run([str(binary), '--test-threads=1'], check=True)
