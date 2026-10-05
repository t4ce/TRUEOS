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
            let mut result = completed(1);
            mutate(&mut result);
            assert!(crate::cleanup_cached_single(Ok(result), false, true).is_none());
            assert!(take_releases().is_empty(), "unretired cached allocation was released");
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
    #[test]
    fn voxy_implicitly_retains_mesh_and_atlas_without_the_load_color_flag() {
        assert_eq!(crate::cleanup_cached_single(Ok(completed(1)), false, true).unwrap().sequence(), 7);
        assert!(take_releases().is_empty());
        assert_eq!(crate::cleanup_cached_single(Ok(completed(1)), true, true).unwrap().sequence(), 7);
        assert!(take_releases().is_empty());
        for reason in ["render-busy", "render-storage-busy", "device-lost"] {
            assert!(crate::cleanup_cached_single(Err(reason), false, true).is_none());
            assert!(take_releases().is_empty());
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
    cleanup_cached_single(rendered, retain_texture, false)
}
fn cleanup_cached_single(rendered: Result<ResidentSceneFrameResult, &'static str>, retain_texture: bool,
    cached_voxy_mesh: bool) -> Option<ResidentSceneReleaseFence> {
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

# Compile the actual Voxy streaming constructor and byte-writing mutator.
# A recording allocator replaces only DMA/map admission; stable layout,
# validation, uploads and the indirect record are production code.
resources = 'src/intel/render/resources.rs'
streaming = '''#![allow(dead_code, unfulfilled_lint_expectations)]
type PicassoCarrierLease = ();
mod intel {
    pub fn align_up(v: usize, a: usize) -> Option<usize> {
        v.checked_add(a - 1).map(|n| n & !(a - 1))
    }
    pub fn voxy_headless_target_active() -> bool { true }
    std::thread_local! {
        static FLUSHES: std::cell::RefCell<Vec<(usize, usize)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    pub fn dma_flush(ptr: *mut u8, bytes: usize) {
        FLUSHES.with(|flushes| flushes.borrow_mut().push((ptr as usize, bytes)));
    }
    pub fn take_flushes() -> Vec<(usize, usize)> {
        FLUSHES.with(|flushes| std::mem::take(&mut *flushes.borrow_mut()))
    }
}
std::thread_local! {
    static ALLOCATIONS: std::cell::RefCell<Vec<Box<[u8]>>> = const { std::cell::RefCell::new(Vec::new()) };
}
'''
streaming += '\n'.join(item('src/intel/render/state.rs', name) for name in (
    'TriangleVertexFormat', 'ResidentTriangleMesh'))
streaming += '\n'.join(constant(resources, name) for name in (
    'VOXY_HEADLESS_MAX_STREAM_CAPACITY', 'VOXY_HEADLESS_VERTEX_STRIDE', 'VOXY_HEADLESS_CAMERA_BYTES'))
streaming += '\n'.join(constant('src/intel/render/constants.rs', name) for name in (
    'DRAW_INDEXED_INDIRECT_DWORDS', 'DRAW_INDEXED_INDIRECT_BYTES'))
streaming += '\n'.join(item(resources, name) for name in (
    'voxy_headless_stream_capacity', 'voxy_headless_stream_layout',
    'voxy_headless_stream_storage_bytes', 'validate_voxy_headless_stream_shape',
    'create_resident_voxy_headless_streaming_mesh', 'update_resident_voxy_headless_streaming_mesh',
    'update_resident_triangle_draw_indexed_indirect', 'voxy_headless_streaming_tests'))
streaming += r'''
fn create_resident_triangle_mesh_typed(
    vertices: &[[f32; 8]], indices: &[u32], format: TriangleVertexFormat,
    carrier: Option<PicassoCarrierLease>,
) -> Result<ResidentTriangleMesh, &'static str> {
    let vertex_bytes = core::mem::size_of_val(vertices);
    let index = intel::align_up(vertex_bytes, 64).unwrap();
    let index_bytes = core::mem::size_of_val(indices);
    let indirect = intel::align_up(index + index_bytes, 64).unwrap();
    let bytes = intel::align_up(indirect + 20, 4096).unwrap();
    let mut allocation = vec![0u8; bytes].into_boxed_slice();
    let ptr = allocation.as_mut_ptr();
    unsafe {
        core::ptr::copy_nonoverlapping(vertices.as_ptr().cast::<u8>(), ptr, vertex_bytes);
        core::ptr::copy_nonoverlapping(indices.as_ptr().cast::<u8>(), ptr.add(index), index_bytes);
    }
    intel::dma_flush(ptr, bytes);
    ALLOCATIONS.with(|allocations| allocations.borrow_mut().push(allocation));
    let gpu = 0x2000_0000;
    Ok(ResidentTriangleMesh {
        storage_phys: 0x1_0010_0000, storage_virt: ptr, storage_bytes: bytes,
        gpu_base: gpu, vertex_gpu_addr: gpu, vertex_count: vertices.len() as u32,
        vertex_bytes: vertex_bytes as u32, vertex_stride: 32, vertex_format: format,
        index_gpu_addr: gpu + index as u64, index_count: indices.len() as u32,
        index_bytes: index_bytes as u32, indirect_args_gpu_addr: gpu + indirect as u64,
        indirect_args_offset: indirect, carrier,
    })
}
fn release_resident_triangle_mesh(_: &ResidentTriangleMesh) -> bool { true }
fn live_camera() -> [f32;20] {
    let mut camera = [1.0;20]; camera[18] = 0.1; camera[19] = 256.0; camera
}
#[test]
fn constructor_keeps_capacity_descriptors_and_flushes_only_live_update_ranges() {
    let mesh = create_resident_voxy_headless_streaming_mesh(
        9, &[[1.0;16];3], &[0,1,2,2,1,0], &live_camera()).unwrap();
    assert_eq!(mesh.vertex_count, 9);
    assert_eq!(mesh.vertex_bytes, 288);
    assert_eq!(mesh.index_count, 9);
    assert_eq!(mesh.index_bytes, 36);
    assert_eq!(mesh.index_gpu_addr - mesh.gpu_base, 384);
    assert_eq!(mesh.indirect_args_offset, 448);
    let ptr = mesh.storage_virt as usize;
    assert_eq!(intel::take_flushes(), vec![(ptr,4096), (ptr,96), (ptr+288,96), (ptr+384,24), (ptr+448,20)]);
    let record = unsafe { core::slice::from_raw_parts(mesh.storage_virt.add(448),20) };
    assert_eq!(record, &[6u32,1,0,0,0].iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<_>>());
    update_resident_voxy_headless_streaming_mesh(&mesh, &[[2.0;16];6], &[0,1,2], &live_camera()).unwrap();
    assert_eq!(intel::take_flushes(), vec![(ptr,192), (ptr+288,96), (ptr+384,12), (ptr+448,20)]);
}
#[test]
fn invalid_constructor_input_does_not_reach_allocation_or_flush() {
    for capacity in [0,4,780003,usize::MAX] {
        assert!(create_resident_voxy_headless_streaming_mesh(capacity, &[[1.0;16];3], &[0,1,2], &live_camera()).is_err());
    }
    assert!(create_resident_voxy_headless_streaming_mesh(3, &[[1.0;16];3], &[0,1,3], &live_camera()).is_err());
    assert!(intel::take_flushes().is_empty());
    assert!(ALLOCATIONS.with(|allocations| allocations.borrow().is_empty()));
}
'''
with tempfile.TemporaryDirectory(prefix='trueos-voxy-streaming-') as tmp:
    src = Path(tmp) / 'streaming.rs'
    exe = Path(tmp) / 'streaming-tests'
    src.write_text(streaming)
    subprocess.run(['rustc', '--edition=2024', '--test', str(src), '-o', str(exe)], check=True)
    subprocess.run([str(exe)], check=True)

# Exercise the broker's actual owner-matching helper: an unrelated indexed
# operation must not unlock CPU writes to a still-live streaming allocation.
lease = '''
type QueueHandle = u64;
type SurfaceHandle = u64;
struct VirtualDevice {
    voxy_stream_owner: Option<(QueueHandle, SurfaceHandle)>,
    voxy_stream_in_flight: bool,
    voxy_stream_quarantined: bool,
}
'''
lease += item('src/gpu/vgpu.rs', 'clear_voxy_stream_lease')
lease += r'''
#[test]
fn foreign_queue_or_surface_keeps_the_exact_stream_owner_and_lease() {
    let mut device = VirtualDevice {
        voxy_stream_owner: Some((11,21)), voxy_stream_in_flight: true,
        voxy_stream_quarantined: false,
    };
    for (queue, surface) in [(12,21), (11,22), (12,22)] {
        clear_voxy_stream_lease(&mut device, queue, surface);
        assert_eq!(device.voxy_stream_owner, Some((11,21)));
        assert!(device.voxy_stream_in_flight);
        assert!(!device.voxy_stream_quarantined);
    }
    // An absent ownership receipt never authorizes release, even if another
    // operation happens to observe the boolean reservation.
    device.voxy_stream_owner = None;
    clear_voxy_stream_lease(&mut device,11,21);
    assert!(device.voxy_stream_in_flight);
}
#[test]
fn exact_owner_clears_only_its_operation_lease_and_preserves_quarantine() {
    let mut device = VirtualDevice {
        voxy_stream_owner: Some((11,21)), voxy_stream_in_flight: true,
        voxy_stream_quarantined: true,
    };
    clear_voxy_stream_lease(&mut device,11,21);
    assert_eq!(device.voxy_stream_owner,None);
    assert!(!device.voxy_stream_in_flight);
    assert!(device.voxy_stream_quarantined);
    clear_voxy_stream_lease(&mut device,11,21);
    assert_eq!(device.voxy_stream_owner,None);
    assert!(!device.voxy_stream_in_flight);
    assert!(device.voxy_stream_quarantined);
}
'''
with tempfile.TemporaryDirectory(prefix='trueos-voxy-lease-') as tmp:
    src = Path(tmp) / 'lease.rs'
    exe = Path(tmp) / 'lease-tests'
    src.write_text(lease)
    subprocess.run(['rustc', '--edition=2024', '--test', str(src), '-o', str(exe)], check=True)
    subprocess.run([str(exe)], check=True)

# Compile the real mutable-atlas guard against recording storage. Hardware
# writes are replaced only at the storage boundary; Arc pinning, validation,
# range admission and error propagation stay in the production helper.
atlas = r'''#![allow(dead_code)]
use std::{cell::{Cell, RefCell}, sync::Arc};
#[derive(Debug, PartialEq)]
enum VgpuError { Busy, Unsupported }
struct RecordingStorage {
    gpu_base: u64,
    pixels: RefCell<Vec<u8>>,
    writes: Cell<usize>,
    flushes: RefCell<Vec<(usize, usize)>>,
}
impl RecordingStorage {
    fn write_and_flush(&self, offset: usize, bytes: &[u8]) -> bool {
        let Some(end) = offset.checked_add(bytes.len()) else { return false; };
        let mut pixels = self.pixels.borrow_mut();
        if end > pixels.len() { return false; }
        pixels[offset..end].copy_from_slice(bytes);
        self.writes.set(self.writes.get() + 1);
        if !bytes.is_empty() { self.flushes.borrow_mut().push((offset, bytes.len())); }
        true
    }
}
mod intel { pub mod render {
    pub struct ResidentSampledTexture { pub storage: crate::RecordingStorage }
}}
'''
atlas += item('src/gpu/vgpu.rs', 'SampledBufferCache')
atlas += item('src/gpu/vgpu.rs', 'update_streaming_sampled_buffer')
atlas += r'''
fn cache(streaming: bool, logical_bytes: usize, storage_bytes: usize) -> SampledBufferCache {
    SampledBufferCache {
        shape: [4, 1, 16, 0], bytes: logical_bytes, streaming,
        resident: Arc::new(intel::render::ResidentSampledTexture {
            storage: RecordingStorage {
                gpu_base: 0x2200_0000, pixels: RefCell::new(vec![0xA5; storage_bytes]),
                writes: Cell::new(0), flushes: RefCell::new(Vec::new()),
            },
        }),
    }
}
fn assert_untouched(cache: &SampledBufferCache) {
    let storage = &cache.resident.storage;
    assert!(storage.pixels.borrow().iter().all(|b| *b == 0xA5));
    assert_eq!(storage.writes.get(), 0);
    assert!(storage.flushes.borrow().is_empty());
}
#[test]
fn exclusive_writes_preserve_resident_address_and_flush_only_dirty_ranges() {
    let cache = cache(true, 16, 16);
    let address = cache.resident.storage.gpu_base;
    let identity = Arc::as_ptr(&cache.resident);
    assert_eq!(update_streaming_sampled_buffer(&cache, 4, &[1, 2, 3, 4]), Ok(()));
    assert_eq!(update_streaming_sampled_buffer(&cache, 12, &[5, 6, 7, 8]), Ok(()));
    assert_eq!(*cache.resident.storage.pixels.borrow(), [
        0xA5, 0xA5, 0xA5, 0xA5, 1, 2, 3, 4, 0xA5, 0xA5, 0xA5, 0xA5, 5, 6, 7, 8,
    ]);
    assert_eq!(*cache.resident.storage.flushes.borrow(), [(4, 4), (12, 4)]);
    assert_eq!(cache.resident.storage.writes.get(), 2);
    assert_eq!(cache.resident.storage.gpu_base, address);
    assert_eq!(Arc::as_ptr(&cache.resident), identity);
    assert_eq!(Arc::strong_count(&cache.resident), 1);
}
#[test]
fn a_staged_gpu_arc_blocks_mutation_until_exact_owner_releases_it() {
    let cache = cache(true, 16, 16);
    let staged = Arc::clone(&cache.resident);
    assert_eq!(update_streaming_sampled_buffer(&cache, 0, &[1, 2, 3, 4]), Err(VgpuError::Busy));
    assert_untouched(&cache);
    drop(staged);
    assert_eq!(update_streaming_sampled_buffer(&cache, 0, &[1, 2, 3, 4]), Ok(()));
    assert_eq!(&cache.resident.storage.pixels.borrow()[..4], &[1, 2, 3, 4]);
    assert_eq!(*cache.resident.storage.flushes.borrow(), [(0, 4)]);
}
#[test]
fn logical_bounds_and_overflow_reject_writes_into_page_padding() {
    let cache = cache(true, 16, 4096);
    for (offset, bytes) in [(usize::MAX, &[1u8][..]), (15, &[1, 2][..]), (16, &[1][..]), (17, &[][..])] {
        assert_eq!(update_streaming_sampled_buffer(&cache, offset, bytes), Err(VgpuError::Unsupported));
        assert_untouched(&cache);
    }
}
#[test]
fn immutable_caches_and_rejected_storage_writes_remain_unchanged() {
    let immutable = cache(false, 16, 16);
    assert_eq!(update_streaming_sampled_buffer(&immutable, 0, &[1, 2, 3, 4]), Err(VgpuError::Unsupported));
    assert_untouched(&immutable);
    // A broken storage extent must fail rather than copy part of an upload.
    let short = cache(true, 16, 8);
    assert_eq!(update_streaming_sampled_buffer(&short, 7, &[1, 2]), Err(VgpuError::Unsupported));
    assert_untouched(&short);
}
'''
with tempfile.TemporaryDirectory(prefix='trueos-voxy-atlas-') as tmp:
    src = Path(tmp) / 'atlas.rs'
    exe = Path(tmp) / 'atlas-tests'
    src.write_text(atlas)
    subprocess.run(['rustc', '--edition=2024', '--test', str(src), '-o', str(exe)], check=True)
    subprocess.run([str(exe)], check=True)
