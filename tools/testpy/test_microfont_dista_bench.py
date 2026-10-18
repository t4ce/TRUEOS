#!/usr/bin/env python3
"""Execute production atlas benchmark lifecycle with mocked hardware boundaries.

Real benchmark control flow, task preparation and JSON records run on the host.
The small mocks inject failures and count ownership operations; no GPU executes.
"""
from pathlib import Path
import json
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

MOCKS = r'''
#![allow(dead_code)]
extern crate alloc;
use std::sync::{LazyLock, Mutex};
#[derive(Clone, Copy, Default, PartialEq)]
enum Fault { #[default] None, Upload, Output, Create, Acquire, Surface, Busy, InFlight,
    Device, Submit, Complete, Fence, DrawClock, TargetClock, ClearClock, WritebackClock, CleanupMesh, CleanupLease,
    CleanupFrame, Spawn, MaskCreate, MaskRelease, Capture }
type Job = Box<dyn FnOnce() + Send>;
#[derive(Default)]
struct State {
    fault: Fault, fail_after: usize, renders: usize, mesh_releases: usize, mesh_drops: usize,
    frames: usize, acquires: usize, cancels: usize, destroys: usize,
    masks: usize, mask_releases: usize, mask_drops: usize, captures: usize,
    logs: Vec<serde_json::Value>, pending: Option<Job>,
}
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));
fn mutate<T>(f: impl FnOnce(&mut State) -> T) -> T { f(&mut STATE.lock().unwrap()) }
fn fault() -> Fault { mutate(|s| s.fault) }
#[macro_export]
macro_rules! log_important {
    (target: $target:literal; $format:literal, $value:expr) => {
        crate::mutate(|s| s.logs.push($value))
    };
}
mod time { pub fn tsc_hz() -> u64 { 1_000_000_000 } }
mod intel {
    pub mod gpgpu {
        #[derive(Clone, Copy)]
        pub struct GpgpuRgba8Surface { pub bytes: usize, pub width: u32, pub height: u32, pub pitch_bytes: u32 }
        #[derive(Clone, Copy)]
        pub struct GpgpuMask8Surface { pub bytes: usize, pub width: u32, pub height: u32, pub pitch_bytes: u32 }
    }
    pub mod shader {
        pub fn microfont_dista_seeds_current() -> bool { true }
        pub fn microfont_dista_artifact() -> (u16, &'static str) { (0x4680, "host-mock") }
        pub fn microfont_dista_active_links(_: char) -> usize { 0 }
    }
    pub mod render {
        use crate::{Fault, fault, mutate};
        pub struct Mesh;
        impl Drop for Mesh { fn drop(&mut self) { mutate(|s| s.mesh_drops += 1); } }
        pub fn create_resident_triangle_mesh(vertices: &[[f32; 3]], indices: &[u32])
            -> Result<Mesh, &'static str> {
            assert!(!vertices.is_empty() && !indices.is_empty());
            if fault() == Fault::Upload { Err("mesh-upload") } else { Ok(Mesh) }
        }
        pub fn release_resident_triangle_mesh(_: &Mesh) -> bool {
            mutate(|s| s.mesh_releases += 1); fault() != Fault::CleanupMesh
        }
        pub struct ResidentDistaMask;
        impl Drop for ResidentDistaMask { fn drop(&mut self) { mutate(|s| s.mask_drops += 1); } }
        impl ResidentDistaMask {
            pub fn surface(&self) -> super::gpgpu::GpgpuMask8Surface {
                super::gpgpu::GpgpuMask8Surface { width:1536, height:1056, bytes:1536*1056, pitch_bytes:1536 }
            }
        }
        pub fn create_resident_dista_mask(width:u32, height:u32) -> Result<ResidentDistaMask,&'static str> {
            assert_eq!((width,height),(1536,1056));
            if fault() == Fault::MaskCreate { Err("mask-create") }
            else { mutate(|s|s.masks+=1); Ok(ResidentDistaMask) }
        }
        pub fn release_resident_dista_mask(_:ResidentDistaMask) -> bool {
            mutate(|s|s.mask_releases+=1); fault() != Fault::MaskRelease
        }
        pub enum ResidentSceneFragmentContract { MicrofontDista }
        pub enum ResidentScenePrimitiveTopology { TriangleList }
        pub struct ResidentSceneDraw<'a> {
            pub mesh: &'a Mesh, pub depth_flags: Option<u32>, pub rgba: [u8; 4],
            pub sampled_texture: Option<()>, pub fragment_contract: ResidentSceneFragmentContract,
            pub viewport_translation_px: [f32; 2], pub topology: ResidentScenePrimitiveTopology,
            pub point_width_px: u32,
        }
        #[derive(Clone, Copy)]
        pub struct Timestamp { pub ns: u64, pub ticks: u64, pub hz: u64 }
        pub struct ResultFrame {
            pub release_fence: Option<()>, pub gpu_target_ready_timestamp: Option<Timestamp>,
            pub gpu_clear_setup_timestamp: Option<Timestamp>, pub gpu_writeback_timestamp: Option<Timestamp>,
            pub gpu_timestamp: Option<Timestamp>, pub geometry_prepare_us: u64,
            pub gpu_poll_us: u64, pub present_copy_performed: bool, error: Option<&'static str>,
        }
        impl ResultFrame { pub fn completion_error(&self) -> Option<&'static str> { self.error } }
        pub fn render_resident_indexed_scene_frame_premultiplied_direct_to_surface(
            draws: &[ResidentSceneDraw<'_>], clear: Option<[u8; 4]>,
            destination: super::gpgpu::GpgpuRgba8Surface, diagnostics: bool,
        ) -> Result<ResultFrame, &'static str> {
            assert_eq!(draws.len(), 1); assert_eq!(draws[0].rgba, [255;4]);
            assert_eq!(clear, Some([0; 4])); assert!(!diagnostics);
            assert_eq!((destination.width, destination.height), (1536, 1056));
            render()
        }
        pub fn render_resident_dista_mask(_: &Mesh, _: &mut ResidentDistaMask) -> Result<ResultFrame,&'static str> { render() }
        fn render() -> Result<ResultFrame,&'static str> {
            let f = mutate(|s| { s.renders += 1;
                if s.renders > s.fail_after { s.fault } else { Fault::None } });
            match f {
                Fault::Busy => return Err("render-busy"),
                Fault::InFlight => return Err("in-flight"),
                Fault::Device => return Err("microfont-dista-device"),
                Fault::Submit => return Err("submit-timeout"),
                _ => {},
            }
            let clock = Some(Timestamp { ns: 500, ticks: 10, hz: 20_000_000 });
            Ok(ResultFrame {
                release_fence: if f == Fault::Fence { None } else { Some(()) },
                gpu_target_ready_timestamp: if f == Fault::TargetClock { None } else { clock },
                gpu_clear_setup_timestamp: if f == Fault::ClearClock { None } else { clock },
                gpu_writeback_timestamp: if f == Fault::WritebackClock { None } else { clock },
                gpu_timestamp: if f == Fault::DrawClock { None } else { clock },
                geometry_prepare_us: 0, gpu_poll_us: 0, present_copy_performed: false,
                error: if f == Fault::Complete { Some("geometry-incomplete") } else { None },
            })
        }
    }
}
mod ui4 {
    use crate::{Fault, fault, mutate};
    pub struct OutputId;
    impl OutputId { pub fn from_slot(slot: u8) -> Option<Self> {
        assert_eq!(slot, 0); if fault() == Fault::Output { None } else { Some(Self) }
    } }
    pub enum FrameContent { BlueprintScene }
    pub enum FrameCadence { Immutable }
    pub enum FrameBuffering { Single }
    pub enum ScanoutFormat { Rgba8888Premultiplied }
    pub struct FrameSpec {
        pub output: OutputId, pub content: FrameContent, pub cadence: FrameCadence,
        pub buffering: FrameBuffering, pub format: ScanoutFormat, pub width: u32,
        pub height: u32, pub base_color: Option<[u8; 4]>,
    }
    pub fn create_frame(spec: FrameSpec) -> Result<u32, ()> {
        assert_eq!((spec.width, spec.height), (1536, 1056));
        mutate(|s| s.frames += 1); if fault() == Fault::Create { Err(()) } else { Ok(1) }
    }
    pub fn acquire_frame_buffer(_: u32) -> Result<u32, ()> {
        mutate(|s| s.acquires += 1); if fault() == Fault::Acquire { Err(()) } else { Ok(2) }
    }
    pub fn gpgpu_rgba_surface(_: u32) -> Result<crate::intel::gpgpu::GpgpuRgba8Surface, ()> {
        if fault() == Fault::Surface { Err(()) } else {
            Ok(crate::intel::gpgpu::GpgpuRgba8Surface { width: 1536, height: 1056, bytes: 1536*1056*4, pitch_bytes:1536*4 })
        }
    }
    pub fn cancel_frame_buffer(_: u32) -> Result<(), ()> {
        mutate(|s| s.cancels += 1); if fault() == Fault::CleanupLease { Err(()) } else { Ok(()) }
    }
    pub fn destroy_frame(_: u32) -> Result<(), ()> {
        mutate(|s| s.destroys += 1); if fault() == Fault::CleanupFrame { Err(()) } else { Ok(()) }
    }
}
'''

