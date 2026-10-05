#!/usr/bin/env python3
"""Host-test drawable-depth admission and indexed GPU resource retirement."""
from pathlib import Path
import subprocess
import tempfile
import test_clip_position3_uv_texture as production

ROOT = Path(__file__).resolve().parents[2]
production.ROOT = ROOT
item, constant = production.item, production.constant
api = 'crates/trueos-v/src/vgpu.rs'
depth = 'src/intel/render/drawable_depth.rs'
source = '''extern crate alloc;
mod intel { pub fn align_up(v: usize, a: usize) -> Option<usize> { v.checked_add(a - 1).map(|n| n & !(a - 1)) } }
'''
source += '\n'.join(constant('src/intel/render/constants.rs', n) for n in ['RESIDENT_SCENE_TARGET_WIDTH', 'RESIDENT_SCENE_TARGET_HEIGHT'])
source += '\n'.join(item(depth, n) for n in ['drawable_depth_bytes', 'drawable_depth_compare', 'drawable_depth_needs_clear', 'drawable_depth_tests'])
source += '\n'.join(constant(api, n) for n in ['INDEXED_DRAW_LOAD_COLOR', 'INDEXED_DRAW_DRAWABLE_DEPTH', 'INDEXED_DRAW_DEPTH_TEST', 'INDEXED_DRAW_DEPTH_WRITE', 'INDEXED_DRAW_CLEAR_DEPTH', 'INDEXED_DRAW_GEOMETRY_CLEAR', 'INDEXED_DRAW_DEPTH_COMPARE_SHIFT', 'INDEXED_DRAW_DEPTH_COMPARE_MASK', 'INDEXED_DRAW_FLAGS_ALL'])
source += item(api, 'indexed_draw_flags_valid')
source += 'mod v { pub mod vgpu { pub use crate::INDEXED_DRAW_CLEAR_DEPTH; } }'
source += '''
#[test]
fn legacy_and_owned_depth_flags_have_distinct_contracts() {
    assert!(indexed_draw_flags_valid(0));
    assert!(indexed_draw_flags_valid(INDEXED_DRAW_LOAD_COLOR));
    assert!(!indexed_draw_flags_valid(INDEXED_DRAW_CLEAR_DEPTH));
    assert!(!indexed_draw_flags_valid(INDEXED_DRAW_DEPTH_TEST));
    assert!(!indexed_draw_flags_valid(INDEXED_DRAW_DRAWABLE_DEPTH | INDEXED_DRAW_DEPTH_WRITE));
    for comparison in 0..8 {
        let flags = INDEXED_DRAW_LOAD_COLOR | INDEXED_DRAW_DRAWABLE_DEPTH | INDEXED_DRAW_DEPTH_TEST
            | (comparison << INDEXED_DRAW_DEPTH_COMPARE_SHIFT);
        assert!(indexed_draw_flags_valid(flags));
        assert!(indexed_draw_flags_valid(flags | INDEXED_DRAW_DEPTH_WRITE));
    }
    assert!(indexed_draw_flags_valid(INDEXED_DRAW_GEOMETRY_CLEAR));
    assert!(!indexed_draw_flags_valid(INDEXED_DRAW_GEOMETRY_CLEAR | INDEXED_DRAW_LOAD_COLOR));
    assert!(!indexed_draw_flags_valid(INDEXED_DRAW_GEOMETRY_CLEAR | INDEXED_DRAW_DRAWABLE_DEPTH));
    let clear = INDEXED_DRAW_GEOMETRY_CLEAR | INDEXED_DRAW_DRAWABLE_DEPTH | INDEXED_DRAW_CLEAR_DEPTH;
    assert!(indexed_draw_flags_valid(clear));
    assert!(indexed_draw_flags_valid(clear | INDEXED_DRAW_LOAD_COLOR));
    assert!(!indexed_draw_flags_valid(clear | INDEXED_DRAW_DEPTH_TEST));
    assert!(!indexed_draw_flags_valid(clear | INDEXED_DRAW_DEPTH_WRITE));
    assert!(!indexed_draw_flags_valid(clear | INDEXED_DRAW_DEPTH_COMPARE_MASK));
    for extra in [1 << 6, 1 << 7, 1 << 11, 1 << 31] {
        assert!(!indexed_draw_flags_valid(INDEXED_DRAW_DRAWABLE_DEPTH | extra));
    }
    assert!(indexed_draw_flags_valid(INDEXED_DRAW_DRAWABLE_DEPTH | INDEXED_DRAW_CLEAR_DEPTH));
    assert!(indexed_draw_flags_valid(INDEXED_DRAW_DRAWABLE_DEPTH | INDEXED_DRAW_CLEAR_DEPTH | INDEXED_DRAW_LOAD_COLOR));
}
'''
# Host and Blueprint consumers must agree on the complete wire flag definition.
peer = ROOT.parent / 'TRUEOS-Blueprints/crates/trueos-v/src/vgpu.rs'
if peer.exists():
    kernel_text = (ROOT / api).read_text()
    peer_text = peer.read_text()
    begin = 'pub const INDEXED_DRAW_LOAD_COLOR'
    end = 'pub const MAX_INDEXED_BATCH_DRAWS'
    assert kernel_text.split(begin)[1].split(end)[0].strip() == peer_text.split(begin)[1].split(end)[0].strip()
