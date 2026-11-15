#!/usr/bin/env python3
"""Exercise pristine GPU capture and CPU-only native samples with owned-buffer mocks."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item

def main():
    release_helper = item("src/spirit/mod.rs", "release_vfx_producers")
    cached_helper = item("src/spirit/mod.rs", "submit_codex_cached_frame")
    source = r'''#![allow(dead_code)]
use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};
static EVENTS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
static GPU_DONE: AtomicBool = AtomicBool::new(false);
static GPU_REQUIRED: AtomicBool = AtomicBool::new(false);
static CPU_DONE: AtomicBool = AtomicBool::new(false);
static CPU_REQUIRED: AtomicBool = AtomicBool::new(false);
static CANCELED: AtomicBool = AtomicBool::new(false);
static FAIL_WRITE: AtomicBool = AtomicBool::new(false);
static FAIL_CAPTURE: AtomicBool = AtomicBool::new(false);
static FAIL_COPY: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static STAMP_VALID: AtomicBool = AtomicBool::new(true);
static PIXELS: Mutex<[u8; 8]> = Mutex::new([41; 8]);
static CACHE: Mutex<[u8; 8]> = Mutex::new([0; 8]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SpiritFence { sequence: u64 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SpiritFrameLease { fence: SpiritFence }
#[derive(Clone, Copy)]
struct SpiritSurfaceLayout;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SpiritFenceId;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpiritBarrierSet { CPU }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpiritFrameProducer { Vfx }
mod lilly {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct LillyResidentFrame(pub u64);
}
#[derive(Clone, Copy)]
struct Stamp { valid: bool }
impl Stamp {
    fn apply(self, pixels: &mut [u8], _: SpiritSurfaceLayout) -> bool {
        assert_cpu_owned();
        assert_eq!(pixels[0], 41, "stamp reads clean character pixels");
        EVENTS.lock().unwrap().push("stamp");
        pixels[0] = 93;
        self.valid
    }
}
struct GpuInflight {
    lease: SpiritFrameLease,
    source_frame: lilly::LillyResidentFrame,
    cpu_required: bool,
    stamp: Option<Stamp>,
}
#[derive(Debug, Eq, PartialEq)]
enum SpiritSubmitError { InvalidCommand, InvalidGpuRelease, StaleLease }
mod intel { pub mod gpgpu { pub struct GpgpuRgba8ReleaseFence { pub valid: bool } } }
mod codex_render {
    use super::*;
    pub fn capture(source: lilly::LillyResidentFrame, pixels: &[u8], _: SpiritSurfaceLayout) -> bool {
        assert_cpu_owned();
        assert!(GPU_REQUIRED.load(Ordering::SeqCst));
        assert!(GPU_DONE.load(Ordering::SeqCst), "capture needs completed GPU ownership");
        assert_eq!(source, lilly::LillyResidentFrame(7));
        assert_eq!(pixels, &[41; 8], "the pristine cache cannot contain an old native stamp");
        EVENTS.lock().unwrap().push("capture-clean");
        if FAIL_CAPTURE.load(Ordering::SeqCst) { return false; }
        CACHE.lock().unwrap().copy_from_slice(pixels);
        true
    }
    pub fn copy_into(pixels: &mut [u8], _: SpiritSurfaceLayout) -> Option<lilly::LillyResidentFrame> {
        assert_cpu_owned();
        assert!(!GPU_REQUIRED.load(Ordering::SeqCst), "native samples own only a CPU lease");
        EVENTS.lock().unwrap().push("copy-clean");
        if FAIL_COPY.load(Ordering::SeqCst) { return None; }
        pixels.copy_from_slice(&*CACHE.lock().unwrap());
        Some(lilly::LillyResidentFrame(7))
    }
}
mod codex_handoff {
    use super::*;
    #[derive(Clone, Copy)]
    pub struct Source { pub surface: lilly::LillyResidentFrame, pub stamp: Option<Stamp> }
    pub fn cached_source(
        default: lilly::LillyResidentFrame, sequence: u64, rendered: lilly::LillyResidentFrame,
    ) -> Option<Source> {
        assert_eq!(default, lilly::LillyResidentFrame(9));
        assert_eq!(rendered, lilly::LillyResidentFrame(7));
        assert_eq!(sequence, 42);
        assert_cpu_owned();
        EVENTS.lock().unwrap().push("sample-native");
        ACTIVE.load(Ordering::SeqCst).then_some(Source {
            surface: lilly::LillyResidentFrame(12),
            stamp: Some(Stamp { valid: STAMP_VALID.load(Ordering::SeqCst) }),
        })
    }
}
fn assert_cpu_owned() {
    assert!(CPU_REQUIRED.load(Ordering::SeqCst));
    assert!(!CPU_DONE.load(Ordering::SeqCst), "released storage cannot be touched");
    if GPU_REQUIRED.load(Ordering::SeqCst) {
        assert!(GPU_DONE.load(Ordering::SeqCst), "GPU must retire before CPU access");
    }
}
fn lease() -> SpiritFrameLease { SpiritFrameLease { fence: SpiritFence { sequence: 42 } } }
fn acquire_frame_for(
    _: SpiritFenceId, required: SpiritBarrierSet, producer: SpiritFrameProducer,
) -> Result<SpiritFrameLease, SpiritSubmitError> {
    assert_eq!(required, SpiritBarrierSet::CPU);
    assert_eq!(producer, SpiritFrameProducer::Vfx);
    assert!(!GPU_REQUIRED.load(Ordering::SeqCst));
    CPU_REQUIRED.store(true, Ordering::SeqCst);
    EVENTS.lock().unwrap().push("reserve-cpu");
    Ok(lease())
}
fn release_gpgpu(
    _: SpiritFrameLease, release: intel::gpgpu::GpgpuRgba8ReleaseFence,
) -> Result<bool, SpiritSubmitError> {
    if !release.valid { return Err(SpiritSubmitError::InvalidGpuRelease); }
    GPU_DONE.store(true, Ordering::SeqCst);
    EVENTS.lock().unwrap().push("gpu-release");
    assert!(!CPU_DONE.load(Ordering::SeqCst));
    Ok(true)
}
fn write_cpu<R>(
    _: SpiritFrameLease, write: impl FnOnce(&mut [u8], SpiritSurfaceLayout) -> R,
) -> Result<R, SpiritSubmitError> {
    assert_cpu_owned();
    assert!(!ready(), "CPU-owned surface cannot present before its release");
    if FAIL_WRITE.load(Ordering::SeqCst) { return Err(SpiritSubmitError::StaleLease); }
    EVENTS.lock().unwrap().push("cpu-invalidate");
    Ok(write(&mut *PIXELS.lock().unwrap(), SpiritSurfaceLayout))
}
fn release_cpu(_: SpiritFrameLease) -> Result<(), SpiritSubmitError> {
    assert_cpu_owned();
    EVENTS.lock().unwrap().push("cpu-flush-release");
    CPU_DONE.store(true, Ordering::SeqCst);
    Ok(())
}
fn cancel_pending(_: SpiritFrameLease) {
    assert!(!CPU_DONE.load(Ordering::SeqCst));
    EVENTS.lock().unwrap().push("cancel");
    CANCELED.store(true, Ordering::SeqCst);
}
fn ready() -> bool {
    !CANCELED.load(Ordering::SeqCst)
        && (!GPU_REQUIRED.load(Ordering::SeqCst) || GPU_DONE.load(Ordering::SeqCst))
        && (!CPU_REQUIRED.load(Ordering::SeqCst) || CPU_DONE.load(Ordering::SeqCst))
}
@RELEASE_HELPER@
@CACHED_HELPER@
fn reset() {
    EVENTS.lock().unwrap().clear();
    for flag in [&GPU_DONE, &GPU_REQUIRED, &CPU_DONE, &CPU_REQUIRED, &CANCELED,
                 &FAIL_WRITE, &FAIL_CAPTURE, &FAIL_COPY, &ACTIVE] {
        flag.store(false, Ordering::SeqCst);
    }
    STAMP_VALID.store(true, Ordering::SeqCst);
    *PIXELS.lock().unwrap() = [41; 8];
    *CACHE.lock().unwrap() = [0; 8];
}
fn setup(cpu_required: bool, stamp: Option<Stamp>) -> GpuInflight {
    reset();
    GPU_REQUIRED.store(true, Ordering::SeqCst);
    CPU_REQUIRED.store(cpu_required, Ordering::SeqCst);
    GpuInflight { lease: lease(), source_frame: lilly::LillyResidentFrame(7), cpu_required, stamp }
}
fn setup_cached(active: bool) {
    reset();
    ACTIVE.store(active, Ordering::SeqCst);
    *CACHE.lock().unwrap() = [41; 8];
    *PIXELS.lock().unwrap() = [93; 8]; // Previous back buffer contains an old stamp.
}
fn release() -> intel::gpgpu::GpgpuRgba8ReleaseFence {
    intel::gpgpu::GpgpuRgba8ReleaseFence { valid: true }
}
#[test]
fn gpu_then_capture_then_cpu_stamp_is_flushed_before_surface_becomes_ready() {
    let frame = setup(true, Some(Stamp { valid: true }));
    assert!(!ready());
    assert_eq!(release_vfx_producers(&frame, release()), Ok(true));
    assert!(ready());
    assert_eq!(*EVENTS.lock().unwrap(), ["gpu-release", "cpu-invalidate", "capture-clean", "stamp", "cpu-flush-release"]);
    assert_eq!(PIXELS.lock().unwrap()[0], 93);
    assert_eq!(*CACHE.lock().unwrap(), [41; 8], "native tile never enters the pristine cache");
}
#[test]
fn ordinary_frames_keep_their_gpu_only_path() {
    let frame = setup(false, None);
    assert_eq!(release_vfx_producers(&frame, release()), Ok(true));
    assert!(ready());
    assert_eq!(*EVENTS.lock().unwrap(), ["gpu-release"]);
    assert_eq!(*PIXELS.lock().unwrap(), [41; 8]);
    assert_eq!(*CACHE.lock().unwrap(), [0; 8]);
}
#[test]
fn arrival_without_inside_stamp_still_captures_clean_character_and_releases_cpu() {
    let frame = setup(true, None);
    assert_eq!(release_vfx_producers(&frame, release()), Ok(true));
    assert!(ready());
    assert_eq!(*EVENTS.lock().unwrap(), ["gpu-release", "cpu-invalidate", "capture-clean", "cpu-flush-release"]);
    assert_eq!(*CACHE.lock().unwrap(), [41; 8]);
}
#[test]
fn unproven_gpu_completion_cannot_capture_or_touch_cpu_pixels() {
    let frame = setup(true, Some(Stamp { valid: true }));
    assert_eq!(release_vfx_producers(&frame, intel::gpgpu::GpgpuRgba8ReleaseFence { valid: false }),
        Err(SpiritSubmitError::InvalidGpuRelease));
    assert!(!ready());
    assert!(EVENTS.lock().unwrap().is_empty());
    assert_eq!(*PIXELS.lock().unwrap(), [41; 8]);
    assert_eq!(*CACHE.lock().unwrap(), [0; 8]);
}
#[test]
fn failed_capture_cannot_stamp_or_release_surface_for_presentation() {
    let frame = setup(true, Some(Stamp { valid: true }));
    FAIL_CAPTURE.store(true, Ordering::SeqCst);
    assert_eq!(release_vfx_producers(&frame, release()), Err(SpiritSubmitError::InvalidCommand));
    assert!(!ready());
    assert_eq!(*EVENTS.lock().unwrap(), ["gpu-release", "cpu-invalidate", "capture-clean"]);
    assert_eq!(*PIXELS.lock().unwrap(), [41; 8]);
}
#[test]
fn failed_stamp_cannot_release_surface_for_presentation() {
    let frame = setup(true, Some(Stamp { valid: false }));
    assert_eq!(release_vfx_producers(&frame, release()), Err(SpiritSubmitError::InvalidCommand));
    assert!(!ready());
    assert!(!CPU_DONE.load(Ordering::SeqCst));
}
#[test]
fn stale_cpu_lease_cannot_capture_stamp_or_release_surface() {
    let frame = setup(true, Some(Stamp { valid: true }));
    FAIL_WRITE.store(true, Ordering::SeqCst);
    assert_eq!(release_vfx_producers(&frame, release()), Err(SpiritSubmitError::StaleLease));
    assert!(!ready());
    assert_eq!(*EVENTS.lock().unwrap(), ["gpu-release"]);
    assert_eq!(*PIXELS.lock().unwrap(), [41; 8]);
}
#[test]
fn cached_native_frame_uses_cpu_only_copy_stamp_and_release() {
    setup_cached(true);
    assert_eq!(submit_codex_cached_frame(SpiritFenceId, lilly::LillyResidentFrame(9)),
        Ok((lease(), lilly::LillyResidentFrame(12))));
    assert!(ready());
    assert!(!GPU_DONE.load(Ordering::SeqCst), "native motion did not wait for a new GPU job");
    assert_eq!(*EVENTS.lock().unwrap(), ["reserve-cpu", "cpu-invalidate", "copy-clean", "sample-native", "stamp", "cpu-flush-release"]);
    assert_eq!(*PIXELS.lock().unwrap(), [93, 41, 41, 41, 41, 41, 41, 41]);
    assert_eq!(*CACHE.lock().unwrap(), [41; 8]);
}
#[test]
fn canceled_arrival_restores_clean_cached_frame_without_a_stamp() {
    setup_cached(false);
    assert_eq!(submit_codex_cached_frame(SpiritFenceId, lilly::LillyResidentFrame(9)),
        Ok((lease(), lilly::LillyResidentFrame(9))));
    assert!(ready());
    assert_eq!(*EVENTS.lock().unwrap(), ["reserve-cpu", "cpu-invalidate", "copy-clean", "sample-native", "cpu-flush-release"]);
    assert_eq!(*PIXELS.lock().unwrap(), [41; 8], "clean copy erases the old native tile");
}
#[test]
fn failed_cached_copy_cancels_the_unreleased_cpu_lease() {
    setup_cached(true);
    FAIL_COPY.store(true, Ordering::SeqCst);
    assert_eq!(submit_codex_cached_frame(SpiritFenceId, lilly::LillyResidentFrame(9)),
        Err(SpiritSubmitError::InvalidCommand));
    assert!(!ready());
    assert!(!CPU_DONE.load(Ordering::SeqCst));
    assert_eq!(*EVENTS.lock().unwrap(), ["reserve-cpu", "cpu-invalidate", "copy-clean", "cancel"]);
    assert_eq!(*PIXELS.lock().unwrap(), [93; 8]);
}
#[test]
fn failed_cached_stamp_cancels_the_unreleased_cpu_lease() {
    setup_cached(true);
    STAMP_VALID.store(false, Ordering::SeqCst);
    assert_eq!(submit_codex_cached_frame(SpiritFenceId, lilly::LillyResidentFrame(9)),
        Err(SpiritSubmitError::InvalidCommand));
    assert!(!ready());
    assert_eq!(*EVENTS.lock().unwrap(), ["reserve-cpu", "cpu-invalidate", "copy-clean", "sample-native", "stamp", "cancel"]);
}
#[test]
fn stale_cached_cpu_lease_cannot_copy_or_stamp_and_is_canceled() {
    setup_cached(true);
    FAIL_WRITE.store(true, Ordering::SeqCst);
    assert_eq!(submit_codex_cached_frame(SpiritFenceId, lilly::LillyResidentFrame(9)),
        Err(SpiritSubmitError::StaleLease));
    assert!(!ready());
    assert_eq!(*EVENTS.lock().unwrap(), ["reserve-cpu", "cancel"]);
    assert_eq!(*PIXELS.lock().unwrap(), [93; 8]);
}
'''.replace("@RELEASE_HELPER@", release_helper).replace("@CACHED_HELPER@", cached_helper)
    with tempfile.TemporaryDirectory(prefix="spirit-vfx-cpu-stamp-") as directory:
        path = Path(directory)
        test = path / "test.rs"
        binary = path / "test"
        test.write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(test), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