VALIDATION = r'''
pub mod microfont_dista_validate {
    pub struct Snapshot;
    fn capture() -> Result<Snapshot,&'static str> {
        crate::mutate(|s|s.captures+=1);
        if crate::fault() == crate::Fault::Capture { Err("capture-failed") } else { Ok(Snapshot) }
    }
    pub unsafe fn capture_rgba_retired(_:crate::intel::gpgpu::GpgpuRgba8Surface) -> Result<Snapshot,&'static str> { capture() }
    pub unsafe fn capture_r8_retired(_:crate::intel::gpgpu::GpgpuMask8Surface) -> Result<Snapshot,&'static str> { capture() }
    pub struct Audit {
        pub checked_pixels:usize, pub mismatched_pixels:usize, pub nonbinary_pixels:usize, pub format_mismatches:usize,
        pub nonblank_glyphs:usize, pub space_pixels:usize, pub unused_pixels:usize,
        pub rgba_nonbinary_pixels:usize, pub rgba_channel_mismatches:usize, pub rgba_nonblank_glyphs:usize,
        pub rgba_space_pixels:usize, pub rgba_unused_pixels:usize, pub nonzero_pixels:usize,
        pub coverage_sum:u64, pub r8_hash:u64, pub rgba_hash:u64,
    }
    impl Audit { pub fn passed(&self)->bool { true } }
    pub fn compare(_: &Snapshot, _: &Snapshot) -> Audit {
        Audit { checked_pixels:1536*1056, mismatched_pixels:0, nonbinary_pixels:0, format_mismatches:0,
            nonblank_glyphs:94, space_pixels:0, unused_pixels:0, rgba_nonbinary_pixels:0,
            rgba_channel_mismatches:0, rgba_nonblank_glyphs:94, rgba_space_pixels:0,
            rgba_unused_pixels:0, nonzero_pixels:1, coverage_sum:255, r8_hash:1, rgba_hash:1 }
    }
}
'''