with tempfile.TemporaryDirectory(prefix='trueos-depth-') as tmp:
    src = Path(tmp) / 'depth.rs'
    exe = Path(tmp) / 'depth-tests'
    src.write_text(source)
    subprocess.run(['rustc', '--edition=2024', '--test', str(src), '-o', str(exe)], check=True)
    subprocess.run([str(exe)], check=True)

# Compile the actual renderer result/fence definitions and the broker's
# retirement policy. Execute both production resource-release blocks with
# instrumented physical releasers, so an incomplete Ok cannot free anything.
primary = 'src/intel/render/primary.rs'
broker = (ROOT / 'src/gpu/vgpu.rs').read_text()

def implementation(name):
    text = (ROOT / primary).read_text()
    start = text.index(f'impl {name} {{')
    end = text.index('\n}\n', start) + 3
    return text[start:end]

single_start = broker.index('    let release = ui4_indexed_target_release(&rendered, 1, phys, bytes);')
single_end = broker.index('    if transient_busy && released_mesh && released_texture {', single_start)
single_busy_start = broker.rindex('    let render_error = rendered.as_ref().err().copied();', 0, single_start)
single_busy_end = broker.index('    if let Some(reason) = render_error {', single_busy_start)
batch_start = broker.index('    let expected_draws = batch.draws.len();\n    let release = ui4_indexed_target_release(')
batch_end = broker.index('    if transient_busy && released_resources {', batch_start)
batch_busy_start = broker.rindex('    let render_error = rendered.as_ref().err().copied();', 0, batch_start)
retirement = '''#![allow(dead_code, unfulfilled_lint_expectations)]
mod intel { pub(crate) mod render {
'''
retirement += '\n'.join(item(primary, name) for name in (
    'ResidentSceneIncompleteStage', 'ResidentSceneFrameResult', 'ResidentSceneReleaseFence'))
retirement += '\n'.join(implementation(name) for name in (
    'ResidentSceneIncompleteStage', 'ResidentSceneFrameResult', 'ResidentSceneReleaseFence'))
