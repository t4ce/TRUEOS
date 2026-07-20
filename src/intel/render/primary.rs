use alloc::vec::Vec;

pub(crate) fn submit_primary_triangle_once() {
    if PRIMARY_TRIANGLE_SUBMITTED.swap(true, Ordering::AcqRel) {
        return;
    }

    let _ = submit_primary_probe_now("boot-once");
}

pub(crate) fn submit_primary_probe_periodic() {
    let _ = submit_primary_probe_now("periodic");
}

pub(crate) struct RenderJokerResult {
    pub(crate) variant: &'static str,
    pub(crate) submit_name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) completed: bool,
    pub(crate) vs_counter: bool,
    pub(crate) ps_state_marker: bool,
    pub(crate) raster_packet: bool,
    pub(crate) clip_counter: bool,
    pub(crate) ps_observed: bool,
}

/// CPU-visible copy of one completed font render target. The bounded 3x3 demo
/// compositor needs all nine independently colored results before committing
/// the single display overlay plane.
pub(crate) struct FontRenderTargetReadback {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

pub(crate) fn submit_font_mesh_once(
    vertices: &[[f32; 2]],
    indices: &[u32],
    bounds: (f32, f32, f32, f32),
) -> Result<RenderJokerResult, &'static str> {
    submit_font_mesh_once_scaled(vertices, indices, bounds, FONT_STAMP_DEFAULT_NATIVE_SCALE)
}

pub(crate) const fn font_native_scale_supported(native_scale: u32) -> bool {
    native_scale > 0 && native_scale <= FONT_STAMP_MAX_NATIVE_SCALE
}

pub(crate) const fn font_native_scale_count() -> u32 {
    FONT_STAMP_MAX_NATIVE_SCALE
}

/// Return the square render-target extent selected by one supported native
/// font scale. Consumers use this instead of duplicating the render backend's
/// scale-to-pixel mapping.
pub(crate) const fn font_native_scale_target_pixels(native_scale: u32) -> Option<u32> {
    if font_native_scale_supported(native_scale) {
        Some(FONT_STAMP_BASE_SIZE as u32 * native_scale)
    } else {
        None
    }
}

pub(crate) fn transient_font_mesh_upload_bytes(
    vertex_count: usize,
    index_count: usize,
) -> Option<usize> {
    let Some(vertex_bytes) = vertex_count.checked_mul(3 * core::mem::size_of::<f32>()) else {
        return None;
    };
    let Some(index_offset) = crate::intel::align_up(vertex_bytes, 64) else {
        return None;
    };
    let Some(index_bytes) = index_count.checked_mul(core::mem::size_of::<u32>()) else {
        return None;
    };
    index_offset.checked_add(index_bytes)
}

pub(crate) const fn transient_font_mesh_upload_capacity_bytes() -> usize {
    WARM_VERTEX_BYTES
}

pub(crate) const fn transient_font_mesh_refinement_budget_bytes() -> usize {
    FONT_MESH_REFINEMENT_BUDGET_BYTES
}

/// Submit the already-tessellated font mesh at a native pixel scale.
///
/// Scaling is performed by the 3D viewport after tessellation: the mesh and
/// index topology remain unchanged, and presentation remains a 1:1 copy.
pub(crate) fn submit_font_mesh_once_scaled(
    vertices: &[[f32; 2]],
    indices: &[u32],
    bounds: (f32, f32, f32, f32),
    native_scale: u32,
) -> Result<RenderJokerResult, &'static str> {
    submit_font_mesh_once_scaled_inner(vertices, indices, bounds, native_scale, None)
}

/// Render a transient font mesh once and return its transparent native-size
/// RGBA target instead of claiming the hardware overlay plane.
pub(crate) fn submit_font_mesh_readback_once_scaled(
    vertices: &[[f32; 2]],
    indices: &[u32],
    bounds: (f32, f32, f32, f32),
    native_scale: u32,
) -> Result<(RenderJokerResult, Option<FontRenderTargetReadback>), &'static str> {
    let mut readback = None;
    let render = submit_font_mesh_once_scaled_inner(
        vertices,
        indices,
        bounds,
        native_scale,
        Some(&mut readback),
    )?;
    Ok((render, readback))
}

/// Render one transient font mesh into a caller-sized rectangular target.
/// The supplied pixel allocation is recycled into the completed readback so
/// repeated shell stamps do not repeatedly allocate native-size RGBA buffers.
pub(crate) fn submit_font_mesh_readback_once_at_extent_reusing(
    vertices: &[[f32; 2]],
    indices: &[u32],
    bounds: (f32, f32, f32, f32),
    target_width: u32,
    target_height: u32,
    padding_pixels: u32,
    reusable_pixels: Vec<u8>,
) -> Result<(RenderJokerResult, FontRenderTargetReadback), &'static str> {
    let mut readback = Some(FontRenderTargetReadback {
        width: 0,
        height: 0,
        pixels: reusable_pixels,
    });
    let render = submit_font_mesh_once_at_extent_inner(
        vertices,
        indices,
        bounds,
        target_width,
        target_height,
        padding_pixels,
        Some(&mut readback),
    )?;
    Ok((render, readback.expect("font readback recycle slot")))
}

fn submit_font_mesh_once_scaled_inner(
    vertices: &[[f32; 2]],
    indices: &[u32],
    bounds: (f32, f32, f32, f32),
    native_scale: u32,
    readback: Option<&mut Option<FontRenderTargetReadback>>,
) -> Result<RenderJokerResult, &'static str> {
    if !font_native_scale_supported(native_scale) {
        return Err("font-native-scale-range");
    }
    let target_size = (FONT_STAMP_BASE_SIZE as u32)
        .checked_mul(native_scale)
        .ok_or("font-target-size-overflow")?;
    submit_font_mesh_once_at_extent_inner(
        vertices,
        indices,
        bounds,
        target_size,
        target_size,
        target_size / 20,
        readback,
    )
}

fn submit_font_mesh_once_at_extent_inner(
    vertices: &[[f32; 2]],
    indices: &[u32],
    bounds: (f32, f32, f32, f32),
    target_width: u32,
    target_height: u32,
    padding_pixels: u32,
    readback: Option<&mut Option<FontRenderTargetReadback>>,
) -> Result<RenderJokerResult, &'static str> {
    if vertices.is_empty() || indices.is_empty() || !indices.len().is_multiple_of(3) {
        return Err("font-mesh-shape");
    }
    if target_width == 0
        || target_height == 0
        || target_width as usize > DRAW3D_SCENE_TARGET_WIDTH
        || target_height as usize > DRAW3D_SCENE_TARGET_HEIGHT
    {
        return Err("font-target-extent-range");
    }
    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("in-flight");
    }

    let (min_x, min_y, max_x, max_y) = bounds;
    let width = (max_x - min_x).max(1.0);
    let height = (max_y - min_y).max(1.0);
    let padding_pixels = padding_pixels
        .min(target_width.saturating_sub(1) / 2)
        .min(target_height.saturating_sub(1) / 2);
    let content_width = target_width
        .saturating_sub(padding_pixels.saturating_mul(2))
        .max(1);
    let content_height = target_height
        .saturating_sub(padding_pixels.saturating_mul(2))
        .max(1);
    let pixel_scale = (content_width as f32 / width).min(content_height as f32 / height);
    let center_x = (min_x + max_x) * 0.5;
    let center_y = (min_y + max_y) * 0.5;
    let ndc_x_scale = 2.0 * pixel_scale / target_width as f32;
    let ndc_y_scale = 2.0 * pixel_scale / target_height as f32;
    let mut draw_vertices = Vec::with_capacity(vertices.len());
    for source in vertices {
        draw_vertices.push([
            (source[0] - center_x) * ndc_x_scale,
            (center_y - source[1]) * ndc_y_scale,
            0.5,
        ]);
    }
    let mut draw_indices = Vec::with_capacity(indices.len());
    for triangle in indices.chunks_exact(3) {
        let Some(v0) = draw_vertices.get(triangle[0] as usize) else {
            PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
            return Err("font-index-range");
        };
        let Some(v1) = draw_vertices.get(triangle[1] as usize) else {
            PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
            return Err("font-index-range");
        };
        let Some(v2) = draw_vertices.get(triangle[2] as usize) else {
            PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
            return Err("font-index-range");
        };
        let area2 = (v1[0] - v0[0]) * (v2[1] - v0[1]) - (v1[1] - v0[1]) * (v2[0] - v0[0]);
        if area2 < 0.0 {
            draw_indices.extend_from_slice(&[triangle[0], triangle[2], triangle[1]]);
        } else {
            draw_indices.extend_from_slice(triangle);
        }
    }

    let result = submit_render_custom_triangle_probe_locked_at_extent(
        &draw_vertices,
        Some(&draw_indices),
        None,
        None,
        "font-tessel-once",
        "font-tessel-3d-once",
        "font-tessel-full-mesh",
        "path-fill-indexed-mesh/gpu-index-fetch",
        TriangleBlendProbeMode::MesaZeroedState,
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::HeavyAll,
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
        TriangleBatchMode::Draw,
        StreamoutProofExperiment::HeaderAndPositionSlots01,
        target_width as usize,
        target_height as usize,
        readback,
    );
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    result
}

/// Draw a kernel-owned font mesh directly from its persistent render-PPGTT
/// allocation. Only transient pipeline state and the target are rebuilt; the
/// vertex and index bytes are neither copied nor uploaded again.
pub(crate) fn submit_resident_font_mesh_once(
    mesh: &ResidentFontMesh,
    native_scale: u32,
    rgba: crate::intel::gpu_font::GpuFontRgba,
) -> Result<RenderJokerResult, &'static str> {
    submit_resident_font_mesh_inner(mesh, native_scale, rgba, None)
}

pub(crate) struct ResidentSceneDraw<'a> {
    pub(crate) mesh: &'a ResidentTriangleMesh,
    pub(crate) rgba: [u8; 4],
    /// Per-draw translation applied by the fixed-function viewport transform.
    /// Resident vertex and index storage is not rewritten or re-uploaded.
    pub(crate) viewport_translation_px: [f32; 2],
}

#[derive(Copy, Clone)]
struct ResidentSceneBatchState {
    phys: u64,
    virt: *mut u8,
}

unsafe impl Send for ResidentSceneBatchState {}

static RESIDENT_SCENE_BATCH_STATE: Mutex<Option<ResidentSceneBatchState>> = Mutex::new(None);
static RESIDENT_SCENE_BATCH_PATH_LOGGED: AtomicBool = AtomicBool::new(false);

fn resident_scene_batch_state(
    warm: RenderWarmState,
) -> Result<ResidentSceneBatchState, &'static str> {
    let mut resident = RESIDENT_SCENE_BATCH_STATE.lock();
    if let Some(state) = *resident {
        return Ok(state);
    }
    let Some((phys, virt)) = crate::dma::alloc(DRAW3D_SCENE_STATE_BYTES, crate::intel::WARM_ALIGN)
    else {
        return Err("scene-frame-state-alloc");
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, DRAW3D_SCENE_STATE_BYTES);
    }
    crate::intel::dma_flush(virt, DRAW3D_SCENE_STATE_BYTES);
    if !map_render_ppgtt_range(GPU_VA_DRAW3D_SCENE_STATE_BASE, phys, DRAW3D_SCENE_STATE_BYTES) {
        crate::dma::dealloc(virt, DRAW3D_SCENE_STATE_BYTES);
        return Err("scene-frame-state-map");
    }
    let state = ResidentSceneBatchState { phys, virt };
    *resident = Some(state);
    crate::log_info!(
        target: "render";
        "draw3d: resident scene batch state online gpu=0x{:X} bytes=0x{:X} slots={} warm_batch_bytes=0x{:X}\n",
        GPU_VA_DRAW3D_SCENE_STATE_BASE,
        DRAW3D_SCENE_STATE_BYTES,
        DRAW3D_SCENE_MAX_DRAWS + 1,
        warm.batch_len,
    );
    Ok(state)
}

fn resident_scene_state_warm(
    state: ResidentSceneBatchState,
    warm: RenderWarmState,
    slot: usize,
) -> Result<(RenderWarmState, u64), &'static str> {
    if slot > DRAW3D_SCENE_MAX_DRAWS {
        return Err("scene-frame-state-slot");
    }
    let offset = slot
        .checked_mul(DRAW3D_SCENE_STATE_SLOT_BYTES)
        .ok_or("scene-frame-state-slot")?;
    Ok((
        RenderWarmState {
            draw_state_phys: state.phys + offset as u64,
            draw_state_virt: unsafe { state.virt.add(offset) },
            draw_state_len: DRAW3D_SCENE_STATE_SLOT_BYTES,
            ..warm
        },
        GPU_VA_DRAW3D_SCENE_STATE_BASE + offset as u64,
    ))
}

/// One persistent R8 analytical font layer composited after triangle resolve.
/// The scene renderer submits the complete slice as one GPGPU batch.
pub(crate) type ResidentSceneCoverageDraw = crate::intel::gpgpu::GpgpuGlyphMaskLayer;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ResidentSceneRasterQuality {
    SingleSample,
    Multisample4x,
}

#[derive(Debug)]
pub(crate) struct ResidentSceneFrameResult {
    pub(crate) completed_draws: usize,
    pub(crate) requested_draws: usize,
    pub(crate) changed_pixels: usize,
    pub(crate) presented: bool,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) frame_us: u64,
    pub(crate) geometry_us: u64,
    /// CPU time spent constructing and publishing scene state/batches before
    /// handing the already-resident workload to GuC.
    pub(crate) geometry_prepare_us: u64,
    /// Time spent waiting for the RCS release cookie after GuC accepted the
    /// submission. This excludes CPU batch preparation and LRC setup.
    pub(crate) gpu_poll_us: u64,
    pub(crate) gpu_poll_iters: u64,
    pub(crate) resolve_us: u64,
    pub(crate) coverage_us: u64,
    pub(crate) present_copy_us: u64,
    pub(crate) present_copy_performed: bool,
    pub(crate) coverage_submits: usize,
    pub(crate) coverage_walkers: usize,
    pub(crate) rgba: Option<Vec<u8>>,
    /// True only when geometry, resolve, coverage, and any compatibility copy
    /// completed. This remains separate from `release_fence`: a caller that
    /// appends one final GPU writer may deliberately defer the release proof.
    pub(crate) frame_complete: bool,
    /// Present only after the final GPU writer's cache release plus ordered
    /// post-sync retirement marker completed for the returned UI4 allocation.
    pub(crate) release_fence: Option<ResidentSceneReleaseFence>,
}

/// Proof that the resident-scene pipeline completed the producer-release
/// command sequence for one exact direct-render allocation. The final writer
/// may be the 3D pixel backend, the MSAA resolve, analytical coverage, or a
/// retained decoration pass. This deliberately does not claim that an
/// independently mapped display GGTT alias has a compatible cache policy;
/// that is the consumer side of the handoff. The fields are private so a UI
/// producer cannot manufacture a release merely from a physical address.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResidentSceneReleaseFence {
    phys: u64,
    byte_len: usize,
    sequence: u64,
}

impl ResidentSceneReleaseFence {
    pub(crate) const fn matches(self, phys: u64, byte_len: usize) -> bool {
        self.phys == phys && self.byte_len == byte_len
    }

    pub(crate) const fn sequence(self) -> u64 {
        self.sequence
    }
}

static RESIDENT_SCENE_RELEASE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static RESIDENT_SCENE_PERF_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn resident_scene_release(
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> ResidentSceneReleaseFence {
    ResidentSceneReleaseFence {
        phys: destination.phys,
        byte_len: destination.bytes,
        sequence: RESIDENT_SCENE_RELEASE_SEQUENCE
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1),
    }
}

#[derive(Copy, Clone)]
enum ResidentSceneFrameOutput {
    Readback,
    GpuSurface(crate::intel::gpgpu::GpgpuRgba8Surface),
    GpuSurfaceDeferredRelease(crate::intel::gpgpu::GpgpuRgba8Surface),
    DirectGpuSurface(crate::intel::gpgpu::GpgpuRgba8Surface),
}

#[derive(Copy, Clone)]
struct ResidentSceneDepthAllocation {
    storage_phys: u64,
    storage_virt: *mut u8,
    storage_bytes: usize,
}

unsafe impl Send for ResidentSceneDepthAllocation {}

#[derive(Copy, Clone)]
struct ResidentSceneDirectUi4Mapping {
    phys: u64,
    bytes: usize,
    gpu: u64,
}

static RESIDENT_SCENE_DEPTH: Mutex<Option<ResidentSceneDepthAllocation>> = Mutex::new(None);
static RESIDENT_SCENE_MSAA_COLOR: Mutex<Option<ResidentSceneDepthAllocation>> = Mutex::new(None);
static RESIDENT_SCENE_MSAA_DEPTH: Mutex<Option<ResidentSceneDepthAllocation>> = Mutex::new(None);
static RESIDENT_SCENE_DIRECT_UI4_TARGETS: Mutex<
    [Option<ResidentSceneDirectUi4Mapping>; DRAW3D_UI4_FRAME_BUFFER_COUNT],
> = Mutex::new([None; DRAW3D_UI4_FRAME_BUFFER_COUNT]);
static RESIDENT_SCENE_DEPTH_CONTRACT_LOGGED: AtomicBool = AtomicBool::new(false);
static RESIDENT_SCENE_MSAA_FALLBACK_LOGGED: AtomicBool = AtomicBool::new(false);
static RESIDENT_SCENE_MSAA_CONTRACT_LOGGED: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone)]
struct ResidentSceneMsaaColorTarget {
    surface: crate::intel::gpgpu::GpgpuRgba8Surface,
}

fn prepare_resident_scene_direct_ui4_target(
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> Result<u64, &'static str> {
    if !destination.is_valid()
        || destination.bytes as u64 > GPU_VA_DRAW3D_UI4_FRAME_STRIDE
    {
        return Err("resident-scene-direct-ui4-shape");
    }
    let mut mappings = RESIDENT_SCENE_DIRECT_UI4_TARGETS.lock();
    if let Some(existing) = mappings
        .iter()
        .flatten()
        .copied()
        .find(|mapping| mapping.phys == destination.phys)
    {
        if existing.bytes != destination.bytes {
            return Err("resident-scene-direct-ui4-shape-changed");
        }
        return Ok(existing.gpu);
    }

    let Some(slot) = mappings.iter().position(Option::is_none) else {
        return Err("resident-scene-direct-ui4-buffer-limit");
    };
    let gpu = GPU_VA_DRAW3D_UI4_FRAME_BASE
        .checked_add(slot as u64 * GPU_VA_DRAW3D_UI4_FRAME_STRIDE)
        .ok_or("resident-scene-direct-ui4-address")?;
    if !map_render_ppgtt_scanout_range(gpu, destination.phys, destination.bytes) {
        return Err("resident-scene-direct-ui4-map");
    }
    mappings[slot] = Some(ResidentSceneDirectUi4Mapping {
        phys: destination.phys,
        bytes: destination.bytes,
        gpu,
    });
    crate::log_info!(
        target: "render";
        "draw3d: acquired UI4 triple buffer render_slot={} render_gpu=0x{:X} phys=0x{:X} bytes=0x{:X} size={}x{} pitch={} ppgtt_pat=3 ppgtt_cache=uc leaf_readback=verified persistent_render_va=1 hot_remap=0\n",
        slot,
        gpu,
        destination.phys,
        destination.bytes,
        destination.width,
        destination.height,
        destination.pitch_bytes,
    );
    Ok(gpu)
}

fn prepare_resident_scene_msaa_allocation(
    slot: &Mutex<Option<ResidentSceneDepthAllocation>>,
    gpu_addr: u64,
    required_bytes: usize,
    label: &'static str,
) -> Result<ResidentSceneDepthAllocation, &'static str> {
    if required_bytes == 0 || required_bytes > 64 * 1024 * 1024 {
        return Err("resident-scene-msaa-shape");
    }
    let mut resident = slot.lock();
    if let Some(allocation) = *resident
        && allocation.storage_bytes >= required_bytes
    {
        return Ok(allocation);
    }

    let Some((storage_phys, storage_virt)) =
        crate::dma::alloc(required_bytes, crate::intel::WARM_ALIGN)
    else {
        return Err("resident-scene-msaa-alloc");
    };
    let previous = *resident;
    if let Some(previous) = previous
        && !unmap_render_ppgtt_range(gpu_addr, previous.storage_bytes)
    {
        crate::dma::dealloc(storage_virt, required_bytes);
        return Err("resident-scene-msaa-unmap");
    }
    if !map_render_ppgtt_range(gpu_addr, storage_phys, required_bytes) {
        if let Some(previous) = previous {
            let _ = map_render_ppgtt_range(gpu_addr, previous.storage_phys, previous.storage_bytes);
        }
        crate::dma::dealloc(storage_virt, required_bytes);
        return Err("resident-scene-msaa-map");
    }
    if let Some(previous) = previous {
        crate::dma::dealloc(previous.storage_virt, previous.storage_bytes);
    }
    let allocation = ResidentSceneDepthAllocation {
        storage_phys,
        storage_virt,
        storage_bytes: required_bytes,
    };
    *resident = Some(allocation);
    crate::log_info!(
        target: "render";
        "resident-scene-msaa: allocated kind={} phys=0x{:X} gpu=0x{:X} bytes=0x{:X} samples=4 tiling=tile64\n",
        label,
        storage_phys,
        gpu_addr,
        required_bytes,
    );
    Ok(allocation)
}

fn prepare_resident_scene_msaa_color(
    device_id: u16,
    target_width: usize,
    target_height: usize,
) -> Result<ResidentSceneMsaaColorTarget, &'static str> {
    if !device_is_gfx125(device_id) {
        return Err("resident-scene-msaa-device");
    }
    let (pitch_bytes, _aligned_height, storage_bytes) =
        resident_scene_msaa_color_layout(target_width, target_height)
            .ok_or("resident-scene-msaa-shape")?;
    let allocation = prepare_resident_scene_msaa_allocation(
        &RESIDENT_SCENE_MSAA_COLOR,
        GPU_VA_RESIDENT_SCENE_MSAA_COLOR_BASE,
        storage_bytes,
        "rgba8-color",
    )?;
    let surface = crate::intel::gpgpu::GpgpuRgba8Surface::new(
        allocation.storage_phys,
        GPU_VA_RESIDENT_SCENE_MSAA_COLOR_BASE,
        allocation.storage_bytes,
        u32::try_from(target_width).map_err(|_| "resident-scene-msaa-shape")?,
        u32::try_from(target_height).map_err(|_| "resident-scene-msaa-shape")?,
        u32::try_from(pitch_bytes).map_err(|_| "resident-scene-msaa-shape")?,
    )
    .ok_or("resident-scene-msaa-surface")?;
    Ok(ResidentSceneMsaaColorTarget { surface })
}

fn resident_scene_msaa_color_layout(
    target_width: usize,
    target_height: usize,
) -> Option<(usize, usize, usize)> {
    let aligned_width =
        crate::intel::align_up(target_width, RESIDENT_SCENE_MSAA_COLOR_TILE_WIDTH_PIXELS)?;
    let aligned_height =
        crate::intel::align_up(target_height, RESIDENT_SCENE_MSAA_COLOR_TILE_HEIGHT_PIXELS)?;
    let pitch_bytes = aligned_width.checked_mul(core::mem::size_of::<u32>())?;
    let storage_bytes = pitch_bytes.checked_mul(aligned_height)?.checked_mul(4)?;
    Some((pitch_bytes, aligned_height, storage_bytes))
}

fn resident_scene_msaa_depth_layout(
    target_width: usize,
    target_height: usize,
) -> Option<(usize, usize, usize)> {
    let sample_width = target_width.checked_mul(2)?;
    let sample_height = target_height.checked_mul(2)?;
    let row_bytes = sample_width.checked_mul(core::mem::size_of::<f32>())?;
    let pitch_bytes =
        crate::intel::align_up(row_bytes, RESIDENT_SCENE_MSAA_DEPTH_TILE_WIDTH_BYTES)?;
    let aligned_sample_height =
        crate::intel::align_up(sample_height, RESIDENT_SCENE_MSAA_DEPTH_TILE_HEIGHT_SAMPLE_ROWS)?;
    let storage_bytes = pitch_bytes.checked_mul(aligned_sample_height)?;
    Some((pitch_bytes, aligned_sample_height, storage_bytes))
}

fn prepare_resident_scene_msaa_depth(
    device_id: u16,
    target_width: usize,
    target_height: usize,
) -> Result<TriangleDepthConfig, &'static str> {
    if !device_is_gfx125(device_id) {
        return Err("draw3d-msaa-depth-device");
    }
    let (pitch_bytes, aligned_sample_height, storage_bytes) =
        resident_scene_msaa_depth_layout(target_width, target_height)
            .ok_or("draw3d-msaa-depth-shape")?;
    let _allocation = prepare_resident_scene_msaa_allocation(
        &RESIDENT_SCENE_MSAA_DEPTH,
        GPU_VA_RESIDENT_SCENE_MSAA_DEPTH_BASE,
        storage_bytes,
        "d32-depth",
    )?;
    Ok(TriangleDepthConfig {
        gpu_addr: GPU_VA_RESIDENT_SCENE_MSAA_DEPTH_BASE,
        pitch_bytes: u32::try_from(pitch_bytes).map_err(|_| "draw3d-msaa-depth-shape")?,
        width: u32::try_from(target_width).map_err(|_| "draw3d-msaa-depth-shape")?,
        height: u32::try_from(target_height).map_err(|_| "draw3d-msaa-depth-shape")?,
        qpitch_rows_div4: u32::try_from(aligned_sample_height / 4)
            .map_err(|_| "draw3d-msaa-depth-shape")?,
        write_enabled: false,
        compare_function: COMPARE_FUNCTION_LEQUAL,
    })
}

#[cfg(test)]
mod resident_scene_msaa_layout_tests {
    use super::{
        GPU_VA_RESIDENT_SCENE_MSAA_COLOR_BASE, GPU_VA_RESIDENT_SCENE_MSAA_DEPTH_BASE,
        resident_scene_msaa_color_layout, resident_scene_msaa_depth_layout,
    };

    #[test]
    fn gridpaper_extent_uses_matching_tile64_color_and_depth_storage() {
        let color = resident_scene_msaa_color_layout(810, 1153).unwrap();
        let depth = resident_scene_msaa_depth_layout(810, 1153).unwrap();
        assert_eq!(color, (3328, 1216, 16_187_392));
        assert_eq!(depth, (6656, 2432, 16_187_392));
    }

    #[test]
    fn maximum_scene_fits_each_reserved_va_window() {
        let color = resident_scene_msaa_color_layout(2560, 1440).unwrap();
        let depth = resident_scene_msaa_depth_layout(2560, 1440).unwrap();
        assert_eq!(color, (10_240, 1472, 60_293_120));
        assert_eq!(depth, (20_480, 2944, 60_293_120));
        assert!(color.2 <= 64 * 1024 * 1024);
        assert!(depth.2 <= 64 * 1024 * 1024);
        assert_eq!(
            GPU_VA_RESIDENT_SCENE_MSAA_DEPTH_BASE - GPU_VA_RESIDENT_SCENE_MSAA_COLOR_BASE,
            64 * 1024 * 1024
        );
    }
}

fn prepare_resident_scene_depth(
    device_id: u16,
    target_width: usize,
    target_height: usize,
) -> Result<TriangleDepthConfig, &'static str> {
    if !device_is_gfx12(device_id) {
        return Err("draw3d-depth-device");
    }
    let row_bytes = target_width
        .checked_mul(core::mem::size_of::<f32>())
        .ok_or("draw3d-depth-shape")?;
    let pitch_bytes = crate::intel::align_up(row_bytes, DRAW3D_SCENE_DEPTH_TILE_WIDTH_BYTES)
        .ok_or("draw3d-depth-shape")?;
    let aligned_height = crate::intel::align_up(target_height, DRAW3D_SCENE_DEPTH_TILE_HEIGHT_ROWS)
        .ok_or("draw3d-depth-shape")?;
    let clear_bytes = pitch_bytes
        .checked_mul(aligned_height)
        .ok_or("draw3d-depth-shape")?;
    if target_width == 0
        || target_height == 0
        || clear_bytes > DRAW3D_SCENE_DEPTH_BYTES
        || !clear_bytes.is_multiple_of(core::mem::size_of::<u32>())
    {
        return Err("draw3d-depth-shape");
    }

    let allocation = {
        let mut resident = RESIDENT_SCENE_DEPTH.lock();
        if let Some(allocation) = *resident {
            allocation
        } else {
            let Some((storage_phys, storage_virt)) =
                crate::dma::alloc(DRAW3D_SCENE_DEPTH_BYTES, crate::intel::WARM_ALIGN)
            else {
                return Err("draw3d-depth-alloc");
            };
            if !map_render_ppgtt_range(
                GPU_VA_DRAW3D_SCENE_DEPTH_BASE,
                storage_phys,
                DRAW3D_SCENE_DEPTH_BYTES,
            ) {
                crate::dma::dealloc(storage_virt, DRAW3D_SCENE_DEPTH_BYTES);
                return Err("draw3d-depth-map");
            }
            let allocation = ResidentSceneDepthAllocation {
                storage_phys,
                storage_virt,
                storage_bytes: DRAW3D_SCENE_DEPTH_BYTES,
            };
            *resident = Some(allocation);
            crate::log_info!(
                target: "render";
                "draw3d-depth: resident surface allocated phys=0x{:X} gpu=0x{:X} bytes=0x{:X} format=d32-float tiling={} max={}x{}\n",
                allocation.storage_phys,
                GPU_VA_DRAW3D_SCENE_DEPTH_BASE,
                allocation.storage_bytes,
                if device_is_gfx125(device_id) { "tile4" } else { "y0" },
                DRAW3D_SCENE_TARGET_WIDTH,
                DRAW3D_SCENE_TARGET_HEIGHT,
            );
            allocation
        }
    };

    if allocation.storage_virt.is_null()
        || allocation.storage_bytes < clear_bytes
        || !allocation
            .storage_phys
            .is_multiple_of(crate::intel::WARM_ALIGN as u64)
    {
        return Err("draw3d-depth-allocation");
    }

    Ok(TriangleDepthConfig {
        gpu_addr: GPU_VA_DRAW3D_SCENE_DEPTH_BASE,
        pitch_bytes: u32::try_from(pitch_bytes).map_err(|_| "draw3d-depth-shape")?,
        width: u32::try_from(target_width).map_err(|_| "draw3d-depth-shape")?,
        height: u32::try_from(target_height).map_err(|_| "draw3d-depth-shape")?,
        qpitch_rows_div4: u32::try_from(aligned_height / 4).map_err(|_| "draw3d-depth-shape")?,
        write_enabled: false,
        compare_function: COMPARE_FUNCTION_LEQUAL,
    })
}

pub(crate) const fn resident_scene_target_dimensions() -> (usize, usize) {
    (DRAW3D_SCENE_TARGET_WIDTH, DRAW3D_SCENE_TARGET_HEIGHT)
}