WORKER = r'''
pub mod blocking {
    pub fn try_spawn_blocking_job_with_purpose(job: crate::Job, purpose: &'static str)
        -> Result<(), crate::Job> {
        assert_eq!(purpose, "microfont-dista-bench");
        if crate::fault() == crate::Fault::Spawn { Err(job) }
        else { crate::mutate(|s| { assert!(s.pending.is_none()); s.pending = Some(job); }); Ok(()) }
    }
}
'''

TESTS = r'''
#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::{Fault, State, mutate};
    fn reset(fault: Fault) {
        mutate(|s| *s = State { fault, ..State::default() });
        RUNNING.store(false, Ordering::Release);
    }
    fn case(fault: Fault) -> Result<(), &'static str> {
        reset(fault); run_case(1, Quality::Fine, FontShape::Meso, false, OutputFormat::Rgba8, 2, 1_000_000_000).map(|_|())
    }
    fn ownership() -> (usize, usize, usize, usize) {
        mutate(|s| (s.mesh_releases, s.cancels, s.destroys, s.mesh_drops))
    }
    #[test]
    fn successful_first_and_warm_samples_reuse_then_release_all_resources() {
        assert_eq!(case(Fault::None), Ok(()));
        assert_eq!(ownership(), (1, 1, 1, 1));
        mutate(|s| {
            assert_eq!((s.frames, s.acquires, s.renders), (1, 1, 3));
            let samples: Vec<_> = s.logs.iter().filter(|r| r["kind"] == "sample").collect();
            assert_eq!(samples.len(), 3);
            assert!(samples[0]["first_target_ready_ns"].is_number());
            assert!(samples[1]["first_target_ready_ns"].is_null());
            assert_eq!(samples[2]["sample"], 2);
            assert!(samples.iter().all(|r| r["gpu_timing_available"] == true));
            assert!(samples.iter().all(|r| r["gpu_clear_setup_ns"] == 500 && r["gpu_writeback_ns"] == 500));
            assert!(samples.iter().all(|r|r["stage"] == "atlas-rebuild" && r["output_format"] == "rgba8"));
            assert_eq!(s.captures,1);
        });
    }
    #[test]
    fn r8_rebuilds_reuse_mask_then_validate_and_release_without_ui4_frame() {
        reset(Fault::None);
        assert!(run_case(1, Quality::Fine, FontShape::Meso, false, OutputFormat::R8, 2, 1_000_000_000).is_ok());
        assert_eq!(ownership(),(1,0,0,1));
        mutate(|s| {
            assert_eq!((s.frames,s.acquires,s.renders,s.masks,s.mask_releases,s.mask_drops,s.captures),(0,0,3,1,1,1,1));
            let setup=s.logs.iter().find(|r|r["kind"] == "setup").unwrap();
            assert_eq!(setup["output_format"],"r8");
            assert_eq!(setup["destination_bytes"],1536*1056);
            assert_eq!(setup["destination_pitch_bytes"],1536);
            assert_eq!(setup["cleanup_mask_released"],true);
            assert!(setup["cleanup_lease_cancelled"].is_null());
        });
    }
    #[test]
    fn r8_failure_paths_release_only_proven_retired_targets() {
        for (fault,error,released,dropped,captures) in [
            (Fault::MaskCreate,"mask-create",0,0,0),
            (Fault::Busy,"render-busy",1,1,0),
            (Fault::Capture,"capture-failed",1,1,1),
            (Fault::MaskRelease,"cleanup-incomplete",1,1,1),
            (Fault::TargetClock,"gpu-timestamps-unavailable",1,1,0),
            (Fault::Submit,"submit-timeout",0,0,0),
            (Fault::Fence,"target-release-missing",0,0,0),
        ] {
            reset(fault);
            assert_eq!(run_case(1, Quality::Fine, FontShape::Meso, false, OutputFormat::R8, 2, 1_000_000_000).map(|_|()),Err(error));
            mutate(|s|assert_eq!((s.mask_releases,s.mask_drops,s.captures),(released,dropped,captures)));
        }
    }
    #[test]
    fn successive_matrices_reverse_format_order_for_every_configuration() {
        let mut orders=Vec::new();
        for run in [1,2] {
            reset(Fault::None); assert_eq!(run_matrix(run,1),Ok(()));
            orders.push(mutate(|s|s.logs.iter().filter(|r|r["kind"] == "setup")
                .map(|r|(r["quality"].clone(),r["shape"].clone(),r["variant"].clone(),r["output_format"].clone())).collect::<Vec<_>>()));
        }
        for (a,b) in orders[0].chunks_exact(2).zip(orders[1].chunks_exact(2)) {
            assert_eq!((&a[0].0,&a[0].1,&a[0].2),(&b[0].0,&b[0].1,&b[0].2));
            assert_ne!(a[0].3,b[0].3); assert_eq!(a[0].3,b[1].3);
        }
    }
    #[test]
    fn pre_submission_failures_release_only_resources_already_created() {
        for (fault, error, expected) in [
            (Fault::Upload, "mesh-upload", (0, 0, 0, 0)),
            (Fault::Output, "output-unavailable", (1, 0, 0, 1)),
            (Fault::Create, "frame-create", (1, 0, 0, 1)),
            (Fault::Acquire, "frame-acquire", (1, 0, 1, 1)),
            (Fault::Surface, "frame-surface", (1, 1, 1, 1)),
            (Fault::Busy, "render-busy", (1, 1, 1, 1)),
            (Fault::InFlight, "in-flight", (1, 1, 1, 1)),
            (Fault::Device, "microfont-dista-device", (1, 1, 1, 1)),
        ] { assert_eq!(case(fault), Err(error)); assert_eq!(ownership(), expected); }
    }
    #[test]
    fn uncertain_writer_quarantines_mesh_and_destination_even_after_a_successful_sample() {
        for (fault, error) in [(Fault::Submit, "submit-timeout"),
            (Fault::Complete, "geometry-incomplete"), (Fault::Fence, "target-release-missing")] {
            for fail_after in [0, 1] {
                reset(fault); mutate(|s| s.fail_after = fail_after);
                assert_eq!(run_case(1, Quality::Fine, FontShape::Meso, false, OutputFormat::Rgba8, 2, 1_000_000_000).map(|_|()), Err(error));
                assert_eq!(ownership(), (0, 0, 0, 0));
                mutate(|s| {
                    assert_eq!(s.renders, fail_after + 1);
                    let setup = s.logs.iter().find(|r| r["kind"] == "setup").unwrap();
                    assert_eq!(setup["retained_on_uncertain_completion"], true);
                    assert!(setup["cleanup_mesh_released"].is_null());
                    assert!(setup["cleanup_lease_cancelled"].is_null());
                    assert!(setup["cleanup_frame_destroyed"].is_null());
                });
            }
        }
    }
    #[test]
    fn absent_gpu_clocks_fail_the_experiment_but_retirement_allows_cleanup() {
        for fault in [Fault::DrawClock, Fault::TargetClock, Fault::ClearClock, Fault::WritebackClock] {
            assert_eq!(case(fault), Err("gpu-timestamps-unavailable"));
            assert_eq!(ownership(), (1, 1, 1, 1));
            mutate(|s| assert_eq!(s.logs.iter().find(|r| r["kind"] == "sample").unwrap()["gpu_timing_available"], false));
        }
    }
    #[test]
    fn cleanup_failures_are_reported_with_the_failed_resource() {
        for (fault, key) in [(Fault::CleanupMesh, "cleanup_mesh_released"),
            (Fault::CleanupLease, "cleanup_lease_cancelled"), (Fault::CleanupFrame, "cleanup_frame_destroyed")] {
            assert_eq!(case(fault), Err("cleanup-incomplete"));
            mutate(|s| assert_eq!(s.logs.iter().find(|r| r["kind"] == "setup").unwrap()[key], false));
        }
    }
    #[test]
    fn request_guard_handles_invalid_busy_rejected_dropped_and_completed_jobs() {
        reset(Fault::None);
        for samples in [0, 16, 255] { assert_eq!(request(samples), Err("samples must be 1..15")); }
        assert!(!RUNNING.load(Ordering::Acquire));
        reset(Fault::Spawn);
        assert_eq!(request(1), Err("background worker busy"));
        assert!(!RUNNING.load(Ordering::Acquire));
        reset(Fault::None);
        assert_eq!(request(1), Ok(())); assert!(RUNNING.load(Ordering::Acquire));
        assert_eq!(request(1), Err("already running"));
        drop(mutate(|s| s.pending.take().unwrap()));
        assert!(!RUNNING.load(Ordering::Acquire));
        assert_eq!(request(1), Ok(()));
        let job = mutate(|s| s.pending.take().unwrap()); job();
        assert!(!RUNNING.load(Ordering::Acquire));
        mutate(|s| {
            assert_eq!((s.mesh_releases, s.cancels, s.destroys, s.renders), (24, 12, 12, 48));
            assert_eq!((s.masks,s.mask_releases,s.mask_drops,s.captures),(12,12,12,24));
            assert_eq!(s.logs.last().unwrap()["kind"], "done");
            assert_eq!(s.logs.last().unwrap()["cases"], 24);
            assert_eq!(s.logs.iter().filter(|r|r["kind"] == "validation").count(),12);
        });
        reset(Fault::Submit);
        assert_eq!(request(1), Ok(()));
        let job = mutate(|s| s.pending.take().unwrap()); job();
        assert!(!RUNNING.load(Ordering::Acquire));
        assert_eq!(ownership(), (0, 0, 0, 0));
        mutate(|s| {
            assert_eq!(s.logs.last().unwrap()["kind"], "error");
            assert_eq!(s.logs.last().unwrap()["reason"], "submit-timeout");
        });
    }
}
'''


