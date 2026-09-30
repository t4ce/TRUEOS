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
    source = '#![allow(dead_code)]\nmod intel {\n'
    source += '\n'.join(extract.item(BLT, name) for name in (
        'GucBcs0RgbaSurface', 'GucBcs0RgbaCopy', 'GucBcs0CopySubmitError',
        'GucBcs0CopyCompletion',
    ))
    source += '\npub(crate) mod gpgpu {\n'
    source += '\n'.join(extract.item(SURFACES, name) for name in (
        'GpgpuRgba8StorageOrder', 'GpgpuRgba8Surface',
    ))
    source += '''
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GpgpuSubmissionOutcome { Complete, Unavailable, SubmittedIncomplete }
}}
use intel::{GucBcs0CopySubmitError as Error, GucBcs0CopyCompletion as Completion};
use intel::gpgpu::{GpgpuSubmissionOutcome as Outcome, GpgpuRgba8Surface as Surface};
use core::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
static CONSUMER_COPIES: AtomicU64 = AtomicU64::new(0);
static CONSUMER_BYTES: AtomicU64 = AtomicU64::new(0);
static CONSUMER_FALLBACKS: AtomicU64 = AtomicU64::new(0);
static CONSUMER_FAILURES: AtomicU64 = AtomicU64::new(0);
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
    source += extract.item(SERVICE, 'copy_rgba8_complete')
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