/// Render an off-screen straight-RGBA frame without changing local scanout.
pub(crate) fn capture_resident_triangle_scene_frame(
    draws: &[ResidentSceneDraw<'_>],
    clear_rgba: Option<[u8; 4]>,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    let (width, height) = resident_scene_target_dimensions();
    submit_resident_triangle_scene_capture(
        draws,
        &[],
        clear_rgba,
        diagnostic_logs,
        true,
        false,
        ResidentSceneRasterQuality::SingleSample,
        width,
        height,
        ResidentSceneFrameOutput::Readback,
    )
}

/// Draw3D capture with opaque depth writes and read-only depth testing for
/// blended meshes. Alpha classification remains an internal renderer policy;
/// the TCP v1 wire format is unchanged.
pub(crate) fn capture_resident_triangle_scene_frame_with_opaque_depth(
    draws: &[ResidentSceneDraw<'_>],
    clear_rgba: Option<[u8; 4]>,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    let (width, height) = resident_scene_target_dimensions();
    submit_resident_triangle_scene_capture(
        draws,
        &[],
        clear_rgba,
        diagnostic_logs,
        true,
        true,
        ResidentSceneRasterQuality::SingleSample,
        width,
        height,
        ResidentSceneFrameOutput::Readback,
    )
}

/// Full-size straight-RGBA Draw3D capture with 4x color/depth coverage.
pub(crate) fn capture_resident_triangle_scene_frame_with_opaque_depth_msaa4(
    draws: &[ResidentSceneDraw<'_>],
    clear_rgba: Option<[u8; 4]>,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    let (width, height) = resident_scene_target_dimensions();
    submit_resident_triangle_scene_capture(
        draws,
        &[],
        clear_rgba,
        diagnostic_logs,
        true,
        true,
        ResidentSceneRasterQuality::Multisample4x,
        width,
        height,
        ResidentSceneFrameOutput::Readback,
    )
}

/// Render an off-screen frame in UI4's native premultiplied-RGBA convention.
///
/// The fixed-function blend target already contains premultiplied RGB.  This
/// entry point preserves those bytes and premultiplies the straight protocol
/// clear color once before the GPU clear, avoiding a full-frame round trip
/// through straight alpha when the consumer is the UI4 compositor.
pub(crate) fn capture_resident_triangle_scene_frame_premultiplied(
    draws: &[ResidentSceneDraw<'_>],
    clear_rgba: Option<[u8; 4]>,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    let (width, height) = resident_scene_target_dimensions();
    submit_resident_triangle_scene_capture(
        draws,
        &[],
        clear_rgba,
        diagnostic_logs,
        false,
        false,
        ResidentSceneRasterQuality::SingleSample,
        width,
        height,
        ResidentSceneFrameOutput::Readback,
    )
}

/// Render a premultiplied UI4 frame at the consumer's actual content extent.
///
/// The screenshot API intentionally retains the full resident-scene target.
/// UI4, however, must not render and CPU-read a 2560x1440 scratch image only to
/// reduce it into a much smaller broker frame.
pub(crate) fn capture_resident_triangle_scene_frame_premultiplied_at_extent(
    draws: &[ResidentSceneDraw<'_>],
    clear_rgba: Option<[u8; 4]>,
    width: u32,
    height: u32,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    submit_resident_triangle_scene_capture(
        draws,
        &[],
        clear_rgba,
        diagnostic_logs,
        false,
        false,
        ResidentSceneRasterQuality::SingleSample,
        width as usize,
        height as usize,
        ResidentSceneFrameOutput::Readback,
    )
}

/// UI4-sized Draw3D capture using the opaque-depth visibility contract.
pub(crate) fn capture_resident_triangle_scene_frame_premultiplied_at_extent_with_opaque_depth(
    draws: &[ResidentSceneDraw<'_>],
    clear_rgba: Option<[u8; 4]>,
    width: u32,
    height: u32,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    submit_resident_triangle_scene_capture(
        draws,
        &[],
        clear_rgba,
        diagnostic_logs,
        false,
        true,
        ResidentSceneRasterQuality::SingleSample,
        width as usize,
        height as usize,
        ResidentSceneFrameOutput::Readback,
    )
}

/// UI4-sized resident scene with native gfx12.5 4x sample coverage and a GPU
/// resolve into the ordinary premultiplied frame buffer.
pub(crate) fn capture_resident_triangle_scene_frame_premultiplied_at_extent_msaa4(
    draws: &[ResidentSceneDraw<'_>],
    clear_rgba: Option<[u8; 4]>,
    width: u32,
    height: u32,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    submit_resident_triangle_scene_capture(
        draws,
        &[],
        clear_rgba,
        diagnostic_logs,
        false,
        false,
        ResidentSceneRasterQuality::Multisample4x,
        width as usize,
        height as usize,
        ResidentSceneFrameOutput::Readback,
    )
}

/// UI4-sized 4x triangle scene followed by persistent analytical font masks.
/// Coverage is composited only after the MSAA resolve, preserving its R8 alpha
/// steps instead of treating the mask as additional fixed-function samples.
pub(crate) fn capture_resident_triangle_scene_frame_premultiplied_at_extent_msaa4_with_coverage(
    draws: &[ResidentSceneDraw<'_>],
    coverage_draws: &[ResidentSceneCoverageDraw],
    clear_rgba: Option<[u8; 4]>,
    width: u32,
    height: u32,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    submit_resident_triangle_scene_capture(
        draws,
        coverage_draws,
        clear_rgba,
        diagnostic_logs,
        false,
        false,
        ResidentSceneRasterQuality::Multisample4x,
        width as usize,
        height as usize,
        ResidentSceneFrameOutput::Readback,
    )
}

/// Render a retained 4x scene directly into a leased UI4 RGBA surface.
///
/// The MSAA resolve and analytical coverage passes write the producer's back
/// buffer themselves. No full-frame CPU readback or staging allocation is
/// performed. On hardware without the 4x path, the ordinary linear scratch
/// target is copied as a compatibility fallback.
pub(crate) fn render_resident_triangle_scene_frame_premultiplied_msaa4_with_coverage_to_surface(
    draws: &[ResidentSceneDraw<'_>],
    coverage_draws: &[ResidentSceneCoverageDraw],
    clear_rgba: Option<[u8; 4]>,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    submit_resident_triangle_scene_capture(
        draws,
        coverage_draws,
        clear_rgba,
        diagnostic_logs,
        false,
        false,
        ResidentSceneRasterQuality::Multisample4x,
        destination.width as usize,
        destination.height as usize,
        ResidentSceneFrameOutput::GpuSurface(destination),
    )
}

/// GridPaper's complete direct-render operation. The cursor rectangles are
/// submitted after MSAA resolve and analytical coverage, so the returned
/// release proof is reminted only after that actual final writer retires.
pub(crate) fn render_resident_triangle_scene_frame_premultiplied_msaa4_with_coverage_and_rects_to_surface(
    draws: &[ResidentSceneDraw<'_>],
    coverage_draws: &[ResidentSceneCoverageDraw],
    final_rects: &[crate::intel::gpgpu::GpgpuSolidRect],
    clear_rgba: Option<[u8; 4]>,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    let mut result = submit_resident_triangle_scene_capture(
        draws,
        coverage_draws,
        clear_rgba,
        diagnostic_logs,
        false,
        false,
        ResidentSceneRasterQuality::Multisample4x,
        destination.width as usize,
        destination.height as usize,
        ResidentSceneFrameOutput::GpuSurfaceDeferredRelease(destination),
    )?;
    if !result.frame_complete {
        return Err("resident-scene-incomplete-before-final-writer");
    }
    let started_ns = crate::chronos::monotonic_nanos();
    if !final_rects.is_empty() {
        if !crate::intel::gpgpu::fill_solid_rects_rgba8_scanout(destination, final_rects) {
            return Err("resident-scene-final-rects");
        }
    }
    let finalizer = crate::intel::gpgpu::release_rgba8_surface_for_scanout(destination);
    if !finalizer.ok
        || !finalizer
            .release
            .is_some_and(|release| release.matches(destination.phys, destination.bytes))
    {
        return Err("resident-scene-final-release");
    }
    result.coverage_us = result.coverage_us.saturating_add(
        crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000,
    );
    result.release_fence = Some(resident_scene_release(destination));
    Ok(result)
}

/// Render a depth-tested retained 4x scene directly into a leased UI4 RGBA
/// surface. This is Draw3D's live presentation path: the final scanout release
/// follows resolve completion, and no CPU readback or full-frame copy runs.
pub(crate) fn render_resident_triangle_scene_frame_premultiplied_with_opaque_depth_msaa4_to_surface(
    draws: &[ResidentSceneDraw<'_>],
    clear_rgba: Option<[u8; 4]>,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    submit_resident_triangle_scene_capture(
        draws,
        &[],
        clear_rgba,
        diagnostic_logs,
        false,
        true,
        ResidentSceneRasterQuality::Multisample4x,
        destination.width as usize,
        destination.height as usize,
        ResidentSceneFrameOutput::GpuSurface(destination),
    )
}

/// Render a depth-tested retained scene directly into the one permanent UI4
/// linear surface used by the compositor-rewire checkpoint. There is no
/// scratch target, resolve, or post-render compute copy on this path.
pub(crate) fn render_resident_triangle_scene_frame_premultiplied_with_opaque_depth_direct_to_surface(
    draws: &[ResidentSceneDraw<'_>],
    clear_rgba: Option<[u8; 4]>,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    submit_resident_triangle_scene_capture(
        draws,
        &[],
        clear_rgba,
        diagnostic_logs,
        false,
        true,
        ResidentSceneRasterQuality::SingleSample,
        destination.width as usize,
        destination.height as usize,
        ResidentSceneFrameOutput::DirectGpuSurface(destination),
    )
}

/// UI4-sized depth-tested resident scene with matching 4x color and depth.
pub(crate) fn capture_resident_triangle_scene_frame_premultiplied_at_extent_with_opaque_depth_msaa4(
    draws: &[ResidentSceneDraw<'_>],
    clear_rgba: Option<[u8; 4]>,
    width: u32,
    height: u32,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    submit_resident_triangle_scene_capture(
        draws,
        &[],
        clear_rgba,
        diagnostic_logs,
        false,
        true,
        ResidentSceneRasterQuality::Multisample4x,
        width as usize,
        height as usize,
        ResidentSceneFrameOutput::Readback,
    )
}

fn stage_resident_scene_secondary(
    warm: RenderWarmState,
    state_warm: RenderWarmState,
    state_gpu: u64,
    mut draw: TriangleDrawPrep,
    blend_mode: TriangleBlendProbeMode,
    depth_config: Option<TriangleDepthConfig>,
    rgba: [u8; 4],
    viewport_translation_px: [f32; 2],
    secondary_index: usize,
) -> Result<usize, &'static str> {
    draw.state_gpu_addr = state_gpu;
    // The cache extractor now identifies this executable from the GEN
    // assembly's mov(16)/sendc(16), rather than assuming the first fragment
    // slice was SIMD16.  With SIMD16 as the only enabled width, variable pixel
    // dispatch selects this executable through KSP0.
    let pipeline = crate::intel::shader::triangle_pipeline_simd16();
    let shader_layout = upload_triangle_shader_pipeline_at(
        state_warm,
        pipeline,
        Some(rgba),
        state_gpu,
        false,
    )?;
    let probe_state = write_triangle_probe_state_unflushed(
        state_warm,
        draw,
        shader_layout,
        blend_mode,
        BackendProbeMode::MesaLike,
        viewport_translation_px,
    )?;
    // Shader bytes and all fixed-function structures occupy one compact
    // prefix. Publish it once, after every CPU write, instead of three
    // independent CLFLUSH+MFENCE passes over partly overlapping state.
    crate::intel::dma_flush(state_warm.draw_state_virt, probe_state.used_bytes as usize);
    let batch_offset = DRAW3D_SCENE_PRIMARY_BATCH_BYTES
        .checked_add(
            secondary_index
                .checked_mul(DRAW3D_SCENE_SECONDARY_BATCH_BYTES)
                .ok_or("scene-frame-batch-slot")?,
        )
        .ok_or("scene-frame-batch-slot")?;
    let batch_end = batch_offset
        .checked_add(DRAW3D_SCENE_SECONDARY_BATCH_BYTES)
        .ok_or("scene-frame-batch-slot")?;
    if batch_end > warm.batch_len {
        return Err("scene-frame-batch-capacity");
    }
    let batch = unsafe {
        core::slice::from_raw_parts_mut(
            warm.batch_virt.add(batch_offset) as *mut u32,
            DRAW3D_SCENE_SECONDARY_BATCH_BYTES / core::mem::size_of::<u32>(),
        )
    };
    let bytes = encode_triangle_probe_batch(
        "draw3d-scene",
        batch,
        state_warm,
        draw,
        blend_mode,
        depth_config,
        pipeline,
        shader_layout,
        probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        TriangleBatchMode::Draw,
        StreamoutProofExperiment::HeaderAndPositionSlots01,
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
        viewport_translation_px,
        BackendProbeMode::MesaLike,
        // All scene secondaries execute below one primary batch. They only
        // need command-stream ordering here; the primary emits the single
        // full render/depth/L3 release fence after the final secondary.
        PostDrawSyncVariant::LightCsNoPostSync,
    )?;
    crate::intel::dma_flush(unsafe { warm.batch_virt.add(batch_offset) }, bytes);
    Ok(bytes)
}

fn encode_resident_scene_primary_batch(
    warm: RenderWarmState,
    secondary_count: usize,
) -> Result<usize, &'static str> {
    let batch = unsafe {
        core::slice::from_raw_parts_mut(
            warm.batch_virt as *mut u32,
            DRAW3D_SCENE_PRIMARY_BATCH_BYTES / core::mem::size_of::<u32>(),
        )
    };
    let mut cursor = 0usize;
    let mut push = |value: u32| -> Result<(), &'static str> {
        let Some(slot) = batch.get_mut(cursor) else {
            return Err("scene-frame-primary-batch-exhausted");
        };
        *slot = value;
        cursor += 1;
        Ok(())
    };
    for secondary_index in 0..secondary_count {
        let offset = DRAW3D_SCENE_PRIMARY_BATCH_BYTES
            .checked_add(
                secondary_index
                    .checked_mul(DRAW3D_SCENE_SECONDARY_BATCH_BYTES)
                    .ok_or("scene-frame-batch-slot")?,
            )
            .ok_or("scene-frame-batch-slot")?;
        let gpu = GPU_VA_BATCH_BASE + offset as u64;
        push(MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_GTT | MI_BATCH_2ND_LEVEL)?;
        push(gpu as u32)?;
        push((gpu >> 32) as u32)?;
    }
    let completion_gpu =
        GPU_VA_RESULT_BASE + (RESULT_SLOT_SCENE_FRAME_DWORD * core::mem::size_of::<u32>()) as u64;
    // Release the color target written by the Gen12 3D pixel backend. Keep
    // this end-of-pipe writeback separate from all top-of-pipe invalidations:
    // mixing them into one packet can invalidate first and only then wait for
    // older rendering. Depth remains private to the ordered RCS workload, so
    // it is not part of the display-ownership release.
    push(PIPE_CONTROL_CMD)?;
    push(PIPE_CONTROL_SCENE_COLOR_RELEASE_BITS)?;
    push(0)?;
    push(0)?;
    push(0)?;
    push(0)?;

    // Retire the release with a second, ordered PIPE_CONTROL. The unique
    // QWord cookie proves that the preceding RT/tile flush and CS stall ran;
    // it does not by itself prove the display GGTT alias is cache-compatible.
    // DEST_GGTT remains clear because the result allocation is in the render
    // PPGTT.
    push(PIPE_CONTROL_CMD)?;
    push(PIPE_CONTROL_SCENE_RELEASE_MARKER_BITS)?;
    push(completion_gpu as u32)?;
    push((completion_gpu >> 32) as u32)?;
    push(RCS_EXEC_RESULT_SCENE_RCS_RELEASE_DONE_LO)?;
    push(RCS_EXEC_RESULT_SCENE_RCS_RELEASE_DONE_HI)?;
    push(MI_BATCH_BUFFER_END)?;
    push(MI_NOOP)?;
    Ok(cursor * core::mem::size_of::<u32>())
}

#[derive(Copy, Clone)]
struct ResidentSceneGeometryResult {
    completed: bool,
    prepare_us: u64,
    gpu_poll_us: u64,
    gpu_poll_iters: u64,
}

fn submit_resident_scene_geometry_batched(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    draws: &[ResidentSceneDraw<'_>],
    clear: [u8; 4],
    opaque_depth_enabled: bool,
    depth_config: Option<TriangleDepthConfig>,
    render_target_gpu: u64,
    render_target_pitch: usize,
    target_width: usize,
    target_height: usize,
) -> Result<ResidentSceneGeometryResult, &'static str> {
    let prepare_started_ns = crate::chronos::monotonic_nanos();
    const CLEAR_TRIANGLE: [[f32; 3]; 3] = [[-1.0, -1.0, 1.0], [3.0, -1.0, 1.0], [-1.0, 3.0, 1.0]];
    if draws.len() > DRAW3D_SCENE_MAX_DRAWS {
        return Err("scene-frame-draw-limit");
    }
    let max_secondary_count = draws.len().saturating_add(1);
    let used_batch_bytes = DRAW3D_SCENE_PRIMARY_BATCH_BYTES
        .checked_add(
            max_secondary_count
                .checked_mul(DRAW3D_SCENE_SECONDARY_BATCH_BYTES)
                .ok_or("scene-frame-batch-capacity")?,
        )
        .ok_or("scene-frame-batch-capacity")?;
    if used_batch_bytes > warm.batch_len {
        return Err("scene-frame-batch-capacity");
    }
    unsafe {
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let state = resident_scene_batch_state(warm)?;

    let mut clear_depth = depth_config;
    if let Some(depth) = clear_depth.as_mut() {
        depth.write_enabled = true;
        depth.compare_function = COMPARE_FUNCTION_ALWAYS;
    }
    let (clear_warm, clear_state_gpu) = resident_scene_state_warm(state, warm, 0)?;
    let clear_draw = prepare_triangle_draw_resources_for_scene_vertex_slice(
        clear_warm,
        render_target_gpu,
        render_target_pitch,
        target_width,
        target_height,
        "draw3d-fullscreen-clear",
        &CLEAR_TRIANGLE,
    )
    .ok_or("target-clear-resources")?;
    stage_resident_scene_secondary(
        warm,
        clear_warm,
        clear_state_gpu,
        clear_draw,
        TriangleBlendProbeMode::MesaZeroedState,
        clear_depth,
        clear,
        [0.0, 0.0],
        0,
    )?;

    let mut secondary_count = 1usize;
    for scene_draw in draws {
        if opaque_depth_enabled && scene_draw.rgba[3] == 0 {
            continue;
        }
        let (blend_mode, draw_depth) = if opaque_depth_enabled {
            let write_enabled = scene_draw.rgba[3] == u8::MAX;
            let mut depth = depth_config.ok_or("scene-frame-depth")?;
            depth.write_enabled = write_enabled;
            (
                if write_enabled {
                    TriangleBlendProbeMode::MesaZeroedState
                } else {
                    TriangleBlendProbeMode::StraightAlpha
                },
                Some(depth),
            )
        } else {
            (TriangleBlendProbeMode::StraightAlpha, None)
        };
        let (state_warm, state_gpu) = resident_scene_state_warm(state, warm, secondary_count)?;
        let draw = prepare_triangle_draw_resources_for_scene_resident_mesh(
            state_warm,
            render_target_gpu,
            render_target_pitch,
            target_width,
            target_height,
            scene_draw.mesh,
        )
        .ok_or("scene-frame-resident-draw")?;
        stage_resident_scene_secondary(
            warm,
            state_warm,
            state_gpu,
            draw,
            blend_mode,
            draw_depth,
            scene_draw.rgba,
            scene_draw.viewport_translation_px,
            secondary_count,
        )?;
        secondary_count += 1;
    }

    let primary_bytes = encode_resident_scene_primary_batch(warm, secondary_count)?;
    crate::intel::dma_flush(warm.batch_virt, primary_bytes);
    if !RESIDENT_SCENE_BATCH_PATH_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log_info!(
            target: "render";
            "draw3d: frame launch path=one-guc-scene-batch draws={} secondaries={} render_submits=1 per_mesh_context_rebuilds=0 target={}x{} fragment_contract=standalone-simd16-corrected dispatch=010 ksp0=simd16 ksp1=off ksp2=off vector_mask=1 color=specialized-per-draw\n",
            draws.len(),
            secondary_count,
            target_width,
            target_height,
        );
    }
    let prepare_us = crate::chronos::monotonic_nanos()
        .saturating_sub(prepare_started_ns)
        / 1_000;
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_SCENE_RCS_RELEASE_DONE_LO,
        RESULT_SLOT_SCENE_FRAME_DWORD,
        "draw3d-scene",
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "draw3d-scene");
    }
    let (gpu_poll_us, gpu_poll_iters) = draw3d_last_gpu_poll_profile();
    Ok(ResidentSceneGeometryResult {
        completed,
        prepare_us,
        gpu_poll_us,
        gpu_poll_iters,
    })
}

fn submit_resident_triangle_scene_capture(
    draws: &[ResidentSceneDraw<'_>],
    coverage_draws: &[ResidentSceneCoverageDraw],
    clear_rgba: Option<[u8; 4]>,
    diagnostic_logs: bool,
    straight_alpha_output: bool,
    opaque_depth_enabled: bool,
    raster_quality: ResidentSceneRasterQuality,
    target_width: usize,
    target_height: usize,
    frame_output: ResidentSceneFrameOutput,
) -> Result<ResidentSceneFrameResult, &'static str> {
    if target_width == 0
        || target_height == 0
        || target_width > DRAW3D_SCENE_TARGET_WIDTH
        || target_height > DRAW3D_SCENE_TARGET_HEIGHT
    {
        return Err("draw3d-capture-shape");
    }
    if let ResidentSceneFrameOutput::GpuSurface(destination)
    | ResidentSceneFrameOutput::GpuSurfaceDeferredRelease(destination)
    | ResidentSceneFrameOutput::DirectGpuSurface(destination) = frame_output
        && (!destination.is_valid()
            || destination.width as usize != target_width
            || destination.height as usize != target_height)
    {
        return Err("resident-scene-output-surface-shape");
    }
    for (index, draw) in coverage_draws.iter().enumerate() {
        if !draw.mask.is_valid() {
            return Err("resident-coverage-mask-shape");
        }
        let draw_end = draw
            .mask
            .gpu
            .checked_add(draw.mask.bytes as u64)
            .ok_or("resident-coverage-mask-range")?;
        for other in &coverage_draws[..index] {
            let other_end = other
                .mask
                .gpu
                .checked_add(other.mask.bytes as u64)
                .ok_or("resident-coverage-mask-range")?;
            if draw.mask.gpu < other_end && other.mask.gpu < draw_end {
                return Err("resident-coverage-mask-va-alias");
            }
        }
    }
    let target_pitch = target_width
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or("draw3d-capture-shape")?;
    let target_bytes = target_pitch
        .checked_mul(target_height)
        .ok_or("draw3d-capture-shape")?;
    let frame_started_ns = crate::chronos::monotonic_nanos();

    let lock_started_ns = crate::chronos::monotonic_nanos();
    let mut lock_spins = 0usize;
    loop {
        if PRIMARY_PROBE_IN_FLIGHT
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            break;
        }
        // Screenshot rendering shares the one physical render context. Give
        // an active GPU job one bounded opportunity to retire.
        if lock_spins.is_multiple_of(256)
            && crate::chronos::monotonic_nanos().saturating_sub(lock_started_ns) >= 50_000_000
        {
            return Err("in-flight-timeout");
        }
        core::hint::spin_loop();
        lock_spins += 1;
    }
    if lock_spins != 0 {
        crate::log_info!(
            target: "render";
            "draw3d-screenshot-lock wait_us={} spins={} acquired=1\n",
            crate::chronos::monotonic_nanos().saturating_sub(lock_started_ns) / 1_000,
            lock_spins,
        );
    }

    // Draw3d reports one scene-level result. Keep the renderer's proof
    // transcript available for a deliberate stalled-frame diagnostic retry,
    // but do not repeat it for every mesh in ordinary scene updates.
    let _summary_only = (!diagnostic_logs).then(RenderSummaryOnlyGuard::enter);

    let result = (|| {
        let Some(dev) = crate::intel::claimed_device() else {
            return Err("no-device");
        };
        let warm = warm_once(dev);
        if warm.streamout_virt.is_null() || warm.streamout_len < target_bytes {
            return Err("warm-scratch");
        }
        if !forcewake_render_acquire(warm) {
            return Err("forcewake");
        }
        if !ensure_smoke_buffers_mapped(dev, warm) {
            return Err("render-map");
        }
        let raster_quality = if raster_quality == ResidentSceneRasterQuality::Multisample4x
            && !device_is_gfx125(warm.device_id)
        {
            if !RESIDENT_SCENE_MSAA_FALLBACK_LOGGED.swap(true, Ordering::AcqRel) {
                crate::log_warn!(
                    target: "render";
                    "resident-scene-msaa: requested=4 effective=1 reason=tile64-msaa-requires-gfx125 device=0x{:04X}\n",
                    warm.device_id,
                );
            }
            ResidentSceneRasterQuality::SingleSample
        } else {
            raster_quality
        };
        let msaa_color = if raster_quality == ResidentSceneRasterQuality::Multisample4x {
            Some(prepare_resident_scene_msaa_color(warm.device_id, target_width, target_height)?)
        } else {
            None
        };
        let direct_output = match frame_output {
            ResidentSceneFrameOutput::DirectGpuSurface(destination) => Some(destination),
            _ => None,
        };
        let (render_target_gpu, render_target_pitch) = if let Some(target) = msaa_color {
            (target.surface.gpu, target.surface.pitch_bytes as usize)
        } else if let Some(destination) = direct_output {
            (
                prepare_resident_scene_direct_ui4_target(destination)?,
                destination.pitch_bytes as usize,
            )
        } else {
            (GPU_VA_STREAMOUT_BASE, target_pitch)
        };
        if let Some(target) = msaa_color
            && !RESIDENT_SCENE_MSAA_CONTRACT_LOGGED.swap(true, Ordering::AcqRel)
        {
            crate::log_info!(
                target: "render";
                "resident-scene-msaa: contract enabled samples=4 color=rgba8-unorm/tile64 color_pitch={} depth={} raster=on-pattern sample_mask=0xF resolve=gpgpu-single-dispatch/linear-rgba8 target={}x{} mesh_storage=unchanged\n",
                target.surface.pitch_bytes,
                if opaque_depth_enabled { "d32-float/tile64-ims" } else { "none" },
                target_width,
                target_height,
            );
        }
        let depth_config = if opaque_depth_enabled {
            Some(if raster_quality == ResidentSceneRasterQuality::Multisample4x {
                prepare_resident_scene_msaa_depth(warm.device_id, target_width, target_height)?
            } else {
                prepare_resident_scene_depth(warm.device_id, target_width, target_height)?
            })
        } else {
            None
        };

        if opaque_depth_enabled
            && !RESIDENT_SCENE_DEPTH_CONTRACT_LOGGED.swap(true, Ordering::AcqRel)
        {
            let opaque = draws.iter().filter(|draw| draw.rgba[3] == u8::MAX).count();
            let blended = draws
                .iter()
                .filter(|draw| draw.rgba[3] != 0 && draw.rgba[3] != u8::MAX)
                .count();
            let skipped = draws.iter().filter(|draw| draw.rgba[3] == 0).count();
            crate::log_info!(
                target: "render";
                "draw3d-depth: contract enabled opaque={} blended={} skipped={} clear=fullscreen-color+depth opaque_order=front-to-back opaque_state=depth-test+write+blend-off transparent_order=back-to-front transparent_state=depth-test+write-off+straight-alpha compare=lequal hiz=off protocol=v1-unchanged\n",
                opaque,
                blended,
                skipped,
            );
        }

        // Draw3D uses straight-alpha blending.  The GPU must see the real
        // clear color as its destination for the first translucent draw;
        // using the old readback sentinel here would blend the first shape
        // against 0xDEAD_BEEF.  Readback below compares against this same
        // clear word to retain changed-pixel accounting.
        let mut clear = clear_rgba.unwrap_or([0, 0, 0, 0]);
        if !straight_alpha_output {
            let alpha = u16::from(clear[3]);
            for channel in &mut clear[..3] {
                *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
            }
        }
        // Clear and every resident draw are second-level batches beneath one
        // frame-level primary. GuC sees one ordered scene submission and the
        // CPU waits only for the final scene fence.
        let geometry = submit_resident_scene_geometry_batched(
            dev,
            warm,
            draws,
            clear,
            opaque_depth_enabled,
            depth_config,
            render_target_gpu,
            render_target_pitch,
            target_width,
            target_height,
        )?;
        let geometry_complete = geometry.completed;
        let mut completed_draws = if geometry_complete { draws.len() } else { 0 };

        // A scene is one atomic visual result.  A timed-out draw leaves the
        // shared target partially updated, so never expose it to either the
        // display or request-render cache.  The caller will retry the same
        // revision on the next scene tick while the last complete frame stays
        // visible.
        let geometry_finished_ns = crate::chronos::monotonic_nanos();
        let scratch_output = crate::intel::gpgpu::GpgpuRgba8Surface::new(
            warm.streamout_phys,
            GPU_VA_STREAMOUT_BASE,
            warm.streamout_len,
            target_width as u32,
            target_height as u32,
            target_pitch as u32,
        )
        .ok_or("resident-scene-resolve-surface")?;
        // On the native 4x path, resolve directly into the UI4 producer back
        // buffer. The scratch surface remains the compatibility target for
        // single-sample hardware and for CPU readback consumers.
        let output = match (frame_output, msaa_color) {
            (ResidentSceneFrameOutput::GpuSurface(destination), Some(_)) => destination,
            (ResidentSceneFrameOutput::GpuSurfaceDeferredRelease(destination), Some(_)) => {
                destination
            }
            (ResidentSceneFrameOutput::DirectGpuSurface(destination), _) => destination,
            _ => scratch_output,
        };
        let direct_scanout_output = match frame_output {
            ResidentSceneFrameOutput::GpuSurface(destination)
            | ResidentSceneFrameOutput::GpuSurfaceDeferredRelease(destination)
            | ResidentSceneFrameOutput::DirectGpuSurface(destination) => {
                output.gpu == destination.gpu
            }
            ResidentSceneFrameOutput::Readback => false,
        };
        let resolved = if geometry_complete {
            if let Some(target) = msaa_color {
                crate::intel::gpgpu::resolve_tile64_msaa4_rgba8_mode(
                    target.surface,
                    output,
                    target_width as u32,
                    target_height as u32,
                    direct_scanout_output,
                )
            } else {
                true
            }
        } else {
            false
        };
        let resolve_finished_ns = crate::chronos::monotonic_nanos();
        let mut completed_coverage_draws = 0usize;
        let mut coverage_submits = 0usize;
        let mut coverage_walkers = 0usize;
        if resolved {
            let batch = crate::intel::gpgpu::glyph_mask_layers_rgba8_2d_mode(
                coverage_draws,
                output,
                direct_scanout_output,
            );
            coverage_submits = batch.submits;
            coverage_walkers = batch.active_walkers;
            if batch.ok && batch.requested_layers == coverage_draws.len() {
                completed_coverage_draws = coverage_draws.len();
            } else if !batch.submitted {
                // Preparation/mapping failures have not touched the target and
                // can safely use the established one-mask submission path.
                // Once a batch was submitted, fail closed: the caller clears
                // and rerenders the whole scene instead of double-blending a
                // possibly partial result.
                coverage_submits = 0;
                coverage_walkers = 0;
                for draw in coverage_draws {
                    let completed = crate::intel::gpgpu::glyph_mask_rgba8_2d_mode(
                        crate::intel::gpgpu::GpgpuGlyphMaskBlit {
                            mask: draw.mask,
                            mask_rect: draw.mask_rect,
                            dst: output,
                            dst_xy: draw.dst_xy,
                            color_rgba: draw.color_rgba,
                        },
                        direct_scanout_output,
                    );
                    if !completed {
                        break;
                    }
                    completed_coverage_draws += 1;
                    coverage_submits += 1;
                    coverage_walkers += 1;
                }
            }
        }
        let coverage_finished_ns = crate::chronos::monotonic_nanos();
        completed_draws = completed_draws.saturating_add(completed_coverage_draws);
        let mut frame_complete = resolved && completed_coverage_draws == coverage_draws.len();
        let present_copy_started_ns = crate::chronos::monotonic_nanos();
        let mut present_copy_performed = false;
        if frame_complete
            && let ResidentSceneFrameOutput::GpuSurface(destination)
            | ResidentSceneFrameOutput::GpuSurfaceDeferredRelease(destination)
            | ResidentSceneFrameOutput::DirectGpuSurface(destination) = frame_output
            && output.gpu != destination.gpu
        {
            present_copy_performed = true;
            frame_complete = crate::intel::gpgpu::copy_rect_rgba8_complete_mode(
                output,
                crate::intel::gpgpu::GpgpuRect::new(
                    0,
                    0,
                    target_width as u32,
                    target_height as u32,
                ),
                destination,
                crate::intel::gpgpu::GpgpuPoint::new(0, 0),
                true,
            );
        }
        let present_copy_finished_ns = crate::chronos::monotonic_nanos();
        let mut changed_pixels = 0usize;
        let mut rgba = None;
        if frame_complete && matches!(frame_output, ResidentSceneFrameOutput::Readback) {
            crate::intel::dma_flush(warm.streamout_virt, target_bytes);
            let pixels = unsafe {
                core::slice::from_raw_parts(
                    warm.streamout_virt as *const u32,
                    target_width * target_height,
                )
            };
            let mut visible_rgba = Vec::with_capacity(target_bytes);
            for pixel in pixels {
                let raw = pixel.to_le_bytes();
                if raw != clear {
                    changed_pixels += 1;
                }
                // Fixed-function over blending produces premultiplied RGB.
                // Screenshots expose straight RGBA, while UI4 consumes the
                // native premultiplied bytes without a redundant conversion.
                let [mut r, mut g, mut b, a] = raw;
                if straight_alpha_output && a != 0 && a != u8::MAX {
                    r = (((u16::from(r) * u16::from(u8::MAX)) + u16::from(a) / 2) / u16::from(a))
                        .min(u16::from(u8::MAX)) as u8;
                    g = (((u16::from(g) * u16::from(u8::MAX)) + u16::from(a) / 2) / u16::from(a))
                        .min(u16::from(u8::MAX)) as u8;
                    b = (((u16::from(b) * u16::from(u8::MAX)) + u16::from(a) / 2) / u16::from(a))
                        .min(u16::from(u8::MAX)) as u8;
                }
                visible_rgba.extend_from_slice(&[r, g, b, a]);
            }
            rgba = Some(visible_rgba);
        } else if frame_complete {
            // Direct output deliberately avoids the CPU changed-pixel scan.
            // The producer publishes full damage, so report the number of
            // pixels written rather than pretending that no frame changed.
            changed_pixels = target_width.saturating_mul(target_height);
        }
        // The direct single-sample path already ends in the renderer's RCS
        // release packet. Multi-pass output receives a dedicated finalizer
        // after resolve/coverage/copy so an older completion marker can never
        // be promoted into a display-ownership proof.
        let release_fence = match frame_output {
            ResidentSceneFrameOutput::DirectGpuSurface(destination)
                if frame_complete
                    && msaa_color.is_none()
                    && coverage_draws.is_empty()
                    && !present_copy_performed =>
            {
                Some(resident_scene_release(destination))
            }
            ResidentSceneFrameOutput::GpuSurface(destination) if frame_complete => {
                let finalizer =
                    crate::intel::gpgpu::release_rgba8_surface_for_scanout(destination);
                if finalizer.ok
                    && finalizer
                        .release
                        .is_some_and(|release| release.matches(destination.phys, destination.bytes))
                {
                    Some(resident_scene_release(destination))
                } else {
                    None
                }
            }
            _ => None,
        };
        let frame_us = crate::chronos::monotonic_nanos().saturating_sub(frame_started_ns) / 1_000;
        let geometry_us = geometry_finished_ns.saturating_sub(frame_started_ns) / 1_000;
        let perf_sequence = RESIDENT_SCENE_PERF_SEQUENCE
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        if perf_sequence == 1 || perf_sequence.is_multiple_of(256) {
            crate::log_info!(
                target: "render";
                "draw3d-perf: seq={} draws={} frame_us={} geometry_us={} prepare_us={} gpu_poll_us={} gpu_poll_iters={} geometry_other_us={} note=geometry_other_includes_lock-forcewake-lrc-guc-submit-result-handoff\n",
                perf_sequence,
                draws.len(),
                frame_us,
                geometry_us,
                geometry.prepare_us,
                geometry.gpu_poll_us,
                geometry.gpu_poll_iters,
                geometry_us.saturating_sub(geometry.prepare_us).saturating_sub(geometry.gpu_poll_us),
            );
        }
        Ok(ResidentSceneFrameResult {
            completed_draws,
            requested_draws: draws.len().saturating_add(coverage_draws.len()),
            changed_pixels,
            presented: false,
            width: target_width as u32,
            height: target_height as u32,
            frame_us,
            geometry_us,
            geometry_prepare_us: geometry.prepare_us,
            gpu_poll_us: geometry.gpu_poll_us,
            gpu_poll_iters: geometry.gpu_poll_iters,
            resolve_us: resolve_finished_ns.saturating_sub(geometry_finished_ns) / 1_000,
            coverage_us: coverage_finished_ns.saturating_sub(resolve_finished_ns) / 1_000,
            present_copy_us: present_copy_finished_ns.saturating_sub(present_copy_started_ns)
                / 1_000,
            present_copy_performed,
            coverage_submits,
            coverage_walkers,
            rgba,
            frame_complete,
            release_fence,
        })
    })();
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    result
}

pub(crate) fn submit_resident_font_mesh_readback_once(
    mesh: &ResidentFontMesh,
    native_scale: u32,
    rgba: crate::intel::gpu_font::GpuFontRgba,
) -> Result<(RenderJokerResult, Option<FontRenderTargetReadback>), &'static str> {
    let mut readback = None;
    let render = submit_resident_font_mesh_inner(mesh, native_scale, rgba, Some(&mut readback))?;
    Ok((render, readback))
}

fn submit_resident_font_mesh_inner(
    mesh: &ResidentFontMesh,
    native_scale: u32,
    rgba: crate::intel::gpu_font::GpuFontRgba,
    readback: Option<&mut Option<FontRenderTargetReadback>>,
) -> Result<RenderJokerResult, &'static str> {
    if mesh.vertex_count < 3
        || mesh.index_count < 3
        || !mesh.index_count.is_multiple_of(3)
        || mesh.vertex_bytes == 0
        || mesh.index_bytes == 0
    {
        return Err("resident-font-shape");
    }
    if !font_native_scale_supported(native_scale) {
        return Err("font-native-scale-range");
    }
    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("in-flight");
    }

    let target_size = FONT_STAMP_BASE_SIZE * native_scale as usize;
    // One RGBA path for every font draw, including the historical default
    // blue. Both enabled SIMD widths receive the same draw-time color and
    // alpha specialization; resident geometry remains untouched.
    let draw_rgba = Some([rgba.r, rgba.g, rgba.b, rgba.a]);
    let result = submit_render_custom_triangle_probe_locked_at_extent(
        &[],
        None,
        Some(mesh),
        draw_rgba,
        "font-resident-reuse",
        "font-resident-3d",
        "kernel-font-service-resident-indexed-mesh",
        "resident-render-ppgtt-vb-ib",
        TriangleBlendProbeMode::MesaZeroedState,
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::HeavyAll,
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
        TriangleBatchMode::Draw,
        StreamoutProofExperiment::HeaderAndPositionSlots01,
        target_size,
        target_size,
        readback,
    );
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    result
}

pub(crate) fn submit_gpu_font_outline_mesh_once(
    mesh: crate::intel::gpgpu::GpgpuFontOutlineMesh,
) -> Result<RenderJokerResult, &'static str> {
    const SUBMIT_NAME: &str = "font-outline-gpu-mesh-3d";

    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("in-flight");
    }
    let result = (|| {
        let Some(dev) = crate::intel::claimed_device() else {
            return Err("no-device");
        };
        let warm = warm_once(dev);
        let target_pitch = FONT_PROOF_TARGET_SIZE * core::mem::size_of::<u32>();
        let target_bytes = target_pitch * FONT_PROOF_TARGET_SIZE;
        if warm.streamout_len < target_bytes || warm.streamout_virt.is_null() {
            return Err("warm-scratch");
        }
        if !forcewake_render_acquire(warm) {
            return Err("forcewake");
        }
        if !ensure_smoke_buffers_mapped(dev, warm) {
            return Err("render-map");
        }

        unsafe {
            let scratch_pixels = core::slice::from_raw_parts_mut(
                warm.streamout_virt as *mut u32,
                FONT_PROOF_TARGET_SIZE * FONT_PROOF_TARGET_SIZE,
            );
            scratch_pixels.fill(0xDEAD_BEEF);
        }
        crate::intel::dma_flush(warm.streamout_virt, target_bytes);

        let probe_seq = PRIMARY_PROBE_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
        intel_render_focus_log!(
            "gpu-font-chain begin seq={} submit={} producer=gpgpu consumer=3d vertices={} indices={} storage_phys=0x{:X} cpu_geometry_copy=0 target={}x{}\n",
            probe_seq,
            SUBMIT_NAME,
            mesh.vertex_count,
            mesh.index_count,
            mesh.storage_phys,
            FONT_PROOF_TARGET_SIZE,
            FONT_PROOF_TARGET_SIZE,
        );
        let completed = submit_triangle_real_vs_draw_probe_vertices_to_surface_ext(
            dev,
            warm,
            GPU_VA_STREAMOUT_BASE,
            target_pitch,
            FONT_PROOF_TARGET_SIZE,
            FONT_PROOF_TARGET_SIZE,
            TriangleBlendProbeMode::MesaZeroedState,
            None,
            &[],
            None,
            Some(mesh),
            None,
            None,
            "skrifa-gpgpu-full-text-outline-stroke",
            SUBMIT_NAME,
            TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
            BackendProbeMode::MesaLike,
            PostDrawSyncVariant::HeavyAll,
            TriangleBatchMode::Draw,
            StreamoutProofExperiment::PositionSlot1,
            [0.0, 0.0],
            None,
        );
        let frontier = latest_render_frontier_summary();
        intel_render_focus_log!(
            "gpu-font-chain end seq={} submit={} completed={} vs={} clip={} ps={} cpu_geometry_copy=0\n",
            probe_seq,
            SUBMIT_NAME,
            completed as u8,
            frontier.vs_counter as u8,
            frontier.clip_counter as u8,
            frontier.ps_observed as u8,
        );
        Ok(RenderJokerResult {
            variant: "gpgpu-full-text-outline-stroke-indexed",
            submit_name: SUBMIT_NAME,
            target: "scratch",
            completed,
            vs_counter: frontier.vs_counter,
            ps_state_marker: frontier.ps_state_marker,
            raster_packet: frontier.raster_packet,
            clip_counter: frontier.clip_counter,
            ps_observed: frontier.ps_observed,
        })
    })();
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    result
}

pub(crate) struct RenderOaControlResult {
    pub(crate) action: &'static str,
    pub(crate) oactx: u32,
    pub(crate) oar: u32,
    pub(crate) ctx_ctrl: u32,
}

pub(crate) struct RenderArtificialFragmentResult {
    pub(crate) mode: &'static str,
    pub(crate) ok: bool,
    pub(crate) descs: usize,
    pub(crate) before: u32,
    pub(crate) after: u32,
    pub(crate) rt_gpu: u64,
    pub(crate) remapped_render: bool,
}

const RENDER_JOKER_VARIANTS: &[&str] = &[
    "canonical",
    "mesa",
    "mesa-retire",
    "bt0",
    "bt0-primary",
    "scratch",
    "oa",
    "point",
    "point-scratch",
    "point-oa",
    "point-oa-pos0",
    "point-oa-header",
    "point-oa-killoff",
    "point-oa-smooth",
    "point-oa-msrast",
    "point-oa-msrast-force",
    "point-oa-deref0",
    "point-oa-hz0",
    "point-oa-wm-normal",
    "point-oa-wm-reemit",
    "point-oa-hz-omit",
    "point-oa-ps-off",
    "point-oa-bt1",
    "point-oa-early",
    "point-oa-early-killoff",
    "point-oa-clip-normal",
    "point-oa-clip-persp",
    "point-oa-clip-disable",
    "point-oa-clip-disable-arm",
    "point-oa-clip-force",
    "point-oa-clip-d3d",
    "point-oa-clip-xy",
    "point-oa-sbe0",
    "point-oa-sbe-pre-clip",
    "point-oa-sbe-pre-sf",
    "point-oa-no-pr",
    "point-oa-vfg",
    "point-oa-w64",
    "point-oa-w64-early",
    "point-oa-w64-early-scissor",
    "point-oa-screen-w64",
    "point-oa-w64-arm",
    "point-oa-w64-wm-normal",
    "point-oa-w64-wm-reemit",
    "point-oa-w64-hz-omit",
    "point-oa-w64-ps-off",
    "point-oa-w64-payload-attr",
    "point-oa-w64-payload-depthw",
    "point-oa-w64-payload-bary",
    "point-oa-w64-sbe-pre-clip",
    "point-oa-w64-sbe-pre-sf",
    "point-oa-w1023",
    "point-oa-w1023-nowmpoint",
    "point-oa-w1023-scissor",
    "point-oa-vtxw",
    "point-oa-early-w1023",
    "point-oa-early-msrast-force",
    "point-bt1",
    "point-slot0",
    "screen-vs-scratch",
    "screen-vs-oa",
    "screen-vs-ndc-oa",
    "screen-vs-ndc-oa-hz0",
    "screen-vs-sbe0",
    "screen-vs-slot0-oa",
    "screen-vs-urb2-oa",
    "screen-vs-urb2-slot0-oa",
    "vf-rect-oa",
    "vf-rect-oa-pos0",
    "vf-rect-oa-header",
    "vf-rect-oa-deref0",
    "vf-rect-ndc-oa",
    "vf-rect-ndc-oa-sbe-pre-clip",
    "vf-rect-ndc-oa-sbe-pre-sf",
    "vf-rect-ndc-oa-drawrect-early",
    "vf-rect-ndc-oa-sample-early",
    "vf-rect-ndc-oa-pc-clip-sf",
    "vf-rect-ndc-oa-hz-pre-wm",
    "vf-rect-ndc-oa-hz-post-extra",
    "vf-rect-ndc-oa-payload-attr",
    "vf-rect-ndc-oa-payload-depthw",
    "vf-rect-ndc-oa-payload-bary",
    "vf-rect-ndc-oa-persp",
    "vf-rect-ndc-oa-clipxy",
    "vf-rect-ndc-oa-clip-disable",
    "vf-rect-ndc-oa-clip-force",
    "vf-rect-ndc-oa-clip-d3d",
    "vf-rect-ndc-oa-early-clipxy",
    "vf-rect-ndc-oa-frontccw",
    "vf-rect-ndc-oa-hz0",
    "vf-rect-ndc-oa-early",
    "vf-rect-ndc-oa-bt1",
    "vf-rect-ndc-order-b-oa",
    "vf-rect-ndc-order-c-oa",
    "vf-rect-ndc-order-c-early-oa",
    "vf-rect-ndc-order-c-clip-disable-oa",
    "vf-rect-ndc-mesa-simple-oa",
    "vf-rect-ndc-mesa-nosrc-header-oa",
    "vf-rect-ndc-small-oa",
    "vf-rect-ndc-cw-oa",
    "vf-rect-ndc-alt-oa",
    "vf-rect-order-b-oa",
    "vf-rect-order-b-early-oa",
    "vf-rect-order-b-scissor-oa",
    "vf-rect-mesa-simple-oa",
    "vf-rect-mesa-simple-oa-early",
    "vf-tri-mesa-simple-oa-early",
    "vf-rect-mesa-simple-oa-arm",
    "vf-rect-mesa-nosrc-header-oa",
    "vf-rect-order-c-oa",
    "vf-tri-ndc-oa",
    "vf-tri-ndc-oa-early",
    "vf-tri-ndc-oa-early-clipxy",
    "vf-tri-ndc-cw-oa-early",
    "screen-rect-scratch",
    "screen-rect-oa-early",
    "so-vf",
    "so-vf-header",
    "so-vs",
    "so-vs-header",
    "bt1",
    "wm-normal",
    "slot0",
    "slot1",
    "slot2",
    "all",
    "simd16",
    "simd16-retire",
    "eot",
    "eot-retire",
    "cps",
    "cps-retire",
    "hz",
    "hz-retire",
    "reemit",
    "reemit-retire",
    "reemit-vs-retire",
    "reemit-vs-slot0-retire",
    "reemit-vs-urb2-retire",
    "reemit-vs-urb2-slot0-retire",
    "payload-push",
    "payload-attr",
    "payload-simple",
    "payload-depthw",
    "payload-bary",
    "grf1",
    "grf2",
    "grf4",
    "mt31",
    "mt15",
    "sync-light",
    "sync-post-no-cs",
    "sync-cs-no-post",
];

pub(crate) fn render_joker_variant_names() -> &'static [&'static str] {
    RENDER_JOKER_VARIANTS
}

pub(crate) fn render_oa_control_action_names() -> &'static [&'static str] {
    &[
        "status",
        "selectors",
        "ctx-on",
        "ctx-off",
        "oactx-on",
        "oactx-off",
        "oar-on",
        "oar-off",
        "full-on",
        "full-off",
    ]
}

