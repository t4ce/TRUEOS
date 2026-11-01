#!/usr/bin/env python3
"""Exercise exact-allocation native producer supersession without booting UI4.

The actual completion functions, sprite admission and clear policy compile
unchanged. Only owner-scoped surfaces, leases and native fences are modeled.
"""
from pathlib import Path
import re
import subprocess
import tempfile
from test_vgpu_batch_overlay import block

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / 'src/ui4/blueprint_ui4.rs').read_text()
    constants = '\n'.join(re.findall(
        r'^(?:pub\(crate\) )?const (?:MAX_SPRITE_QUADS|ERROR_INVALID|ERROR_NOT_FOUND|ERROR_STATE|ERROR_BUSY|ERROR_UI4):[^;]+;', source, re.M))
    harness = r'''#![allow(dead_code)]
use std::sync::Mutex;
type WindowOwner = u32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fence { phys: u64, bytes: usize, sequence: u64 }
impl Fence { fn matches(self, phys: u64, bytes: usize) -> bool { self.phys == phys && self.bytes == bytes } }
type GpgpuRgba8ReleaseFence = Fence;
mod intel { pub mod render { pub(crate) type ResidentSceneReleaseFence = crate::Fence; } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Lease { phys: u64, bytes: usize, valid: bool }
struct Destination { phys: u64, bytes: usize }
fn gpgpu_rgba_surface(lease: Lease) -> Result<Destination, ()> {
    if !lease.valid { return Err(()); }
    Ok(Destination { phys: lease.phys, bytes: lease.bytes })
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct SpriteSceneUpload { expected: usize, quads: Vec<()> }
#[derive(Clone, Debug, PartialEq, Eq)]
struct BlueprintSceneSurface {
    owner: WindowOwner, window_id: u32, write_lease: Option<Lease>,
    vgpu_surface: Option<u64>, gpu_submission_unretired: bool,
    pending_gpu_release: Option<Fence>, pending_render_release: Option<Fence>,
    sprite_clear_rgba: Option<u32>, sprite_scene_upload: Option<SpriteSceneUpload>,
}
struct Surfaces(Mutex<Vec<BlueprintSceneSurface>>);
impl Surfaces { fn lock(&self) -> std::sync::MutexGuard<'_, Vec<BlueprintSceneSurface>> { self.0.lock().unwrap() } }
static SURFACES: Surfaces = Surfaces(Mutex::new(Vec::new()));
fn surface_mut(s: &mut [BlueprintSceneSurface], owner: WindowOwner, window_id: u32) -> Option<&mut BlueprintSceneSurface> {
    s.iter_mut().find(|surface| surface.owner == owner && surface.window_id == window_id)
}
fn old_compute() -> Fence { Fence { phys: 0x1000, bytes: 4096, sequence: 1 } }
fn old_resident() -> Fence { Fence { phys: 0x1000, bytes: 4096, sequence: 2 } }
fn final_release() -> Fence { Fence { phys: 0x1000, bytes: 4096, sequence: 3 } }
fn surface() -> BlueprintSceneSurface {
    BlueprintSceneSurface {
        owner: 7, window_id: 42,
        write_lease: Some(Lease { phys: 0x1000, bytes: 4096, valid: true }),
        vgpu_surface: Some(99), gpu_submission_unretired: true,
        pending_gpu_release: Some(old_compute()), pending_render_release: Some(old_resident()),
        sprite_clear_rgba: Some(0x12345678), sprite_scene_upload: None,
    }
}
fn install(surface: BlueprintSceneSurface) { *SURFACES.lock() = vec![surface]; }
fn snapshot() -> BlueprintSceneSurface { SURFACES.lock()[0].clone() }
'''
    harness += constants + '\n'
    for start in [
        'pub(crate) fn complete_vgpu_surface_submission(',
        'pub(crate) fn complete_vgpu_resident_surface_submission(',
        'fn sprite_scene_clear_rgba(',
        'pub(crate) fn begin_sprite_scene(',
        'const fn sprite_scene_needs_clear(',
    ]:
        harness += block(source, start) + '\n'
    harness += r'''
#[test]
fn matching_resident_completion_supersedes_compute_and_retains_sprite_overlay_admission() {
    for initial_clear in [None, Some(0x12345678)] {
        let mut initial = surface(); initial.sprite_clear_rgba = initial_clear;
        install(initial);
        assert_eq!(complete_vgpu_resident_surface_submission(7, 42, 99, final_release()), Ok(()));
        let current = snapshot();
        assert_eq!(current.pending_gpu_release, None);
        assert_eq!(current.pending_render_release, Some(final_release()));
        assert_eq!(current.sprite_clear_rgba, initial_clear);
        assert_eq!(current.vgpu_surface, None);
        assert!(!current.gpu_submission_unretired);
        assert_eq!(begin_sprite_scene(7, 42, 1), 0);
        assert!(!sprite_scene_needs_clear(current.pending_render_release.is_some(), false));
    }
}
#[test]
fn matching_compute_completion_supersedes_resident_without_fabricating_clear_marker() {
    for initial_clear in [None, Some(0x12345678)] {
        let mut initial = surface(); initial.sprite_clear_rgba = initial_clear;
        install(initial);
        assert_eq!(complete_vgpu_surface_submission(7, 42, 99, final_release()), Ok(()));
        let current = snapshot();
        assert_eq!(current.pending_gpu_release, Some(final_release()));
        assert_eq!(current.pending_render_release, None);
        assert_eq!(current.sprite_clear_rgba, initial_clear);
        assert_eq!(current.vgpu_surface, None);
        assert!(!current.gpu_submission_unretired);
    }
}
#[test]
fn failed_completion_does_not_replace_either_proof_or_release_imported_surface() {
    type Completion = fn(WindowOwner, u32, u64, Fence) -> Result<(), i32>;
    for complete in [complete_vgpu_surface_submission as Completion, complete_vgpu_resident_surface_submission as Completion] {
        for case in 0..8 {
            let mut before = surface();
            let (mut owner, mut window, mut handle, mut release) = (7, 42, 99, final_release());
            let expected = match case {
                0 => { owner = 8; ERROR_NOT_FOUND }
                1 => { window = 43; ERROR_NOT_FOUND }
                2 => { handle = 100; ERROR_STATE }
                3 => { before.gpu_submission_unretired = false; ERROR_STATE }
                4 => { before.write_lease = None; ERROR_STATE }
                5 => { before.write_lease.as_mut().unwrap().valid = false; ERROR_UI4 }
                6 => { release.phys += 4096; ERROR_UI4 }
                _ => { release.bytes += 4096; ERROR_UI4 }
            };
            install(before.clone());
            assert_eq!(complete(owner, window, handle, release), Err(expected), "case {case}");
            assert_eq!(snapshot(), before, "failed completion modified state in case {case}");
        }
    }
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-release-supersession-') as temporary:
        directory = Path(temporary)
        fixture = directory / 'test.rs'
        binary = directory / 'tests'
        fixture.write_text(harness)
        subprocess.run(['rustc', '--edition=2024', '--test', str(fixture), '-o', str(binary)], check=True)
        subprocess.run([str(binary), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