retirement += r'''
    std::thread_local! {
        static RELEASES: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    pub(crate) struct TestMesh(pub usize);
    pub(crate) struct TestTexture;
    pub(crate) fn release_resident_triangle_mesh(mesh: &TestMesh) -> bool {
        RELEASES.with(|releases| releases.borrow_mut().push(mesh.0)); true
    }
    pub(crate) fn release_resident_sampled_texture(_: &TestTexture) -> bool {
        RELEASES.with(|releases| releases.borrow_mut().push(100)); true
    }
    fn take_releases() -> Vec<usize> {
        RELEASES.with(|releases| std::mem::take(&mut *releases.borrow_mut()))
    }
    fn completed(draws: usize) -> ResidentSceneFrameResult {
        ResidentSceneFrameResult {
            completed_draws: draws, requested_draws: draws, changed_pixels: 0,
            presented: false, width: 1280, height: 720, frame_us: 0, geometry_us: 0,
            geometry_prepare_us: 0, gpu_poll_us: 0, gpu_poll_iters: 0, resolve_us: 0,
            coverage_us: 0, present_copy_us: 0, present_copy_performed: false,
            coverage_submits: 0, coverage_walkers: 0, rgba: None, frame_complete: true,
            incomplete_stage: None,
            release_fence: Some(ResidentSceneReleaseFence { phys: 0x1000, byte_len: 4096, sequence: 7 }),
        }
    }
    #[test]
    fn incomplete_ok_or_untrusted_fence_retains_every_gpu_resource() {
        let mutations: [fn(&mut ResidentSceneFrameResult); 9] = [
            |result| { result.frame_complete = false; result.completed_draws = 0;
                result.incomplete_stage = Some(ResidentSceneIncompleteStage::Geometry);
                result.release_fence = None; },
            |result| result.frame_complete = false,
            |result| result.incomplete_stage = Some(ResidentSceneIncompleteStage::Geometry),
            |result| result.completed_draws -= 1,
            |result| result.requested_draws -= 1,
            |result| result.present_copy_performed = true,
            |result| result.release_fence = None,
            |result| result.release_fence.as_mut().unwrap().phys += 4096,
            |result| result.release_fence.as_mut().unwrap().byte_len -= 1,
        ];
        for mutate in mutations {
            for batch in [false, true] {
                let mut result = completed(if batch { 2 } else { 1 });
                mutate(&mut result);
                let release = if batch { crate::cleanup_batch(Ok(result)) }
                    else { crate::cleanup_single(Ok(result), false) };
                assert!(release.is_none());
                assert!(take_releases().is_empty(), "unretired allocation was released");
            }
        }
    }
    #[test]
    fn exact_completion_releases_single_mesh_texture_and_batch_meshes() {
        assert_eq!(crate::cleanup_single(Ok(completed(1)), false).unwrap().sequence(), 7);
        assert_eq!(take_releases(), [1, 100]);
        assert_eq!(crate::cleanup_single(Ok(completed(1)), true).unwrap().sequence(), 7);
        assert_eq!(take_releases(), [1]);
        assert_eq!(crate::cleanup_batch(Ok(completed(2))).unwrap().sequence(), 7);
        assert_eq!(take_releases(), [1, 2]);
    }
    #[test]
    fn only_pre_submission_busy_rejections_release_new_resources() {
        for reason in ["render-busy", "render-storage-busy", "timeout", "forcewake", "device-lost"] {
            let busy = matches!(reason, "render-busy" | "render-storage-busy");
            assert!(crate::cleanup_single(Err(reason), false).is_none());
            assert_eq!(take_releases(), if busy { vec![1, 100] } else { vec![] });
            assert!(crate::cleanup_batch(Err(reason)).is_none());
            assert_eq!(take_releases(), if busy { vec![1, 2] } else { vec![] });
        }
    }
}}
'''
retirement += item('src/gpu/vgpu.rs', 'ui4_indexed_target_release')
retirement += r'''
use intel::render::{ResidentSceneFrameResult, ResidentSceneReleaseFence, TestMesh, TestTexture};
struct TestDraw { retain_texture: bool }
struct TestBatch { draws: Vec<()> }
fn cleanup_single(rendered: Result<ResidentSceneFrameResult, &'static str>, retain_texture: bool)
    -> Option<ResidentSceneReleaseFence> {
    let phys = 0x1000; let bytes = 4096;
    let mesh = TestMesh(1); let draw = TestDraw { retain_texture };
    let sampled_texture = Some(Box::new(TestTexture));
'''
retirement += broker[single_busy_start:single_busy_end]
retirement += broker[single_start:single_end]
retirement += '    release.filter(|_| released_mesh && released_texture)\n}\n'
retirement += r'''
fn cleanup_batch(rendered: Result<ResidentSceneFrameResult, &'static str>)
    -> Option<ResidentSceneReleaseFence> {
    let phys = 0x1000; let bytes = 4096;
    let batch = TestBatch { draws: vec![(), ()] };
    let meshes = [TestMesh(1), TestMesh(2)];
'''
retirement += broker[batch_busy_start:batch_start]
retirement += broker[batch_start:batch_end]
retirement += '    release.filter(|_| released_resources)\n}\n'
with tempfile.TemporaryDirectory(prefix='trueos-indexed-retirement-') as tmp:
    src = Path(tmp) / 'retirement.rs'
    exe = Path(tmp) / 'retirement-tests'
    src.write_text(retirement)
    subprocess.run(['rustc', '--edition=2024', '--test', str(src), '-o', str(exe)], check=True)
    subprocess.run([str(exe)], check=True)