def harness_source() -> str:
    tasks = ROOT / "src/r/services/microfont_dista_tasks.rs"
    bench = (ROOT / "src/r/services/microfont_dista_bench.rs").read_text()
    # The inner module doc belongs to the generated enclosing module.
    bench = "\n".join(line for line in bench.splitlines() if not line.startswith("//!"))
    return (MOCKS + "\nmod r {\n" + WORKER + "\npub mod services {\n"
            + f'#[path = {json.dumps(str(tasks))}] pub mod microfont_dista_tasks;\n' + VALIDATION
            + "pub mod microfont_dista_bench {\n" + bench + TESTS + "\n}\n}\n}\n")


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-dista-bench-tests-") as temporary:
        directory = Path(temporary)
        (directory / "src").mkdir()
        (directory / "src/lib.rs").write_text(harness_source())
        (directory / "Cargo.toml").write_text(
            '[package]\nname="trueos-dista-bench-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[features]\nmicrofont_dista_bench=[]\n[dependencies]\nserde_json="1.0"\n'
            f'microfont={{path={json.dumps(str(ROOT / "vendor/microfont"))},default-features=false,features=["raster"]}}\n'
        )
        subprocess.run(
            ["cargo", "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
             "--target", "x86_64-unknown-linux-gnu", "--features", "microfont_dista_bench",
             "--quiet", "--", "--test-threads=1"], cwd=directory, check=True,
        )


if __name__ == "__main__":
    main()
