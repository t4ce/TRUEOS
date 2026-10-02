#!/usr/bin/env python3
"""Run the production synchronous consumer against a scripted BCS transport."""
from pathlib import Path
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT
SERVICE = 'src/r/services/vcpy_service.rs'
BLT = 'src/intel/copy/blt.rs'
SURFACES = 'src/intel/gpgpu/types/surfaces.rs'


def main():
    source = '#![allow(dead_code, unused_variables)]\nmod intel {\n'
    source += '\n'.join(extract.item(BLT, name) for name in (
        'GucBcs0RgbaSurface', 'GucBcs0RgbaCopy', 'GucBcs0CopySubmitError',
        'GucBcs0CopyCompletion',
    ))
    source += """
pub(crate) fn queue_guc_bcs0_rgba_fill(dst: GucBcs0RgbaSurface, color: u32) -> Result<u32, GucBcs0CopySubmitError> {
    assert_eq!((dst.width, dst.height, dst.pitch_bytes, color), (20, 3, 256, 0x80402010));
    let mut script = crate::SCRIPT.lock().unwrap();
    script.2 += 1;
    match script.0 { Some(error) => Err(error), None => Ok(77) }
}
"""
    source += '\npub(crate) mod gpgpu {\n'
    source += '\n'.join(extract.item(SURFACES, name) for name in (
        'GpgpuRgba8StorageOrder', 'GpgpuRgba8Surface',
    ))
    source += '''
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GpgpuSubmissionOutcome { Complete, Unavailable, SubmittedIncomplete }
pub(crate) struct GpgpuRect(u32,u32,u32,u32);
impl GpgpuRect { pub fn new(x:u32,y:u32,w:u32,h:u32)->Self { Self(x,y,w,h) } }
pub(crate) struct GpgpuPoint(u32,u32);
impl GpgpuPoint { pub fn new(x:u32,y:u32)->Self { Self(x,y) } }
pub(crate) fn copy_rect_rgba8_complete_mode(src:GpgpuRgba8Surface, rect:GpgpuRect,
    _dst:GpgpuRgba8Surface, point:GpgpuPoint, complete:bool)->bool {
    assert_eq!((rect.0,rect.1,rect.2,rect.3), (0,0,src.width,src.height));
    assert_eq!((point.0,point.1,complete), (0,0,true));
    crate::COMPUTE_CALLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    crate::COMPUTE_OK.load(core::sync::atomic::Ordering::Relaxed)
}
}}
use intel::{GucBcs0CopySubmitError as Error, GucBcs0CopyCompletion as Completion};
use intel::gpgpu::{GpgpuSubmissionOutcome as Outcome, GpgpuRgba8Surface as Surface};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
static CONSUMER_COPIES: AtomicU64 = AtomicU64::new(0);
static CONSUMER_BYTES: AtomicU64 = AtomicU64::new(0);
static CONSUMER_FALLBACKS: AtomicU64 = AtomicU64::new(0);
static CONSUMER_FAILURES: AtomicU64 = AtomicU64::new(0);
static SCENE_COPIES: AtomicU64 = AtomicU64::new(0);
static SCENE_BYTES: AtomicU64 = AtomicU64::new(0);
static SCENE_FALLBACKS: AtomicU64 = AtomicU64::new(0);
static SCENE_FAILURES: AtomicU64 = AtomicU64::new(0);
static FILL_COUNTS: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
static COMPUTE_CALLS: AtomicU64 = AtomicU64::new(0);
static COMPUTE_OK: AtomicBool = AtomicBool::new(true);
static RESIDENT_SCENE_COPY_QUARANTINED: AtomicBool = AtomicBool::new(false);
static QUARANTINE_DURING_RESERVE: AtomicBool = AtomicBool::new(false);
static STORAGE_RESERVES: AtomicU64 = AtomicU64::new(0);
static STORAGE_RELEASES: AtomicU64 = AtomicU64::new(0);
#[macro_export]
macro_rules! log { ($($arg:tt)*) => {}; }
mod gpu {
    pub mod vgpu {
        #[derive(Clone, Copy)]
        pub enum KernelClient { Render }
    }
    pub mod executor {
        use crate::*;
        pub struct KernelContextLease;
        impl Drop for KernelContextLease {
            fn drop(&mut self) { STORAGE_RELEASES.fetch_add(1, Ordering::Relaxed); }
        }
        pub fn reserve_kernel_context(_: super::vgpu::KernelClient)->Result<KernelContextLease, ()> {
            STORAGE_RESERVES.fetch_add(1, Ordering::Relaxed);
            if QUARANTINE_DURING_RESERVE.load(Ordering::Relaxed) {
                RESIDENT_SCENE_COPY_QUARANTINED.store(true, Ordering::Release);
            }
            Ok(KernelContextLease)
        }
    }
}

mod r { pub mod services { pub mod vcpy_service {
    pub(crate) use crate::{copy_rgba8_complete_for, RgbaCopyConsumer};
}}}
static SCRIPT: Mutex<(Option<Error>, Vec<Completion>, usize, usize)> =
    Mutex::new((None, Vec::new(), 0, 0));
fn queue_rgba_copies(dst: intel::GucBcs0RgbaSurface, copies: &[intel::GucBcs0RgbaCopy]) -> Result<u32, Error> {
    let mut script = SCRIPT.lock().unwrap();
    script.2 += 1;
    assert_eq!(copies.len(), 1);
    let copy = copies[0];
    assert_eq!((copy.source_x, copy.source_y, copy.destination_x, copy.destination_y), (0,0,0,0));
    assert_eq!((copy.width, copy.height, copy.source.pitch_bytes, dst.pitch_bytes), (17,3,128,256));
    match script.0 { Some(error) => Err(error), None => Ok(77) }
}
fn poll_rgba_copies(submission: u32) -> Completion {
    assert_eq!(submission, 77);
    let mut script = SCRIPT.lock().unwrap();
    script.3 += 1;
    script.1.remove(0)
}
'''
    source += '\n'.join(extract.item(SERVICE, name) for name in (
        'RgbaCopyConsumer', 'copy_rgba8_complete', 'copy_rgba8_complete_for',
        'RgbaFillConsumer', 'fill_rgba8_complete', 'fill_stats'))
    source += extract.item('src/intel/render/primary.rs', 'copy_resident_scene_output')
    source += extract.item('src/intel/render/submit.rs', 'reserve_warm_render_storage')
    source += '''
#[test]
fn completion_and_admission_failures_preserve_the_fallback_boundary() {
    let src = Surface { phys: 0x100000, gpu: 0x100000, bytes: 4096,
        width: 17, height: 3, pitch_bytes: 128, ..Surface::default() };
    let dst = Surface { phys: 0x200000, gpu: 0x200000, bytes: 4096,
        width: 20, height: 3, pitch_bytes: 256, ..Surface::default() };
    *SCRIPT.lock().unwrap() = (None, vec![Completion::Pending, Completion::Pending, Completion::Complete], 0, 0);
    assert_eq!(copy_rgba8_complete(src, dst), Outcome::Complete);
    assert_eq!(SCRIPT.lock().unwrap().3, 3);
    assert_eq!(CONSUMER_BYTES.load(Ordering::Relaxed), 17*3*4);
    for error in [Error::Busy, Error::Unavailable, Error::InvalidRequest] {
        *SCRIPT.lock().unwrap() = (Some(error), vec![], 0, 0);
        assert_eq!(copy_rgba8_complete(src, dst), Outcome::Unavailable);
        assert_eq!(SCRIPT.lock().unwrap().3, 0);
    }
    *SCRIPT.lock().unwrap() = (Some(Error::SubmitFailed), vec![], 0, 0);
    assert_eq!(copy_rgba8_complete(src, dst), Outcome::SubmittedIncomplete);
    for completion in [Completion::Failed, Completion::InvalidSubmission] {
        *SCRIPT.lock().unwrap() = (None, vec![Completion::Pending, completion], 0, 0);
        assert_eq!(copy_rgba8_complete(src, dst), Outcome::SubmittedIncomplete);
        assert_eq!(SCRIPT.lock().unwrap().3, 2);
    }
    for unsupported in [Surface { width: 16, ..dst }, Surface {
        storage_order: intel::gpgpu::GpgpuRgba8StorageOrder::Bgra, ..dst
    }] {
        *SCRIPT.lock().unwrap() = (None, vec![], 0, 0);
        assert_eq!(copy_rgba8_complete(src, unsupported), Outcome::Unavailable);
        assert_eq!(SCRIPT.lock().unwrap().2, 0);
    }
    assert_eq!(CONSUMER_COPIES.load(Ordering::Relaxed), 1);
    assert_eq!(CONSUMER_FALLBACKS.load(Ordering::Relaxed), 5);
    assert_eq!(CONSUMER_FAILURES.load(Ordering::Relaxed), 3);
    *SCRIPT.lock().unwrap() = (None, vec![Completion::Pending, Completion::Complete], 0, 0);
    assert_eq!(fill_rgba8_complete(dst, 0x80402010, RgbaFillConsumer::Font), Outcome::Complete);
    assert_eq!(fill_stats(RgbaFillConsumer::Font), [1, 240, 0, 0]);
    for error in [Error::Busy, Error::Unavailable, Error::InvalidRequest] {
        *SCRIPT.lock().unwrap() = (Some(error), vec![], 0, 0);
        assert_eq!(fill_rgba8_complete(dst, 0x80402010, RgbaFillConsumer::Gridpaper), Outcome::Unavailable);
        assert_eq!(SCRIPT.lock().unwrap().3, 0);
    }
    *SCRIPT.lock().unwrap() = (Some(Error::SubmitFailed), vec![], 0, 0);
    assert_eq!(fill_rgba8_complete(dst, 0x80402010, RgbaFillConsumer::Gridpaper), Outcome::SubmittedIncomplete);
    for completion in [Completion::Failed, Completion::InvalidSubmission] {
        *SCRIPT.lock().unwrap() = (None, vec![completion], 0, 0);
        assert_eq!(fill_rgba8_complete(dst, 0x80402010, RgbaFillConsumer::Gridpaper), Outcome::SubmittedIncomplete);
    }
    assert_eq!(fill_stats(RgbaFillConsumer::Gridpaper), [0, 0, 3, 3]);
    *SCRIPT.lock().unwrap() = (None, vec![Completion::Complete], 0, 0);
    assert_eq!(fill_rgba8_complete(dst, 0x80402010, RgbaFillConsumer::Ui4), Outcome::Complete);
    *SCRIPT.lock().unwrap() = (Some(Error::Busy), vec![], 0, 0);
    assert_eq!(fill_rgba8_complete(dst, 0x80402010, RgbaFillConsumer::Ui4), Outcome::Unavailable);
    *SCRIPT.lock().unwrap() = (None, vec![Completion::Failed], 0, 0);
    assert_eq!(fill_rgba8_complete(dst, 0x80402010, RgbaFillConsumer::Ui4), Outcome::SubmittedIncomplete);
    assert_eq!(fill_stats(RgbaFillConsumer::Ui4), [1, 240, 1, 1]);
    assert_eq!(fill_stats(RgbaFillConsumer::Font), [1, 240, 0, 0]);
    // Exercise the actual renderer dispatch, not a duplicate policy.
    *SCRIPT.lock().unwrap() = (None, vec![Completion::Complete], 0, 0);
    assert_eq!(copy_resident_scene_output(src, dst), Outcome::Complete);
    assert_eq!(COMPUTE_CALLS.load(Ordering::Relaxed), 0);
    for error in [Error::Busy, Error::Unavailable, Error::InvalidRequest] {
        *SCRIPT.lock().unwrap() = (Some(error), vec![], 0, 0);
        assert_eq!(copy_resident_scene_output(src, dst), Outcome::Complete);
    }
    assert_eq!(COMPUTE_CALLS.load(Ordering::Relaxed), 3);
    *SCRIPT.lock().unwrap() = (Some(Error::SubmitFailed), vec![], 0, 0);
    assert_eq!(copy_resident_scene_output(src, dst), Outcome::SubmittedIncomplete);
    for completion in [Completion::Failed, Completion::InvalidSubmission] {
        *SCRIPT.lock().unwrap() = (None, vec![completion], 0, 0);
        assert_eq!(copy_resident_scene_output(src, dst), Outcome::SubmittedIncomplete);
    }
    assert_eq!(COMPUTE_CALLS.load(Ordering::Relaxed), 3);
    COMPUTE_OK.store(false, Ordering::Relaxed);
    *SCRIPT.lock().unwrap() = (Some(Error::Busy), vec![], 0, 0);
    assert_eq!(copy_resident_scene_output(src, dst), Outcome::SubmittedIncomplete);
    assert!(reserve_warm_render_storage("test").is_some());
    assert_eq!(STORAGE_RESERVES.load(Ordering::Relaxed), 1);
    RESIDENT_SCENE_COPY_QUARANTINED.store(true, Ordering::Release);
    assert!(reserve_warm_render_storage("test").is_none());
    assert_eq!(STORAGE_RESERVES.load(Ordering::Relaxed), 1);
    RESIDENT_SCENE_COPY_QUARANTINED.store(false, Ordering::Release);
    QUARANTINE_DURING_RESERVE.store(true, Ordering::Relaxed);
    assert!(reserve_warm_render_storage("raced").is_none());
    assert_eq!(STORAGE_RESERVES.load(Ordering::Relaxed), 2);
    assert_eq!(STORAGE_RELEASES.load(Ordering::Relaxed), 2);
    assert_eq!(SCENE_COPIES.load(Ordering::Relaxed), 1);
    assert_eq!(SCENE_BYTES.load(Ordering::Relaxed), 17*3*4);
    assert_eq!(SCENE_FALLBACKS.load(Ordering::Relaxed), 4);
    assert_eq!(SCENE_FAILURES.load(Ordering::Relaxed), 3);
    assert_eq!(CONSUMER_COPIES.load(Ordering::Relaxed), 1);
    assert_eq!(CONSUMER_FALLBACKS.load(Ordering::Relaxed), 5);
    assert_eq!(CONSUMER_FAILURES.load(Ordering::Relaxed), 3);
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-vcpy-consumer-') as temporary:
        path = Path(temporary)
        (path / 'tests.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'tests.rs'),
                        '-o', str(path / 'tests')], cwd=ROOT, check=True)
        subprocess.run([str(path / 'tests')], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
