#!/usr/bin/env python3
"""Exercise the production async WD snapshot with scripted allocation and BCS."""
from pathlib import Path
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT
BLT = 'src/intel/copy/blt.rs'


def main():
    source = r'''#![allow(dead_code, unused_variables)]
use std::future::Future;
extern crate self as spin;
extern crate self as trueos_time;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
#[macro_export] macro_rules! log_info { ($($tokens:tt)*) => {}; }
#[macro_export] macro_rules! log_warn { ($($tokens:tt)*) => {}; }
#[macro_export] macro_rules! log_error { ($($tokens:tt)*) => {}; }
pub struct Duration;
impl Duration { pub fn from_millis(_: u64) -> Self { Self } }
pub struct Timer { yielded: bool }
impl Timer { pub fn after(_: Duration) -> Self { Self { yielded: false } } }
impl std::future::Future for Timer {
    type Output = ();
    fn poll(mut self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<()> {
        if self.yielded { std::task::Poll::Ready(()) }
        else { self.yielded = true; std::task::Poll::Pending }
    }
}
static NOW: AtomicU64 = AtomicU64::new(0);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static FREES: AtomicUsize = AtomicUsize::new(0);
static SOURCE_PINNED: AtomicBool = AtomicBool::new(false);
static SOURCE_QUARANTINED: AtomicBool = AtomicBool::new(false);
static ALLOC_FAIL: AtomicBool = AtomicBool::new(false);
static EVENTS: Mutex<Vec<&str>> = Mutex::new(Vec::new());
mod chronos {
    pub fn monotonic_nanos() -> u64 { super::NOW.fetch_add(1_000_000, super::Ordering::SeqCst) }
}
mod dma {
    pub fn alloc_with_max(bytes: usize, align: usize, _: Option<u64>) -> Option<(u64, *mut u8)> {
        if super::ALLOC_FAIL.load(super::Ordering::SeqCst) { return None; }
        let layout = std::alloc::Layout::from_size_align(bytes, align).unwrap();
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        super::ALLOCATIONS.fetch_add(1, super::Ordering::SeqCst);
        Some((ptr as u64, ptr))
    }
    pub fn dealloc(ptr: *mut u8, bytes: usize) {
        unsafe { std::alloc::dealloc(ptr, std::alloc::Layout::from_size_align(bytes, 4096).unwrap()) };
        super::FREES.fetch_add(1, super::Ordering::SeqCst);
    }
}
mod intel {
    pub const WARM_ALIGN: usize = 4096;
    pub fn dma_flush(_: *mut u8, _: usize) { super::EVENTS.lock().push("cpu-cache-boundary"); }
    pub struct Lease { submitted: bool }
    impl Lease {
        pub fn mark_submitted(&mut self) { self.submitted = true; }
        pub fn mark_retired(&mut self) { self.submitted = false; }
    }
    impl Drop for Lease {
        fn drop(&mut self) {
            if self.submitted { super::SOURCE_QUARANTINED.store(true, super::Ordering::SeqCst); }
            else { super::SOURCE_PINNED.store(false, super::Ordering::SeqCst); }
        }
    }
    pub fn pin_ui4_wd_frame_for_copy(_: u64, _: u64) -> Result<Lease, ()> {
        assert!(!super::SOURCE_PINNED.swap(true, super::Ordering::SeqCst));
        Ok(Lease { submitted: false })
    }
    pub struct WdXyuv8888Frame {
        pub width: u32, pub height: u32, pub phys: u64, pub virt: *const u8,
        pub byte_len: usize, pub pitch_bytes: u32, pub sequence: u64,
    }
'''
    source += '\n'.join(extract.item(BLT, name) for name in (
        'GucBcs0RgbaSurface', 'GucBcs0RgbaCopy', 'GucBcs0CopySubmitError', 'GucBcs0CopyCompletion',
    ))
    source += '''
    pub mod media {
        pub mod avc_encode_probe {
            pub const FRAME_WIDTH: usize = 2560;
            pub const FRAME_HEIGHT: usize = 1440;
            pub struct AvcXyuv8888DmaSurface;
            impl AvcXyuv8888DmaSurface { pub fn new(_: u64, _: usize) -> Option<Self> { Some(Self) } }
        }
'''
    source += f'#[path = "{ROOT / "src/intel/media/wd_xyuv8888.rs"}"] pub mod wd_xyuv8888;\n}}}}\n'
    source += r'''
use intel::{GucBcs0CopySubmitError as Error, GucBcs0CopyCompletion as Completion};
static QUEUE: Mutex<Vec<Result<u32, Error>>> = Mutex::new(Vec::new());
static POLL: Mutex<Vec<Completion>> = Mutex::new(Vec::new());
static DESTINATION: AtomicUsize = AtomicUsize::new(0);
mod r { pub mod services { pub mod vcpy_service {
    pub fn queue_uncached_copies(dst: crate::intel::GucBcs0RgbaSurface, copies: &[crate::intel::GucBcs0RgbaCopy]) -> Result<u32, crate::Error> {
        assert!(crate::SOURCE_PINNED.load(crate::Ordering::SeqCst));
        assert_eq!(dst.gpu, 0x31000000);
        assert_eq!(copies[0].source.gpu, 0x30000000);
        assert_eq!(copies[0].width, 2560);
        assert_eq!(copies[0].height, 1440);
        assert_eq!(copies[0].source.pitch_bytes, 10240);
        crate::DESTINATION.store(dst.phys as usize, crate::Ordering::SeqCst);
        crate::EVENTS.lock().push("submit");
        crate::QUEUE.lock().remove(0)
    }
    pub fn poll_rgba_copies(_: u32) -> crate::Completion {
        assert!(crate::SOURCE_PINNED.load(crate::Ordering::SeqCst));
        let result = crate::POLL.lock().remove(0);
        if result == crate::Completion::Complete {
            unsafe { std::ptr::write_bytes(crate::DESTINATION.load(crate::Ordering::SeqCst) as *mut u8, 0xa5, 2560*1440*4) };
            crate::EVENTS.lock().push("retire");
        }
        result
    }
}}}
fn run<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    for _ in 0..1000 {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) { return value; }
    }
    panic!("unbounded wait")
}
#[test]
fn owned_snapshot_is_one_shot_and_cancellation_never_releases_inflight_dma() {
    use intel::media::wd_xyuv8888::*;
    let source = unsafe { WdXyuv8888DmaSurface::new(0x100000, 0x100000 as *const u8,
        WD_XYUV8888_BYTES, WD_XYUV8888_PITCH, 9).unwrap() };
    assert!(!run(try_refresh_requested_screenshot(source)));
    assert_eq!(ALLOCATIONS.load(Ordering::SeqCst), 0);
    request_screenshot().unwrap();
    assert!(request_screenshot().is_err());
    *QUEUE.lock() = vec![Err(Error::Busy), Ok(1)];
    *POLL.lock() = vec![Completion::Pending, Completion::Complete];
    let mut future = Box::pin(try_refresh_requested_screenshot(source));
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(future.as_mut().poll(&mut cx).is_pending());
    assert!(SOURCE_PINNED.load(Ordering::SeqCst));
    assert!(with_screenshot(|_, _| ()).is_none());
    assert!(run(future));
    assert!(!SOURCE_PINNED.load(Ordering::SeqCst));
    assert_eq!(*EVENTS.lock(), ["cpu-cache-boundary", "submit", "submit", "retire", "cpu-cache-boundary"]);
    assert!(request_screenshot().is_err()); // Ready may not be overwritten.
    assert_eq!(with_screenshot(|sequence, bytes| {
        assert_eq!(sequence, 9);
        assert!(bytes.iter().all(|b| *b == 0xa5));
        assert_eq!(FREES.load(Ordering::SeqCst), 0);
        bytes.len()
    }), Some(WD_XYUV8888_BYTES));
    assert_eq!(FREES.load(Ordering::SeqCst), 1);
    assert!(with_screenshot(|_, _| ()).is_none());
    assert_eq!(snapshot_copy_stats().copies, 1);

    // Allocation failure releases the source and produces a failure result.
    request_screenshot().unwrap(); ALLOC_FAIL.store(true, Ordering::SeqCst);
    assert!(!run(try_refresh_requested_screenshot(source)));
    assert!(take_failed_screenshot());
    assert!(!SOURCE_PINNED.load(Ordering::SeqCst));
    ALLOC_FAIL.store(false, Ordering::SeqCst);

    // Cancellation while waiting for admission frees the unsubmitted target.
    request_screenshot().unwrap(); *QUEUE.lock() = vec![Err(Error::Busy)];
    let mut future = Box::pin(try_refresh_requested_screenshot(source));
    assert!(future.as_mut().poll(&mut cx).is_pending()); drop(future);
    assert!(take_failed_screenshot()); assert_eq!(FREES.load(Ordering::SeqCst), 2);
    assert!(!SOURCE_PINNED.load(Ordering::SeqCst));

    // Every ambiguous result retains both allocations and reports failure.
    for mode in 0..3 {
        request_screenshot().unwrap();
        *QUEUE.lock() = vec![if mode == 0 { Err(Error::SubmitFailed) } else { Ok(1) }];
        *POLL.lock() = vec![if mode == 1 { Completion::Failed } else { Completion::Pending }];
        if mode == 2 {
            let mut future = Box::pin(try_refresh_requested_screenshot(source));
            assert!(future.as_mut().poll(&mut cx).is_pending()); drop(future);
        } else { assert!(!run(try_refresh_requested_screenshot(source))); }
        assert!(take_failed_screenshot());
        assert_eq!(request_screenshot(), Err(ScreenshotRequestError::Quarantined));
        assert!(SOURCE_PINNED.load(Ordering::SeqCst));
        assert!(SOURCE_QUARANTINED.load(Ordering::SeqCst));
        assert_eq!(FREES.load(Ordering::SeqCst), 2);
        // Test-only reset of the simulated hardware and intentionally retained allocation.
        unsafe { std::alloc::dealloc(DESTINATION.load(Ordering::SeqCst) as *mut u8,
            std::alloc::Layout::from_size_align(WD_XYUV8888_BYTES, 4096).unwrap()); }
        intel::media::wd_xyuv8888::reset_test_quarantine();
        SOURCE_PINNED.store(false, Ordering::SeqCst);
        SOURCE_QUARANTINED.store(false, Ordering::SeqCst);
    }
    assert_eq!(snapshot_copy_stats().failures, 5);
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-wd-bcs-') as tmp:
        path = Path(tmp)
        production = ROOT / 'src/intel/media/wd_xyuv8888.rs'
        (path / 'wd.rs').write_text(production.read_text() + "\npub fn reset_test_quarantine() { SCREENSHOT_DISABLED.store(false, Ordering::Release); }\n")
        source = source.replace(str(production), str(path / 'wd.rs'))
        (path / 'test.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'test.rs'), '-o', str(path / 'test')], check=True, cwd=ROOT)
        subprocess.run([str(path / 'test')], check=True, cwd=ROOT)


if __name__ == '__main__':
    main()
