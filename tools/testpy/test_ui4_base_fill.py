#!/usr/bin/env python3
"""Exercise production UI4 initialization with a scripted BCS fill transport."""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT
POOL = 'src/ui4/frame_pool.rs'
SURFACES = 'src/intel/gpgpu/types/surfaces.rs'


def main():
    source = '''#![allow(dead_code)]
use std::sync::Mutex;
const FRAME_BUFFER_CAPACITY: usize = 4;
type UiSurfaceHandle = usize;
#[derive(Debug, PartialEq)]
enum FramePoolError { UnsupportedFormat, Busy }
#[derive(Clone, Copy)]
struct PremultipliedRgba8(u32);
impl PremultipliedRgba8 { fn to_native_bytes(self) -> [u8; 4] { self.0.to_le_bytes() } }
#[macro_export]
macro_rules! log_error { ($($arg:tt)*) => {}; }
static CALLS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static SCRIPT: Mutex<Vec<intel::gpgpu::GpgpuSubmissionOutcome>> = Mutex::new(Vec::new());
static BACKING: Mutex<Vec<Vec<u32>>> = Mutex::new(Vec::new());
static FLUSHES: Mutex<Vec<usize>> = Mutex::new(Vec::new());
mod intel {
    pub(crate) fn dma_flush(_: *mut u8, len: usize) { crate::FLUSHES.lock().unwrap().push(len); }
    pub(crate) mod gpgpu {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub(crate) enum GpgpuSubmissionOutcome { Complete, Unavailable, SubmittedIncomplete }
'''
    source += '\n'.join(extract.item(SURFACES, name) for name in (
        'GpgpuPoint', 'GpgpuRect', 'GpgpuRgba8StorageOrder', 'GpgpuRgba8Surface'))
    text = (ROOT / SURFACES).read_text()
    for name in ('GpgpuRect', 'GpgpuRgba8Surface'):
        source += re.search(rf'^impl {name} \{{.*?^}}', text, re.M | re.S).group()
    source += '''
}}
mod ui_surface {
'''
    source += extract.item('src/r/ui_surface.rs', 'UiSurfaceRgbaAccess')
    source += '''
    pub(crate) fn rgba_access(handle: usize) -> Option<UiSurfaceRgbaAccess> {
        let mut backing = crate::BACKING.lock().unwrap();
        let bytes = backing.get_mut(handle)?;
        Some(UiSurfaceRgbaAccess { phys: (handle as u64 + 1) * 4096,
            gpu: (handle as u64 + 1) * 4096, virt: bytes.as_mut_ptr().cast(),
            byte_len: bytes.len() * 4, width: 17, height: 3, pitch: 128 })
    }
    pub(crate) fn destroy_surface(handle: usize) -> bool {
        crate::BACKING.lock().unwrap()[handle].clear(); true
    }
}
mod r { pub mod services { pub mod vcpy_service {
    pub(crate) enum RgbaFillConsumer { Ui4 }
    pub(crate) fn fill_rgba8_complete(dst: crate::intel::gpgpu::GpgpuRgba8Surface,
        pixel: u32, _: RgbaFillConsumer) -> crate::intel::gpgpu::GpgpuSubmissionOutcome {
        use crate::intel::gpgpu::GpgpuSubmissionOutcome as Outcome;
        let index = (dst.phys / 4096 - 1) as usize;
        crate::CALLS.lock().unwrap().push(index);
        assert_eq!((dst.width, dst.height, dst.pitch_bytes), (32, 3, 128));
        let outcome = crate::SCRIPT.lock().unwrap().remove(0);
        if outcome == Outcome::Complete {
            crate::BACKING.lock().unwrap()[index][..96].fill(pixel);
        }
        outcome
    }
}}}
'''
    source += '\n'.join(extract.item(POOL, name) for name in (
        'initialize_rgba_surface', 'initialize_rgba_surfaces', 'destroy_surfaces'))
    source += '''
use intel::gpgpu::GpgpuSubmissionOutcome as Outcome;
const POISON: u32 = 0xDEADBEEF;
fn setup(script: Vec<Outcome>) {
    *BACKING.lock().unwrap() = vec![vec![POISON; 1024]; 4];
    *SCRIPT.lock().unwrap() = script;
    CALLS.lock().unwrap().clear();
    FLUSHES.lock().unwrap().clear();
}
#[test]
fn every_buffer_and_padding_is_initialized_before_return() {
    for count in 1..=4 {
        for color in [0, 0xFF808080, 0x80402010] {
            setup(vec![Outcome::Complete; count]);
            let mut handles = [Some(0), Some(1), Some(2), Some(3)];
            assert_eq!(initialize_rgba_surfaces(&mut handles, count, PremultipliedRgba8(color)), Ok(()));
            assert_eq!(*CALLS.lock().unwrap(), (0..count).collect::<Vec<_>>());
            let backing = BACKING.lock().unwrap();
            for (i, buffer) in backing.iter().enumerate() {
                assert!(buffer.iter().all(|word| *word == if i < count { color } else { POISON }));
            }
        }
    }
}
#[test]
fn unavailable_falls_back_and_initializes_entire_allocation() {
    setup(vec![Outcome::Unavailable, Outcome::Complete]);
    let mut handles = [Some(0), Some(1), None, None];
    assert_eq!(initialize_rgba_surfaces(&mut handles, 2, PremultipliedRgba8(0x80402010)), Ok(()));
    let backing = BACKING.lock().unwrap();
    assert!(backing[..2].iter().flatten().all(|word| *word == 0x80402010));
}
#[test]
fn uncertain_retirement_never_cpu_fills_or_frees_that_buffer() {
    setup(vec![Outcome::Complete, Outcome::SubmittedIncomplete]);
    let mut handles = [Some(0), Some(1), Some(2), Some(3)];
    assert_eq!(initialize_rgba_surfaces(&mut handles, 4, PremultipliedRgba8(0xFF808080)), Err(FramePoolError::Busy));
    assert_eq!(handles, [Some(0), None, Some(2), Some(3)]);
    assert_eq!(*CALLS.lock().unwrap(), [0, 1]);
    destroy_surfaces(handles);
    let backing = BACKING.lock().unwrap();
    assert!(backing[0].is_empty() && backing[2].is_empty() && backing[3].is_empty());
    assert_eq!(backing[1], vec![POISON; 1024]);
}
#[test]
fn malformed_storage_is_rejected_before_submission() {
    setup(vec![]);
    let mut access = ui_surface::rgba_access(0).unwrap();
    access.byte_len = 380; // Cannot contain three pitched rows.
    assert_eq!(initialize_rgba_surface(access, 0), Outcome::Unavailable);
    access.byte_len = 4096;
    access.virt = core::ptr::null_mut();
    assert_eq!(initialize_rgba_surface(access, 0), Outcome::Unavailable);
    assert!(CALLS.lock().unwrap().is_empty());
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-ui4-base-fill-') as temporary:
        path = Path(temporary)
        (path / 'tests.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'tests.rs'),
                        '-o', str(path / 'tests')], cwd=ROOT, check=True)
        subprocess.run([str(path / 'tests'), '--test-threads=1'], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