fn retired_render_joker_variant_reason(name: &str) -> Option<&'static str> {
    if name.eq_ignore_ascii_case("point-oa-w8")
        || name.eq_ignore_ascii_case("point-oa-w8-clipmax")
        || name.eq_ignore_ascii_case("point-oa-w64-clipmax")
    {
        Some("retired-invalid-point-width-hw-contract")
    } else {
        None
    }
}

pub(crate) fn render_oa_control_action(
    action: &str,
) -> Result<RenderOaControlResult, &'static str> {
    let Some(dev) = crate::intel::claimed_device() else {
        return Err("no-device");
    };
    if !forcewake_render_acquire(warm_once(dev)) {
        return Err("forcewake");
    }

    let action = if action.eq_ignore_ascii_case("status") {
        "status"
    } else if action.eq_ignore_ascii_case("selectors") {
        "selectors"
    } else if action.eq_ignore_ascii_case("ctx-on") {
        "ctx-on"
    } else if action.eq_ignore_ascii_case("ctx-off") {
        "ctx-off"
    } else if action.eq_ignore_ascii_case("oactx-on") {
        "oactx-on"
    } else if action.eq_ignore_ascii_case("oactx-off") {
        "oactx-off"
    } else if action.eq_ignore_ascii_case("oar-on") {
        "oar-on"
    } else if action.eq_ignore_ascii_case("oar-off") {
        "oar-off"
    } else if action.eq_ignore_ascii_case("full-on") {
        "full-on"
    } else if action.eq_ignore_ascii_case("full-off") {
        "full-off"
    } else {
        return Err("unknown-action");
    };

    let before_oactx = crate::intel::mmio_read(dev, RCS_OACTXCONTROL);
    let before_oar = crate::intel::mmio_read(dev, OAR_OACONTROL);
    let before_ctx = crate::intel::mmio_read(dev, RCS_RING_CONTEXT_CONTROL);
    intel_render_focus_log!(
        "oa-control begin action={} oactx=0x{:08X} oar=0x{:08X} ctx_ctrl=0x{:08X}\n",
        action,
        before_oactx,
        before_oar,
        before_ctx,
    );

    match action {
        "status" => {}
        "selectors" => write_raster_wm_oa_selectors(dev),
        "ctx-on" => crate::intel::mmio_write(
            dev,
            RCS_RING_CONTEXT_CONTROL,
            masked_bits_update(CTX_CTRL_OAC_CONTEXT_ENABLE, 0),
        ),
        "ctx-off" => crate::intel::mmio_write(
            dev,
            RCS_RING_CONTEXT_CONTROL,
            masked_bits_update(0, CTX_CTRL_OAC_CONTEXT_ENABLE),
        ),
        "oactx-on" => crate::intel::mmio_write(dev, RCS_OACTXCONTROL, OACTXCONTROL_COUNTER_RESUME),
        "oactx-off" => crate::intel::mmio_write(dev, RCS_OACTXCONTROL, 0),
        "oar-on" => crate::intel::mmio_write(
            dev,
            OAR_OACONTROL,
            OAR_OACONTROL_FORMAT_A24_A14_B8_C8 | OAR_OACONTROL_COUNTER_ENABLE,
        ),
        "oar-off" => crate::intel::mmio_write(dev, OAR_OACONTROL, 0),
        "full-on" => {
            write_raster_wm_oa_selectors(dev);
            crate::intel::mmio_write(dev, RCS_OACTXCONTROL, OACTXCONTROL_COUNTER_RESUME);
            crate::intel::mmio_write(
                dev,
                OAR_OACONTROL,
                OAR_OACONTROL_FORMAT_A24_A14_B8_C8 | OAR_OACONTROL_COUNTER_ENABLE,
            );
            crate::intel::mmio_write(
                dev,
                RCS_RING_CONTEXT_CONTROL,
                masked_bits_update(CTX_CTRL_OAC_CONTEXT_ENABLE, 0),
            );
        }
        "full-off" => {
            crate::intel::mmio_write(dev, RCS_OACTXCONTROL, 0);
            crate::intel::mmio_write(dev, OAR_OACONTROL, 0);
            crate::intel::mmio_write(
                dev,
                RCS_RING_CONTEXT_CONTROL,
                masked_bits_update(0, CTX_CTRL_OAC_CONTEXT_ENABLE),
            );
        }
        _ => return Err("unknown-action"),
    }

    let after_oactx = crate::intel::mmio_read(dev, RCS_OACTXCONTROL);
    let after_oar = crate::intel::mmio_read(dev, OAR_OACONTROL);
    let after_ctx = crate::intel::mmio_read(dev, RCS_RING_CONTEXT_CONTROL);
    intel_render_focus_log!(
        "oa-control end action={} oactx=0x{:08X}->0x{:08X} oar=0x{:08X}->0x{:08X} ctx_ctrl=0x{:08X}->0x{:08X}\n",
        action,
        before_oactx,
        after_oactx,
        before_oar,
        after_oar,
        before_ctx,
        after_ctx,
    );

    Ok(RenderOaControlResult {
        action,
        oactx: after_oactx,
        oar: after_oar,
        ctx_ctrl: after_ctx,
    })
}

fn write_raster_wm_oa_selectors(dev: crate::intel::Dev) {
    crate::intel::mmio_write(dev, OAG_OASTARTTRIG1, 0);
    crate::intel::mmio_write(dev, OAG_OASTARTTRIG2, 0x0080_0000);
    crate::intel::mmio_write(dev, OAG_OASTARTTRIG3, 0);
    crate::intel::mmio_write(dev, OAG_OASTARTTRIG4, 0x0080_0000);
    crate::intel::mmio_write(dev, OAG_OAREPORTTRIG1, 0);
    crate::intel::mmio_write(dev, OAG_SPCTR_CNF, 0);
    crate::intel::mmio_write(dev, OAA_LENABLE_REG, 0);
    crate::intel::mmio_write(dev, OAG_OA_PESS, 0);
}

#[derive(Copy, Clone)]
struct RenderJokerSpec {
    variant: &'static str,
    submit_name: &'static str,
    target: RenderJokerTarget,
    blend: TriangleBlendProbeMode,
    geometry: VfPrimitiveGeometry,
    backend: BackendProbeMode,
    sync: PostDrawSyncVariant,
}

#[derive(Copy, Clone)]
enum RenderJokerTarget {
    Primary,
    ScratchRt,
}

fn parse_render_joker_spec(name: &str) -> Option<RenderJokerSpec> {
    let surface = RenderJokerTarget::Primary;
    let scratch = RenderJokerTarget::ScratchRt;
    let explicit = TriangleBlendProbeMode::ExplicitRt0;
    let zeroed = TriangleBlendProbeMode::MesaZeroedState;
    let canonical = VfPrimitiveGeometry::Canonical;
    let big = VfPrimitiveGeometry::Oversized;
    let point = VfPrimitiveGeometry::CenterPoint;
    let screen_point = VfPrimitiveGeometry::ScreenSpacePoint8x8;
    let screen_space = VfPrimitiveGeometry::ScreenSpace8x8;
    let screen_rect = VfPrimitiveGeometry::ScreenSpaceRect8x8;
    let screen_tri_order_b = VfPrimitiveGeometry::ScreenSpaceTri8x8OrderB;
    let screen_rect_order_b = VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB;
    let screen_rect_order_c = VfPrimitiveGeometry::ScreenSpaceRect8x8OrderC;
    let ndc_triangle = VfPrimitiveGeometry::NdcTriangleLarge;
    let ndc_triangle_cw = VfPrimitiveGeometry::NdcTriangleLargeCw;
    let ndc_rect = VfPrimitiveGeometry::NdcRect;
    let ndc_rect_cw = VfPrimitiveGeometry::NdcRectCw;
    let ndc_rect_alt = VfPrimitiveGeometry::NdcRectAlt;
    let ndc_rect_order_c = VfPrimitiveGeometry::NdcRectUrLrUl;
    let ndc_rect_small = VfPrimitiveGeometry::NdcRectSmall;
    let heavy = PostDrawSyncVariant::HeavyAll;
    let light_post_no_cs = PostDrawSyncVariant::LightPostSyncNoCs;

    let spec = if name.eq_ignore_ascii_case("canonical") {
        RenderJokerSpec {
            variant: "canonical",
            submit_name: "vf-draw-path",
            target: surface,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mesa") || name.eq_ignore_ascii_case("big") {
        RenderJokerSpec {
            variant: "mesa",
            submit_name: "ps-launch-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mesa-retire") {
        RenderJokerSpec {
            variant: "mesa-retire",
            submit_name: "ps-launch-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::MesaLike,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("bt0") || name.eq_ignore_ascii_case("scratch") {
        RenderJokerSpec {
            variant: if name.eq_ignore_ascii_case("scratch") {
                "scratch"
            } else {
                "bt0"
            },
            submit_name: "ps-bt0-scratch-rt",
            target: scratch,
            blend: zeroed,
            geometry: big,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("bt0-primary") {
        RenderJokerSpec {
            variant: "bt0-primary",
            submit_name: "ps-bt0-primary-rt",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("oa") {
        RenderJokerSpec {
            variant: "oa",
            submit_name: "raster-wm-oa-probe",
            target: scratch,
            blend: zeroed,
            geometry: big,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point") || name.eq_ignore_ascii_case("giant-point") {
        RenderJokerSpec {
            variant: "point",
            submit_name: "point-vf-giant",
            target: surface,
            blend: explicit,
            geometry: point,
            backend: BackendProbeMode::MesaLike,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-scratch") {
        RenderJokerSpec {
            variant: "point-scratch",
            submit_name: "point-vf-giant-scratch",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa") {
        RenderJokerSpec {
            variant: "point-oa",
            submit_name: "point-vf-giant-oa",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-pos0") {
        RenderJokerSpec {
            variant: "point-oa-pos0",
            submit_name: "point-vf-giant-oa-pos0",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-header") {
        RenderJokerSpec {
            variant: "point-oa-header",
            submit_name: "point-vf-giant-oa-header",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-killoff") {
        RenderJokerSpec {
            variant: "point-oa-killoff",
            submit_name: "point-vf-giant-oa-killoff",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaKillOff,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-smooth") {
        RenderJokerSpec {
            variant: "point-oa-smooth",
            submit_name: "point-vf-giant-oa-smooth",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaSmoothPoint,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-msrast") {
        RenderJokerSpec {
            variant: "point-oa-msrast",
            submit_name: "point-vf-giant-oa-msrast",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaMsRaster,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-msrast-force") {
        RenderJokerSpec {
            variant: "point-oa-msrast-force",
            submit_name: "point-vf-giant-oa-msrast-force",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaMsRasterForced,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-deref0") {
        RenderJokerSpec {
            variant: "point-oa-deref0",
            submit_name: "point-vf-giant-oa-deref0",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaDerefBlock0,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-hz0") {
        RenderJokerSpec {
            variant: "point-oa-hz0",
            submit_name: "point-vf-giant-oa-hz0",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaNoHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-wm-normal") {
        RenderJokerSpec {
            variant: "point-oa-wm-normal",
            submit_name: "point-vf-giant-oa-wm-normal",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaWmNormalDispatch,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-wm-reemit") {
        RenderJokerSpec {
            variant: "point-oa-wm-reemit",
            submit_name: "point-vf-giant-oa-wm-reemit",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaWmReemitAfterPsExtra,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-hz-omit") {
        RenderJokerSpec {
            variant: "point-oa-hz-omit",
            submit_name: "point-vf-giant-oa-hz-omit",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaOmitHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-ps-off") {
        RenderJokerSpec {
            variant: "point-oa-ps-off",
            submit_name: "point-vf-giant-oa-ps-off",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPsDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-bt1") {
        RenderJokerSpec {
            variant: "point-oa-bt1",
            submit_name: "point-vf-giant-oa-bt1",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaBtCountOne,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-early") {
        RenderJokerSpec {
            variant: "point-oa-early",
            submit_name: "point-vf-giant-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-early-killoff") {
        RenderJokerSpec {
            variant: "point-oa-early-killoff",
            submit_name: "point-vf-giant-oa-early-killoff",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaEarlyKillOff,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-normal") {
        RenderJokerSpec {
            variant: "point-oa-clip-normal",
            submit_name: "point-vf-giant-oa-clip-normal",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipNormal,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-persp") {
        RenderJokerSpec {
            variant: "point-oa-clip-persp",
            submit_name: "point-vf-giant-oa-clip-persp",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipPerspective,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-disable") {
        RenderJokerSpec {
            variant: "point-oa-clip-disable",
            submit_name: "point-vf-giant-oa-clip-disable",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-disable-arm") {
        RenderJokerSpec {
            variant: "point-oa-clip-disable-arm",
            submit_name: "point-vf-giant-oa-clip-disable-arm",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipDisabledArtificial,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-force") {
        RenderJokerSpec {
            variant: "point-oa-clip-force",
            submit_name: "point-vf-giant-oa-clip-force",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipForceMode,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-d3d") {
        RenderJokerSpec {
            variant: "point-oa-clip-d3d",
            submit_name: "point-vf-giant-oa-clip-d3d",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipApiD3d,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-xy") {
        RenderJokerSpec {
            variant: "point-oa-clip-xy",
            submit_name: "point-vf-giant-oa-clip-xy",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipViewportXy,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-sbe0") {
        RenderJokerSpec {
            variant: "point-oa-sbe0",
            submit_name: "point-vf-giant-oa-sbe0",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaSbeRead0,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-sbe-pre-clip") {
        RenderJokerSpec {
            variant: "point-oa-sbe-pre-clip",
            submit_name: "point-vf-giant-oa-sbe-pre-clip",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaSbeBeforeClip,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-sbe-pre-sf") {
        RenderJokerSpec {
            variant: "point-oa-sbe-pre-sf",
            submit_name: "point-vf-giant-oa-sbe-pre-sf",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaSbeBeforeSf,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-no-pr") {
        RenderJokerSpec {
            variant: "point-oa-no-pr",
            submit_name: "point-vf-giant-oa-no-pr",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaNoPrimitiveReplication,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-vfg") {
        RenderJokerSpec {
            variant: "point-oa-vfg",
            submit_name: "point-vf-giant-oa-vfg",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaVfGeometryDistribution,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w8") {
        RenderJokerSpec {
            variant: "point-oa-w8",
            submit_name: "point-vf-giant-oa-w8",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth8,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w8-clipmax") {
        RenderJokerSpec {
            variant: "point-oa-w8-clipmax",
            submit_name: "point-vf-giant-oa-w8-clipmax",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth8ClipMax,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64") {
        RenderJokerSpec {
            variant: "point-oa-w64",
            submit_name: "point-vf-giant-oa-w64",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-halign128") {
        RenderJokerSpec {
            variant: "point-oa-w64-halign128",
            submit_name: "point-vf-giant-oa-w64-halign128",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64SurfaceHalign128,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-clipmax") {
        RenderJokerSpec {
            variant: "point-oa-w64-clipmax",
            submit_name: "point-vf-giant-oa-w64-clipmax",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64ClipMax,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-early") {
        RenderJokerSpec {
            variant: "point-oa-w64-early",
            submit_name: "point-vf-giant-oa-w64-early",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64Early,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-early-scissor") {
        RenderJokerSpec {
            variant: "point-oa-w64-early-scissor",
            submit_name: "point-vf-giant-oa-w64-early-scissor",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64EarlyScissor,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-screen-w64") {
        RenderJokerSpec {
            variant: "point-oa-screen-w64",
            submit_name: "point-vf-screen-oa-w64",
            target: scratch,
            blend: zeroed,
            geometry: screen_point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64Screen,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-arm") {
        RenderJokerSpec {
            variant: "point-oa-w64-arm",
            submit_name: "point-vf-giant-oa-w64-arm",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64Artificial,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-wm-normal") {
        RenderJokerSpec {
            variant: "point-oa-w64-wm-normal",
            submit_name: "point-vf-giant-oa-w64-wm-normal",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64WmNormalDispatch,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-wm-reemit") {
        RenderJokerSpec {
            variant: "point-oa-w64-wm-reemit",
            submit_name: "point-vf-giant-oa-w64-wm-reemit",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64WmReemitAfterPsExtra,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-hz-omit") {
        RenderJokerSpec {
            variant: "point-oa-w64-hz-omit",
            submit_name: "point-vf-giant-oa-w64-hz-omit",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64OmitHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-ps-off") {
        RenderJokerSpec {
            variant: "point-oa-w64-ps-off",
            submit_name: "point-vf-giant-oa-w64-ps-off",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64PsDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-payload-attr") {
        RenderJokerSpec {
            variant: "point-oa-w64-payload-attr",
            submit_name: "point-vf-giant-oa-w64-payload-attr",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64PayloadAttributeEnable,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-payload-depthw") {
        RenderJokerSpec {
            variant: "point-oa-w64-payload-depthw",
            submit_name: "point-vf-giant-oa-w64-payload-depthw",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64PayloadSourceDepthW,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-payload-bary") {
        RenderJokerSpec {
            variant: "point-oa-w64-payload-bary",
            submit_name: "point-vf-giant-oa-w64-payload-bary",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64PayloadBaryPlanes,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-sbe-pre-clip") {
        RenderJokerSpec {
            variant: "point-oa-w64-sbe-pre-clip",
            submit_name: "point-vf-giant-oa-w64-sbe-pre-clip",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64SbeBeforeClip,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-sbe-pre-sf") {
        RenderJokerSpec {
            variant: "point-oa-w64-sbe-pre-sf",
            submit_name: "point-vf-giant-oa-w64-sbe-pre-sf",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64SbeBeforeSf,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w1023") {
        RenderJokerSpec {
            variant: "point-oa-w1023",
            submit_name: "point-vf-giant-oa-w1023",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth1023,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w1023-nowmpoint") {
        RenderJokerSpec {
            variant: "point-oa-w1023-nowmpoint",
            submit_name: "point-vf-giant-oa-w1023-nowmpoint",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth1023NoWmPoint,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w1023-scissor") {
        RenderJokerSpec {
            variant: "point-oa-w1023-scissor",
            submit_name: "point-vf-giant-oa-w1023-scissor",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth1023Scissor,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-vtxw") {
        RenderJokerSpec {
            variant: "point-oa-vtxw",
            submit_name: "point-vf-giant-oa-vtxw",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidthVertex,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-early-w1023") {
        RenderJokerSpec {
            variant: "point-oa-early-w1023",
            submit_name: "point-vf-giant-oa-early-w1023",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaEarlyPointWidth1023,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-early-msrast-force") {
        RenderJokerSpec {
            variant: "point-oa-early-msrast-force",
            submit_name: "point-vf-giant-oa-early-msrast-force",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaEarlyMsRasterForced,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-bt1") {
        RenderJokerSpec {
            variant: "point-bt1",
            submit_name: "point-vf-giant-bt1",
            target: surface,
            blend: explicit,
            geometry: point,
            backend: BackendProbeMode::PsBindingTableCountOne,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-slot0") {
        RenderJokerSpec {
            variant: "point-slot0",
            submit_name: "point-vf-giant-slot0",
            target: surface,
            blend: explicit,
            geometry: point,
            backend: BackendProbeMode::PsDispatchSlot0,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-scratch") {
        RenderJokerSpec {
            variant: "screen-vs-scratch",
            submit_name: "screen-vs-scratch",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-oa") {
        RenderJokerSpec {
            variant: "screen-vs-oa",
            submit_name: "screen-vs-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-ndc-oa") {
        RenderJokerSpec {
            variant: "screen-vs-ndc-oa",
            submit_name: "screen-vs-ndc-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-ndc-oa-hz0") {
        RenderJokerSpec {
            variant: "screen-vs-ndc-oa-hz0",
            submit_name: "screen-vs-ndc-oa-hz0",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOaNoHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-sbe0") {
        RenderJokerSpec {
            variant: "screen-vs-sbe0",
            submit_name: "screen-vs-sbe0",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-slot0-oa") {
        RenderJokerSpec {
            variant: "screen-vs-slot0-oa",
            submit_name: "screen-vs-slot0-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-urb2-oa") {
        RenderJokerSpec {
            variant: "screen-vs-urb2-oa",
            submit_name: "screen-vs-urb2-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-urb2-slot0-oa") {
        RenderJokerSpec {
            variant: "screen-vs-urb2-slot0-oa",
            submit_name: "screen-vs-urb2-slot0-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-oa") {
        RenderJokerSpec {
            variant: "vf-rect-oa",
            submit_name: "vf-rect-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-oa-pos0") {
        RenderJokerSpec {
            variant: "vf-rect-oa-pos0",
            submit_name: "vf-rect-oa-pos0",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-oa-header") {
        RenderJokerSpec {
            variant: "vf-rect-oa-header",
            submit_name: "vf-rect-oa-header",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-oa-deref0") {
        RenderJokerSpec {
            variant: "vf-rect-oa-deref0",
            submit_name: "vf-rect-oa-deref0",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOaDerefBlock0,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa",
            submit_name: "vf-rect-ndc-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-halign128") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-halign128",
            submit_name: "vf-rect-ndc-oa-halign128",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaSurfaceHalign128,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-sbe-pre-clip") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-sbe-pre-clip",
            submit_name: "vf-rect-ndc-oa-sbe-pre-clip",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaSbeBeforeClip,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-sbe-pre-sf") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-sbe-pre-sf",
            submit_name: "vf-rect-ndc-oa-sbe-pre-sf",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaSbeBeforeSf,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-drawrect-early") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-drawrect-early",
            submit_name: "vf-rect-ndc-oa-drawrect-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaDrawRectEarlyOnly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-sample-early") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-sample-early",
            submit_name: "vf-rect-ndc-oa-sample-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaSampleMaskEarlyOnly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-pc-clip-sf") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-pc-clip-sf",
            submit_name: "vf-rect-ndc-oa-pc-clip-sf",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaPipeControlClipSf,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-hz-pre-wm") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-hz-pre-wm",
            submit_name: "vf-rect-ndc-oa-hz-pre-wm",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaWmHzOpBeforeWm,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-hz-post-extra") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-hz-post-extra",
            submit_name: "vf-rect-ndc-oa-hz-post-extra",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaWmHzOpAfterPsExtra,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-payload-attr") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-payload-attr",
            submit_name: "vf-rect-ndc-oa-payload-attr",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaPayloadAttributeEnable,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-payload-depthw") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-payload-depthw",
            submit_name: "vf-rect-ndc-oa-payload-depthw",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaPayloadSourceDepthW,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-payload-bary") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-payload-bary",
            submit_name: "vf-rect-ndc-oa-payload-bary",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaPayloadBaryPlanes,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-persp") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-persp",
            submit_name: "vf-rect-ndc-oa-persp",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipPerspective,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-clipxy") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-clipxy",
            submit_name: "vf-rect-ndc-oa-clipxy",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipViewportXy,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-clip-disable") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-clip-disable",
            submit_name: "vf-rect-ndc-oa-clip-disable",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-clip-force") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-clip-force",
            submit_name: "vf-rect-ndc-oa-clip-force",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipForceMode,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-clip-d3d") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-clip-d3d",
            submit_name: "vf-rect-ndc-oa-clip-d3d",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipApiD3d,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-early-clipxy") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-early-clipxy",
            submit_name: "vf-rect-ndc-oa-early-clipxy",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaEarlyClipViewportXy,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-frontccw") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-frontccw",
            submit_name: "vf-rect-ndc-oa-frontccw",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaFrontCcw,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-hz0") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-hz0",
            submit_name: "vf-rect-ndc-oa-hz0",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaNoHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-early") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-early",
            submit_name: "vf-rect-ndc-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-bt1") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-bt1",
            submit_name: "vf-rect-ndc-oa-bt1",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaBtCountOne,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-order-b-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-order-b-oa",
            submit_name: "vf-rect-ndc-order-b-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_cw,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-order-c-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-order-c-oa",
            submit_name: "vf-rect-ndc-order-c-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-order-c-early-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-order-c-early-oa",
            submit_name: "vf-rect-ndc-order-c-early-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-order-c-clip-disable-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-order-c-clip-disable-oa",
            submit_name: "vf-rect-ndc-order-c-clip-disable-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOaClipDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-mesa-simple-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-mesa-simple-oa",
            submit_name: "vf-rect-ndc-mesa-simple-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRect,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-mesa-nosrc-header-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-mesa-nosrc-header-oa",
            submit_name: "vf-rect-ndc-mesa-nosrc-header-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRectNoSrcHeader,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-small-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-small-oa",
            submit_name: "vf-rect-ndc-small-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_small,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-cw-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-cw-oa",
            submit_name: "vf-rect-ndc-cw-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_cw,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-alt-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-alt-oa",
            submit_name: "vf-rect-ndc-alt-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_alt,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-order-b-oa") {
        RenderJokerSpec {
            variant: "vf-rect-order-b-oa",
            submit_name: "vf-rect-order-b-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-order-b-early-oa") {
        RenderJokerSpec {
            variant: "vf-rect-order-b-early-oa",
            submit_name: "vf-rect-order-b-early-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-order-b-scissor-oa") {
        RenderJokerSpec {
            variant: "vf-rect-order-b-scissor-oa",
            submit_name: "vf-rect-order-b-scissor-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaScissorOnly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-mesa-simple-oa") {
        RenderJokerSpec {
            variant: "vf-rect-mesa-simple-oa",
            submit_name: "vf-rect-mesa-simple-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRect,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-mesa-simple-oa-early") {
        RenderJokerSpec {
            variant: "vf-rect-mesa-simple-oa-early",
            submit_name: "vf-rect-mesa-simple-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRectEarly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-tri-mesa-simple-oa-early") {
        RenderJokerSpec {
            variant: "vf-tri-mesa-simple-oa-early",
            submit_name: "vf-tri-mesa-simple-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: screen_tri_order_b,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRectEarly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-mesa-simple-oa-arm") {
        RenderJokerSpec {
            variant: "vf-rect-mesa-simple-oa-arm",
            submit_name: "vf-rect-mesa-simple-oa-arm",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRectArtificial,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-mesa-nosrc-header-oa") {
        RenderJokerSpec {
            variant: "vf-rect-mesa-nosrc-header-oa",
            submit_name: "vf-rect-mesa-nosrc-header-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRectNoSrcHeader,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-order-c-oa") {
        RenderJokerSpec {
            variant: "vf-rect-order-c-oa",
            submit_name: "vf-rect-order-c-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-tri-ndc-oa") {
        RenderJokerSpec {
            variant: "vf-tri-ndc-oa",
            submit_name: "vf-tri-ndc-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-tri-ndc-oa-early") {
        RenderJokerSpec {
            variant: "vf-tri-ndc-oa-early",
            submit_name: "vf-tri-ndc-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-tri-ndc-oa-early-clipxy") {
        RenderJokerSpec {
            variant: "vf-tri-ndc-oa-early-clipxy",
            submit_name: "vf-tri-ndc-oa-early-clipxy",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOaEarlyClipViewportXy,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-tri-ndc-cw-oa-early") {
        RenderJokerSpec {
            variant: "vf-tri-ndc-cw-oa-early",
            submit_name: "vf-tri-ndc-cw-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle_cw,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-rect-scratch") {
        RenderJokerSpec {
            variant: "screen-rect-scratch",
            submit_name: "screen-rect-scratch",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-rect-oa-early") {
        RenderJokerSpec {
            variant: "screen-rect-oa-early",
            submit_name: "screen-rect-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("so-vf") {
        RenderJokerSpec {
            variant: "so-vf",
            submit_name: "joker-vf-streamout",
            target: surface,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("so-vf-header") {
        RenderJokerSpec {
            variant: "so-vf-header",
            submit_name: "joker-vf-streamout-header",
            target: surface,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("so-vs") {
        RenderJokerSpec {
            variant: "so-vs",
            submit_name: "joker-vs-streamout",
            target: surface,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("so-vs-header") {
        RenderJokerSpec {
            variant: "so-vs-header",
            submit_name: "joker-vs-streamout-header",
            target: surface,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("bt1") {
        RenderJokerSpec {
            variant: "bt1",
            submit_name: "ps-bt1-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsBindingTableCountOne,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("wm-normal") || name.eq_ignore_ascii_case("wm") {
        RenderJokerSpec {
            variant: "wm-normal",
            submit_name: "ps-wm-normal-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmNormalDispatch,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("slot0") {
        RenderJokerSpec {
            variant: "slot0",
            submit_name: "ps-dispatch-slot0-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchSlot0,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("slot1") {
        RenderJokerSpec {
            variant: "slot1",
            submit_name: "ps-dispatch-slot1-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchSlot1,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("slot2") {
        RenderJokerSpec {
            variant: "slot2",
            submit_name: "ps-dispatch-slot2-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchSlot2,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("all") || name.eq_ignore_ascii_case("slots-all") {
        RenderJokerSpec {
            variant: "all",
            submit_name: "ps-dispatch-all-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchAllKspSlots,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("simd16") {
        RenderJokerSpec {
            variant: "simd16",
            submit_name: "ps-simd16-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsSimd16,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("simd16-retire") {
        RenderJokerSpec {
            variant: "simd16-retire",
            submit_name: "ps-simd16-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsSimd16,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("eot") {
        RenderJokerSpec {
            variant: "eot",
            submit_name: "ps-eot-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsEotOnly,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("eot-retire") {
        RenderJokerSpec {
            variant: "eot-retire",
            submit_name: "ps-eot-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsEotOnly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("cps") || name.eq_ignore_ascii_case("cps-disabled") {
        RenderJokerSpec {
            variant: "cps",
            submit_name: "ps-cps-disabled-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsCpsDisabled,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("cps-retire") {
        RenderJokerSpec {
            variant: "cps-retire",
            submit_name: "ps-cps-disabled-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsCpsDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("hz") || name.eq_ignore_ascii_case("wm-hz") {
        RenderJokerSpec {
            variant: "hz",
            submit_name: "wm-hz-sample-mask-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmHzSampleMask,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("hz-retire") || name.eq_ignore_ascii_case("wm-hz-retire") {
        RenderJokerSpec {
            variant: "hz-retire",
            submit_name: "wm-hz-sample-mask-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmHzSampleMask,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit") || name.eq_ignore_ascii_case("late-reemit") {
        RenderJokerSpec {
            variant: "reemit",
            submit_name: "wm-late-reemit-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("reemit-retire")
        || name.eq_ignore_ascii_case("late-reemit-retire")
    {
        RenderJokerSpec {
            variant: "reemit-retire",
            submit_name: "wm-late-reemit-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit-vs-retire")
        || name.eq_ignore_ascii_case("late-reemit-vs-retire")
    {
        RenderJokerSpec {
            variant: "reemit-vs-retire",
            submit_name: "wm-late-reemit-vs-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit-vs-slot0-retire")
        || name.eq_ignore_ascii_case("late-reemit-vs-slot0-retire")
    {
        RenderJokerSpec {
            variant: "reemit-vs-slot0-retire",
            submit_name: "wm-late-reemit-vs-slot0-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit-vs-urb2-retire")
        || name.eq_ignore_ascii_case("late-reemit-vs-urb2-retire")
    {
        RenderJokerSpec {
            variant: "reemit-vs-urb2-retire",
            submit_name: "wm-late-reemit-vs-urb2-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit-vs-urb2-slot0-retire")
        || name.eq_ignore_ascii_case("late-reemit-vs-urb2-slot0-retire")
    {
        RenderJokerSpec {
            variant: "reemit-vs-urb2-slot0-retire",
            submit_name: "wm-late-reemit-vs-urb2-slot0-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("payload-push") {
        RenderJokerSpec {
            variant: "payload-push",
            submit_name: "ps-payload-push-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadPushConstant,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-attr") {
        RenderJokerSpec {
            variant: "payload-attr",
            submit_name: "ps-payload-attr-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadAttributeEnable,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-simple") {
        RenderJokerSpec {
            variant: "payload-simple",
            submit_name: "ps-payload-simple-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadSimpleHint,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-depthw") {
        RenderJokerSpec {
            variant: "payload-depthw",
            submit_name: "ps-payload-source-depth-w-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadSourceDepthW,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-bary") || name.eq_ignore_ascii_case("bary") {
        RenderJokerSpec {
            variant: "payload-bary",
            submit_name: "ps-payload-bary-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadBaryPlanes,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("grf1") {
        RenderJokerSpec {
            variant: "grf1",
            submit_name: "ps-grf-start-r1-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfStartR1,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("grf2") {
        RenderJokerSpec {
            variant: "grf2",
            submit_name: "ps-grf-start-r2-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfStartR2,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("grf4") {
        RenderJokerSpec {
            variant: "grf4",
            submit_name: "ps-grf-start-r4-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfStartR4,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mt31") {
        RenderJokerSpec {
            variant: "mt31",
            submit_name: "ps-grf-maxthreads-31-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfMaxThreads31,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mt15") {
        RenderJokerSpec {
            variant: "mt15",
            submit_name: "ps-grf-maxthreads-15-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfMaxThreads15,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("sync-light") {
        RenderJokerSpec {
            variant: "sync-light",
            submit_name: "postdraw-light-only-retire",
            target: surface,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: PostDrawSyncVariant::LightOnlyRetire,
        }
    } else if name.eq_ignore_ascii_case("sync-post-no-cs") {
        RenderJokerSpec {
            variant: "sync-post-no-cs",
            submit_name: "postdraw-pc-postsync-no-cs",
            target: surface,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("sync-cs-no-post") {
        RenderJokerSpec {
            variant: "sync-cs-no-post",
            submit_name: "postdraw-pc-cs-no-postsync",
            target: surface,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: PostDrawSyncVariant::LightCsNoPostSync,
        }
    } else {
        return None;
    };
    Some(spec)
}

fn render_joker_real_vs_front_end_contract(variant: &str) -> Option<TriangleFrontEndContract> {
    match variant {
        "reemit-vs-retire" => Some(TRIANGLE_DEFAULT_FRONT_END_CONTRACT),
        "reemit-vs-slot0-retire" => Some(VS_DRAW_FRONTIER_CONTRACTS[1]),
        "reemit-vs-urb2-retire" => Some(VS_DRAW_FRONTIER_CONTRACTS[2]),
        "reemit-vs-urb2-slot0-retire" => Some(VS_DRAW_FRONTIER_CONTRACTS[3]),
        "screen-vs-sbe0" => Some(VS_DRAW_SBE_READ0_CONTRACT),
        "screen-vs-ndc-oa" | "screen-vs-ndc-oa-hz0" => Some(TRIANGLE_DEFAULT_FRONT_END_CONTRACT),
        "screen-vs-slot0-oa" => Some(VS_DRAW_FRONTIER_CONTRACTS[1]),
        "screen-vs-urb2-oa" => Some(VS_DRAW_FRONTIER_CONTRACTS[2]),
        "screen-vs-urb2-slot0-oa" => Some(VS_DRAW_FRONTIER_CONTRACTS[3]),
        "screen-vs-scratch" | "screen-vs-oa" | "screen-rect-scratch" | "screen-rect-oa-early" => {
            Some(TRIANGLE_DEFAULT_FRONT_END_CONTRACT)
        }
        _ => None,
    }
}

fn render_joker_vf_experiment(variant: &str) -> StreamoutProofExperiment {
    match variant {
        "point-oa-pos0" => StreamoutProofExperiment::PositionSlot0,
        "vf-rect-mesa-simple-oa"
        | "vf-rect-mesa-simple-oa-early"
        | "vf-rect-mesa-simple-oa-arm"
        | "vf-rect-ndc-mesa-simple-oa"
        | "vf-rect-mesa-nosrc-header-oa"
        | "vf-rect-ndc-mesa-nosrc-header-oa" => StreamoutProofExperiment::PositionSlot0,
        "vf-rect-oa-pos0" => StreamoutProofExperiment::PositionSlot0,
        "point-oa-header" | "vf-rect-oa-header" | "so-vf-header" | "so-vs-header" => {
            StreamoutProofExperiment::HeaderAndPositionSlots01
        }
        "point-oa-vtxw" => StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        _ => StreamoutProofExperiment::PositionSlot1,
    }
}

fn render_joker_streamout_kind(variant: &str) -> Option<&'static str> {
    match variant {
        "so-vf" | "so-vf-header" => Some("vf"),
        "so-vs" | "so-vs-header" => Some("vs"),
        _ => None,
    }
}

pub(crate) fn submit_render_joker_probe(name: &str) -> Result<RenderJokerResult, &'static str> {
    if let Some(reason) = retired_render_joker_variant_reason(name) {
        return Err(reason);
    }

    let Some(spec) = parse_render_joker_spec(name) else {
        return Err("unknown-variant");
    };

    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("in-flight");
    }

    let result = submit_render_joker_probe_locked(spec);
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    result
}

pub(crate) fn submit_render_font_clip_field_isolate_probe<const N: usize>(
    vertices: [[f32; 3]; N],
) -> Result<RenderJokerResult, &'static str> {
    if N == 0 || N % TRIANGLE_DRAW_VERTICES != 0 {
        return Err("vertex-count");
    }
    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("in-flight");
    }

    let ndc_vertices = scratch_vertices_to_ndc(vertices);
    let (variant, submit_name, geometry_label, source_label, backend_probe_mode, sync_variant) =
        match N {
            TRIANGLE_DRAW_VERTICES => (
                "font-clip-field-real-vs-urb2-isolate-scratch",
                "font-tessel-clip-field-real-vs-urb2-isolate-scratch",
                "font-tessel-clip-field-real-vs-urb2-isolate",
                "lyon-font-mirrored-clip-field-isolate-first-triangle/real-vs-urb2",
                BackendProbeMode::WmLateReemit,
                PostDrawSyncVariant::LightPostSyncNoCs,
            ),
            n if n == TRIANGLE_DRAW_VERTICES * 2 => (
                "font-clip-field-real-vs-urb2-two-scratch",
                "font-tessel-clip-field-real-vs-urb2-two-scratch",
                "font-tessel-clip-field-real-vs-urb2-two",
                "lyon-font-mirrored-clip-field-isolate-first-two-triangles/real-vs-urb2",
                BackendProbeMode::WmLateReemit,
                PostDrawSyncVariant::LightPostSyncNoCs,
            ),
            crate::graphics::font::FONT_CLIP_FIELD_VERTICES => (
                "font-clip-field-real-vs-urb2-all-scratch",
                "font-tessel-clip-field-real-vs-urb2-all-scratch",
                "font-tessel-clip-field-real-vs-urb2-all",
                "lyon-font-mirrored-clip-field-isolate-all-triangles/real-vs-urb2",
                BackendProbeMode::WmLateReemit,
                PostDrawSyncVariant::LightPostSyncNoCs,
            ),
            _ => (
                "font-clip-field-real-vs-urb2-n-scratch",
                "font-tessel-clip-field-real-vs-urb2-n-scratch",
                "font-tessel-clip-field-real-vs-urb2-n",
                "lyon-font-mirrored-clip-field-isolate-n-triangles/real-vs-urb2",
                BackendProbeMode::WmLateReemit,
                PostDrawSyncVariant::LightPostSyncNoCs,
            ),
        };
    let result = submit_render_custom_triangle_probe_locked(
        &ndc_vertices,
        None,
        variant,
        submit_name,
        geometry_label,
        source_label,
        TriangleBlendProbeMode::MesaZeroedState,
        backend_probe_mode,
        sync_variant,
        VS_DRAW_FRONTIER_CONTRACTS[2],
        TriangleBatchMode::Draw,
        StreamoutProofExperiment::HeaderAndPositionSlots01,
    );
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    result
}

pub(crate) fn submit_render_font_clip_field_vf_vue_probe<const N: usize>(
    vertices: [[f32; 3]; N],
) -> Result<RenderJokerResult, &'static str> {
    if N == 0 || N % TRIANGLE_DRAW_VERTICES != 0 {
        return Err("vertex-count");
    }
    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("in-flight");
    }

    let (variant, submit_name, geometry_label, source_label) = match N {
        TRIANGLE_DRAW_VERTICES => (
            "font-clip-field-vf-vue-isolate-scratch",
            "font-tessel-clip-field-vf-vue-isolate-scratch",
            "font-tessel-clip-field-vf-vue-isolate",
            "lyon-font-mirrored-clip-field-isolate-first-triangle/vf-vue",
        ),
        n if n == TRIANGLE_DRAW_VERTICES * 2 => (
            "font-clip-field-vf-vue-isolate-two-scratch",
            "font-tessel-clip-field-vf-vue-isolate-two-scratch",
            "font-tessel-clip-field-vf-vue-isolate-two",
            "lyon-font-mirrored-clip-field-isolate-first-two-triangles/vf-vue",
        ),
        crate::graphics::font::FONT_CLIP_FIELD_VERTICES => (
            "font-clip-field-vf-vue-isolate-all-scratch",
            "font-tessel-clip-field-vf-vue-isolate-all-scratch",
            "font-tessel-clip-field-vf-vue-isolate-all",
            "lyon-font-mirrored-clip-field-isolate-all-triangles/vf-vue",
        ),
        _ => (
            "font-clip-field-vf-vue-isolate-n-scratch",
            "font-tessel-clip-field-vf-vue-isolate-n-scratch",
            "font-tessel-clip-field-vf-vue-isolate-n",
            "lyon-font-mirrored-clip-field-isolate-n-triangles/vf-vue",
        ),
    };
    let coverage_vertices = [[0.5, 0.5, 0.5], [7.5, 0.5, 0.5], [0.5, 7.5, 0.5]];
    let launch_vertices: &[[f32; 3]] = if N == TRIANGLE_DRAW_VERTICES {
        intel_render_focus_log!(
            "{} coverage-override accepted=1 source=hardcoded-screen-space target=8x8 vf_slot=position0 implicit_w=1 sf_viewport_transform=0 v0=[{:.3},{:.3},{:.3}] v1=[{:.3},{:.3},{:.3}] v2=[{:.3},{:.3},{:.3}] note=single-triangle-valley\n",
            submit_name,
            coverage_vertices[0][0],
            coverage_vertices[0][1],
            coverage_vertices[0][2],
            coverage_vertices[1][0],
            coverage_vertices[1][1],
            coverage_vertices[1][2],
            coverage_vertices[2][0],
            coverage_vertices[2][1],
            coverage_vertices[2][2],
        );
        &coverage_vertices
    } else {
        &vertices
    };
    let result = submit_render_custom_triangle_probe_locked(
        launch_vertices,
        None,
        variant,
        submit_name,
        geometry_label,
        source_label,
        TriangleBlendProbeMode::MesaZeroedState,
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::LightPostSyncNoCs,
        VF_VUE_REAL_VS_FRONT_END_CONTRACT,
        TriangleBatchMode::VfScreenSpaceDraw,
        StreamoutProofExperiment::PositionSlot0,
    );
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    result
}

#[derive(Copy, Clone)]
struct FontPsLaunchReplayCase {
    index: u8,
    variant: &'static str,
    submit_name: &'static str,
    geometry_label: &'static str,
    source_label: &'static str,
    backend: BackendProbeMode,
    batch_mode: TriangleBatchMode,
    screen_space: bool,
    note: &'static str,
}

const FONT_PS_LAUNCH_REPLAY_CASES: [FontPsLaunchReplayCase; 5] = [
    FontPsLaunchReplayCase {
        index: 1,
        variant: "font-clip-field-vf-vue-ps-replay-01-mesa-clip-scratch",
        submit_name: "font-tessel-clip-field-vf-vue-ps-replay-01-mesa-clip-scratch",
        geometry_label: "font-tessel-clip-field-vf-vue-ps-replay-01-mesa-clip",
        source_label: "hardcoded-full-coverage-clip-space/normal-ps",
        backend: BackendProbeMode::MesaLike,
        batch_mode: TriangleBatchMode::VfDraw,
        screen_space: false,
        note: "full-coverage-clip-normal-ps",
    },
    FontPsLaunchReplayCase {
        index: 2,
        variant: "font-clip-field-vf-vue-ps-replay-02-wm-normal-scratch",
        submit_name: "font-tessel-clip-field-vf-vue-ps-replay-02-wm-normal-scratch",
        geometry_label: "font-tessel-clip-field-vf-vue-ps-replay-02-wm-normal",
        source_label: "hardcoded-full-coverage-clip-space/wm-normal-dispatch",
        backend: BackendProbeMode::WmNormalDispatch,
        batch_mode: TriangleBatchMode::VfDraw,
        screen_space: false,
        note: "wm-force-dispatch-off",
    },
    FontPsLaunchReplayCase {
        index: 3,
        variant: "font-clip-field-vf-vue-ps-replay-03-ps-extra-before-scratch",
        submit_name: "font-tessel-clip-field-vf-vue-ps-replay-03-ps-extra-before-scratch",
        geometry_label: "font-tessel-clip-field-vf-vue-ps-replay-03-ps-extra-before",
        source_label: "hardcoded-full-coverage-clip-space/ps-extra-before-ps",
        backend: BackendProbeMode::PsExtraBeforePs,
        batch_mode: TriangleBatchMode::VfDraw,
        screen_space: false,
        note: "ps-extra-before-ps",
    },
    FontPsLaunchReplayCase {
        index: 4,
        variant: "font-clip-field-vf-vue-ps-replay-04-wm-reemit-scratch",
        submit_name: "font-tessel-clip-field-vf-vue-ps-replay-04-wm-reemit-scratch",
        geometry_label: "font-tessel-clip-field-vf-vue-ps-replay-04-wm-reemit",
        source_label: "hardcoded-full-coverage-clip-space/wm-reemit-after-ps-extra",
        backend: BackendProbeMode::PsWmReemitAfterPsExtra,
        batch_mode: TriangleBatchMode::VfDraw,
        screen_space: false,
        note: "wm-reemit-after-ps-extra",
    },
    FontPsLaunchReplayCase {
        index: 5,
        variant: "font-clip-field-vf-vue-ps-replay-05-no-hz-op-scratch",
        submit_name: "font-tessel-clip-field-vf-vue-ps-replay-05-no-hz-op-scratch",
        geometry_label: "font-tessel-clip-field-vf-vue-ps-replay-05-no-hz-op",
        source_label: "hardcoded-full-coverage-clip-space/omit-wm-hz-op",
        backend: BackendProbeMode::PsOmitWmHzOp,
        batch_mode: TriangleBatchMode::VfDraw,
        screen_space: false,
        note: "omit-wm-hz-op",
    },
];

pub(crate) fn submit_render_font_clip_field_vf_vue_ps_replay_probe(
    _vertices: [[f32; 3]; TRIANGLE_DRAW_VERTICES],
) -> Result<RenderJokerResult, &'static str> {
    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("in-flight");
    }

    let clip_vertices = [[-1.0, -1.0, 0.5], [3.0, -1.0, 0.5], [-1.0, 3.0, 0.5]];
    let screen_vertices = [[0.5, 0.5, 0.5], [7.5, 0.5, 0.5], [0.5, 7.5, 0.5]];
    let mut last_result = None;
    let mut completed_count = 0u8;

    for case in FONT_PS_LAUNCH_REPLAY_CASES {
        let launch_vertices: &[[f32; 3]] = if case.screen_space {
            &screen_vertices
        } else {
            &clip_vertices
        };
        intel_render_focus_log!(
            "{} ps-replay-case begin index={} backend={} batch={:?} vertices={} note={}\n",
            case.submit_name,
            case.index,
            case.backend.label(),
            case.batch_mode,
            if case.screen_space {
                "hardcoded-screen-space-8x8"
            } else {
                "hardcoded-full-coverage-clip"
            },
            case.note
        );
        match submit_render_custom_triangle_probe_locked(
            launch_vertices,
            None,
            case.variant,
            case.submit_name,
            case.geometry_label,
            case.source_label,
            TriangleBlendProbeMode::MesaZeroedState,
            case.backend,
            PostDrawSyncVariant::HeavyAll,
            VF_VUE_REAL_VS_FRONT_END_CONTRACT,
            case.batch_mode,
            StreamoutProofExperiment::PositionSlot0,
        ) {
            Ok(render) => {
                completed_count += render.completed as u8;
                intel_render_focus_log!(
                    "{} ps-replay-case result index={} completed={} target={} note={}\n",
                    case.submit_name,
                    case.index,
                    render.completed as u8,
                    render.target,
                    case.note
                );
                last_result = Some(render);
            }
            Err(err) => intel_render_focus_log!(
                "{} ps-replay-case result index={} status=error reason={} note={}\n",
                case.submit_name,
                case.index,
                err,
                case.note
            ),
        }
    }

    intel_render_focus_log!(
        "font-tessel-clip-field-vf-vue-ps-replay-suite completed_cases={} total_cases={} latest=shot-5 note=replayable-ps-frontier-regression\n",
        completed_count,
        FONT_PS_LAUNCH_REPLAY_CASES.len()
    );
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    last_result.ok_or("no-replay-result")
}

#[derive(Copy, Clone)]
struct FontPsAdmissionProbeCase {
    index: u8,
    variant: &'static str,
    submit_name: &'static str,
    geometry_label: &'static str,
    source_label: &'static str,
    blend_mode: TriangleBlendProbeMode,
    backend: BackendProbeMode,
    note: &'static str,
}

const FONT_PS_ADMISSION_PROBE_CASES: [FontPsAdmissionProbeCase; 6] = [
    FontPsAdmissionProbeCase {
        index: 1,
        variant: "font-clip-field-vf-vue-ps-admit-01-baseline-scratch",
        submit_name: "font-tessel-clip-field-vf-vue-ps-admit-01-baseline-scratch",
        geometry_label: "font-tessel-clip-field-vf-vue-ps-admit-01-baseline",
        source_label: "ps-admission/full-coverage-clip/mesa-zeroed",
        blend_mode: TriangleBlendProbeMode::MesaZeroedState,
        backend: BackendProbeMode::MesaLike,
        note: "baseline-mesa-zeroed-cc-valid",
    },
    FontPsAdmissionProbeCase {
        index: 2,
        variant: "font-clip-field-vf-vue-ps-admit-02-explicit-rt0-scratch",
        submit_name: "font-tessel-clip-field-vf-vue-ps-admit-02-explicit-rt0-scratch",
        geometry_label: "font-tessel-clip-field-vf-vue-ps-admit-02-explicit-rt0",
        source_label: "ps-admission/full-coverage-clip/explicit-rt0-blend",
        blend_mode: TriangleBlendProbeMode::ExplicitRt0,
        backend: BackendProbeMode::MesaLike,
        note: "explicit-rt0-blend-state",
    },
    FontPsAdmissionProbeCase {
        index: 3,
        variant: "font-clip-field-vf-vue-ps-admit-03-no-blend-ptr-scratch",
        submit_name: "font-tessel-clip-field-vf-vue-ps-admit-03-no-blend-ptr-scratch",
        geometry_label: "font-tessel-clip-field-vf-vue-ps-admit-03-no-blend-ptr",
        source_label: "ps-admission/full-coverage-clip/no-blend-pointer",
        blend_mode: TriangleBlendProbeMode::MesaZeroedNoBlendPointer,
        backend: BackendProbeMode::MesaLike,
        note: "zero-blend-state-pointer",
    },
    FontPsAdmissionProbeCase {
        index: 4,
        variant: "font-clip-field-vf-vue-ps-admit-04-no-cc-ptr-scratch",
        submit_name: "font-tessel-clip-field-vf-vue-ps-admit-04-no-cc-ptr-scratch",
        geometry_label: "font-tessel-clip-field-vf-vue-ps-admit-04-no-cc-ptr",
        source_label: "ps-admission/full-coverage-clip/no-cc-pointer",
        blend_mode: TriangleBlendProbeMode::MesaZeroedState,
        backend: BackendProbeMode::PsNoCcPointer,
        note: "zero-cc-state-pointer",
    },
    FontPsAdmissionProbeCase {
        index: 5,
        variant: "font-clip-field-vf-vue-ps-admit-05-bt1-explicit-scratch",
        submit_name: "font-tessel-clip-field-vf-vue-ps-admit-05-bt1-explicit-scratch",
        geometry_label: "font-tessel-clip-field-vf-vue-ps-admit-05-bt1-explicit",
        source_label: "ps-admission/full-coverage-clip/bt1-explicit-rt0",
        blend_mode: TriangleBlendProbeMode::ExplicitRt0,
        backend: BackendProbeMode::PsBindingTableCountOne,
        note: "bt-count-one-plus-explicit-rt0",
    },
    FontPsAdmissionProbeCase {
        index: 6,
        variant: "font-clip-field-vf-vue-ps-admit-06-prm-vue-xywz-scratch",
        submit_name: "font-tessel-clip-field-vf-vue-ps-admit-06-prm-vue-xywz-scratch",
        geometry_label: "font-tessel-clip-field-vf-vue-ps-admit-06-prm-vue-xywz",
        source_label: "ps-admission/full-coverage-clip/prm-vue-xywz+raster-gate+sbe-before-sf+no-swizzle+no-prim-repl+vp-extents",
        blend_mode: TriangleBlendProbeMode::MesaZeroedState,
        backend: BackendProbeMode::PsPrmNoPrimitiveReplication,
        note: "vf-written-vue-prm-header-xywz+early-raster-gate+sbe-before-sf+no-attr-swizzle+no-primitive-replication+sf-vp-extents",
    },
];

const FONT_PS_ADMISSION_ACTIVE_CASE: u8 = 6;

pub(crate) fn submit_render_font_clip_field_vf_vue_ps_admission_probe(
    _vertices: [[f32; 3]; TRIANGLE_DRAW_VERTICES],
) -> Result<RenderJokerResult, &'static str> {
    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("in-flight");
    }

    let clip_vertices = [[-1.0, -1.0, 0.5], [3.0, -1.0, 0.5], [-1.0, 3.0, 0.5]];
    let mut last_result = None;
    let mut completed_count = 0u8;
    let mut positive_index = 0u8;

    for case in FONT_PS_ADMISSION_PROBE_CASES {
        if case.index != FONT_PS_ADMISSION_ACTIVE_CASE {
            continue;
        }
        intel_render_focus_log!(
            "{} ps-admission-probe begin index={} backend={} blend={} vertices=hardcoded-full-coverage-clip suspect=prm-vue-position-xywz note={}\n",
            case.submit_name,
            case.index,
            case.backend.label(),
            case.blend_mode.label(),
            case.note
        );
        match submit_render_custom_triangle_probe_locked(
            &clip_vertices,
            None,
            case.variant,
            case.submit_name,
            case.geometry_label,
            case.source_label,
            case.blend_mode,
            case.backend,
            PostDrawSyncVariant::LightPostSyncNoCs,
            VF_VUE_REAL_VS_FRONT_END_CONTRACT,
            TriangleBatchMode::VfDraw,
            StreamoutProofExperiment::PrmVueHeaderPositionXywzSlots01,
        ) {
            Ok(render) => {
                completed_count += render.completed as u8;
                let positive = render.ps_observed;
                intel_render_focus_log!(
                    "{} ps-admission-probe result index={} completed={} ps_observed={} target={} note={}\n",
                    case.submit_name,
                    case.index,
                    render.completed as u8,
                    positive as u8,
                    render.target,
                    case.note
                );
                if positive {
                    positive_index = case.index;
                }
                last_result = Some(render);
            }
            Err(err) => intel_render_focus_log!(
                "{} ps-admission-probe result index={} status=error reason={} note={}\n",
                case.submit_name,
                case.index,
                err,
                case.note
            ),
        }
    }

    intel_render_focus_log!(
        "font-tessel-clip-field-vf-vue-ps-admission active_case={} completed_cases={} total_cases={} positive_index={} note=vf-written-vue-prm-header-xywz+early-raster-gate+sbe-before-sf+no-attr-swizzle+no-primitive-replication+sf-vp-extents\n",
        FONT_PS_ADMISSION_ACTIVE_CASE,
        completed_count,
        FONT_PS_ADMISSION_PROBE_CASES.len(),
        positive_index
    );
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    last_result.ok_or("no-admission-result")
}

pub(crate) fn submit_render_font_clip_counter_sweep_probe()
-> Result<RenderJokerResult, &'static str> {
    let vertices = [[0.25, 0.25, 0.0], [7.75, 0.25, 0.0], [0.25, 7.75, 0.0]];

    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("in-flight");
    }

    let result = submit_render_custom_triangle_probe_locked(
        &vertices,
        None,
        "font-clip-counter-sweep-known-vs-big-inbounds-scratch",
        "font-tessel-clip-counter-sweep-known-vs-big-inbounds-scratch",
        "font-tessel-clip-counter-sweep-known-vs-big-inbounds",
        "font-lyon-big-inbounds-screen-space/known-vs-clip-counter-sweep",
        TriangleBlendProbeMode::MesaZeroedState,
        BackendProbeMode::PsBindingTableCountZero,
        PostDrawSyncVariant::LightPostSyncNoCs,
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
        TriangleBatchMode::DrawScreenSpace,
        StreamoutProofExperiment::PositionSlot1,
    );
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    result
}

pub(crate) fn submit_render_font_clip_counter_vf_vue_probe()
-> Result<RenderJokerResult, &'static str> {
    let vertices = [[0.25, 0.25, 0.0], [7.75, 0.25, 0.0], [0.25, 7.75, 0.0]];

    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("in-flight");
    }

    let result = submit_render_custom_triangle_probe_locked(
        &vertices,
        None,
        "font-clip-counter-vf-vue-big-inbounds-scratch",
        "font-tessel-clip-counter-vf-vue-big-inbounds-scratch",
        "font-tessel-clip-counter-vf-vue-big-inbounds",
        "font-lyon-big-inbounds-screen-space/vf-synthesized-vue-clip-counter",
        TriangleBlendProbeMode::MesaZeroedState,
        BackendProbeMode::PsBindingTableCountZero,
        PostDrawSyncVariant::LightPostSyncNoCs,
        VF_VUE_REAL_VS_FRONT_END_CONTRACT,
        TriangleBatchMode::VfScreenSpaceDraw,
        StreamoutProofExperiment::PositionSlot0,
    );
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    result
}

fn scratch_vertices_to_ndc<const N: usize>(vertices: [[f32; 3]; N]) -> [[f32; 3]; N] {
    let mut ndc = vertices;
    for vertex in &mut ndc {
        vertex[0] = (vertex[0] / 4.0) - 1.0;
        vertex[1] = 1.0 - (vertex[1] / 4.0);
        vertex[2] = vertex[2].clamp(-1.0, 1.0);
    }
    for triangle in ndc.chunks_exact_mut(3) {
        let area2 = (triangle[1][0] - triangle[0][0]) * (triangle[2][1] - triangle[0][1])
            - (triangle[1][1] - triangle[0][1]) * (triangle[2][0] - triangle[0][0]);
        if area2 < 0.0 {
            triangle.swap(1, 2);
        }
    }
    ndc
}

pub(crate) fn submit_render_artificial_fragment_sentinel()
-> Result<RenderArtificialFragmentResult, &'static str> {
    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("in-flight");
    }

    let result = submit_render_artificial_fragment_sentinel_locked();
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    result
}

fn submit_render_custom_triangle_probe_locked(
    vertices: &[[f32; 3]],
    indices: Option<&[u32]>,
    variant: &'static str,
    submit_name: &'static str,
    geometry_label: &'static str,
    source_label: &'static str,
    blend_mode: TriangleBlendProbeMode,
    backend_probe_mode: BackendProbeMode,
    post_draw_sync_variant: PostDrawSyncVariant,
    front_end_contract: TriangleFrontEndContract,
    batch_mode: TriangleBatchMode,
    streamout_experiment: StreamoutProofExperiment,
) -> Result<RenderJokerResult, &'static str> {
    const LEGACY_PROBE_SIZE: usize = 8;

    let target_size = if submit_name == "font-tessel-3d-once" {
        FONT_STAMP_BASE_SIZE
    } else {
        LEGACY_PROBE_SIZE
    };
    submit_render_custom_triangle_probe_locked_at_extent(
        vertices,
        indices,
        None,
        None,
        variant,
        submit_name,
        geometry_label,
        source_label,
        blend_mode,
        backend_probe_mode,
        post_draw_sync_variant,
        front_end_contract,
        batch_mode,
        streamout_experiment,
        target_size,
        target_size,
        None,
    )
}

fn submit_render_custom_triangle_probe_locked_at_extent(
    vertices: &[[f32; 3]],
    indices: Option<&[u32]>,
    resident_mesh: Option<&ResidentFontMesh>,
    draw_rgba: Option<[u8; 4]>,
    variant: &'static str,
    submit_name: &'static str,
    geometry_label: &'static str,
    source_label: &'static str,
    blend_mode: TriangleBlendProbeMode,
    backend_probe_mode: BackendProbeMode,
    post_draw_sync_variant: PostDrawSyncVariant,
    front_end_contract: TriangleFrontEndContract,
    batch_mode: TriangleBatchMode,
    streamout_experiment: StreamoutProofExperiment,
    target_width: usize,
    target_height: usize,
    readback: Option<&mut Option<FontRenderTargetReadback>>,
) -> Result<RenderJokerResult, &'static str> {
    let probe_seq = PRIMARY_PROBE_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if PRIMARY_DISABLE_RENDER_BRINGUP && !RENDER_JOKER_SUBMIT_WHEN_PRIMARY_RENDER_DISABLED {
        crate::log!(
            "custom-triangle skipped reason=disabled seq={} submit={}\n",
            probe_seq,
            submit_name
        );
        return Err("disabled");
    } else if PRIMARY_DISABLE_RENDER_BRINGUP {
        intel_render_focus_log!(
            "custom-triangle override primary-disabled seq={} submit={} reason=manual-scratch-probe\n",
            probe_seq,
            submit_name
        );
    }

    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("custom-triangle skipped reason=no-device submit={}\n", submit_name);
        return Err("no-device");
    };
    let warm = warm_once(dev);
    let target_row_bytes = target_width
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or("font-target-row-overflow")?;
    let target_pitch = crate::intel::align_up(target_row_bytes, LINEAR_RENDER_TARGET_PITCH_ALIGN)
        .ok_or("font-target-pitch-overflow")?;
    let target_bytes = target_pitch
        .checked_mul(target_height)
        .ok_or("font-target-bytes-overflow")?;
    if warm.ring_len == 0
        || warm.context_len == 0
        || warm.batch_len == 0
        || warm.draw_state_len == 0
        || warm.vertex_len == 0
        || warm.result_len == 0
        || warm.streamout_len < target_bytes
        || warm.streamout_virt.is_null()
    {
        crate::log!("custom-triangle skipped reason=warm-buffers submit={}\n", submit_name);
        return Err("warm-buffers");
    }
    if !forcewake_render_acquire(warm) {
        crate::log!("custom-triangle skipped reason=forcewake submit={}\n", submit_name);
        return Err("forcewake");
    }
    if !ensure_smoke_buffers_mapped(dev, warm) {
        crate::log!("custom-triangle skipped reason=ggtt-map submit={}\n", submit_name);
        return Err("ggtt-map");
    }

    unsafe {
        core::ptr::write_bytes(warm.streamout_virt, 0, target_bytes);
        let scratch_pixels = core::slice::from_raw_parts_mut(
            warm.streamout_virt as *mut u32,
            target_bytes / core::mem::size_of::<u32>(),
        );
        scratch_pixels.fill(0xDEAD_BEEF);
    }
    crate::intel::dma_flush(warm.streamout_virt, target_bytes);

    intel_render_focus_log!(
        "custom-triangle begin seq={} submit={} target=scratch size={}x{} extent_mode={} backend={} blend={} sync={} front_end={} source={}\n",
        probe_seq,
        submit_name,
        target_width,
        target_height,
        if matches!(submit_name, "font-tessel-3d-once" | "font-resident-3d") {
            "font-native"
        } else {
            "generic"
        },
        backend_probe_mode.label(),
        blend_mode.label(),
        post_draw_sync_variant.label(),
        front_end_contract.label,
        source_label,
    );
    let completed = submit_triangle_real_vs_draw_probe_vertices_to_surface_ext(
        dev,
        warm,
        GPU_VA_STREAMOUT_BASE,
        target_pitch,
        target_width,
        target_height,
        blend_mode,
        None,
        vertices,
        indices,
        None,
        resident_mesh,
        draw_rgba,
        geometry_label,
        submit_name,
        front_end_contract,
        backend_probe_mode,
        post_draw_sync_variant,
        batch_mode,
        streamout_experiment,
        [0.0, 0.0],
        readback,
    );
    intel_render_focus_log!(
        "custom-triangle end seq={} submit={} target=scratch completed={}\n",
        probe_seq,
        submit_name,
        completed as u8,
    );
    let frontier = latest_render_frontier_summary();

    Ok(RenderJokerResult {
        variant,
        submit_name,
        target: "scratch",
        completed,
        vs_counter: frontier.vs_counter,
        ps_state_marker: frontier.ps_state_marker,
        raster_packet: frontier.raster_packet,
        clip_counter: frontier.clip_counter,
        ps_observed: frontier.ps_observed,
    })
}

fn submit_render_artificial_fragment_sentinel_locked()
-> Result<RenderArtificialFragmentResult, &'static str> {
    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("artificial-fragment-sentinel skipped reason=no-device\n");
        return Err("no-device");
    };
    let warm = warm_once(dev);
    if warm.streamout_len < 8 * 8 * core::mem::size_of::<u32>()
        || warm.streamout_virt.is_null()
        || warm.streamout_phys == 0
    {
        crate::log!("artificial-fragment-sentinel skipped reason=warm-scratch\n");
        return Err("warm-scratch");
    }
    if !forcewake_render_acquire(warm) {
        crate::log!("artificial-fragment-sentinel skipped reason=forcewake\n");
        return Err("forcewake");
    }
    if !ensure_smoke_buffers_mapped(dev, warm) {
        crate::log!("artificial-fragment-sentinel skipped reason=ggtt-map\n");
        return Err("ggtt-map");
    }

    const SENTINEL_COLOR: u32 = 0xA17F_F00D;
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
        core::ptr::write_volatile(warm.streamout_virt as *mut u32, 0xDEAD_BEEF);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.batch_virt, warm.batch_len);
    crate::intel::dma_flush(warm.ring_virt, warm.ring_len);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
    let before = unsafe { core::ptr::read_volatile(warm.streamout_virt as *const u32) };

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = encode_3d_no_draw_probe_batch(
        batch,
        warm,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_MI_PROBE_DONE,
        Some((GPU_VA_STREAMOUT_BASE, SENTINEL_COLOR)),
    )
    .map_err(|_| "batch")?;
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_MI_PROBE_DONE,
        RESULT_SLOT_PRE3D_DWORD,
        "artificial-fragment-sentinel",
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "artificial-fragment-sentinel");
    }

    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
    let after = unsafe { core::ptr::read_volatile(warm.streamout_virt as *const u32) };
    let remapped_render = ensure_smoke_buffers_mapped(dev, warm);
    WARM_BUFFERS_MAPPED.store(remapped_render, Ordering::Release);
    let ok = completed && after == SENTINEL_COLOR && remapped_render;
    intel_render_focus_log!(
        "artificial-fragment-sentinel mode=mi-store ok={} completed={} stores=1 rt_gpu=0x{:X} size=8x8 pitch=0x{:X} before=0x{:08X} after=0x{:08X} remapped_render={} meaning=artificial-fragment-not-wm does_not_prove=raster_or_ps\n",
        ok as u8,
        completed as u8,
        GPU_VA_STREAMOUT_BASE,
        8 * core::mem::size_of::<u32>() as u32,
        before,
        after,
        remapped_render as u8,
    );

    Ok(RenderArtificialFragmentResult {
        mode: "mi-store",
        ok,
        descs: 1,
        before,
        after,
        rt_gpu: GPU_VA_STREAMOUT_BASE,
        remapped_render,
    })
}

fn submit_render_joker_probe_locked(
    spec: RenderJokerSpec,
) -> Result<RenderJokerResult, &'static str> {
    let probe_seq = PRIMARY_PROBE_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if PRIMARY_DISABLE_RENDER_BRINGUP && !RENDER_JOKER_SUBMIT_WHEN_PRIMARY_RENDER_DISABLED {
        crate::log!("joker skipped reason=disabled variant={} seq={}\n", spec.variant, probe_seq);
        return Err("disabled");
    } else if PRIMARY_DISABLE_RENDER_BRINGUP {
        intel_render_focus_log!(
            "joker override primary-disabled variant={} seq={} reason=manual-scratch-probe\n",
            spec.variant,
            probe_seq
        );
    }

    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("joker skipped reason=no-device variant={}\n", spec.variant);
        return Err("no-device");
    };
    let Some(surface_gpu) = crate::intel::display::primary_surface_gpu_addr() else {
        crate::log!("joker skipped reason=no-surface variant={}\n", spec.variant);
        return Err("no-surface");
    };
    let Some((width, height)) = crate::intel::display::active_scanout_dimensions() else {
        crate::log!("joker skipped reason=no-dimensions variant={}\n", spec.variant);
        return Err("no-dimensions");
    };
    let Some(pitch_bytes) = width
        .checked_mul(4)
        .and_then(|v| crate::intel::align_up(v as usize, 64))
    else {
        crate::log!("joker skipped reason=bad-pitch width={}\n", width);
        return Err("bad-pitch");
    };

    let warm = warm_once(dev);
    if warm.ring_len == 0
        || warm.context_len == 0
        || warm.batch_len == 0
        || warm.draw_state_len == 0
        || warm.vertex_len == 0
        || warm.result_len == 0
        || warm.streamout_len == 0
    {
        crate::log!("joker skipped reason=warm-buffers variant={}\n", spec.variant);
        return Err("warm-buffers");
    }
    if !forcewake_render_acquire(warm) {
        crate::log!("joker skipped reason=forcewake variant={}\n", spec.variant);
        return Err("forcewake");
    }
    if !ensure_smoke_buffers_mapped(dev, warm) {
        crate::log!("joker skipped reason=ggtt-map variant={}\n", spec.variant);
        return Err("ggtt-map");
    }

    let (target_gpu, target_pitch, target_w, target_h, target_label) = match spec.target {
        RenderJokerTarget::Primary => {
            (surface_gpu, pitch_bytes, width as usize, height as usize, "primary")
        }
        RenderJokerTarget::ScratchRt => {
            unsafe {
                core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
                core::ptr::write_volatile(warm.streamout_virt as *mut u32, 0xDEAD_BEEF);
            }
            crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
            (GPU_VA_STREAMOUT_BASE, 8 * core::mem::size_of::<u32>(), 8, 8, "scratch")
        }
    };

    let streamout_kind = render_joker_streamout_kind(spec.variant);
    let real_vs_contract = render_joker_real_vs_front_end_contract(spec.variant);
    let front_end_label = real_vs_contract
        .map(|contract| contract.label)
        .or(streamout_kind)
        .unwrap_or("vf-synthesized");
    intel_render_focus_log!(
        "joker begin seq={} variant={} submit={} target={} backend={} geometry={} blend={} sync={} front_end={}\n",
        probe_seq,
        spec.variant,
        spec.submit_name,
        target_label,
        spec.backend.label(),
        spec.geometry.label(),
        spec.blend.label(),
        spec.sync.label(),
        front_end_label,
    );
    let completed = if let Some(kind) = streamout_kind {
        let experiment = render_joker_vf_experiment(spec.variant);
        if kind == "vs" {
            submit_triangle_vs_streamout_proof(
                dev,
                warm,
                target_gpu,
                target_pitch,
                target_w,
                target_h,
                experiment,
            )
        } else {
            submit_triangle_vf_streamout_proof(
                dev,
                warm,
                target_gpu,
                target_pitch,
                target_w,
                target_h,
                experiment,
            )
        }
    } else if let Some(front_end_contract) = real_vs_contract {
        submit_triangle_real_vs_draw_probe_to_surface_ext(
            dev,
            warm,
            target_gpu,
            target_pitch,
            target_w,
            target_h,
            spec.blend,
            spec.geometry,
            spec.submit_name,
            front_end_contract,
            spec.backend,
            spec.sync,
            None,
        )
    } else {
        submit_triangle_vf_draw_to_surface_ext(
            spec.submit_name,
            dev,
            warm,
            target_gpu,
            target_pitch,
            target_w,
            target_h,
            spec.blend,
            spec.geometry,
            spec.backend,
            spec.sync,
            render_joker_vf_experiment(spec.variant),
        )
    };
    intel_render_focus_log!(
        "joker end seq={} variant={} submit={} target={} completed={}\n",
        probe_seq,
        spec.variant,
        spec.submit_name,
        target_label,
        completed as u8,
    );
    let frontier = latest_render_frontier_summary();

    Ok(RenderJokerResult {
        variant: spec.variant,
        submit_name: spec.submit_name,
        target: target_label,
        completed,
        vs_counter: frontier.vs_counter,
        ps_state_marker: frontier.ps_state_marker,
        raster_packet: frontier.raster_packet,
        clip_counter: frontier.clip_counter,
        ps_observed: frontier.ps_observed,
    })
}

fn submit_primary_probe_now(reason: &'static str) -> bool {
    let probe_seq = PRIMARY_PROBE_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        crate::log!("primary-probe skipped reason=in-flight trigger={}\n", reason);
        return false;
    }

    if PRIMARY_DISABLE_RENDER_BRINGUP {
        crate::log!("primary-probe skipped reason=disabled trigger={} seq={}\n", reason, probe_seq);
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }

    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("primary-triangle skipped reason=no-device\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };
    let Some(surface_gpu) = crate::intel::display::primary_surface_gpu_addr() else {
        crate::log!("primary-triangle skipped reason=no-surface\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };
    let Some((width, height)) = crate::intel::display::active_scanout_dimensions() else {
        crate::log!("primary-triangle skipped reason=no-dimensions\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };
    let Some(pitch_bytes) = width
        .checked_mul(4)
        .and_then(|v| crate::intel::align_up(v as usize, 64))
    else {
        crate::log!("primary-triangle skipped reason=bad-pitch width={}\n", width);
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };

    let warm = warm_once(dev);
    if warm.ring_len == 0
        || warm.context_len == 0
        || warm.batch_len == 0
        || warm.draw_state_len == 0
        || warm.vertex_len == 0
        || warm.result_len == 0
        || warm.streamout_len == 0
    {
        crate::log!("primary-triangle skipped reason=warm-buffers\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }
    if !forcewake_render_acquire(warm) {
        crate::log!("primary-triangle skipped reason=forcewake\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }
    if !ensure_smoke_buffers_mapped(dev, warm) {
        crate::log!("primary-triangle skipped reason=ggtt-map\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }
    if PRIMARY_USE_MI_SCANOUT_PROOF
        && reason == "boot-once"
        && !PRIMARY_MI_SCANOUT_PROOF_SUBMITTED.swap(true, Ordering::AcqRel)
    {
        let accepted = submit_mi_scanout_store_proof(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !accepted {
            intel_render_verbose_log!("primary-mi-scanout-store proof failed trigger={}\n", reason);
        }
    }
    let completed = if PRIMARY_USE_DRAW_PATH_BOOT_ONCE && reason == "boot-once" {
        let completed = submit_primary_triangle_with_retries(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !completed {
            intel_render_verbose_log!(
                "primary-draw-path submit failed trigger={} mode=clean-boot-once\n",
                reason
            );
        }
        completed
    } else if PRIMARY_USE_MI_STRIPE_PROBE {
        let completed = submit_vertical_stripes_to_surface(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !completed {
            intel_render_verbose_log!("primary-mi-stripes submit failed trigger={}\n", reason);
        }
        completed
    } else if PRIMARY_USE_3D_NO_DRAW_PROBE {
        let completed = submit_3d_no_draw_probe(dev, warm);
        if !completed {
            intel_render_verbose_log!("primary-3d-no-draw submit failed trigger={}\n", reason);
        }
        completed
    } else if submit_primary_triangle_with_retries(
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width as usize,
        height as usize,
    ) {
        true
    } else {
        let completed = submit_triangle_to_surface(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !completed {
            intel_render_verbose_log!("primary-triangle submit failed trigger={}\n", reason);
        }
        completed
    };
    if should_log_primary_probe(reason, probe_seq) {
        intel_render_verbose_log!(
            "primary-probe seq={} trigger={} completed={} mode={}\n",
            probe_seq,
            reason,
            completed as u8,
            if PRIMARY_USE_MI_STRIPE_PROBE {
                "mi-stripes"
            } else if PRIMARY_USE_DRAW_PATH_BOOT_ONCE && reason == "boot-once" {
                "draw-path"
            } else if PRIMARY_USE_3D_NO_DRAW_PROBE {
                "3d-no-draw"
            } else {
                "3d"
            }
        );
    }
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    completed
}

fn seed_render_scratch_rt(warm: RenderWarmState) {
    unsafe {
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
        core::ptr::write_volatile(warm.streamout_virt as *mut u32, 0xDEAD_BEEF);
    }
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
}

fn submit_primary_triangle_with_retries(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    surface_gpu: u64,
    pitch_bytes: usize,
    width: usize,
    height: usize,
) -> bool {
    if !PRIMARY_BOOT_3D_PROBES_ENABLED {
        let completed = submit_triangle_vf_draw_to_surface(
            "primary-single-submit",
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            TriangleBlendProbeMode::ExplicitRt0,
            VfPrimitiveGeometry::Canonical,
            BackendProbeMode::MesaLike,
            PostDrawSyncVariant::HeavyAll,
        );
        intel_render_focus_log!(
            "primary-single-submit completed={} action=stop-after-one-submit reason=boot-3d-probes-disabled\n",
            completed as u8,
        );
        return completed;
    }
    intel_render_focus_log!(
        "primary-boot-3d-probes enabled=1 action=run-frontier-ladder vf_streamout=1 ps_spectrum=1 vs_frontier=1 revision=nonvisual-vs-scratch-rt32-trilist-split\n",
    );

    let initial_streamout_experiment =
        select_streamout_proof_experiment(PRIMARY_PROBE_SEQ.load(Ordering::Acquire));
    let vf_streamout_precheck = submit_triangle_vf_streamout_proof(
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        initial_streamout_experiment,
    );
    intel_render_verbose_log!(
        "primary-vf-streamout-precheck experiment={} accepted={}\n",
        initial_streamout_experiment.label(),
        vf_streamout_precheck as u8,
    );
    if !vf_streamout_precheck {
        return false;
    }

    let vf_draw_precheck = submit_triangle_vf_draw_to_surface(
        "vf-draw-path",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Canonical,
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!("primary-vf-draw-precheck completed={}\n", vf_draw_precheck as u8,);
    if vf_draw_precheck {
        return true;
    }
    reset_fragment_boundary_probe();
    let ps_launch_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-launch-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "primary-ps-launch-big-primitive completed={}\n",
        ps_launch_big_primitive as u8,
    );
    if ps_launch_big_primitive {
        return true;
    }

    run_postdraw_pc_retire_spectrum(dev, warm, surface_gpu, pitch_bytes, width, height);

    seed_render_scratch_rt(warm);
    let ps_bt0_scratch_rt = submit_triangle_vf_draw_to_surface(
        "ps-bt0-scratch-rt",
        dev,
        warm,
        GPU_VA_STREAMOUT_BASE,
        8 * core::mem::size_of::<u32>(),
        8,
        8,
        TriangleBlendProbeMode::MesaZeroedState,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsBindingTableCountZero,
        PostDrawSyncVariant::LightPostSyncNoCs,
    );
    intel_render_verbose_log!("primary-ps-bt0-scratch-rt completed={}\n", ps_bt0_scratch_rt as u8,);
    intel_render_focus_log!(
        "primary-ps-bt0-scratch-rt diagnostic completed={} note=no-cs-tail-completion-is-not-a-fence\n",
        ps_bt0_scratch_rt as u8,
    );
    if ps_bt0_scratch_rt {
        recover_render_engine_after_nonretired_submit(dev, warm, "ps-bt0-scratch-rt");
    }

    seed_render_scratch_rt(warm);
    let raster_wm_oa_probe = submit_triangle_vf_draw_to_surface(
        "raster-wm-oa-probe",
        dev,
        warm,
        GPU_VA_STREAMOUT_BASE,
        8 * core::mem::size_of::<u32>(),
        8,
        8,
        TriangleBlendProbeMode::MesaZeroedState,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::RasterWmInputOa,
        PostDrawSyncVariant::LightPostSyncNoCs,
    );
    intel_render_verbose_log!(
        "primary-raster-wm-oa-probe completed={}\n",
        raster_wm_oa_probe as u8,
    );
    intel_render_focus_log!(
        "primary-raster-wm-oa-probe diagnostic completed={} note=no-cs-tail-completion-is-not-a-fence\n",
        raster_wm_oa_probe as u8,
    );
    if raster_wm_oa_probe {
        recover_render_engine_after_nonretired_submit(dev, warm, "raster-wm-oa-probe");
    }

    let fragment_candidate_ready = fragment_candidate_ready();
    let fragment_boundary_seen = fragment_boundary_observed();
    intel_render_focus_log!(
        "primary-fragment-boundary-gate candidate_ready={} fragment_observed={} action={} reason=shape_to_fragment_boundary_precedes_ps_spectrum\n",
        fragment_candidate_ready as u8,
        fragment_boundary_seen as u8,
        if fragment_boundary_seen {
            "continue-ps-spectrum"
        } else {
            "continue-ps-spectrum-diagnostic"
        },
    );

    let ps_bt1_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-bt1-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsBindingTableCountOne,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "primary-ps-bt1-big-primitive completed={}\n",
        ps_bt1_big_primitive as u8,
    );
    if ps_bt1_big_primitive {
        return true;
    }

    let ps_wm_normal_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-wm-normal-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::WmNormalDispatch,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "primary-ps-wm-normal-big-primitive completed={}\n",
        ps_wm_normal_big_primitive as u8,
    );
    if ps_wm_normal_big_primitive {
        return true;
    }

    let ps_dispatch_slot0_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-dispatch-slot0-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsDispatchSlot0,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "primary-ps-dispatch-slot0-big-primitive completed={}\n",
        ps_dispatch_slot0_big_primitive as u8,
    );
    if ps_dispatch_slot0_big_primitive {
        return true;
    }

    let ps_dispatch_slot1_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-dispatch-slot1-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsDispatchSlot1,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "primary-ps-dispatch-slot1-big-primitive completed={}\n",
        ps_dispatch_slot1_big_primitive as u8,
    );
    if ps_dispatch_slot1_big_primitive {
        return true;
    }

    let ps_dispatch_slot2_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-dispatch-slot2-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsDispatchSlot2,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "primary-ps-dispatch-slot2-big-primitive completed={}\n",
        ps_dispatch_slot2_big_primitive as u8,
    );
    if ps_dispatch_slot2_big_primitive {
        return true;
    }

    let payload_variants = [
        ("ps-payload-push-big-primitive", BackendProbeMode::PsPayloadPushConstant),
        ("ps-payload-attr-big-primitive", BackendProbeMode::PsPayloadAttributeEnable),
        ("ps-payload-simple-big-primitive", BackendProbeMode::PsPayloadSimpleHint),
        ("ps-payload-source-depth-w-big-primitive", BackendProbeMode::PsPayloadSourceDepthW),
        ("ps-payload-bary-big-primitive", BackendProbeMode::PsPayloadBaryPlanes),
    ];
    for (payload_submit_name, payload_mode) in payload_variants {
        let completed = submit_triangle_vf_draw_to_surface(
            payload_submit_name,
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            TriangleBlendProbeMode::ExplicitRt0,
            VfPrimitiveGeometry::Oversized,
            payload_mode,
            PostDrawSyncVariant::HeavyAll,
        );
        intel_render_verbose_log!(
            "primary-{} completed={}\n",
            payload_submit_name,
            completed as u8,
        );
        if completed {
            return true;
        }
    }

    let grf_variants = [
        ("ps-grf-start-r1-big-primitive", BackendProbeMode::PsGrfStartR1),
        ("ps-grf-start-r2-big-primitive", BackendProbeMode::PsGrfStartR2),
        ("ps-grf-start-r4-big-primitive", BackendProbeMode::PsGrfStartR4),
        ("ps-grf-maxthreads-31-big-primitive", BackendProbeMode::PsGrfMaxThreads31),
        ("ps-grf-maxthreads-15-big-primitive", BackendProbeMode::PsGrfMaxThreads15),
    ];
    for (grf_submit_name, grf_mode) in grf_variants {
        let completed = submit_triangle_vf_draw_to_surface(
            grf_submit_name,
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            TriangleBlendProbeMode::ExplicitRt0,
            VfPrimitiveGeometry::Oversized,
            grf_mode,
            PostDrawSyncVariant::HeavyAll,
        );
        intel_render_verbose_log!("primary-{} completed={}\n", grf_submit_name, completed as u8,);
        if completed {
            return true;
        }
    }

    let ps_dispatch_all_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-dispatch-all-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsDispatchAllKspSlots,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "primary-ps-dispatch-all-big-primitive completed={}\n",
        ps_dispatch_all_big_primitive as u8,
    );
    if ps_dispatch_all_big_primitive {
        return true;
    }

    reset_fragment_boundary_probe();
    let fragment_shape_frontier = run_fragment_shape_frontier_spectrum(dev, warm);
    intel_render_focus_log!(
        "primary-fragment-shape-spectrum completed={} observed={} note=shape_clip_sf_axis_after_ps_state_axis\n",
        fragment_shape_frontier as u8,
        fragment_boundary_observed() as u8,
    );
    if fragment_shape_frontier {
        return true;
    }

    let vs_draw_frontier_scratch = submit_triangle_vs_draw_frontier_to_scratch(dev, warm);
    intel_render_focus_log!(
        "primary-vs-draw-frontier-scratch completed={} observed={} note=nonvisual-vs-clip-join-probe\n",
        vs_draw_frontier_scratch as u8,
        fragment_boundary_observed() as u8,
    );
    if vs_draw_frontier_scratch {
        return true;
    }
    intel_render_focus_log!(
        "primary-vs-draw-frontier-precheck skipped reason=scratch-frontier-unobserved avoid_visible_scanout_flash surface=0x{:X} size={}x{} pitch=0x{:X}\n",
        surface_gpu,
        width,
        height,
        pitch_bytes,
    );

    let mut vs_streamout_experiment = initial_streamout_experiment;
    let mut vs_streamout_precheck = false;
    for attempt in 1..=3 {
        let accepted = submit_triangle_vs_streamout_proof(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            vs_streamout_experiment,
        );
        intel_render_verbose_log!(
            "primary-vs-streamout-precheck experiment={} accepted={} attempt={}/3\n",
            vs_streamout_experiment.label(),
            accepted as u8,
            attempt
        );
        if accepted {
            vs_streamout_precheck = true;
            break;
        }
        vs_streamout_experiment = vs_streamout_experiment.alternate();
    }
    if !vs_streamout_precheck {
        return false;
    }

    let mut streamout_experiment = vs_streamout_experiment;
    let mut streamout_precheck = false;
    for attempt in 1..=3 {
        let accepted = submit_triangle_streamout_proof(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            streamout_experiment,
        );
        intel_render_verbose_log!(
            "primary-streamout-precheck experiment={} accepted={} attempt={}/3\n",
            streamout_experiment.label(),
            accepted as u8,
            attempt
        );
        if accepted {
            streamout_precheck = true;
            break;
        }
        streamout_experiment = streamout_experiment.alternate();
    }
    if !streamout_precheck {
        return false;
    }

    let mut completed_any = false;
    for attempt in 1..=PRIMARY_TRIANGLE_SUBMIT_ATTEMPTS {
        let blend_mode = TriangleBlendProbeMode::for_attempt(attempt);
        let completed = submit_triangle_draw_to_surface(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            blend_mode,
        );
        intel_render_verbose_log!(
            "primary-triangle attempt={}/{} target=0x{:X} blend_probe={} completed={}\n",
            attempt,
            PRIMARY_TRIANGLE_SUBMIT_ATTEMPTS,
            surface_gpu,
            blend_mode.label(),
            completed as u8
        );
        completed_any |= completed;
        if !completed {
            intel_render_verbose_log!(
                "primary-streamout-proof skipped trigger=draw-fail attempt={} reason=post-hang-state-not-clean\n",
                attempt,
            );
            break;
        }
    }
    completed_any
}

fn run_fragment_shape_frontier_spectrum(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    let scratch_pitch = 8 * core::mem::size_of::<u32>();
    let aligned_scratch_pitch = 32 * core::mem::size_of::<u32>();
    let probes = [
        (
            "point-vf-giant-oa-w64",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaPointWidth64,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-w64-halign128",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaPointWidth64SurfaceHalign128,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-w1023",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaPointWidth1023,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-msrast",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaMsRaster,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-msrast-force",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaMsRasterForced,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-early-msrast-force",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaEarlyMsRasterForced,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-early-w1023",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaEarlyPointWidth1023,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-w64-early",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaPointWidth64Early,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-w64-early-scissor",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaPointWidth64EarlyScissor,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-w1023-scissor",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaPointWidth1023Scissor,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-hammer",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaHammer,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-screen-oa-w64",
            VfPrimitiveGeometry::ScreenSpacePoint8x8,
            BackendProbeMode::RasterWmInputOaPointWidth64Screen,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "point-vf-screen-oa-hammer",
            VfPrimitiveGeometry::ScreenSpacePoint8x8,
            BackendProbeMode::RasterWmInputOaScreenHammer,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "line-vf-screen-oa-hammer",
            VfPrimitiveGeometry::ScreenSpaceLine8x8,
            BackendProbeMode::RasterWmInputOaScreenHammer,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "point-vf-giant-oa-w64-wm-normal",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaPointWidth64WmNormalDispatch,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOa,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-halign128",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaSurfaceHalign128,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-early",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaEarlySample,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-sample-early",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaSampleMaskEarlyOnly,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-pc-clip-sf",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaPipeControlClipSf,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-hz-pre-wm",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaWmHzOpBeforeWm,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-hz-post-extra",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaWmHzOpAfterPsExtra,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-payload-attr",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaPayloadAttributeEnable,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-payload-depthw",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaPayloadSourceDepthW,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-payload-bary",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaPayloadBaryPlanes,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-sample-all",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaSampleAll,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-wm-handoff",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaWmHandoff,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-sample-all-wm-handoff",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaSampleAllWmHandoff,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-frontccw",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaFrontCcw,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-hz0",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaNoHzOp,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-clip-disable",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaClipDisabled,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-bt1",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaBtCountOne,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-order-b-oa",
            VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB,
            BackendProbeMode::RasterWmInputOa,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-order-b-scissor-oa",
            VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB,
            BackendProbeMode::RasterWmInputOaScissorOnly,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-order-c-early-oa",
            VfPrimitiveGeometry::NdcRectUrLrUl,
            BackendProbeMode::RasterWmInputOaEarlySample,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-mesa-simple-oa-early",
            VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB,
            BackendProbeMode::RasterWmInputOaMesaSimpleRectEarly,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-tri-mesa-simple-oa-early",
            VfPrimitiveGeometry::ScreenSpaceTri8x8OrderB,
            BackendProbeMode::RasterWmInputOaMesaSimpleRectEarly,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "screen-rect-oa-early",
            VfPrimitiveGeometry::ScreenSpaceRect8x8,
            BackendProbeMode::RasterWmInputOaEarlySample,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "screen-rect-oa-hammer",
            VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB,
            BackendProbeMode::RasterWmInputOaScreenHammer,
            StreamoutProofExperiment::PositionSlot1,
        ),
    ];
    let aligned_target_probes = [
        (
            "vf-rect-ndc-oa-rt32",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOa,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-halign128-rt32",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaSurfaceHalign128,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-early-rt32",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaEarlySample,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-payload-attr-rt32",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaPayloadAttributeEnable,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-payload-bary-rt32",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaPayloadBaryPlanes,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-sample-all-rt32",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaSampleAll,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-wm-handoff-rt32",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaWmHandoff,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-sample-all-wm-handoff-rt32",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaSampleAllWmHandoff,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-hammer-rt32",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaHammer,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-line-ndc-oa-hammer-rt32",
            VfPrimitiveGeometry::NdcLine,
            BackendProbeMode::RasterWmInputOaHammer,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-mesa-simple-oa-early-rt32",
            VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB,
            BackendProbeMode::RasterWmInputOaMesaSimpleRectEarly,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-tri-mesa-simple-oa-early-rt32",
            VfPrimitiveGeometry::ScreenSpaceTri8x8OrderB,
            BackendProbeMode::RasterWmInputOaMesaSimpleRectEarly,
            StreamoutProofExperiment::PositionSlot1,
        ),
    ];

    intel_render_focus_log!(
        "primary-fragment-shape-spectrum begin probes={} target=scratch-8x8+rt32 truth=fragment_boundary_observed\n",
        probes.len() + aligned_target_probes.len(),
    );
    for (submit_name, geometry, backend, vf_experiment) in probes {
        seed_render_scratch_rt(warm);
        let completed = submit_triangle_vf_draw_to_surface_ext(
            submit_name,
            dev,
            warm,
            GPU_VA_STREAMOUT_BASE,
            scratch_pitch,
            8,
            8,
            TriangleBlendProbeMode::MesaZeroedState,
            geometry,
            backend,
            PostDrawSyncVariant::LightPostSyncNoCs,
            vf_experiment,
        );
        let observed = fragment_boundary_observed();
        intel_render_focus_log!(
            "primary-fragment-shape-spectrum submit={} geometry={} backend={} vf_contract={} completed={} candidate_ready={} observed={}\n",
            submit_name,
            geometry.label(),
            backend.label(),
            vf_experiment.vf_slot_contract(),
            completed as u8,
            fragment_candidate_ready() as u8,
            observed as u8,
        );
        if completed {
            recover_render_engine_after_nonretired_submit(dev, warm, submit_name);
        }
        if observed {
            return true;
        }
    }
    for (submit_name, geometry, backend, vf_experiment) in aligned_target_probes {
        seed_render_scratch_rt(warm);
        let completed = submit_triangle_vf_draw_to_surface_ext(
            submit_name,
            dev,
            warm,
            GPU_VA_STREAMOUT_BASE,
            aligned_scratch_pitch,
            32,
            32,
            TriangleBlendProbeMode::MesaZeroedState,
            geometry,
            backend,
            PostDrawSyncVariant::LightPostSyncNoCs,
            vf_experiment,
        );
        let observed = fragment_boundary_observed();
        intel_render_focus_log!(
            "primary-fragment-shape-spectrum submit={} geometry={} backend={} vf_contract={} completed={} candidate_ready={} observed={}\n",
            submit_name,
            geometry.label(),
            backend.label(),
            vf_experiment.vf_slot_contract(),
            completed as u8,
            fragment_candidate_ready() as u8,
            observed as u8,
        );
        if completed {
            recover_render_engine_after_nonretired_submit(dev, warm, submit_name);
        }
        if observed {
            return true;
        }
    }
    false
}

fn run_postdraw_pc_retire_spectrum(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    surface_gpu: u64,
    pitch_bytes: usize,
    width: usize,
    height: usize,
) {
    for variant in POST_DRAW_PC_RETIRE_SPECTRUM {
        let submit_name = variant.submit_name();
        let completed = submit_triangle_vf_draw_to_surface(
            submit_name,
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            TriangleBlendProbeMode::ExplicitRt0,
            VfPrimitiveGeometry::Canonical,
            BackendProbeMode::MesaLike,
            variant,
        );
        intel_render_focus_log!(
            "postdraw-pc-retire-spectrum submit={} variant={} completed={} note=diagnostic_only\n",
            submit_name,
            variant.label(),
            completed as u8,
        );
        if completed {
            intel_render_focus_log!(
                "postdraw-pc-retire-spectrum cleanup submit={} variant={} reason=completed-diagnostic-not-a-fence\n",
                submit_name,
                variant.label(),
            );
            recover_render_engine_after_nonretired_submit(dev, warm, submit_name);
        }
    }
}

fn submit_triangle_vf_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(draw) = prepare_vf_streamout_proof_resources(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        experiment,
        VfPrimitiveGeometry::Canonical,
    ) else {
        crate::log!(
            "vf-streamout-proof skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };
    let slice_hash_table_offset = match write_vf_streamout_probe_state(warm) {
        Ok(offset) => offset,
        Err(reason) => {
            crate::log!("vf-streamout-proof skipped reason=probe-state detail={}\n", reason);
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_vf_streamout_proof_batch(
        batch,
        warm,
        draw,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        experiment,
        slice_hash_table_offset,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!("vf-streamout-proof batch build failed detail={}\n", reason);
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    intel_render_verbose_log!(
        "vf-streamout-proof batch-ready experiment={} bytes=0x{:X} so_gpu=0x{:X} so_pitch={} vertices={}\n",
        experiment.label(),
        batch_tail_bytes,
        GPU_VA_STREAMOUT_BASE,
        experiment.vertex_bytes(),
        draw.vertex_count
    );

    let stats_before = capture_triangle_stage_stats(dev);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        "vf-streamout-proof",
    );
    let stats_after = capture_triangle_stage_stats(dev);
    let accepted = completed
        || maybe_soft_accept_streamout_submit(
            "vf-streamout-proof",
            warm,
            stats_before,
            stats_after,
            false,
            experiment.vertex_bytes() * draw.vertex_count as usize,
        );
    log_streamout_proof_result(
        "vf-streamout-proof",
        warm,
        completed,
        draw.vertex_count as usize,
        experiment,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "vf-streamout-proof");
    }
    accepted
}

fn submit_triangle_vf_draw_to_surface(
    submit_name: &'static str,
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
    geometry: VfPrimitiveGeometry,
    backend_probe_mode: BackendProbeMode,
    post_draw_sync_variant: PostDrawSyncVariant,
) -> bool {
    submit_triangle_vf_draw_to_surface_ext(
        submit_name,
        dev,
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        blend_mode,
        geometry,
        backend_probe_mode,
        post_draw_sync_variant,
        StreamoutProofExperiment::PositionSlot1,
    )
}

fn submit_triangle_vf_draw_to_surface_ext(
    submit_name: &'static str,
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
    geometry: VfPrimitiveGeometry,
    backend_probe_mode: BackendProbeMode,
    post_draw_sync_variant: PostDrawSyncVariant,
    vf_experiment: StreamoutProofExperiment,
) -> bool {
    let Some(draw) = prepare_vf_streamout_proof_resources(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        vf_experiment,
        geometry,
    ) else {
        crate::log!(
            "{} staging skipped reason=resource-layout size={}x{} pitch=0x{:X} geometry={}\n",
            submit_name,
            rect_w,
            rect_h,
            pitch,
            geometry.label(),
        );
        return false;
    };

    let (pipeline, pipeline_note) = match backend_probe_mode {
        BackendProbeMode::PsSimd16 => (
            crate::intel::shader::triangle_pipeline_simd16(),
            crate::intel::shader::triangle_pipeline_simd16_note(),
        ),
        BackendProbeMode::PsEotOnly => (
            crate::intel::shader::triangle_pipeline_ps_eot(),
            crate::intel::shader::triangle_pipeline_ps_eot_note(),
        ),
        _ => (
            crate::intel::shader::triangle_pipeline(),
            crate::intel::shader::triangle_pipeline_note(),
        ),
    };
    log_render_buffer_layout(warm, Some(dst_gpu_addr));
    log_render_packet_encodings();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!(
            "{} staged rt=0x{:X} vb=0x{:X} state=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} geometry={} status=awaiting-igc-or-spec-triangle-shaders vs_src={} ps_src={} note={}\n",
            submit_name,
            draw.rt_gpu_addr,
            draw.vertex_gpu_addr,
            draw.state_gpu_addr,
            draw.target_w,
            draw.target_h,
            draw.rt_pitch,
            draw.vertex_count,
            draw.vertex_stride,
            geometry.label(),
            crate::intel::shader::TRIANGLE_VERTEX_SOURCE_PATH,
            crate::intel::shader::TRIANGLE_FRAGMENT_SOURCE_PATH,
            pipeline_note
        );
        return false;
    }

    intel_render_verbose_log!(
        "{} ps-meta dispatch={:?} grf_start={} grf_used={} ksp_off=0x{:X} size={} header_only={} geometry={} vf_contract={} backend={} postdraw_sync={} note={}\n",
        submit_name,
        pipeline.ps.meta.kernel.dispatch_mode,
        pipeline.ps.meta.kernel.grf_start_register,
        pipeline.ps.meta.kernel.grf_used,
        pipeline.ps.meta.kernel.ksp_offset_bytes,
        pipeline.ps.meta.kernel.code_size_bytes,
        (pipeline.ps.meta.num_varying_inputs == 0
            && pipeline.ps.meta.kernel.push_constant_bytes == 0) as u8,
        geometry.label(),
        vf_experiment.vf_slot_contract(),
        backend_probe_mode.label(),
        post_draw_sync_variant.label(),
        pipeline_note
    );
    if geometry.fullscreen_candidate() {
        intel_render_focus_log!(
            "{} fragment-candidate-shape accepted=1 geometry={} ndc=v0[-1.000,-1.000] v1[3.000,-1.000] v2[-1.000,3.000] screen_bbox=[0,0..{},{}] sample_points=full-surface coverage_contract=oversized-triangle does_not_prove=raster_samples_or_ps\n",
            submit_name,
            geometry.label(),
            draw.target_w.saturating_sub(1),
            draw.target_h.saturating_sub(1),
        );
    } else if geometry.point_candidate() {
        let point_width_raw = backend_probe_mode
            .point_width_raw_override()
            .unwrap_or(0x200);
        intel_render_focus_log!(
            "{} fragment-candidate-shape accepted=1 geometry={} topology=pointlist ndc=center point_width_raw=0x{:X} point_width_source={} vf_contract={} screen_center=[{},{}] coverage_contract=giant-point does_not_prove=raster_samples_or_ps\n",
            submit_name,
            geometry.label(),
            point_width_raw,
            if backend_probe_mode.point_width_from_vertex() {
                "vertex"
            } else {
                "state"
            },
            vf_experiment.vf_slot_contract(),
            draw.target_w / 2,
            draw.target_h / 2,
        );
    } else if geometry.line_candidate() {
        intel_render_focus_log!(
            "{} fragment-candidate-shape accepted=1 geometry={} topology=linelist vf_contract={} target={}x{} coverage_contract=diagonal-line does_not_prove=raster_samples_or_ps\n",
            submit_name,
            geometry.label(),
            vf_experiment.vf_slot_contract(),
            draw.target_w,
            draw.target_h,
        );
    }

    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline, None) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "{} staging skipped reason=shader-layout-error detail={} note={}\n",
                submit_name,
                reason,
                pipeline_note
            );
            return false;
        }
    };
    let ps_ksp_code_dword_index =
        (pipeline.ps.meta.kernel.ksp_offset_bytes / core::mem::size_of::<u32>() as u32) as usize;
    let ps_ksp_packet_offset = shader_layout
        .ps
        .code_offset_bytes
        .saturating_add(shader_layout.ps.ksp_offset_bytes);
    let ps_ksp_base = ps_ksp_packet_offset & !0x3F;
    let ps_ksp0 = if matches!(backend_probe_mode.ps_dispatch_slot(), Some(1 | 2)) {
        0
    } else {
        ps_ksp_base
    };
    let ps_ksp1 = if matches!(
        backend_probe_mode,
        BackendProbeMode::PsDispatchSlot1 | BackendProbeMode::PsDispatchAllKspSlots
    ) {
        ps_ksp_base
    } else {
        0
    };
    let ps_ksp2 = if matches!(
        backend_probe_mode,
        BackendProbeMode::PsDispatchSlot2 | BackendProbeMode::PsDispatchAllKspSlots
    ) {
        ps_ksp_base
    } else {
        0
    };
    let baked_ps_first = pipeline
        .ps
        .code
        .get(ps_ksp_code_dword_index)
        .copied()
        .unwrap_or(0);
    let uploaded_ps_first = unsafe {
        let ptr = (warm.draw_state_virt as *const u8).add(
            shader_layout.ps.code_offset_bytes as usize
                + shader_layout.ps.ksp_offset_bytes as usize,
        ) as *const u32;
        core::ptr::read_volatile(ptr)
    };
    let ps_ksp_contract_ok = baked_ps_first != 0 && baked_ps_first == uploaded_ps_first;
    intel_render_focus_log!(
        "{} ps-ksp-proof accepted={} backend={} ksp0=0x{:X} ksp1=0x{:X} ksp2=0x{:X} ksp_off=0x{:X} first_dw=0x{:08X} baked_first=0x{:08X} dispatch={:?} does_not_prove=ps_thread_launch\n",
        submit_name,
        ps_ksp_contract_ok as u8,
        backend_probe_mode.label(),
        ps_ksp0,
        ps_ksp1,
        ps_ksp2,
        ps_ksp_packet_offset,
        uploaded_ps_first,
        baked_ps_first,
        pipeline.ps.meta.kernel.dispatch_mode,
    );

    intel_render_verbose_log!(
        "{} staged rt=0x{:X} vb=0x{:X} state=0x{:X} used_end=0x{:X} state_off=0x{:X} state_region=0x{:X} free=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} geometry={} backend={} status=pipeline-ready vs_bytes={} vs_off=0x{:X} vs_gpu=0x{:X} vs_ksp_off=0x{:X} vs_ksp=0x{:X} ps_bytes={} ps_off=0x{:X} ps_gpu=0x{:X} ps_ksp_off=0x{:X} ps_ksp=0x{:X} varyings={} ps_dispatch={:?}\n",
        submit_name,
        draw.rt_gpu_addr,
        draw.vertex_gpu_addr,
        draw.state_gpu_addr,
        shader_layout.used_bytes,
        shader_layout.state_region_offset_bytes,
        shader_layout.state_region_gpu_addr,
        warm.draw_state_len
            .saturating_sub(shader_layout.state_region_offset_bytes as usize),
        draw.target_w,
        draw.target_h,
        draw.rt_pitch,
        draw.vertex_count,
        draw.vertex_stride,
        geometry.label(),
        backend_probe_mode.label(),
        shader_layout.vs.code_size_bytes,
        shader_layout.vs.code_offset_bytes,
        shader_layout.vs.code_gpu_addr,
        shader_layout.vs.ksp_offset_bytes,
        shader_layout.vs.ksp_gpu_addr,
        shader_layout.ps.code_size_bytes,
        shader_layout.ps.code_offset_bytes,
        shader_layout.ps.code_gpu_addr,
        shader_layout.ps.ksp_offset_bytes,
        shader_layout.ps.ksp_gpu_addr,
        pipeline.ps.meta.num_varying_inputs,
        pipeline.ps.meta.kernel.dispatch_mode
    );

    let probe_state = match write_triangle_probe_state(
        warm,
        draw,
        shader_layout,
        blend_mode,
        backend_probe_mode,
        [0.0, 0.0],
    ) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "{} staging skipped reason=probe-state-error detail={}\n",
                submit_name,
                reason
            );
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_mode = if geometry.point_candidate() {
        TriangleBatchMode::VfPointDraw
    } else if geometry.line_candidate() {
        TriangleBatchMode::VfLineDraw
    } else if geometry.ndc_rect_candidate() {
        TriangleBatchMode::VfRectClipDraw
    } else if geometry.rect_candidate() {
        TriangleBatchMode::VfRectDraw
    } else if geometry.screen_space_candidate() {
        TriangleBatchMode::VfScreenSpaceDraw
    } else {
        TriangleBatchMode::VfDraw
    };
    let batch_tail_bytes = match encode_triangle_probe_batch(
        submit_name,
        batch,
        warm,
        draw,
        blend_mode,
        None,
        pipeline,
        shader_layout,
        probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        batch_mode,
        vf_experiment,
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
        [0.0, 0.0],
        backend_probe_mode,
        post_draw_sync_variant,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "{} staging skipped reason=probe-batch-error detail={}\n",
                submit_name,
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    intel_render_verbose_log!(
        "{} batch-ready bytes=0x{:X} bt_off=0x{:X} samp_off=0x{:X} blend_off=0x{:X} cc_state_off=0x{:X} cc_vp_off=0x{:X} sf_vp_off=0x{:X} geometry={}\n",
        submit_name,
        batch_tail_bytes,
        probe_state.binding_table_offset_bytes,
        probe_state.sampler_state_offset_bytes,
        probe_state.blend_state_offset_bytes,
        probe_state.color_calc_state_offset_bytes,
        probe_state.cc_viewport_offset_bytes,
        probe_state.sf_clip_viewport_offset_bytes,
        geometry.label(),
    );
    intel_render_verbose_log!(
        "{} blend-probe={} geometry={}\n",
        submit_name,
        blend_mode.label(),
        geometry.label(),
    );
    log_triangle_probe_state(warm, shader_layout, probe_state);

    let scratch_rt_before = if should_capture_scratch_rt_proof(submit_name) {
        crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
        let center_x = draw.target_w / 2;
        let center_y = draw.target_h / 2;
        let center_offset = center_y
            .saturating_mul(draw.rt_pitch)
            .saturating_add(center_x.saturating_mul(4)) as usize;
        let post_offset =
            center_offset.saturating_add(if center_x + 1 < draw.target_w { 4 } else { 0 });
        let read_scratch_dword = |byte_offset: usize| -> u32 {
            if byte_offset.saturating_add(core::mem::size_of::<u32>()) > warm.streamout_len {
                return 0;
            }
            unsafe {
                let ptr = (warm.streamout_virt as *const u8).add(byte_offset) as *const u32;
                core::ptr::read_volatile(ptr)
            }
        };
        Some((
            read_scratch_dword(0),
            read_scratch_dword(center_offset),
            read_scratch_dword(post_offset),
            center_offset,
            post_offset,
        ))
    } else {
        None
    };
    let scratch_stats_before = if scratch_rt_before.is_some() {
        Some(capture_triangle_stage_stats(dev))
    } else {
        None
    };

    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        submit_name,
    );
    if let (
        Some((scratch_before, center_before, post_before, center_offset, post_offset)),
        Some(_stats_before),
    ) = (scratch_rt_before, scratch_stats_before)
    {
        crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
        let read_scratch_dword = |byte_offset: usize| -> u32 {
            if byte_offset.saturating_add(core::mem::size_of::<u32>()) > warm.streamout_len {
                return 0;
            }
            unsafe {
                let ptr = (warm.streamout_virt as *const u8).add(byte_offset) as *const u32;
                core::ptr::read_volatile(ptr)
            }
        };
        let scratch_after = read_scratch_dword(0);
        let center_after = read_scratch_dword(center_offset);
        let post_after = read_scratch_dword(post_offset);
        // submit_warm_render_batch installs a freshly zeroed LRC image. The
        // pre-submit MMIO sample is from the outgoing context, while these are
        // the complete counters of the submitted context.
        let delta = capture_triangle_stage_stats(dev);
        let ps_counter_accept =
            delta.ps_invocations > 0 || delta.cps_invocations > 0 || delta.ps_depth > 0;
        let rt_changed = scratch_after != scratch_before
            || center_after != center_before
            || post_after != post_before;
        let artificial_markers = is_artificial_fragment_marker_submit_name(submit_name);
        let artificial_pre_marker = center_after == RCS_ARTIFICIAL_FRAGMENT_PRE_COLOR;
        let artificial_post_marker = post_after == RCS_ARTIFICIAL_FRAGMENT_POST_COLOR;
        let possible_draw_window_write = artificial_markers
            && artificial_post_marker
            && center_after != RCS_ARTIFICIAL_FRAGMENT_PRE_COLOR;
        let accepted =
            ps_counter_accept || (!artificial_markers && rt_changed) || possible_draw_window_write;
        record_fragment_boundary_probe(true, accepted);
        intel_render_focus_log!(
            "{} scratch-rt-fragment-proof accepted={} completed={} rt_gpu=0x{:X} size={}x{} pitch=0x{:X} before=0x{:08X} after=0x{:08X} center_before=0x{:08X} center_after=0x{:08X} post_before=0x{:08X} post_after=0x{:08X} changed={} artificial={} artificial_pre_marker={} artificial_post_marker={} possible_draw_window_write={} ps_delta={} cps_delta={} ps_depth_delta={} does_not_prove=display_scanout\n",
            submit_name,
            accepted as u8,
            completed as u8,
            draw.rt_gpu_addr,
            draw.target_w,
            draw.target_h,
            draw.rt_pitch,
            scratch_before,
            scratch_after,
            center_before,
            center_after,
            post_before,
            post_after,
            rt_changed as u8,
            artificial_markers as u8,
            artificial_pre_marker as u8,
            artificial_post_marker as u8,
            possible_draw_window_write as u8,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
        );
        if is_raster_wm_oa_submit_name(submit_name) {
            log_raster_wm_oa_probe(submit_name, warm, completed, draw, delta);
        }
    }
    if is_raster_wm_oa_submit_name(submit_name) {
        disable_raster_wm_oa_context(dev, submit_name);
    }
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, submit_name);
    }
    completed
}

fn disable_raster_wm_oa_context(dev: crate::intel::Dev, submit_name: &'static str) {
    crate::intel::mmio_write(dev, RCS_OACTXCONTROL, 0);
    crate::intel::mmio_write(dev, OAR_OACONTROL, 0);
    crate::intel::mmio_write(
        dev,
        RCS_RING_CONTEXT_CONTROL,
        masked_bits_update(0, CTX_CTRL_OAC_CONTEXT_ENABLE),
    );
    intel_render_focus_log!(
        "{} raster-wm-oa cleanup oactx=0 oar=0 reason=diagnostic-counter-disable\n",
        submit_name,
    );
}

fn oa_report_slice(warm: RenderWarmState, base_dword: usize) -> Option<&'static [u32]> {
    if base_dword
        .checked_add(RESULT_OA_REPORT_DWORDS)?
        .checked_mul(core::mem::size_of::<u32>())?
        > warm.result_len
    {
        return None;
    }
    let dwords =
        unsafe { core::slice::from_raw_parts(warm.result_virt as *const u32, warm.result_len / 4) };
    dwords.get(base_dword..base_dword + RESULT_OA_REPORT_DWORDS)
}

fn oa_counter_delta(before: u64, after: u64, bits: u32) -> u64 {
    if after >= before {
        after - before
    } else {
        (1u64 << bits).saturating_add(after).saturating_sub(before)
    }
}

fn oa_a_counter_gfx125(report: &[u32], index: usize) -> Option<u64> {
    if report.len() < RESULT_OA_REPORT_DWORDS || index >= 36 {
        return None;
    }
    if index < 4 {
        Some(report[4 + index] as u64)
    } else if index < 24 {
        let high_bytes =
            unsafe { core::slice::from_raw_parts(report.as_ptr().add(40) as *const u8, 32) };
        Some(report[4 + index] as u64 | ((high_bytes[index] as u64) << 32))
    } else if index < 28 {
        Some(report[28 + (index - 24)] as u64)
    } else if index < 32 {
        let high_bytes =
            unsafe { core::slice::from_raw_parts(report.as_ptr().add(40) as *const u8, 32) };
        Some(report[4 + index] as u64 | ((high_bytes[index] as u64) << 32))
    } else {
        Some(report[36 + (index - 32)] as u64)
    }
}

fn oa_a_delta_gfx125(begin: &[u32], end: &[u32], index: usize) -> u64 {
    let Some(before) = oa_a_counter_gfx125(begin, index) else {
        return 0;
    };
    let Some(after) = oa_a_counter_gfx125(end, index) else {
        return 0;
    };
    let bits = if (4..24).contains(&index) || (28..32).contains(&index) {
        40
    } else {
        32
    };
    oa_counter_delta(before, after, bits)
}

fn log_raster_wm_oa_raw_deltas(submit_name: &'static str, begin: &[u32], end: &[u32]) {
    let mut a = [0u64; 36];
    let mut changed = 0usize;
    let mut i = 0usize;
    while i < a.len() {
        a[i] = oa_a_delta_gfx125(begin, end, i);
        if a[i] != 0 {
            changed += 1;
        }
        i += 1;
    }
    intel_render_verbose_log!(
        "{} oa-raw-a-delta changed={} a00={} a01={} a02={} a03={} a04={} a05={} a06={} a07={} a08={} a09={} a10={} a11={}\n",
        submit_name,
        changed,
        a[0],
        a[1],
        a[2],
        a[3],
        a[4],
        a[5],
        a[6],
        a[7],
        a[8],
        a[9],
        a[10],
        a[11],
    );
    intel_render_verbose_log!(
        "{} oa-raw-a-delta a12={} a13={} a14={} a15={} a16={} a17={} a18={} a19={} a20={} a21={} a22={} a23={}\n",
        submit_name,
        a[12],
        a[13],
        a[14],
        a[15],
        a[16],
        a[17],
        a[18],
        a[19],
        a[20],
        a[21],
        a[22],
        a[23],
    );
    intel_render_verbose_log!(
        "{} oa-raw-a-delta a24={} a25={} a26={} a27={} a28={} a29={} a30={} a31={} a32={} a33={} a34={} a35={} note=raw-counter-index-audit\n",
        submit_name,
        a[24],
        a[25],
        a[26],
        a[27],
        a[28],
        a[29],
        a[30],
        a[31],
        a[32],
        a[33],
        a[34],
        a[35],
    );
}

fn log_raster_wm_oa_probe(
    submit_name: &'static str,
    warm: RenderWarmState,
    completed: bool,
    draw: TriangleDrawPrep,
    delta: TriangleStageStats,
) {
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let begin = oa_report_slice(warm, RESULT_OA_BEGIN_DWORD);
    let end = oa_report_slice(warm, RESULT_OA_END_DWORD);
    let begin_id = begin.and_then(|r| r.first().copied()).unwrap_or(0);
    let end_id = end.and_then(|r| r.first().copied()).unwrap_or(0);
    let reports_valid =
        begin_id == RESULT_OA_RASTER_WM_BEGIN_ID && end_id == RESULT_OA_RASTER_WM_END_ID;

    let (ps_threads_delta, raster_samples_delta, samples_killed_delta, postps_fail_delta) =
        if reports_valid {
            let begin = begin.unwrap_or(&[]);
            let end = end.unwrap_or(&[]);
            (
                oa_a_delta_gfx125(begin, end, 6),
                oa_a_delta_gfx125(begin, end, 21).saturating_mul(4),
                oa_a_delta_gfx125(begin, end, 24).saturating_mul(4),
                oa_a_delta_gfx125(begin, end, 25).saturating_mul(4),
            )
        } else {
            (0, 0, 0, 0)
        };
    let (pixel_write_delta, pixel_blend_delta) = if reports_valid {
        let begin = begin.unwrap_or(&[]);
        let end = end.unwrap_or(&[]);
        (
            oa_a_delta_gfx125(begin, end, 26).saturating_mul(4),
            oa_a_delta_gfx125(begin, end, 27).saturating_mul(4),
        )
    } else {
        (0, 0)
    };
    let accepted = reports_valid
        && (raster_samples_delta != 0
            || ps_threads_delta != 0
            || samples_killed_delta != 0
            || postps_fail_delta != 0
            || pixel_write_delta != 0
            || pixel_blend_delta != 0);
    if reports_valid && !accepted {
        let begin = begin.unwrap_or(&[]);
        let end = end.unwrap_or(&[]);
        log_raster_wm_oa_raw_deltas(submit_name, begin, end);
    }
    record_fragment_boundary_probe(true, accepted);
    intel_render_focus_log!(
        "{} raster-wm-input-proof accepted={} completed={} reports_valid={} begin_id=0x{:08X} end_id=0x{:08X} rt_gpu=0x{:X} size={}x{} pitch=0x{:X} raster_samples_delta={} ps_threads_delta={} samples_killed_delta={} postps_fail_delta={} pixel_write_delta={} pixel_blend_delta={} ps_delta={} cps_delta={} ps_depth_delta={} observable=oar-mi-rpc-a21 does_not_prove=rt_visible\n",
        submit_name,
        accepted as u8,
        completed as u8,
        reports_valid as u8,
        begin_id,
        end_id,
        draw.rt_gpu_addr,
        draw.target_w,
        draw.target_h,
        draw.rt_pitch,
        raster_samples_delta,
        ps_threads_delta,
        samples_killed_delta,
        postps_fail_delta,
        pixel_write_delta,
        pixel_blend_delta,
        delta.ps_invocations,
        delta.cps_invocations,
        delta.ps_depth,
    );
}

fn submit_triangle_vs_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(draw) = prepare_triangle_draw_resources(warm, dst_gpu_addr, pitch, rect_w, rect_h)
    else {
        crate::log!(
            "vs-streamout-proof skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };
    let pipeline = crate::intel::shader::triangle_pipeline();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!("vs-streamout-proof skipped reason=placeholder-pipeline\n");
        return false;
    }
    let slice_hash_table_offset = match write_vf_streamout_probe_state(warm) {
        Ok(offset) => offset,
        Err(reason) => {
            crate::log!("vs-streamout-proof skipped reason=probe-state detail={}\n", reason);
            return false;
        }
    };
    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline, None) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!("vs-streamout-proof skipped reason=shader-layout detail={}\n", reason);
            return false;
        }
    };
    if slice_hash_table_offset != 0
        && usize::try_from(shader_layout.used_bytes)
            .ok()
            .unwrap_or(usize::MAX)
            > slice_hash_table_offset as usize
    {
        crate::log!(
            "vs-streamout-proof skipped reason=slice-hash-overlap used_end=0x{:X} slice_hash_off=0x{:X}\n",
            shader_layout.used_bytes,
            slice_hash_table_offset
        );
        return false;
    }

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_vs_streamout_proof_batch(
        batch,
        warm,
        draw,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        experiment,
        slice_hash_table_offset,
        VsStreamoutProofConfig {
            pipeline,
            shader_layout,
        },
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!("vs-streamout-proof batch build failed detail={}\n", reason);
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    intel_render_verbose_log!(
        "vs-streamout-proof batch-ready experiment={} bytes=0x{:X} so_gpu=0x{:X} so_pitch={} vertices={}\n",
        experiment.label(),
        batch_tail_bytes,
        GPU_VA_STREAMOUT_BASE,
        experiment.vertex_bytes(),
        draw.vertex_count
    );

    let stats_before = capture_triangle_stage_stats(dev);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        "vs-streamout-proof",
    );
    let stats_after = capture_triangle_stage_stats(dev);
    let accepted = completed
        || maybe_soft_accept_streamout_submit(
            "vs-streamout-proof",
            warm,
            stats_before,
            stats_after,
            true,
            experiment.vertex_bytes() * draw.vertex_count as usize,
        );
    log_streamout_proof_result(
        "vs-streamout-proof",
        warm,
        completed,
        draw.vertex_count as usize,
        experiment,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "vs-streamout-proof");
    }
    accepted
}

fn submit_triangle_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(draw) = prepare_triangle_draw_resources(warm, dst_gpu_addr, pitch, rect_w, rect_h)
    else {
        crate::log!(
            "streamout-proof skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };
    let pipeline = crate::intel::shader::triangle_pipeline();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!("streamout-proof skipped reason=placeholder-pipeline\n");
        return false;
    }
    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline, None) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!("streamout-proof skipped reason=shader-layout detail={}\n", reason);
            return false;
        }
    };
    let probe_state = match write_triangle_probe_state(
        warm,
        draw,
        shader_layout,
        TriangleBlendProbeMode::ExplicitRt0,
        BackendProbeMode::MesaLike,
        [0.0, 0.0],
    ) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!("streamout-proof skipped reason=probe-state detail={}\n", reason);
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_triangle_probe_batch(
        "streamout-proof",
        batch,
        warm,
        draw,
        TriangleBlendProbeMode::ExplicitRt0,
        None,
        pipeline,
        shader_layout,
        probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        TriangleBatchMode::StreamoutProof,
        experiment,
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
        [0.0, 0.0],
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::HeavyAll,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!("streamout-proof batch build failed detail={}\n", reason);
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    intel_render_verbose_log!(
        "streamout-proof batch-ready experiment={} bytes=0x{:X} so_gpu=0x{:X} so_pitch={} vertices={}\n",
        experiment.label(),
        batch_tail_bytes,
        GPU_VA_STREAMOUT_BASE,
        experiment.vertex_bytes(),
        draw.vertex_count
    );

    let stats_before = capture_triangle_stage_stats(dev);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        "streamout-proof",
    );
    let stats_after = capture_triangle_stage_stats(dev);
    let accepted = completed
        || maybe_soft_accept_streamout_submit(
            "streamout-proof",
            warm,
            stats_before,
            stats_after,
            true,
            experiment.vertex_bytes() * draw.vertex_count as usize,
        );
    log_streamout_proof_result(
        "streamout-proof",
        warm,
        completed,
        draw.vertex_count as usize,
        experiment,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "streamout-proof");
    }
    accepted
}

fn submit_triangle_vs_draw_frontier_to_surface(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
) -> bool {
    for contract in VS_DRAW_FRONTIER_CONTRACTS {
        let completed = submit_triangle_real_vs_draw_probe_to_surface(
            dev,
            warm,
            dst_gpu_addr,
            pitch,
            rect_w,
            rect_h,
            blend_mode,
            "vs-draw-frontier",
            contract,
        );
        intel_render_verbose_log!(
            "primary-vs-draw-frontier-contract variant={} completed={}\n",
            contract.label,
            completed as u8,
        );
        if completed {
            return true;
        }
    }
    false
}

fn submit_triangle_vs_draw_frontier_to_scratch(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
) -> bool {
    let scratch_pitch = 32 * core::mem::size_of::<u32>();
    if warm.streamout_len < scratch_pitch * 32 {
        intel_render_focus_log!(
            "vs-draw-frontier-scratch skipped reason=streamout-too-small len=0x{:X} required=0x{:X}\n",
            warm.streamout_len,
            scratch_pitch * 32,
        );
        return false;
    }
    let variants = [
        ("vs-draw-frontier-scratch", VfPrimitiveGeometry::Canonical, None),
        ("vs-draw-frontier-scratch-ndc-rect", VfPrimitiveGeometry::NdcRect, None),
        (
            "vs-draw-frontier-scratch-ndc-rect-trilist",
            VfPrimitiveGeometry::NdcRect,
            Some(TriangleBatchMode::Draw),
        ),
        ("vs-draw-frontier-scratch-ndc-rect-cw", VfPrimitiveGeometry::NdcRectCw, None),
        (
            "vs-draw-frontier-scratch-ndc-rect-cw-trilist",
            VfPrimitiveGeometry::NdcRectCw,
            Some(TriangleBatchMode::Draw),
        ),
        (
            "vs-draw-frontier-scratch-screen-rect",
            VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB,
            None,
        ),
        ("vs-draw-frontier-scratch-ndc-large", VfPrimitiveGeometry::NdcTriangleLarge, None),
    ];
    let contracts = [
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
        VS_DRAW_SBE_READ0_CONTRACT,
        VS_DRAW_FRONTIER_CONTRACTS[2],
    ];
    for (submit_name, geometry, batch_mode_override) in variants {
        for contract in contracts {
            seed_render_scratch_rt(warm);
            let completed = submit_triangle_real_vs_draw_probe_to_surface_ext(
                dev,
                warm,
                GPU_VA_STREAMOUT_BASE,
                scratch_pitch,
                32,
                32,
                TriangleBlendProbeMode::MesaZeroedState,
                geometry,
                submit_name,
                contract,
                BackendProbeMode::MesaLike,
                PostDrawSyncVariant::LightPostSyncNoCs,
                batch_mode_override,
            );
            let observed = fragment_boundary_observed();
            intel_render_focus_log!(
                "vs-draw-frontier-scratch variant={} geometry={} contract={} completed={} observed={} target=scratch-rt32\n",
                submit_name,
                geometry.label(),
                contract.label,
                completed as u8,
                observed as u8,
            );
            if observed {
                return true;
            }
        }
    }
    false
}

fn wait_eq(dev: crate::intel::Dev, reg: usize, mask: u32, want: u32, n: usize) -> bool {
    for _ in 0..n {
        if (crate::intel::mmio_read(dev, reg) & mask) == want {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn map_smoke_buffers(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    let ok_ring = super::map_ggtt(dev, warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE);
    let ok_context = super::map_ggtt(dev, warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE);
    let ok_batch = super::map_ggtt(dev, warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE);
    let ok_draw_state =
        super::map_ggtt(dev, warm.draw_state_phys, warm.draw_state_len, GPU_VA_DRAW_STATE_BASE);
    let ok_vertex = super::map_ggtt(dev, warm.vertex_phys, warm.vertex_len, GPU_VA_VERTEX_BASE);
    let ok_result = super::map_ggtt(dev, warm.result_phys, warm.result_len, GPU_VA_RESULT_BASE);
    let ok_streamout =
        super::map_ggtt(dev, warm.streamout_phys, warm.streamout_len, GPU_VA_STREAMOUT_BASE);
    if ok_ring && ok_context && ok_batch && ok_draw_state && ok_vertex && ok_result && ok_streamout
    {
        super::ggtt_invalidate(dev);
        true
    } else {
        false
    }
}

fn read_first_dword(virt: *mut u8, len: usize) -> u32 {
    if virt.is_null() || len < core::mem::size_of::<u32>() {
        return 0;
    }
    unsafe { core::ptr::read_volatile(virt as *const u32) }
}

fn log_render_memory_proof(warm: RenderWarmState) {
    crate::intel::dma_flush(warm.ring_virt, warm.ring_len);
    crate::intel::dma_flush(warm.context_virt, warm.context_len);
    crate::intel::dma_flush(warm.batch_virt, warm.batch_len);
    crate::intel::dma_flush(warm.draw_state_virt, warm.draw_state_len);
    crate::intel::dma_flush(warm.vertex_virt, warm.vertex_len);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);

    let ring_rb = read_first_dword(warm.ring_virt, warm.ring_len);
    let context_rb = read_first_dword(warm.context_virt, warm.context_len);
    let batch_rb = read_first_dword(warm.batch_virt, warm.batch_len);
    let state_rb = read_first_dword(warm.draw_state_virt, warm.draw_state_len);
    let vertex_rb = read_first_dword(warm.vertex_virt, warm.vertex_len);
    let result_rb = read_first_dword(warm.result_virt, warm.result_len);
    let streamout_rb = read_first_dword(warm.streamout_virt, warm.streamout_len);

    intel_render_focus_log!(
        "memory-proof accepted=1 map=1 ggtt_invalidated=1 flush=all readback=cpu-first-dword ring[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] context[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] batch[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] state[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] vertex[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] result[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] streamout[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] does_not_prove=fragment_ps_rt_progress\n",
        warm.ring_phys,
        GPU_VA_RING_BASE,
        warm.ring_len,
        ring_rb,
        warm.context_phys,
        GPU_VA_CONTEXT_BASE,
        warm.context_len,
        context_rb,
        warm.batch_phys,
        GPU_VA_BATCH_BASE,
        warm.batch_len,
        batch_rb,
        warm.draw_state_phys,
        GPU_VA_DRAW_STATE_BASE,
        warm.draw_state_len,
        state_rb,
        warm.vertex_phys,
        GPU_VA_VERTEX_BASE,
        warm.vertex_len,
        vertex_rb,
        warm.result_phys,
        GPU_VA_RESULT_BASE,
        warm.result_len,
        result_rb,
        warm.streamout_phys,
        GPU_VA_STREAMOUT_BASE,
        warm.streamout_len,
        streamout_rb,
    );
}

fn ensure_smoke_buffers_mapped(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    if !map_smoke_buffers(dev, warm) {
        WARM_BUFFERS_MAPPED.store(false, Ordering::Release);
        return false;
    }
    if !MEMORY_PROOF_LOGGED.swap(true, Ordering::AcqRel) {
        log_render_memory_proof(warm);
    }
    WARM_BUFFERS_MAPPED.store(true, Ordering::Release);
    true
}

fn should_log_primary_probe(reason: &str, seq: u32) -> bool {
    reason == "boot-once" || seq <= 3 || seq.is_multiple_of(PRIMARY_PERIODIC_LOG_EVERY)
}

fn should_log_primary_probe_detail() -> bool {
    if crate::log_os::flags::INTEL_STAGE1_LOGS || !render_detail_logs_enabled() {
        return false;
    }
    let seq = PRIMARY_PROBE_SEQ.load(Ordering::Acquire);
    seq <= 3 || seq.is_multiple_of(PRIMARY_PERIODIC_LOG_EVERY)
}

fn submit_triangle_draw_to_surface(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
) -> bool {
    submit_triangle_real_vs_draw_probe_to_surface(
        dev,
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        blend_mode,
        "draw-path",
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
    )
}

fn submit_triangle_real_vs_draw_probe_to_surface(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
    submit_name: &'static str,
    front_end_contract: TriangleFrontEndContract,
) -> bool {
    submit_triangle_real_vs_draw_probe_to_surface_ext(
        dev,
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        blend_mode,
        VfPrimitiveGeometry::Canonical,
        submit_name,
        front_end_contract,
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::HeavyAll,
        None,
    )
}

fn submit_triangle_real_vs_draw_probe_to_surface_ext(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
    geometry: VfPrimitiveGeometry,
    submit_name: &'static str,
    front_end_contract: TriangleFrontEndContract,
    backend_probe_mode: BackendProbeMode,
    post_draw_sync_variant: PostDrawSyncVariant,
    batch_mode_override: Option<TriangleBatchMode>,
) -> bool {
    let Some(draw) = prepare_triangle_draw_resources_for_geometry(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        geometry,
    ) else {
        crate::log!(
            "{} staging skipped reason=resource-layout size={}x{} pitch=0x{:X} geometry={}\n",
            submit_name,
            rect_w,
            rect_h,
            pitch,
            geometry.label(),
        );
        return false;
    };

    let (pipeline, pipeline_note) = match backend_probe_mode {
        BackendProbeMode::PsSimd16 => (
            crate::intel::shader::triangle_pipeline_simd16(),
            crate::intel::shader::triangle_pipeline_simd16_note(),
        ),
        BackendProbeMode::PsEotOnly => (
            crate::intel::shader::triangle_pipeline_ps_eot(),
            crate::intel::shader::triangle_pipeline_ps_eot_note(),
        ),
        _ => (
            crate::intel::shader::triangle_pipeline(),
            crate::intel::shader::triangle_pipeline_note(),
        ),
    };
    log_render_buffer_layout(warm, Some(dst_gpu_addr));
    log_render_packet_encodings();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!(
            "{} staged rt=0x{:X} vb=0x{:X} state=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} status=awaiting-igc-or-spec-triangle-shaders vs_src={} ps_src={} note={}\n",
            submit_name,
            draw.rt_gpu_addr,
            draw.vertex_gpu_addr,
            draw.state_gpu_addr,
            draw.target_w,
            draw.target_h,
            draw.rt_pitch,
            draw.vertex_count,
            draw.vertex_stride,
            crate::intel::shader::TRIANGLE_VERTEX_SOURCE_PATH,
            crate::intel::shader::TRIANGLE_FRAGMENT_SOURCE_PATH,
            pipeline_note
        );
        return false;
    }

    intel_render_verbose_log!(
        "{} ps-meta dispatch={:?} grf_start={} grf_used={} ksp_off=0x{:X} size={} header_only={} geometry={} backend={} postdraw_sync={} note={}\n",
        submit_name,
        pipeline.ps.meta.kernel.dispatch_mode,
        pipeline.ps.meta.kernel.grf_start_register,
        pipeline.ps.meta.kernel.grf_used,
        pipeline.ps.meta.kernel.ksp_offset_bytes,
        pipeline.ps.meta.kernel.code_size_bytes,
        (pipeline.ps.meta.num_varying_inputs == 0
            && pipeline.ps.meta.kernel.push_constant_bytes == 0) as u8,
        geometry.label(),
        backend_probe_mode.label(),
        post_draw_sync_variant.label(),
        pipeline_note
    );
    if geometry.fullscreen_candidate() {
        intel_render_focus_log!(
            "{} fragment-candidate-shape accepted=1 geometry={} ndc=v0[-1.000,-1.000] v1[3.000,-1.000] v2[-1.000,3.000] screen_bbox=[0,0..{},{}] sample_points=full-surface coverage_contract=oversized-triangle does_not_prove=raster_samples_or_ps\n",
            submit_name,
            geometry.label(),
            draw.target_w.saturating_sub(1),
            draw.target_h.saturating_sub(1),
        );
    } else if geometry.screen_space_candidate() {
        intel_render_focus_log!(
            "{} fragment-candidate-shape accepted=1 geometry={} topology=trilist sf_viewport_transform=0 screen_vertices=v0[0.5,0.5] v1[7.5,0.5] v2[0.5,7.5] target={}x{} coverage_contract=screen-space-scratch-triangle does_not_prove=raster_samples_or_ps\n",
            submit_name,
            geometry.label(),
            draw.target_w,
            draw.target_h,
        );
    }
    let programmed_vs_urb_output_length = front_end_contract
        .vs_urb_output_length_override
        .or(TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE)
        .unwrap_or(pipeline.vs.meta.urb_entry_output_length);
    if submit_name == "vs-draw-frontier" {
        intel_render_focus_log!(
            "{} contract variant={} baked_vs_urb_out_len={} programmed_vs_urb_out_len={} sbe[read_offset={} read_length={} force_offset={} force_length={} num_sf_attrs={}]\n",
            submit_name,
            front_end_contract.label,
            pipeline.vs.meta.urb_entry_output_length,
            programmed_vs_urb_output_length,
            front_end_contract.sbe_read_offset,
            front_end_contract.sbe_read_length,
            front_end_contract.force_sbe_read_offset as u8,
            front_end_contract.force_sbe_read_length as u8,
            pipeline.ps.meta.num_varying_inputs,
        );
    } else {
        intel_render_verbose_log!(
            "{} contract variant={} baked_vs_urb_out_len={} programmed_vs_urb_out_len={} sbe[read_offset={} read_length={} force_offset={} force_length={} num_sf_attrs={}]\n",
            submit_name,
            front_end_contract.label,
            pipeline.vs.meta.urb_entry_output_length,
            programmed_vs_urb_output_length,
            front_end_contract.sbe_read_offset,
            front_end_contract.sbe_read_length,
            front_end_contract.force_sbe_read_offset as u8,
            front_end_contract.force_sbe_read_length as u8,
            pipeline.ps.meta.num_varying_inputs,
        );
    }

    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline, None) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "{} staging skipped reason=shader-layout-error detail={} note={}\n",
                submit_name,
                reason,
                pipeline_note
            );
            return false;
        }
    };
    log_uploaded_triangle_shader_verification(warm, pipeline, shader_layout, submit_name, None);

    intel_render_verbose_log!(
        "{} staged rt=0x{:X} vb=0x{:X} state=0x{:X} used_end=0x{:X} state_off=0x{:X} state_region=0x{:X} free=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} status=pipeline-ready vs_bytes={} vs_off=0x{:X} vs_gpu=0x{:X} vs_ksp_off=0x{:X} vs_ksp=0x{:X} ps_bytes={} ps_off=0x{:X} ps_gpu=0x{:X} ps_ksp_off=0x{:X} ps_ksp=0x{:X} varyings={} ps_dispatch={:?}\n",
        submit_name,
        draw.rt_gpu_addr,
        draw.vertex_gpu_addr,
        draw.state_gpu_addr,
        shader_layout.used_bytes,
        shader_layout.state_region_offset_bytes,
        shader_layout.state_region_gpu_addr,
        warm.draw_state_len
            .saturating_sub(shader_layout.state_region_offset_bytes as usize),
        draw.target_w,
        draw.target_h,
        draw.rt_pitch,
        draw.vertex_count,
        draw.vertex_stride,
        shader_layout.vs.code_size_bytes,
        shader_layout.vs.code_offset_bytes,
        shader_layout.vs.code_gpu_addr,
        shader_layout.vs.ksp_offset_bytes,
        shader_layout.vs.ksp_gpu_addr,
        shader_layout.ps.code_size_bytes,
        shader_layout.ps.code_offset_bytes,
        shader_layout.ps.code_gpu_addr,
        shader_layout.ps.ksp_offset_bytes,
        shader_layout.ps.ksp_gpu_addr,
        pipeline.ps.meta.num_varying_inputs,
        pipeline.ps.meta.kernel.dispatch_mode
    );

    let probe_state = match write_triangle_probe_state(
        warm,
        draw,
        shader_layout,
        blend_mode,
        backend_probe_mode,
        [0.0, 0.0],
    ) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "{} staging skipped reason=probe-state-error detail={}\n",
                submit_name,
                reason
            );
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_mode = batch_mode_override.unwrap_or_else(|| {
        if geometry.rect_candidate() {
            TriangleBatchMode::DrawScreenSpaceRect
        } else if geometry.screen_space_candidate() {
            TriangleBatchMode::DrawScreenSpace
        } else {
            TriangleBatchMode::Draw
        }
    });
    let batch_tail_bytes = match encode_triangle_probe_batch(
        submit_name,
        batch,
        warm,
        draw,
        blend_mode,
        None,
        pipeline,
        shader_layout,
        probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        batch_mode,
        StreamoutProofExperiment::PositionSlot1,
        front_end_contract,
        [0.0, 0.0],
        backend_probe_mode,
        post_draw_sync_variant,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "{} staging skipped reason=probe-batch-error detail={}\n",
                submit_name,
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    intel_render_verbose_log!(
        "{} batch-ready bytes=0x{:X} bt_off=0x{:X} samp_off=0x{:X} blend_off=0x{:X} cc_state_off=0x{:X} cc_vp_off=0x{:X} sf_vp_off=0x{:X} geometry={} backend={}\n",
        submit_name,
        batch_tail_bytes,
        probe_state.binding_table_offset_bytes,
        probe_state.sampler_state_offset_bytes,
        probe_state.blend_state_offset_bytes,
        probe_state.color_calc_state_offset_bytes,
        probe_state.cc_viewport_offset_bytes,
        probe_state.sf_clip_viewport_offset_bytes,
        geometry.label(),
        backend_probe_mode.label(),
    );
    intel_render_verbose_log!("{} blend-probe={}\n", submit_name, blend_mode.label());
    log_triangle_probe_state(warm, shader_layout, probe_state);

    let scratch_rt_before = if should_capture_scratch_rt_proof(submit_name) {
        crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
        let center_x = draw.target_w / 2;
        let center_y = draw.target_h / 2;
        let center_offset = center_y
            .saturating_mul(draw.rt_pitch)
            .saturating_add(center_x.saturating_mul(4)) as usize;
        let post_offset =
            center_offset.saturating_add(if center_x + 1 < draw.target_w { 4 } else { 0 });
        let read_scratch_dword = |byte_offset: usize| -> u32 {
            if byte_offset.saturating_add(core::mem::size_of::<u32>()) > warm.streamout_len {
                return 0;
            }
            unsafe {
                let ptr = (warm.streamout_virt as *const u8).add(byte_offset) as *const u32;
                core::ptr::read_volatile(ptr)
            }
        };
        Some((
            read_scratch_dword(0),
            read_scratch_dword(center_offset),
            read_scratch_dword(post_offset),
            center_offset,
            post_offset,
        ))
    } else {
        None
    };
    let scratch_stats_before = if scratch_rt_before.is_some() {
        Some(capture_triangle_stage_stats(dev))
    } else {
        None
    };

    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        submit_name,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, submit_name);
    }
    if let (
        Some((scratch_before, center_before, post_before, center_offset, post_offset)),
        Some(_stats_before),
    ) = (scratch_rt_before, scratch_stats_before)
    {
        crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
        let read_scratch_dword = |byte_offset: usize| -> u32 {
            if byte_offset.saturating_add(core::mem::size_of::<u32>()) > warm.streamout_len {
                return 0;
            }
            unsafe {
                let ptr = (warm.streamout_virt as *const u8).add(byte_offset) as *const u32;
                core::ptr::read_volatile(ptr)
            }
        };
        let scratch_after = read_scratch_dword(0);
        let center_after = read_scratch_dword(center_offset);
        let post_after = read_scratch_dword(post_offset);
        let delta = capture_triangle_stage_stats(dev);
        let ps_counter_accept =
            delta.ps_invocations > 0 || delta.cps_invocations > 0 || delta.ps_depth > 0;
        let rt_changed = scratch_after != scratch_before
            || center_after != center_before
            || post_after != post_before;
        let accepted = ps_counter_accept || rt_changed;
        record_fragment_boundary_probe(true, accepted);
        intel_render_focus_log!(
            "{} scratch-rt-fragment-proof accepted={} completed={} rt_gpu=0x{:X} size={}x{} pitch=0x{:X} before=0x{:08X} after=0x{:08X} center_before=0x{:08X} center_after=0x{:08X} post_before=0x{:08X} post_after=0x{:08X} changed={} ps_delta={} cps_delta={} ps_depth_delta={} source=real-vs does_not_prove=display_scanout\n",
            submit_name,
            accepted as u8,
            completed as u8,
            draw.rt_gpu_addr,
            draw.target_w,
            draw.target_h,
            draw.rt_pitch,
            scratch_before,
            scratch_after,
            center_before,
            center_after,
            post_before,
            post_after,
            rt_changed as u8,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
        );
    }
    completed
}

fn submit_triangle_real_vs_draw_probe_vertices_to_surface_ext(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
    depth_config: Option<TriangleDepthConfig>,
    vertices: &[[f32; 3]],
    indices: Option<&[u32]>,
    gpu_mesh: Option<crate::intel::gpgpu::GpgpuFontOutlineMesh>,
    resident_mesh: Option<&ResidentFontMesh>,
    draw_rgba: Option<[u8; 4]>,
    geometry_label: &'static str,
    submit_name: &'static str,
    front_end_contract: TriangleFrontEndContract,
    backend_probe_mode: BackendProbeMode,
    post_draw_sync_variant: PostDrawSyncVariant,
    batch_mode: TriangleBatchMode,
    streamout_experiment: StreamoutProofExperiment,
    viewport_translation_px: [f32; 2],
    mut readback: Option<&mut Option<FontRenderTargetReadback>>,
) -> bool {
    let draw = if let Some(mesh) = resident_mesh {
        if batch_mode.vf_synthesized_vue() {
            None
        } else {
            prepare_triangle_draw_resources_for_resident_font_mesh(
                warm,
                dst_gpu_addr,
                pitch,
                rect_w,
                rect_h,
                mesh,
            )
        }
    } else if let Some(mesh) = gpu_mesh {
        if batch_mode.vf_synthesized_vue() {
            None
        } else {
            prepare_triangle_draw_resources_for_gpu_font_mesh(
                warm,
                dst_gpu_addr,
                pitch,
                rect_w,
                rect_h,
                mesh,
            )
        }
    } else if let Some(indices) = indices {
        if batch_mode.vf_synthesized_vue() {
            None
        } else {
            prepare_triangle_draw_resources_for_indexed_vertex_slice(
                warm,
                dst_gpu_addr,
                pitch,
                rect_w,
                rect_h,
                geometry_label,
                vertices,
                indices,
            )
        }
    } else if batch_mode.vf_synthesized_vue() {
        prepare_triangle_draw_resources_for_vf_vue_vertex_slice(
            warm,
            dst_gpu_addr,
            pitch,
            rect_w,
            rect_h,
            geometry_label,
            vertices,
            streamout_experiment,
        )
    } else {
        prepare_triangle_draw_resources_for_vertex_slice(
            warm,
            dst_gpu_addr,
            pitch,
            rect_w,
            rect_h,
            geometry_label,
            vertices,
        )
    };
    let Some(draw) = draw else {
        crate::log!(
            "{} staging skipped reason=resource-layout size={}x{} pitch=0x{:X} geometry={}\n",
            submit_name,
            rect_w,
            rect_h,
            pitch,
            geometry_label,
        );
        return false;
    };

    let (base_pipeline, base_pipeline_note) = match backend_probe_mode {
        BackendProbeMode::PsSimd16 => (
            crate::intel::shader::triangle_pipeline_simd16(),
            crate::intel::shader::triangle_pipeline_simd16_note(),
        ),
        BackendProbeMode::PsEotOnly => (
            crate::intel::shader::triangle_pipeline_ps_eot(),
            crate::intel::shader::triangle_pipeline_ps_eot_note(),
        ),
        _ => (
            crate::intel::shader::triangle_pipeline(),
            crate::intel::shader::triangle_pipeline_note(),
        ),
    };
    let (pipeline, pipeline_note) = (base_pipeline, base_pipeline_note);
    log_render_buffer_layout(warm, Some(dst_gpu_addr));
    log_render_packet_encodings();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!(
            "{} staged rt=0x{:X} vb=0x{:X} state=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} status=awaiting-igc-or-spec-triangle-shaders vs_src={} ps_src={} note={}\n",
            submit_name,
            draw.rt_gpu_addr,
            draw.vertex_gpu_addr,
            draw.state_gpu_addr,
            draw.target_w,
            draw.target_h,
            draw.rt_pitch,
            draw.vertex_count,
            draw.vertex_stride,
            crate::intel::shader::TRIANGLE_VERTEX_SOURCE_PATH,
            crate::intel::shader::TRIANGLE_FRAGMENT_SOURCE_PATH,
            pipeline_note
        );
        return false;
    }

    intel_render_verbose_log!(
        "{} ps-meta dispatch={:?} grf_start={} grf_used={} ksp_off=0x{:X} size={} header_only={} geometry={} backend={} postdraw_sync={} note={}\n",
        submit_name,
        pipeline.ps.meta.kernel.dispatch_mode,
        pipeline.ps.meta.kernel.grf_start_register,
        pipeline.ps.meta.kernel.grf_used,
        pipeline.ps.meta.kernel.ksp_offset_bytes,
        pipeline.ps.meta.kernel.code_size_bytes,
        (pipeline.ps.meta.num_varying_inputs == 0
            && pipeline.ps.meta.kernel.push_constant_bytes == 0) as u8,
        geometry_label,
        backend_probe_mode.label(),
        post_draw_sync_variant.label(),
        pipeline_note
    );
    let sf_viewport_transform = !batch_mode.screen_space_raster();
    let coverage_contract = if resident_mesh.is_some() {
        "kernel-font-service-resident-indexed-clip-space"
    } else if gpu_mesh.is_some() {
        "gpgpu-generated-full-text-outline-stroke-clip-space"
    } else if sf_viewport_transform {
        "font-lyon-clip-field-viewport-transform"
    } else {
        "font-lyon-clip-field-screen-space"
    };
    let unique_vertex_count = resident_mesh
        .map(|mesh| mesh.vertex_count as usize)
        .or_else(|| gpu_mesh.map(|mesh| mesh.vertex_count as usize))
        .unwrap_or(vertices.len());
    if let Some(mesh) = resident_mesh {
        intel_render_focus_log!(
            "{} fragment-candidate-shape accepted=1 geometry={} producer=kernel-font-service authority=borrowed-gpu-resident topology=trilist indexed=1 unique_vertices={} draw_vertices={} triangles={} sf_viewport_transform={} vb_gpu=0x{:X} ib_gpu=0x{:X} resident_bytes=0x{:X} target={}x{} coverage_contract={} cpu_geometry_copy=0 does_not_prove=raster_samples_or_ps\n",
            submit_name,
            geometry_label,
            mesh.vertex_count,
            draw.vertex_count,
            draw.vertex_count / 3,
            sf_viewport_transform as u8,
            mesh.vertex_gpu_addr,
            mesh.index_gpu_addr,
            mesh.storage_bytes,
            draw.target_w,
            draw.target_h,
            coverage_contract,
        );
    } else if let Some(mesh) = gpu_mesh {
        intel_render_focus_log!(
            "{} fragment-candidate-shape accepted=1 geometry={} producer=gpgpu topology=trilist indexed=1 unique_vertices={} draw_vertices={} triangles={} sf_viewport_transform={} bounds=[{:.2},{:.2}..{:.2},{:.2}] target={}x{} coverage_contract={} cpu_vertex_readback=0 does_not_prove=raster_samples_or_ps\n",
            submit_name,
            geometry_label,
            mesh.vertex_count,
            draw.vertex_count,
            draw.vertex_count / 3,
            sf_viewport_transform as u8,
            mesh.min_x,
            mesh.min_y,
            mesh.max_x,
            mesh.max_y,
            draw.target_w,
            draw.target_h,
            coverage_contract,
        );
    } else {
        let first_triangle_indices = indices
            .map(|indices| {
                [
                    indices[0] as usize,
                    indices[1] as usize,
                    indices[2] as usize,
                ]
            })
            .unwrap_or([0, 1, 2]);
        let first_triangle = [
            vertices[first_triangle_indices[0]],
            vertices[first_triangle_indices[1]],
            vertices[first_triangle_indices[2]],
        ];
        intel_render_focus_log!(
            "{} fragment-candidate-shape accepted=1 geometry={} topology=trilist indexed={} unique_vertices={} draw_vertices={} triangles={} sf_viewport_transform={} first_triangle=v0[{:.3},{:.3},{:.3}] v1[{:.3},{:.3},{:.3}] v2[{:.3},{:.3},{:.3}] target={}x{} coverage_contract={} does_not_prove=raster_samples_or_ps\n",
            submit_name,
            geometry_label,
            draw.index_buffer.is_some() as u8,
            unique_vertex_count,
            draw.vertex_count,
            draw.vertex_count / 3,
            sf_viewport_transform as u8,
            first_triangle[0][0],
            first_triangle[0][1],
            first_triangle[0][2],
            first_triangle[1][0],
            first_triangle[1][1],
            first_triangle[1][2],
            first_triangle[2][0],
            first_triangle[2][1],
            first_triangle[2][2],
            draw.target_w,
            draw.target_h,
            coverage_contract,
        );
    }

    let programmed_vs_urb_output_length = front_end_contract
        .vs_urb_output_length_override
        .or(TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE)
        .unwrap_or(pipeline.vs.meta.urb_entry_output_length);
    intel_render_verbose_log!(
        "{} contract variant={} baked_vs_urb_out_len={} programmed_vs_urb_out_len={} sbe[read_offset={} read_length={} force_offset={} force_length={} num_sf_attrs={}]\n",
        submit_name,
        front_end_contract.label,
        pipeline.vs.meta.urb_entry_output_length,
        programmed_vs_urb_output_length,
        front_end_contract.sbe_read_offset,
        front_end_contract.sbe_read_length,
        front_end_contract.force_sbe_read_offset as u8,
        front_end_contract.force_sbe_read_length as u8,
        pipeline.ps.meta.num_varying_inputs,
    );

    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline, draw_rgba) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "{} staging skipped reason=shader-layout-error detail={} note={}\n",
                submit_name,
                reason,
                pipeline_note
            );
            return false;
        }
    };
    log_uploaded_triangle_shader_verification(
        warm,
        pipeline,
        shader_layout,
        submit_name,
        draw_rgba,
    );

    intel_render_verbose_log!(
        "{} staged rt=0x{:X} vb=0x{:X} state=0x{:X} used_end=0x{:X} state_off=0x{:X} state_region=0x{:X} free=0x{:X} size={}x{} pitch=0x{:X} indexed={} unique_vertices={} draw_vertices={} stride={} status=pipeline-ready vs_bytes={} vs_off=0x{:X} vs_gpu=0x{:X} vs_ksp_off=0x{:X} vs_ksp=0x{:X} ps_bytes={} ps_off=0x{:X} ps_gpu=0x{:X} ps_ksp_off=0x{:X} ps_ksp=0x{:X} varyings={} ps_dispatch={:?}\n",
        submit_name,
        draw.rt_gpu_addr,
        draw.vertex_gpu_addr,
        draw.state_gpu_addr,
        shader_layout.used_bytes,
        shader_layout.state_region_offset_bytes,
        shader_layout.state_region_gpu_addr,
        warm.draw_state_len
            .saturating_sub(shader_layout.state_region_offset_bytes as usize),
        draw.target_w,
        draw.target_h,
        draw.rt_pitch,
        draw.index_buffer.is_some() as u8,
        unique_vertex_count,
        draw.vertex_count,
        draw.vertex_stride,
        shader_layout.vs.code_size_bytes,
        shader_layout.vs.code_offset_bytes,
        shader_layout.vs.code_gpu_addr,
        shader_layout.vs.ksp_offset_bytes,
        shader_layout.vs.ksp_gpu_addr,
        shader_layout.ps.code_size_bytes,
        shader_layout.ps.code_offset_bytes,
        shader_layout.ps.code_gpu_addr,
        shader_layout.ps.ksp_offset_bytes,
        shader_layout.ps.ksp_gpu_addr,
        pipeline.ps.meta.num_varying_inputs,
        pipeline.ps.meta.kernel.dispatch_mode
    );

    let probe_state = match write_triangle_probe_state(
        warm,
        draw,
        shader_layout,
        blend_mode,
        backend_probe_mode,
        viewport_translation_px,
    ) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "{} staging skipped reason=probe-state-error detail={}\n",
                submit_name,
                reason
            );
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_triangle_probe_batch(
        submit_name,
        batch,
        warm,
        draw,
        blend_mode,
        depth_config,
        pipeline,
        shader_layout,
        probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        batch_mode,
        streamout_experiment,
        front_end_contract,
        viewport_translation_px,
        backend_probe_mode,
        post_draw_sync_variant,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "{} staging skipped reason=probe-batch-error detail={}\n",
                submit_name,
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    intel_render_verbose_log!(
        "{} batch-ready bytes=0x{:X} bt_off=0x{:X} samp_off=0x{:X} blend_off=0x{:X} cc_state_off=0x{:X} cc_vp_off=0x{:X} sf_vp_off=0x{:X} geometry={} backend={}\n",
        submit_name,
        batch_tail_bytes,
        probe_state.binding_table_offset_bytes,
        probe_state.sampler_state_offset_bytes,
        probe_state.blend_state_offset_bytes,
        probe_state.color_calc_state_offset_bytes,
        probe_state.cc_viewport_offset_bytes,
        probe_state.sf_clip_viewport_offset_bytes,
        geometry_label,
        backend_probe_mode.label(),
    );
    intel_render_verbose_log!("{} blend-probe={}\n", submit_name, blend_mode.label());
    log_triangle_probe_state(warm, shader_layout, probe_state);

    let scratch_rt_before = if should_capture_scratch_rt_proof(submit_name) {
        let scratch_surface_bytes = (draw.rt_pitch as usize)
            .saturating_mul(draw.target_h as usize)
            .min(warm.streamout_len);
        crate::intel::dma_flush(warm.streamout_virt, scratch_surface_bytes);
        let center_x = draw.target_w / 2;
        let center_y = draw.target_h / 2;
        let center_offset = center_y
            .saturating_mul(draw.rt_pitch)
            .saturating_add(center_x.saturating_mul(4)) as usize;
        let post_offset =
            center_offset.saturating_add(if center_x + 1 < draw.target_w { 4 } else { 0 });
        let read_scratch_dword = |byte_offset: usize| -> u32 {
            if byte_offset.saturating_add(core::mem::size_of::<u32>()) > warm.streamout_len {
                return 0;
            }
            unsafe {
                let ptr = (warm.streamout_virt as *const u8).add(byte_offset) as *const u32;
                core::ptr::read_volatile(ptr)
            }
        };
        Some((
            read_scratch_dword(0),
            read_scratch_dword(center_offset),
            read_scratch_dword(post_offset),
            center_offset,
            post_offset,
        ))
    } else {
        None
    };
    let scratch_stats_before = if scratch_rt_before.is_some() {
        Some(capture_triangle_stage_stats(dev))
    } else {
        None
    };

    let (completion_value, completion_slot, completion_kind) = if matches!(
        submit_name,
        "draw3d-scene" | "font-tessel-3d-once" | "font-outline-gpu-mesh-3d" | "font-resident-3d"
    ) && post_draw_sync_variant
        == PostDrawSyncVariant::HeavyAll
    {
        (
            RCS_EXEC_RESULT_DRAW_POST3D,
            RESULT_SLOT_POST3D_LIGHT_PIPE_CONTROL_LO_DWORD,
            "full-cache-drain-then-cs-stalled-postsync-rt-flush",
        )
    } else {
        (RCS_EXEC_RESULT_DONE, RESULT_SLOT_FINAL_DWORD, "mi-tail-store")
    };
    let completed =
        submit_warm_render_batch(dev, warm, completion_value, completion_slot, submit_name);
    if let (
        Some((scratch_before, center_before, post_before, center_offset, post_offset)),
        Some(_stats_before),
    ) = (scratch_rt_before, scratch_stats_before)
    {
        let scratch_surface_bytes = (draw.rt_pitch as usize)
            .saturating_mul(draw.target_h as usize)
            .min(warm.streamout_len);
        crate::intel::dma_flush(warm.streamout_virt, scratch_surface_bytes);
        let read_scratch_dword = |byte_offset: usize| -> u32 {
            if byte_offset.saturating_add(core::mem::size_of::<u32>()) > warm.streamout_len {
                return 0;
            }
            unsafe {
                let ptr = (warm.streamout_virt as *const u8).add(byte_offset) as *const u32;
                core::ptr::read_volatile(ptr)
            }
        };
        let scratch_after = read_scratch_dword(0);
        let center_after = read_scratch_dword(center_offset);
        let post_after = read_scratch_dword(post_offset);
        let target_width = draw.target_w as usize;
        let target_height = draw.target_h as usize;
        let pixel_count = target_width.saturating_mul(target_height);
        let mut changed_pixels = 0usize;
        let mut first_changed_pixel = None;
        for y in 0..target_height {
            let row_offset = y.saturating_mul(draw.rt_pitch as usize);
            for x in 0..target_width {
                let byte_offset = row_offset.saturating_add(x.saturating_mul(4));
                if byte_offset.saturating_add(4) > scratch_surface_bytes {
                    break;
                }
                if read_scratch_dword(byte_offset) != 0xDEAD_BEEF {
                    changed_pixels = changed_pixels.saturating_add(1);
                    if first_changed_pixel.is_none() {
                        first_changed_pixel =
                            Some(y.saturating_mul(target_width).saturating_add(x));
                    }
                }
            }
        }
        let delta = capture_triangle_stage_stats(dev);
        let ps_counter_accept =
            delta.ps_invocations > 0 || delta.cps_invocations > 0 || delta.ps_depth > 0;
        let rt_changed = changed_pixels != 0;
        let accepted = ps_counter_accept || rt_changed;
        record_fragment_boundary_probe(true, accepted);
        intel_render_focus_log!(
            "{} scratch-rt-fragment-proof accepted={} completed={} completion={} rt_gpu=0x{:X} size={}x{} pitch=0x{:X} changed_pixels={} first_changed_pixel={} before=0x{:08X} after=0x{:08X} center_before=0x{:08X} center_after=0x{:08X} post_before=0x{:08X} post_after=0x{:08X} ia_vtx_delta={} ia_prim_delta={} vs_delta={} cl_delta={} cl_prim_delta={} ps_delta={} cps_delta={} ps_depth_delta={} source={} does_not_prove=display_scanout\n",
            submit_name,
            accepted as u8,
            completed as u8,
            completion_kind,
            draw.rt_gpu_addr,
            draw.target_w,
            draw.target_h,
            draw.rt_pitch,
            changed_pixels,
            first_changed_pixel.map(|index| index as i32).unwrap_or(-1),
            scratch_before,
            scratch_after,
            center_before,
            center_after,
            post_before,
            post_after,
            delta.ia_vertices,
            delta.ia_primitives,
            delta.vs_invocations,
            delta.cl_invocations,
            delta.cl_primitives,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
            if resident_mesh.is_some() {
                "kernel-font-service-resident-indexed"
            } else if gpu_mesh.is_some() {
                "gpgpu-generated-indexed-stroke"
            } else {
                "font-path-fill-triangle"
            },
        );
        if matches!(
            submit_name,
            "font-tessel-3d-once" | "font-outline-gpu-mesh-3d" | "font-resident-3d"
        ) && completed
            && changed_pixels != 0
        {
            // Read back the complete target once. It was seeded with a constant
            // poison value, so full-size before/after vectors are redundant.
            // Reuse the caller's allocation when this is a transient stamp.
            let target_bytes = pixel_count.saturating_mul(4);
            let mut visible_rgba = readback
                .as_deref_mut()
                .and_then(Option::take)
                .map(|previous| previous.pixels)
                .unwrap_or_default();
            visible_rgba.clear();
            visible_rgba.reserve_exact(target_bytes);
            for y in 0..target_height {
                let row_offset = y.saturating_mul(draw.rt_pitch as usize);
                for x in 0..target_width {
                    let after = read_scratch_dword(row_offset.saturating_add(x.saturating_mul(4)));
                    if after == 0xDEAD_BEEF {
                        visible_rgba.extend_from_slice(&[0, 0, 0, 0]);
                    } else {
                        visible_rgba.extend_from_slice(&after.to_le_bytes());
                    }
                }
            }
            let display_width = target_width;
            let display_height = target_height;
            let display_pitch = display_width.saturating_mul(4);
            let captured = readback.is_some();
            let presented = if let Some(output) = readback.as_deref_mut() {
                *output = Some(FontRenderTargetReadback {
                    width: display_width as u32,
                    height: display_height as u32,
                    pixels: visible_rgba,
                });
                false
            } else {
                crate::intel::display::present_rgba_overlay_at(
                    &visible_rgba,
                    display_width as u32,
                    display_height as u32,
                    display_pitch,
                    96,
                    96,
                    true,
                    "font-tessel-render-target",
                )
            };
            intel_render_focus_log!(
                "{} visible-stamp presented={} captured={} pos={} source_size={}x{} display_size={}x{} scale=1 native_1to1=1 changed_pixels={} readback_buffers=1 source=whole-linear-rgba8-readback\n",
                submit_name,
                presented as u8,
                captured as u8,
                if captured { "deferred-grid" } else { "96x96" },
                draw.target_w,
                draw.target_h,
                display_width,
                display_height,
                changed_pixels,
            );
        }
    }
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, submit_name);
    }
    completed
}

fn submit_result_store_probe(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let Ok(batch_tail_bytes) =
        encode_result_store_probe_batch(batch, GPU_VA_RESULT_BASE, RCS_EXEC_RESULT_MI_PROBE_DONE)
    else {
        crate::log!("mi-store-probe batch build failed\n");
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_MI_PROBE_DONE,
        RESULT_SLOT_PRE3D_DWORD,
        "mi-store-probe",
    )
}

fn submit_3d_no_draw_probe(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
        core::ptr::write_volatile((warm.result_virt as *mut u32).add(1), 0xC0DE_7700);
        core::ptr::write_volatile((warm.result_virt as *mut u32).add(2), 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let Ok(batch_tail_bytes) = encode_3d_no_draw_probe_batch(
        batch,
        warm,
        GPU_VA_RESULT_BASE + (RESULT_SLOT_POST3D_DWORD as u64) * 4,
        RCS_EXEC_RESULT_3D_NO_DRAW_DONE,
        None,
    ) else {
        crate::log!("3d-no-draw-probe batch build failed\n");
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_3D_NO_DRAW_DONE,
        RESULT_SLOT_POST3D_DWORD,
        "3d-no-draw",
    )
}
