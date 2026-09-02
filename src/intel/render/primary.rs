use alloc::vec::Vec;

pub(crate) struct RenderJokerResult {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) variant: &'static str,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) submit_name: &'static str,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) target: &'static str,
    pub(crate) completed: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) vs_counter: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) ps_state_marker: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) raster_packet: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) clip_counter: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) ps_observed: bool,
}

/// CPU-visible copy of one completed offscreen font render target. Diagnostic
/// consumers may compare the independently colored results, but visibility
/// requires an ordinary UI4 frame/window producer.
pub(crate) struct FontRenderTargetReadback {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) const fn transient_font_mesh_upload_capacity_bytes() -> usize {
    WARM_VERTEX_BYTES
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) const fn transient_font_mesh_refinement_budget_bytes() -> usize {
    FONT_MESH_REFINEMENT_BUDGET_BYTES
}

/// Submit the already-tessellated font mesh at a native pixel scale.
///
/// Scaling is performed by the 3D viewport after tessellation: the mesh and
/// index topology remain unchanged, and presentation remains a 1:1 copy.
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
        || target_width as usize > RESIDENT_SCENE_TARGET_WIDTH
        || target_height as usize > RESIDENT_SCENE_TARGET_HEIGHT
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    pub(crate) sampled_texture: Option<&'a ResidentSampledTexture>,
    pub(crate) fragment_contract: ResidentSceneFragmentContract,
    /// Per-draw translation applied by the fixed-function viewport transform.
    /// Resident vertex and index storage is not rewritten or re-uploaded.
    pub(crate) viewport_translation_px: [f32; 2],
    pub(crate) topology: ResidentScenePrimitiveTopology,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResidentScenePrimitiveTopology {
    PointList,
    LineList,
    /// Intel's native `3DPRIM_LINELIST_ADJ`; four VF vertices form one line
    /// object with its two adjacent-only neighbours.
    LineListAdj,
    /// A source `LINE_LOOP`; its retained index plan includes the explicit
    /// final-to-first edge and is emitted as a hardware line strip.
    LineLoop,
    LineStrip,
    /// Intel's native `3DPRIM_LINESTRIP_ADJ`; its endpoint vertices are
    /// adjacent-only data and the interior forms the visible line strip.
    LineStripAdj,
    TriangleList,
    /// Intel's native `3DPRIM_TRILIST_ADJ`; six VF vertices form one triangle
    /// and its three edge neighbours.
    TriangleListAdj,
    TriangleStrip,
    /// Intel's native `3DPRIM_TRISTRIP_ADJ`; even VF vertices form the strip
    /// and odd vertices carry adjacency-only data.
    TriangleStripAdj,
    TriangleFan,
    /// Intel's native `3DPRIM_QUADLIST`; four VF vertices form one polygon.
    QuadList,
    /// Intel's native `3DPRIM_QUADSTRIP`; its VF input is converted to the
    /// downstream polygon representation by the hardware front end.
    QuadStrip,
    /// Intel's native screen-space `3DPRIM_RECTLIST`; each three vertices
    /// define a rectangle and the fourth corner is implied by hardware.
    RectList,
}

impl ResidentScenePrimitiveTopology {
    pub(crate) const fn requires_adjacency_geometry_shader(self) -> bool {
        matches!(
            self,
            Self::LineListAdj | Self::LineStripAdj | Self::TriangleListAdj | Self::TriangleStripAdj
        )
    }

    pub(crate) const fn accepts_index_count(self, count: usize) -> bool {
        match self {
            Self::PointList => count >= 1,
            Self::LineList => count >= 2 && count.is_multiple_of(2),
            Self::LineListAdj => count >= 4 && count.is_multiple_of(4),
            Self::LineLoop | Self::LineStrip => count >= 2,
            Self::LineStripAdj => count >= 4,
            Self::TriangleList => count >= 3 && count.is_multiple_of(3),
            Self::TriangleListAdj => count >= 6 && count.is_multiple_of(6),
            Self::TriangleStrip | Self::TriangleFan => count >= 3,
            Self::TriangleStripAdj => count >= 6 && count.is_multiple_of(2),
            Self::QuadList => count >= 4 && count.is_multiple_of(4),
            Self::QuadStrip => count >= 4 && count.is_multiple_of(2),
            Self::RectList => count >= 3 && count.is_multiple_of(3),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResidentSceneFragmentContract {
    ConstantRgba,
}

#[derive(Copy, Clone)]
struct ResidentSceneBatchState {
    phys: u64,
    virt: *mut u8,
}

unsafe impl Send for ResidentSceneBatchState {}

static RESIDENT_SCENE_BATCH_STATE: Mutex<Option<ResidentSceneBatchState>> = Mutex::new(None);
static RESIDENT_SCENE_BATCH_PATH_LOGGED: AtomicBool = AtomicBool::new(false);
static RESIDENT_CHURN_FORWARD_GPU_NATIVE_PATH_LOGGED: AtomicBool = AtomicBool::new(false);
static PICASSO_RETAINED_TEXTURED_SUBMIT_LOGGED: AtomicBool = AtomicBool::new(false);
static PICASSO_RETAINED_TEXTURED_PATH_LOGGED: AtomicBool = AtomicBool::new(false);
static RESIDENT_CHURN_FORWARD_GPU_EXPANDED_PATH_LOGGED: AtomicBool = AtomicBool::new(false);
static RESIDENT_CHURN_FORWARD_CPU_PATH_LOGGED: AtomicBool = AtomicBool::new(false);
static RESIDENT_CHURN_NATIVE_FLUSHED_PACKET_LOGGED: AtomicBool = AtomicBool::new(false);

// The entry packet sits before either opening PIPE_CONTROL. Slot 14 proves
// secondary fetch; slot 15 proves both opening controls plus PIPELINE_SELECT.
const RESIDENT_SECONDARY_ENTRY_PREFIX_DWORDS: usize = 4;
const RESIDENT_SECONDARY_OPENING_PIPE_CONTROL_DWORDS: usize = 6;
const RESIDENT_SECONDARY_POST_OPENING_MARKER_DWORD: usize =
    RESIDENT_SECONDARY_OPENING_PIPE_CONTROL_DWORDS * 2 + 1;
const RESULT_SLOT_POST_OPENING_DWORD: usize = 15;
const RCS_EXEC_RESULT_DRAW_POST_OPENING: u32 = 0xC0DE_772B;
const RESULT_SLOT_SECONDARY_RETURN_DWORD: usize = 30;
const RCS_EXEC_RESULT_SECONDARY_RETURN_BASE: u32 = 0xC0DE_7800;

fn finish_resident_secondary_breadcrumbs(
    batch: &mut [u32],
    encoded_payload_bytes: usize,
    secondary_index: usize,
    result_ggtt_gpu: u64,
) -> Result<usize, &'static str> {
    if !encoded_payload_bytes.is_multiple_of(core::mem::size_of::<u32>()) {
        return Err("scene-frame-secondary-size");
    }
    let payload_dwords = encoded_payload_bytes / core::mem::size_of::<u32>();
    let payload_end = RESIDENT_SECONDARY_ENTRY_PREFIX_DWORDS
        .checked_add(payload_dwords)
        .ok_or("scene-frame-secondary-size")?;
    if payload_end > batch.len() {
        return Err("scene-frame-secondary-size");
    }

    let result_entry_gpu =
        result_ggtt_gpu + (RESULT_SLOT_BATCH_ENTRY_DWORD * core::mem::size_of::<u32>()) as u64;
    let result_post_opening_gpu =
        result_ggtt_gpu + (RESULT_SLOT_POST_OPENING_DWORD * core::mem::size_of::<u32>()) as u64;
    let payload = &mut batch[RESIDENT_SECONDARY_ENTRY_PREFIX_DWORDS..payload_end];
    let marker = RESIDENT_SECONDARY_POST_OPENING_MARKER_DWORD;
    if payload.len() < marker + RESIDENT_SECONDARY_ENTRY_PREFIX_DWORDS
        || payload[0] != (PIPE_CONTROL_CMD | PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER)
        || payload[RESIDENT_SECONDARY_OPENING_PIPE_CONTROL_DWORDS] != PIPE_CONTROL_CMD
        || payload[marker - 1] != PIPELINE_SELECT_3D
        || payload[marker] != MI_STORE_DATA_IMM_GGTT_DW1
        || payload[marker + 1] != result_entry_gpu as u32
        || payload[marker + 2] != (result_entry_gpu >> 32) as u32
        || payload[marker + 3] != RCS_EXEC_RESULT_DRAW_BATCH_ENTRY
    {
        return Err("scene-frame-secondary-opening-layout");
    }

    payload[marker + 1] = result_post_opening_gpu as u32;
    payload[marker + 2] = (result_post_opening_gpu >> 32) as u32;
    payload[marker + 3] =
        resident_secondary_marker(RCS_EXEC_RESULT_DRAW_POST_OPENING, secondary_index)?;

    batch[0] = MI_STORE_DATA_IMM_GGTT_DW1;
    batch[1] = result_entry_gpu as u32;
    batch[2] = (result_entry_gpu >> 32) as u32;
    batch[3] = resident_secondary_marker(RCS_EXEC_RESULT_DRAW_BATCH_ENTRY, secondary_index)?;

    encoded_payload_bytes
        .checked_add(RESIDENT_SECONDARY_ENTRY_PREFIX_DWORDS * core::mem::size_of::<u32>())
        .ok_or("scene-frame-secondary-size")
}

// Retained secondaries intentionally share the same result slots. Encode the
// current secondary starting at bit 8 so a CAT leaves an exact parser frontier
// instead of an ambiguous marker from the preceding empty draw.
fn resident_secondary_marker(base: u32, secondary_index: usize) -> Result<u32, &'static str> {
    let index = u32::try_from(secondary_index).map_err(|_| "scene-frame-secondary-index")?;
    if secondary_index > RESIDENT_SCENE_MAX_DRAWS {
        return Err("scene-frame-secondary-index");
    }
    base.checked_add(index << 8)
        .ok_or("scene-frame-secondary-index")
}

fn log_resident_churn_flushed_binding_packets(
    batch: &[u32],
    bytes: usize,
    batch_gpu: u64,
    secondary_index: usize,
) {
    // ACTHD 0x0180D614 resolves into retained secondary 3.  Each secondary
    // owns a different state slot, so sample that exact command stream rather
    // than consuming the one-shot on the preceding (empty) native group.
    if secondary_index != 3 {
        return;
    }
    if RESIDENT_CHURN_NATIVE_FLUSHED_PACKET_LOGGED.swap(true, Ordering::AcqRel) {
        return;
    }
    let dwords = bytes
        .checked_div(core::mem::size_of::<u32>())
        .unwrap_or(0)
        .min(batch.len());
    let submitted = &batch[..dwords];
    let sba = submitted
        .iter()
        .position(|dw| *dw == STATE_BASE_ADDRESS_CMD);
    let pool = submitted
        .iter()
        .position(|dw| *dw == CMD_3DSTATE_BINDING_TABLE_POOL_ALLOC);
    let vs_count = submitted
        .iter()
        .filter(|dw| **dw == CMD_3DSTATE_BINDING_TABLE_POINTERS_VS)
        .count();
    let vs = submitted
        .iter()
        .rposition(|dw| *dw == CMD_3DSTATE_BINDING_TABLE_POINTERS_VS);
    crate::log_info!(
        target: "gpgpu";
        "helio-churn: flushed-native-secondary secondary={} batch_gpu=0x{:X} bytes=0x{:X} sba_dw={:?} pool_dw={:?} vs_bt_dw={:?} vs_bt_count={}\n",
        secondary_index,
        batch_gpu,
        bytes,
        sba,
        pool,
        vs,
        vs_count,
    );
    if let Some(offset) = sba
        && let Some(packet) = submitted.get(offset..offset.saturating_add(22))
    {
        crate::log_info!(
            target: "gpgpu";
            "helio-churn: flushed-sba offset={} dwords={:X?}\n",
            offset,
            packet,
        );
    }
    if let Some(offset) = pool
        && let Some(packet) = submitted.get(offset..offset.saturating_add(4))
    {
        crate::log_info!(
            target: "gpgpu";
            "helio-churn: flushed-binding-pool offset={} dwords={:X?}\n",
            offset,
            packet,
        );
    }
    if let Some(offset) = vs
        && let Some(packet) = submitted.get(offset..offset.saturating_add(2))
    {
        crate::log_info!(
            target: "gpgpu";
            "helio-churn: flushed-vs-binding-pointer offset={} dwords={:X?}\n",
            offset,
            packet,
        );
    }
}

fn resident_scene_batch_state(
    warm: RenderWarmState,
) -> Result<ResidentSceneBatchState, &'static str> {
    let mut resident = RESIDENT_SCENE_BATCH_STATE.lock();
    if let Some(state) = *resident {
        return Ok(state);
    }
    let Some((phys, virt)) =
        crate::dma::alloc(RESIDENT_SCENE_STATE_BYTES, crate::intel::WARM_ALIGN)
    else {
        return Err("scene-frame-state-alloc");
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, RESIDENT_SCENE_STATE_BYTES);
    }
    crate::intel::dma_flush(virt, RESIDENT_SCENE_STATE_BYTES);
    if !map_render_ppgtt_range(GPU_VA_RESIDENT_SCENE_STATE_BASE, phys, RESIDENT_SCENE_STATE_BYTES) {
        crate::dma::dealloc(virt, RESIDENT_SCENE_STATE_BYTES);
        return Err("scene-frame-state-map");
    }
    let state = ResidentSceneBatchState { phys, virt };
    *resident = Some(state);
    crate::log_info!(
        target: "render";
        "resident-scene: resident scene batch state online gpu=0x{:X} bytes=0x{:X} slots={} warm_batch_bytes=0x{:X}\n",
        GPU_VA_RESIDENT_SCENE_STATE_BASE,
        RESIDENT_SCENE_STATE_BYTES,
        RESIDENT_SCENE_MAX_DRAWS + 1,
        warm.batch_len,
    );
    Ok(state)
}

fn resident_scene_batch_state_for_carrier(
    warm: RenderWarmState,
    carrier: Option<PicassoCarrierLease>,
) -> Result<ResidentSceneBatchState, &'static str> {
    let Some(lease) = carrier else {
        return resident_scene_batch_state(warm);
    };
    let physical = crate::gpu::physical::physical_device().ok_or("picasso-physical-gpu")?;
    let storage =
        prepare_picasso_render1_scene_storage(lease, physical).ok_or("picasso-scene-storage")?;
    crate::log_trace!(target: "render";
        "picasso-carrier scene-state carrier={} gpu=0x{:X} phys=0x{:X} bytes=0x{:X} warm_batch=0x{:X}\n",
        lease.carrier().label(), GPU_VA_RESIDENT_SCENE_STATE_BASE, storage.state_phys,
        RESIDENT_SCENE_STATE_BYTES, warm.batch_len,
    );
    Ok(ResidentSceneBatchState {
        phys: storage.state_phys,
        virt: storage.state_virt,
    })
}

fn resident_scene_state_warm(
    state: ResidentSceneBatchState,
    warm: RenderWarmState,
    slot: usize,
) -> Result<(RenderWarmState, u64), &'static str> {
    if slot > RESIDENT_SCENE_MAX_DRAWS {
        return Err("scene-frame-state-slot");
    }
    let offset = slot
        .checked_mul(RESIDENT_SCENE_STATE_SLOT_BYTES)
        .ok_or("scene-frame-state-slot")?;
    Ok((
        RenderWarmState {
            draw_state_phys: state.phys + offset as u64,
            draw_state_virt: unsafe { state.virt.add(offset) },
            draw_state_len: RESIDENT_SCENE_STATE_SLOT_BYTES,
            ..warm
        },
        GPU_VA_RESIDENT_SCENE_STATE_BASE + offset as u64,
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ResidentSceneIncompleteStage {
    Geometry,
    Resolve,
    Coverage,
    PresentCopy,
    Unknown,
}

impl ResidentSceneIncompleteStage {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    const fn error(self) -> &'static str {
        match self {
            Self::Geometry => "resident-scene-geometry-incomplete",
            Self::Resolve => "resident-scene-resolve-incomplete",
            Self::Coverage => "resident-scene-coverage-incomplete",
            Self::PresentCopy => "resident-scene-present-copy-incomplete",
            Self::Unknown => "resident-scene-incomplete-unknown-stage",
        }
    }
}

#[derive(Debug)]
pub(crate) struct ResidentSceneFrameResult {
    pub(crate) completed_draws: usize,
    pub(crate) requested_draws: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) changed_pixels: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) presented: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) width: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) resolve_us: u64,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) coverage_us: u64,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) present_copy_us: u64,
    pub(crate) present_copy_performed: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) coverage_submits: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) coverage_walkers: usize,
    pub(crate) rgba: Option<Vec<u8>>,
    /// True only when geometry, resolve, coverage, and any compatibility copy
    /// completed. This remains separate from `release_fence`: a caller that
    /// appends one final GPU writer may deliberately defer the release proof.
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) frame_complete: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    incomplete_stage: Option<ResidentSceneIncompleteStage>,
    /// Present only after the final GPU writer's cache release plus ordered
    /// post-sync retirement marker completed for the returned UI4 allocation.
    pub(crate) release_fence: Option<ResidentSceneReleaseFence>,
}

impl ResidentSceneFrameResult {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn completion_error(&self) -> Option<&'static str> {
        match self.incomplete_stage {
            Some(stage) => Some(stage.error()),
            None if !self.frame_complete => Some(ResidentSceneIncompleteStage::Unknown.error()),
            None => None,
        }
    }
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
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Readback,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    GpuSurface(crate::intel::gpgpu::GpgpuRgba8Surface),
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    GpuSurfaceDeferredRelease(crate::intel::gpgpu::GpgpuRgba8Surface),
    DirectGpuSurface(crate::intel::gpgpu::GpgpuRgba8Surface),
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    DirectGpuSurfaceDeferredRelease(crate::intel::gpgpu::GpgpuRgba8Surface),
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

fn resident_scene_direct_ui4_gpu_for_slot(slot: usize) -> Option<u64> {
    if slot >= RESIDENT_UI4_DIRECT_MAPPING_COUNT {
        return None;
    }
    let gpu = GPU_VA_RESIDENT_UI4_FRAME_BASE.checked_add(
        u64::try_from(slot)
            .ok()?
            .checked_mul(GPU_VA_RESIDENT_UI4_FRAME_STRIDE)?,
    )?;
    let end = gpu.checked_add(GPU_VA_RESIDENT_UI4_FRAME_STRIDE)?;
    (end <= GPU_VA_RESIDENT_UI4_FRAME_LIMIT).then_some(gpu)
}

fn resident_scene_direct_ui4_mapping_slot(
    mappings: &[Option<ResidentSceneDirectUi4Mapping>; RESIDENT_UI4_DIRECT_MAPPING_COUNT],
    phys: u64,
) -> Option<usize> {
    mappings
        .iter()
        .position(|mapping| mapping.is_some_and(|mapping| mapping.phys == phys))
}

fn resident_scene_direct_ui4_vacant_slot(
    mappings: &[Option<ResidentSceneDirectUi4Mapping>; RESIDENT_UI4_DIRECT_MAPPING_COUNT],
) -> Option<usize> {
    mappings.iter().position(Option::is_none)
}

static RESIDENT_SCENE_DEPTH: Mutex<Option<ResidentSceneDepthAllocation>> = Mutex::new(None);
static RESIDENT_SCENE_MSAA_COLOR: Mutex<Option<ResidentSceneDepthAllocation>> = Mutex::new(None);
static RESIDENT_SCENE_MSAA_DEPTH: Mutex<Option<ResidentSceneDepthAllocation>> = Mutex::new(None);
static RESIDENT_SCENE_DIRECT_UI4_TARGETS: Mutex<
    [Option<ResidentSceneDirectUi4Mapping>; RESIDENT_UI4_DIRECT_MAPPING_COUNT],
> = Mutex::new([None; RESIDENT_UI4_DIRECT_MAPPING_COUNT]);
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
    if !destination.is_valid() || destination.bytes as u64 > GPU_VA_RESIDENT_UI4_FRAME_STRIDE {
        return Err("resident-scene-direct-ui4-shape");
    }
    let mut mappings = RESIDENT_SCENE_DIRECT_UI4_TARGETS.lock();
    if let Some(slot) = resident_scene_direct_ui4_mapping_slot(&mappings, destination.phys) {
        let existing = mappings[slot].ok_or("resident-scene-direct-ui4-table")?;
        if existing.bytes != destination.bytes {
            return Err("resident-scene-direct-ui4-shape-changed");
        }
        return Ok(existing.gpu);
    }

    let Some(slot) = resident_scene_direct_ui4_vacant_slot(&mappings) else {
        return Err("resident-scene-direct-ui4-buffer-limit");
    };
    let gpu =
        resident_scene_direct_ui4_gpu_for_slot(slot).ok_or("resident-scene-direct-ui4-address")?;
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
        "resident-scene: acquired UI4 direct target render_slot={} render_slots={} render_gpu=0x{:X} phys=0x{:X} bytes=0x{:X} size={}x{} pitch={} ppgtt_pat=3 ppgtt_cache=uc leaf_readback=verified persistent_render_va=1 hot_remap=0\n",
        slot,
        RESIDENT_UI4_DIRECT_MAPPING_COUNT,
        gpu,
        destination.phys,
        destination.bytes,
        destination.width,
        destination.height,
        destination.pitch_bytes,
    );
    Ok(gpu)
}

/// Release one Render0-only alias after UI4 has proved that the owning frame
/// has no producer or presentation readers. The physical surface remains live
/// until this succeeds, so a failed PPGTT unmap cannot leave an alias pointing
/// at subsequently recycled DMA storage.
pub(crate) fn release_resident_scene_direct_ui4_target(phys: u64, bytes: usize) -> bool {
    let mut mappings = RESIDENT_SCENE_DIRECT_UI4_TARGETS.lock();
    let Some(slot) = resident_scene_direct_ui4_mapping_slot(&mappings, phys) else {
        return true;
    };
    let Some(mapping) = mappings[slot] else {
        return false;
    };
    if mapping.bytes != bytes {
        crate::log_warn!(
            target: "render";
            "resident-scene: refused UI4 direct target release render_slot={} render_gpu=0x{:X} phys=0x{:X} mapped_bytes=0x{:X} release_bytes=0x{:X} reason=shape-changed\n",
            slot,
            mapping.gpu,
            phys,
            mapping.bytes,
            bytes,
        );
        return false;
    }
    if !unmap_render_ppgtt_range(mapping.gpu, mapping.bytes) {
        crate::log_warn!(
            target: "render";
            "resident-scene: deferred UI4 direct target release render_slot={} render_gpu=0x{:X} phys=0x{:X} bytes=0x{:X} reason=ppgtt-unmap\n",
            slot,
            mapping.gpu,
            phys,
            bytes,
        );
        return false;
    }
    mappings[slot] = None;
    crate::log_info!(
        target: "render";
        "resident-scene: released UI4 direct target render_slot={} render_gpu=0x{:X} phys=0x{:X} bytes=0x{:X} lifecycle=ui4-frame-destroy slot_reusable=1 next_draw_tlb_invalidate=1\n",
        slot,
        mapping.gpu,
        phys,
        bytes,
    );
    true
}

#[cfg(test)]
mod resident_scene_direct_ui4_mapping_tests {
    use super::{
        GPU_VA_RESIDENT_UI4_FRAME_BASE, GPU_VA_RESIDENT_UI4_FRAME_LIMIT,
        GPU_VA_RESIDENT_UI4_FRAME_STRIDE, RESIDENT_UI4_DIRECT_MAPPING_COUNT,
        ResidentSceneDirectUi4Mapping, resident_scene_direct_ui4_gpu_for_slot,
        resident_scene_direct_ui4_mapping_slot, resident_scene_direct_ui4_vacant_slot,
    };

    #[test]
    fn render0_alias_arena_has_thirty_unique_full_slots() {
        assert_eq!(RESIDENT_UI4_DIRECT_MAPPING_COUNT, 30);
        for slot in 0..RESIDENT_UI4_DIRECT_MAPPING_COUNT {
            let gpu = resident_scene_direct_ui4_gpu_for_slot(slot).unwrap();
            assert_eq!(
                gpu,
                GPU_VA_RESIDENT_UI4_FRAME_BASE + slot as u64 * GPU_VA_RESIDENT_UI4_FRAME_STRIDE
            );
            assert!(gpu + GPU_VA_RESIDENT_UI4_FRAME_STRIDE <= GPU_VA_RESIDENT_UI4_FRAME_LIMIT);
            if slot != 0 {
                assert_eq!(
                    gpu - resident_scene_direct_ui4_gpu_for_slot(slot - 1).unwrap(),
                    GPU_VA_RESIDENT_UI4_FRAME_STRIDE
                );
            }
        }
        assert!(
            resident_scene_direct_ui4_gpu_for_slot(RESIDENT_UI4_DIRECT_MAPPING_COUNT).is_none()
        );
    }

    #[test]
    fn lifecycle_release_makes_the_exact_slot_reusable() {
        let mut mappings = [None; RESIDENT_UI4_DIRECT_MAPPING_COUNT];
        for (slot, entry) in mappings.iter_mut().enumerate() {
            *entry = Some(ResidentSceneDirectUi4Mapping {
                phys: 0x1000_0000 + slot as u64 * 0x0100_0000,
                bytes: 0x00E1_0000,
                gpu: resident_scene_direct_ui4_gpu_for_slot(slot).unwrap(),
            });
        }
        assert!(resident_scene_direct_ui4_vacant_slot(&mappings).is_none());

        let retired_slot = 7;
        let retired = mappings[retired_slot].unwrap();
        assert_eq!(
            resident_scene_direct_ui4_mapping_slot(&mappings, retired.phys),
            Some(retired_slot)
        );
        mappings[retired_slot] = None;
        assert_eq!(resident_scene_direct_ui4_vacant_slot(&mappings), Some(retired_slot));
        assert_eq!(resident_scene_direct_ui4_mapping_slot(&mappings, retired.phys), None);

        let replacement_phys = 0x5000_0000;
        mappings[retired_slot] = Some(ResidentSceneDirectUi4Mapping {
            phys: replacement_phys,
            bytes: retired.bytes,
            gpu: resident_scene_direct_ui4_gpu_for_slot(retired_slot).unwrap(),
        });
        assert_eq!(
            resident_scene_direct_ui4_mapping_slot(&mappings, replacement_phys),
            Some(retired_slot)
        );
    }
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
        return Err("resident-scene-msaa-depth-device");
    }
    let (pitch_bytes, aligned_sample_height, storage_bytes) =
        resident_scene_msaa_depth_layout(target_width, target_height)
            .ok_or("resident-scene-msaa-depth-shape")?;
    let _allocation = prepare_resident_scene_msaa_allocation(
        &RESIDENT_SCENE_MSAA_DEPTH,
        GPU_VA_RESIDENT_SCENE_MSAA_DEPTH_BASE,
        storage_bytes,
        "d32-depth",
    )?;
    Ok(TriangleDepthConfig {
        gpu_addr: GPU_VA_RESIDENT_SCENE_MSAA_DEPTH_BASE,
        pitch_bytes: u32::try_from(pitch_bytes).map_err(|_| "resident-scene-msaa-depth-shape")?,
        width: u32::try_from(target_width).map_err(|_| "resident-scene-msaa-depth-shape")?,
        height: u32::try_from(target_height).map_err(|_| "resident-scene-msaa-depth-shape")?,
        qpitch_rows_div4: u32::try_from(aligned_sample_height / 4)
            .map_err(|_| "resident-scene-msaa-depth-shape")?,
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
        return Err("resident-scene-depth-device");
    }
    let row_bytes = target_width
        .checked_mul(core::mem::size_of::<f32>())
        .ok_or("resident-scene-depth-shape")?;
    let pitch_bytes = crate::intel::align_up(row_bytes, RESIDENT_SCENE_DEPTH_TILE_WIDTH_BYTES)
        .ok_or("resident-scene-depth-shape")?;
    let aligned_height =
        crate::intel::align_up(target_height, RESIDENT_SCENE_DEPTH_TILE_HEIGHT_ROWS)
            .ok_or("resident-scene-depth-shape")?;
    let clear_bytes = pitch_bytes
        .checked_mul(aligned_height)
        .ok_or("resident-scene-depth-shape")?;
    if target_width == 0
        || target_height == 0
        || clear_bytes > RESIDENT_SCENE_DEPTH_BYTES
        || !clear_bytes.is_multiple_of(core::mem::size_of::<u32>())
    {
        return Err("resident-scene-depth-shape");
    }

    let allocation = {
        let mut resident = RESIDENT_SCENE_DEPTH.lock();
        if let Some(allocation) = *resident {
            allocation
        } else {
            let Some((storage_phys, storage_virt)) =
                crate::dma::alloc(RESIDENT_SCENE_DEPTH_BYTES, crate::intel::WARM_ALIGN)
            else {
                return Err("resident-scene-depth-alloc");
            };
            if !map_render_ppgtt_range(
                GPU_VA_RESIDENT_SCENE_DEPTH_BASE,
                storage_phys,
                RESIDENT_SCENE_DEPTH_BYTES,
            ) {
                crate::dma::dealloc(storage_virt, RESIDENT_SCENE_DEPTH_BYTES);
                return Err("resident-scene-depth-map");
            }
            let allocation = ResidentSceneDepthAllocation {
                storage_phys,
                storage_virt,
                storage_bytes: RESIDENT_SCENE_DEPTH_BYTES,
            };
            *resident = Some(allocation);
            crate::log_info!(
                target: "render";
                "resident-scene-depth: resident surface allocated phys=0x{:X} gpu=0x{:X} bytes=0x{:X} format=d32-float tiling={} max={}x{}\n",
                allocation.storage_phys,
                GPU_VA_RESIDENT_SCENE_DEPTH_BASE,
                allocation.storage_bytes,
                if device_is_gfx125(device_id) { "tile4" } else { "y0" },
                RESIDENT_SCENE_TARGET_WIDTH,
                RESIDENT_SCENE_TARGET_HEIGHT,
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
        return Err("resident-scene-depth-allocation");
    }

    Ok(TriangleDepthConfig {
        gpu_addr: GPU_VA_RESIDENT_SCENE_DEPTH_BASE,
        pitch_bytes: u32::try_from(pitch_bytes).map_err(|_| "resident-scene-depth-shape")?,
        width: u32::try_from(target_width).map_err(|_| "resident-scene-depth-shape")?,
        height: u32::try_from(target_height).map_err(|_| "resident-scene-depth-shape")?,
        qpitch_rows_div4: u32::try_from(aligned_height / 4)
            .map_err(|_| "resident-scene-depth-shape")?,
        write_enabled: false,
        compare_function: COMPARE_FUNCTION_LEQUAL,
    })
}

fn prepare_picasso_resident_scene_depth(
    lease: PicassoCarrierLease,
    device_id: u16,
    target_width: usize,
    target_height: usize,
) -> Result<TriangleDepthConfig, &'static str> {
    if !device_is_gfx12(device_id) {
        return Err("picasso-scene-depth-device");
    }
    let row_bytes = target_width
        .checked_mul(core::mem::size_of::<f32>())
        .ok_or("picasso-scene-depth-shape")?;
    let pitch_bytes = crate::intel::align_up(row_bytes, RESIDENT_SCENE_DEPTH_TILE_WIDTH_BYTES)
        .ok_or("picasso-scene-depth-shape")?;
    let aligned_height =
        crate::intel::align_up(target_height, RESIDENT_SCENE_DEPTH_TILE_HEIGHT_ROWS)
            .ok_or("picasso-scene-depth-shape")?;
    let clear_bytes = pitch_bytes
        .checked_mul(aligned_height)
        .ok_or("picasso-scene-depth-shape")?;
    if target_width == 0
        || target_height == 0
        || clear_bytes > RESIDENT_SCENE_DEPTH_BYTES
        || !clear_bytes.is_multiple_of(core::mem::size_of::<u32>())
    {
        return Err("picasso-scene-depth-shape");
    }
    let physical = crate::gpu::physical::physical_device().ok_or("picasso-physical-gpu")?;
    let storage =
        prepare_picasso_render1_scene_storage(lease, physical).ok_or("picasso-scene-storage")?;
    if storage.depth_virt.is_null() || storage.depth_bytes < clear_bytes {
        return Err("picasso-scene-depth-allocation");
    }
    Ok(TriangleDepthConfig {
        gpu_addr: GPU_VA_RESIDENT_SCENE_DEPTH_BASE,
        pitch_bytes: u32::try_from(pitch_bytes).map_err(|_| "picasso-scene-depth-shape")?,
        width: u32::try_from(target_width).map_err(|_| "picasso-scene-depth-shape")?,
        height: u32::try_from(target_height).map_err(|_| "picasso-scene-depth-shape")?,
        qpitch_rows_div4: u32::try_from(aligned_height / 4)
            .map_err(|_| "picasso-scene-depth-shape")?,
        write_enabled: false,
        compare_function: COMPARE_FUNCTION_LEQUAL,
    })
}

pub(crate) const fn resident_scene_target_dimensions() -> (usize, usize) {
    (RESIDENT_SCENE_TARGET_WIDTH, RESIDENT_SCENE_TARGET_HEIGHT)
}

/// UI4-sized 4x triangle scene followed by persistent analytical font masks.
/// Coverage is composited only after the MSAA resolve, preserving its R8 alpha
/// steps instead of treating the mask as additional fixed-function samples.
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

/// GridPaper's complete direct-render operation. Geometry targets the leased
/// UI4 allocation directly; analytical coverage and cursor rectangles append
/// their writes before one final scanout release is minted.
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn render_resident_triangle_scene_frame_premultiplied_with_coverage_and_rects_direct_to_surface(
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
        ResidentSceneRasterQuality::SingleSample,
        destination.width as usize,
        destination.height as usize,
        ResidentSceneFrameOutput::DirectGpuSurfaceDeferredRelease(destination),
    )?;
    if let Some(error) = result.completion_error() {
        return Err(error);
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
    result.coverage_us = result
        .coverage_us
        .saturating_add(crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000);
    result.release_fence = Some(resident_scene_release(destination));
    Ok(result)
}

/// Render an ordered immediate indexed scene directly into one UI4 surface.
/// The public immediate pipeline has no depth-state descriptor, so draw order
/// is authoritative and this path must not allocate or silently enable depth.
pub(crate) fn render_resident_indexed_scene_frame_premultiplied_direct_to_surface(
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
        false,
        ResidentSceneRasterQuality::SingleSample,
        destination.width as usize,
        destination.height as usize,
        ResidentSceneFrameOutput::DirectGpuSurface(destination),
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

/// Helio Churn rendered into UI4's leased linear RGBA surface in one scene
/// submission. Retained-transform frames can feed GPU-authored matrices and
/// compacted indices straight into the artifact-native VS; the GPU-expanded
/// Float3 stream remains an explicit fallback during physical admission.
pub(crate) fn render_resident_churn_forward_frame_direct_to_surface(
    resident: &ResidentChurnForward,
    clear_rgba: Option<[u8; 4]>,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    submit_resident_scene_capture_inner(
        &[],
        Some(resident),
        None,
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

/// Submit one retained GPU-transform scene together with ordinary resident
/// primitives. The retained object owns the compute/native-matrix secondary;
/// `static_draws` are encoded afterwards and cannot inherit its transform
/// bindings.
pub(crate) fn render_resident_retained_with_static_draws_direct_to_surface(
    resident: &ResidentChurnForward,
    sampled_material: Option<ResidentRetainedMaterial<'_>>,
    static_draws: &[ResidentSceneDraw<'_>],
    clear_rgba: Option<[u8; 4]>,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    diagnostic_logs: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    let carrier = resident
        .carrier()
        .ok_or("picasso-retained-carrier-missing")?;
    submit_resident_scene_capture_inner_for_carrier(
        static_draws,
        Some(resident),
        sampled_material,
        &[],
        clear_rgba,
        diagnostic_logs,
        false,
        true,
        ResidentSceneRasterQuality::SingleSample,
        destination.width as usize,
        destination.height as usize,
        ResidentSceneFrameOutput::DirectGpuSurface(destination),
        Some(carrier),
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
    sampled_texture: Option<&ResidentSampledTexture>,
    fragment_contract: ResidentSceneFragmentContract,
    viewport_translation_px: [f32; 2],
    topology: ResidentScenePrimitiveTopology,
    secondary_index: usize,
    result_ggtt_gpu: u64,
) -> Result<usize, &'static str> {
    draw.state_gpu_addr = state_gpu;
    if fragment_contract != ResidentSceneFragmentContract::ConstantRgba || sampled_texture.is_some()
    {
        return Err("scene-fragment-contract-texture-mismatch");
    }
    let pipeline = crate::intel::shader::triangle_pipeline_simd16();
    draw.sampled_texture = sampled_texture.map(|texture| TriangleSampledTextureBinding {
        gpu_addr: texture.storage.gpu_base(),
        width: texture.width,
        height: texture.height,
        pitch: texture.pitch,
        sampler_flags: texture.sampler_flags,
    });
    let shader_layout = upload_triangle_shader_pipeline_at(
        state_warm,
        pipeline,
        sampled_texture.is_none().then_some(rgba),
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
    let batch_offset = RESIDENT_SCENE_PRIMARY_BATCH_BYTES
        .checked_add(
            secondary_index
                .checked_mul(RESIDENT_SCENE_SECONDARY_BATCH_BYTES)
                .ok_or("scene-frame-batch-slot")?,
        )
        .ok_or("scene-frame-batch-slot")?;
    let batch_end = batch_offset
        .checked_add(RESIDENT_SCENE_SECONDARY_BATCH_BYTES)
        .ok_or("scene-frame-batch-slot")?;
    if batch_end > warm.batch_len {
        return Err("scene-frame-batch-capacity");
    }
    let batch = unsafe {
        core::slice::from_raw_parts_mut(
            warm.batch_virt.add(batch_offset) as *mut u32,
            RESIDENT_SCENE_SECONDARY_BATCH_BYTES / core::mem::size_of::<u32>(),
        )
    };
    batch.fill(0);
    let encoded_payload_bytes = encode_triangle_probe_batch(
        "resident-scene",
        &mut batch[RESIDENT_SECONDARY_ENTRY_PREFIX_DWORDS..],
        state_warm,
        draw,
        blend_mode,
        depth_config,
        pipeline,
        shader_layout,
        probe_state,
        result_ggtt_gpu,
        resident_secondary_marker(RCS_EXEC_RESULT_DRAW_PRE3D, secondary_index)?,
        resident_secondary_marker(RCS_EXEC_RESULT_DRAW_POST3D, secondary_index)?,
        resident_secondary_marker(RCS_EXEC_RESULT_DONE, secondary_index)?,
        match topology {
            ResidentScenePrimitiveTopology::PointList => TriangleBatchMode::PointDraw,
            ResidentScenePrimitiveTopology::LineList => TriangleBatchMode::LineDraw,
            ResidentScenePrimitiveTopology::LineListAdj => TriangleBatchMode::LineAdjDraw,
            ResidentScenePrimitiveTopology::LineLoop
            | ResidentScenePrimitiveTopology::LineStrip => TriangleBatchMode::LineStripDraw,
            ResidentScenePrimitiveTopology::LineStripAdj => TriangleBatchMode::LineStripAdjDraw,
            ResidentScenePrimitiveTopology::TriangleList => TriangleBatchMode::Draw,
            ResidentScenePrimitiveTopology::TriangleListAdj => TriangleBatchMode::TriangleAdjDraw,
            ResidentScenePrimitiveTopology::TriangleStrip => TriangleBatchMode::TriangleStripDraw,
            ResidentScenePrimitiveTopology::TriangleStripAdj => {
                TriangleBatchMode::TriangleStripAdjDraw
            }
            ResidentScenePrimitiveTopology::TriangleFan => TriangleBatchMode::TriangleFanDraw,
            ResidentScenePrimitiveTopology::QuadList => TriangleBatchMode::QuadListDraw,
            ResidentScenePrimitiveTopology::QuadStrip => TriangleBatchMode::QuadStripDraw,
            ResidentScenePrimitiveTopology::RectList => TriangleBatchMode::RectListDraw,
        },
        StreamoutProofExperiment::HeaderAndPositionSlots01,
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
        viewport_translation_px,
        BackendProbeMode::MesaLike,
        // All scene secondaries execute below one primary batch. They only
        // need command-stream ordering here; the primary emits the single
        // full render/depth/L3 release fence after the final secondary.
        PostDrawSyncVariant::LightCsNoPostSync,
    )?;
    let bytes = finish_resident_secondary_breadcrumbs(
        batch,
        encoded_payload_bytes,
        secondary_index,
        result_ggtt_gpu,
    )?;
    crate::intel::dma_flush(unsafe { warm.batch_virt.add(batch_offset) }, bytes);
    Ok(bytes)
}

fn stage_resident_churn_forward_secondary(
    warm: RenderWarmState,
    state_warm: RenderWarmState,
    state_gpu: u64,
    mut draw: TriangleDrawPrep,
    depth_config: TriangleDepthConfig,
    resident: &ResidentChurnForward,
    secondary_index: usize,
    result_ggtt_gpu: u64,
) -> Result<usize, &'static str> {
    draw.state_gpu_addr = state_gpu;
    let shader_layout =
        upload_triangle_shader_pipeline_at(state_warm, &resident.pipeline, None, state_gpu, false)?;
    let probe_state = write_triangle_probe_state_unflushed(
        state_warm,
        draw,
        shader_layout,
        TriangleBlendProbeMode::MesaZeroedState,
        BackendProbeMode::MesaLike,
        [0.0, 0.0],
    )?;
    crate::intel::dma_flush(state_warm.draw_state_virt, probe_state.used_bytes as usize);
    let batch_offset = RESIDENT_SCENE_PRIMARY_BATCH_BYTES
        .checked_add(
            secondary_index
                .checked_mul(RESIDENT_SCENE_SECONDARY_BATCH_BYTES)
                .ok_or("scene-frame-batch-slot")?,
        )
        .ok_or("scene-frame-batch-slot")?;
    let batch_end = batch_offset
        .checked_add(RESIDENT_SCENE_SECONDARY_BATCH_BYTES)
        .ok_or("scene-frame-batch-slot")?;
    if batch_end > warm.batch_len {
        return Err("scene-frame-batch-capacity");
    }
    let batch = unsafe {
        core::slice::from_raw_parts_mut(
            warm.batch_virt.add(batch_offset) as *mut u32,
            RESIDENT_SCENE_SECONDARY_BATCH_BYTES / core::mem::size_of::<u32>(),
        )
    };
    batch.fill(0);
    let encoded_payload_bytes = encode_triangle_probe_batch(
        "helio-churn-forward",
        &mut batch[RESIDENT_SECONDARY_ENTRY_PREFIX_DWORDS..],
        state_warm,
        draw,
        TriangleBlendProbeMode::MesaZeroedState,
        Some(depth_config),
        &resident.pipeline,
        shader_layout,
        probe_state,
        result_ggtt_gpu,
        resident_secondary_marker(RCS_EXEC_RESULT_DRAW_PRE3D, secondary_index)?,
        resident_secondary_marker(RCS_EXEC_RESULT_DRAW_POST3D, secondary_index)?,
        resident_secondary_marker(RCS_EXEC_RESULT_DONE, secondary_index)?,
        match resident.topology() {
            ResidentScenePrimitiveTopology::PointList => TriangleBatchMode::PointDraw,
            ResidentScenePrimitiveTopology::LineList => TriangleBatchMode::LineDraw,
            ResidentScenePrimitiveTopology::LineListAdj => TriangleBatchMode::LineAdjDraw,
            ResidentScenePrimitiveTopology::LineLoop
            | ResidentScenePrimitiveTopology::LineStrip => TriangleBatchMode::LineStripDraw,
            ResidentScenePrimitiveTopology::LineStripAdj => TriangleBatchMode::LineStripAdjDraw,
            ResidentScenePrimitiveTopology::TriangleList => TriangleBatchMode::Draw,
            ResidentScenePrimitiveTopology::TriangleListAdj => TriangleBatchMode::TriangleAdjDraw,
            ResidentScenePrimitiveTopology::TriangleStrip => TriangleBatchMode::TriangleStripDraw,
            ResidentScenePrimitiveTopology::TriangleStripAdj => {
                TriangleBatchMode::TriangleStripAdjDraw
            }
            ResidentScenePrimitiveTopology::TriangleFan => TriangleBatchMode::TriangleFanDraw,
            ResidentScenePrimitiveTopology::QuadList => TriangleBatchMode::QuadListDraw,
            ResidentScenePrimitiveTopology::QuadStrip => TriangleBatchMode::QuadStripDraw,
            ResidentScenePrimitiveTopology::RectList => TriangleBatchMode::RectListDraw,
        },
        StreamoutProofExperiment::HeaderAndPositionSlots01,
        resident.front_end_contract,
        [0.0, 0.0],
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::LightCsNoPostSync,
    )?;
    let bytes = finish_resident_secondary_breadcrumbs(
        batch,
        encoded_payload_bytes,
        secondary_index,
        result_ggtt_gpu,
    )?;
    crate::intel::dma_flush(unsafe { warm.batch_virt.add(batch_offset) }, bytes);
    log_resident_churn_flushed_binding_packets(
        batch,
        bytes,
        GPU_VA_BATCH_BASE + batch_offset as u64,
        secondary_index,
    );
    Ok(bytes)
}

fn stage_resident_churn_transform_secondary(
    warm: RenderWarmState,
    resident: &ResidentChurnForward,
    secondary_index: usize,
    dispatch: crate::intel::gpgpu::GpgpuHelioRetainedTransformDispatch,
    result_ggtt_gpu: u64,
) -> Result<usize, &'static str> {
    let batch_offset = RESIDENT_SCENE_PRIMARY_BATCH_BYTES
        .checked_add(
            secondary_index
                .checked_mul(RESIDENT_SCENE_SECONDARY_BATCH_BYTES)
                .ok_or("churn-transform-batch-slot")?,
        )
        .ok_or("churn-transform-batch-slot")?;
    let batch_end = batch_offset
        .checked_add(RESIDENT_SCENE_SECONDARY_BATCH_BYTES)
        .ok_or("churn-transform-batch-slot")?;
    if batch_end > warm.batch_len {
        return Err("churn-transform-batch-capacity");
    }
    let batch_gpu = GPU_VA_BATCH_BASE
        .checked_add(batch_offset as u64)
        .ok_or("churn-transform-batch-address")?;
    let batch_bytes = unsafe {
        core::slice::from_raw_parts_mut(
            warm.batch_virt.add(batch_offset),
            RESIDENT_SCENE_SECONDARY_BATCH_BYTES,
        )
    };
    let mut state =
        crate::intel::gpgpu::GpgpuHelioRetainedTransformStateBlob::new(batch_bytes, batch_gpu)
            .map_err(|_| "churn-transform-state")?;
    let artifact = resident
        .transform_artifact()
        .ok_or("churn-transform-artifact")?;
    let encoded = crate::intel::gpgpu::encode_helio_retained_transform_secondary(
        &mut state,
        artifact,
        dispatch,
        result_ggtt_gpu,
    )
    .map_err(|_| "churn-transform-encode")?;
    Ok(encoded.command_dwords * core::mem::size_of::<u32>())
}

fn encode_resident_scene_primary_batch(
    warm: RenderWarmState,
    secondary_count: usize,
    result_ggtt_gpu: u64,
    result_ppgtt_gpu: u64,
    secondary_ppgtt: bool,
) -> Result<usize, &'static str> {
    let batch = unsafe {
        core::slice::from_raw_parts_mut(
            warm.batch_virt as *mut u32,
            RESIDENT_SCENE_PRIMARY_BATCH_BYTES / core::mem::size_of::<u32>(),
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
    let secondary_return_gpu =
        result_ggtt_gpu + (RESULT_SLOT_SECONDARY_RETURN_DWORD * core::mem::size_of::<u32>()) as u64;
    for secondary_index in 0..secondary_count {
        let offset = RESIDENT_SCENE_PRIMARY_BATCH_BYTES
            .checked_add(
                secondary_index
                    .checked_mul(RESIDENT_SCENE_SECONDARY_BATCH_BYTES)
                    .ok_or("scene-frame-batch-slot")?,
            )
            .ok_or("scene-frame-batch-slot")?;
        let gpu = GPU_VA_BATCH_BASE + offset as u64;
        push(
            MI_BATCH_BUFFER_START_GEN8
                | MI_BATCH_2ND_LEVEL
                | if secondary_ppgtt { MI_BATCH_PPGTT } else { 0 },
        )?;
        push(gpu as u32)?;
        push((gpu >> 32) as u32)?;
        push(MI_STORE_DATA_IMM_GGTT_DW1)?;
        push(secondary_return_gpu as u32)?;
        push((secondary_return_gpu >> 32) as u32)?;
        push(
            RCS_EXEC_RESULT_SECONDARY_RETURN_BASE
                .checked_add(
                    u32::try_from(secondary_index)
                        .map_err(|_| "scene-frame-secondary-count")?
                        .checked_add(1)
                        .ok_or("scene-frame-secondary-count")?,
                )
                .ok_or("scene-frame-secondary-count")?,
        )?;
    }
    // Secondary breadcrumbs use MI_STORE_DATA_IMM with its GGTT bit set, but
    // this PIPE_CONTROL post-sync write deliberately has DEST_GGTT clear.
    // Render0 happens to use the same numeric VA in both domains; Render1
    // does not, so keep the two addresses explicit rather than faulting the
    // completion write through an unmapped tenant PPGTT leaf.
    let completion_gpu =
        result_ppgtt_gpu + (RESULT_SLOT_SCENE_FRAME_DWORD * core::mem::size_of::<u32>()) as u64;
    // Release the color target written by the Gen12 3D pixel backend. Keep
    // this end-of-pipe writeback separate from all top-of-pipe invalidations:
    // mixing them into one packet can invalidate first and only then wait for
    // older rendering. Gen12 Tile Cache Flush requires the paired depth-cache
    // flush; Wa_1409600907 in turn requires Depth Stall in the same packet.
    push(PIPE_CONTROL_CMD | PIPE_CONTROL_SCENE_COLOR_RELEASE_HEADER_BITS)?;
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
    render_target_surface_format: u32,
    render_target_pitch: usize,
    target_width: usize,
    target_height: usize,
    carrier: Option<PicassoCarrierLease>,
) -> Result<ResidentSceneGeometryResult, &'static str> {
    let render_lease = if carrier.is_none() {
        Some(reserve_warm_render_storage("resident-scene").ok_or("render-storage-busy")?)
    } else {
        None
    };
    let prepare_started_ns = crate::chronos::monotonic_nanos();
    let result_ggtt_gpu = carrier.map_or(GPU_VA_RESULT_BASE, picasso_render1_result_ggtt);
    const CLEAR_TRIANGLE: [[f32; 3]; 3] = [[-1.0, -1.0, 1.0], [3.0, -1.0, 1.0], [-1.0, 3.0, 1.0]];
    if draws.len() > RESIDENT_SCENE_MAX_DRAWS {
        return Err("scene-frame-draw-limit");
    }
    if !matches!(
        render_target_surface_format,
        SURFACE_FORMAT_R8G8B8A8_UNORM | SURFACE_FORMAT_B8G8R8A8_UNORM
    ) {
        return Err("scene-frame-target-format");
    }
    let max_secondary_count = draws.len().saturating_add(1);
    let used_batch_bytes = RESIDENT_SCENE_PRIMARY_BATCH_BYTES
        .checked_add(
            max_secondary_count
                .checked_mul(RESIDENT_SCENE_SECONDARY_BATCH_BYTES)
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
    let state = resident_scene_batch_state_for_carrier(warm, carrier)?;

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
        "resident-scene-fullscreen-clear",
        &CLEAR_TRIANGLE,
    )
    .ok_or("target-clear-resources")?
    .with_rt_surface_format(render_target_surface_format);
    stage_resident_scene_secondary(
        warm,
        clear_warm,
        clear_state_gpu,
        clear_draw,
        TriangleBlendProbeMode::MesaZeroedState,
        clear_depth,
        clear,
        None,
        ResidentSceneFragmentContract::ConstantRgba,
        [0.0, 0.0],
        ResidentScenePrimitiveTopology::TriangleList,
        0,
        result_ggtt_gpu,
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
        .ok_or("scene-frame-resident-draw")?
        .with_rt_surface_format(render_target_surface_format);
        stage_resident_scene_secondary(
            warm,
            state_warm,
            state_gpu,
            draw,
            blend_mode,
            draw_depth,
            scene_draw.rgba,
            scene_draw.sampled_texture,
            scene_draw.fragment_contract,
            scene_draw.viewport_translation_px,
            scene_draw.topology,
            secondary_count,
            result_ggtt_gpu,
        )?;
        secondary_count += 1;
    }

    let primary_bytes = encode_resident_scene_primary_batch(
        warm,
        secondary_count,
        result_ggtt_gpu,
        GPU_VA_RESULT_BASE,
        carrier.is_some(),
    )?;
    crate::intel::dma_flush(warm.batch_virt, primary_bytes);
    if !RESIDENT_SCENE_BATCH_PATH_LOGGED.swap(true, Ordering::AcqRel) {
        let textured = draws.iter().any(|draw| draw.sampled_texture.is_some());
        crate::log_info!(
            target: "render";
            "resident-scene: frame launch path=helio-indexed-indirect-v1->one-guc-scene-schedule draws={} secondaries={} command_owner=helio-gpu-record draw_parameter_translation=0 guc_role=schedule-only render_submits=1 per_mesh_context_rebuilds=0 target={}x{} fragment_contract={} dispatch=010 ksp0=simd16 ksp1=off ksp2=off vector_mask=1 color={}\n",
            draws.len(),
            secondary_count,
            target_width,
            target_height,
            if textured { "sampled-rgba8-simd16" } else { "constant-rgba-simd16" },
            if textured { "sampled-texture" } else { "specialized-per-draw" },
        );
    }
    let prepare_us = crate::chronos::monotonic_nanos().saturating_sub(prepare_started_ns) / 1_000;
    let submit_name = "resident-scene";
    let completed = match carrier {
        Some(lease) => submit_picasso_render1_batch(
            lease,
            GPU_VA_BATCH_BASE,
            RCS_EXEC_RESULT_SCENE_RCS_RELEASE_DONE_LO,
            RESULT_SLOT_SCENE_FRAME_DWORD,
        )
        .is_ok(),
        None => submit_warm_render_batch(
            render_lease.as_ref().expect("render0 storage lease"),
            dev,
            warm,
            RCS_EXEC_RESULT_SCENE_RCS_RELEASE_DONE_LO,
            RESULT_SLOT_SCENE_FRAME_DWORD,
            submit_name,
        ),
    };
    if !completed && carrier.is_none() {
        record_render_engine_after_nonretired_submit(dev, submit_name);
    }
    let (gpu_poll_us, gpu_poll_iters) = resident_scene_last_gpu_poll_profile();
    Ok(ResidentSceneGeometryResult {
        completed,
        prepare_us,
        gpu_poll_us,
        gpu_poll_iters,
    })
}

fn submit_resident_churn_forward_geometry_batched(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    resident: &ResidentChurnForward,
    sampled_material: Option<ResidentRetainedMaterial<'_>>,
    static_draws: &[ResidentSceneDraw<'_>],
    clear: [u8; 4],
    depth_config: TriangleDepthConfig,
    render_target_gpu: u64,
    render_target_surface_format: u32,
    render_target_pitch: usize,
    target_width: usize,
    target_height: usize,
    carrier: Option<PicassoCarrierLease>,
) -> Result<ResidentSceneGeometryResult, &'static str> {
    let render_lease = if carrier.is_none() {
        Some(reserve_warm_render_storage("helio-churn-forward").ok_or("render-storage-busy")?)
    } else {
        None
    };
    let prepare_started_ns = crate::chronos::monotonic_nanos();
    let result_ggtt_gpu = carrier.map_or(GPU_VA_RESULT_BASE, picasso_render1_result_ggtt);
    const CLEAR_TRIANGLE: [[f32; 3]; 3] = [[-1.0, -1.0, 1.0], [3.0, -1.0, 1.0], [-1.0, 3.0, 1.0]];
    let transform_dispatch = resident.transform_dispatch();
    let transform_handoff = transform_dispatch.map(|dispatch| dispatch.output.into());
    let transform_secondary_count = usize::from(transform_dispatch.is_some());
    let resident_draw_count = resident.draw_group_count();
    let secondary_count = resident_draw_count
        .checked_add(static_draws.len())
        .and_then(|count| count.checked_add(1 + transform_secondary_count))
        .ok_or("scene-frame-batch-capacity")?;
    let used_batch_bytes = RESIDENT_SCENE_PRIMARY_BATCH_BYTES
        .checked_add(
            secondary_count
                .checked_mul(RESIDENT_SCENE_SECONDARY_BATCH_BYTES)
                .ok_or("scene-frame-batch-capacity")?,
        )
        .ok_or("scene-frame-batch-capacity")?;
    if used_batch_bytes > warm.batch_len {
        return Err("scene-frame-batch-capacity");
    }
    if !matches!(
        render_target_surface_format,
        SURFACE_FORMAT_R8G8B8A8_UNORM | SURFACE_FORMAT_B8G8R8A8_UNORM
    ) {
        return Err("scene-frame-target-format");
    }
    unsafe {
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let state = resident_scene_batch_state_for_carrier(warm, carrier)?;

    if let Some(dispatch) = transform_dispatch {
        stage_resident_churn_transform_secondary(warm, resident, 0, dispatch, result_ggtt_gpu)?;
    }

    let clear_secondary_index = transform_secondary_count;
    let mut clear_depth = depth_config;
    clear_depth.write_enabled = true;
    clear_depth.compare_function = COMPARE_FUNCTION_ALWAYS;
    let (clear_warm, clear_state_gpu) =
        resident_scene_state_warm(state, warm, clear_secondary_index)?;
    let clear_draw = prepare_triangle_draw_resources_for_scene_vertex_slice(
        clear_warm,
        render_target_gpu,
        render_target_pitch,
        target_width,
        target_height,
        "helio-churn-native-clear",
        &CLEAR_TRIANGLE,
    )
    .ok_or("target-clear-resources")?
    .with_rt_surface_format(render_target_surface_format);
    stage_resident_scene_secondary(
        warm,
        clear_warm,
        clear_state_gpu,
        clear_draw,
        TriangleBlendProbeMode::MesaZeroedState,
        Some(clear_depth),
        clear,
        None,
        ResidentSceneFragmentContract::ConstantRgba,
        [0.0, 0.0],
        ResidentScenePrimitiveTopology::TriangleList,
        clear_secondary_index,
        result_ggtt_gpu,
    )?;

    let mut draw_depth = depth_config;
    draw_depth.write_enabled = true;
    draw_depth.compare_function = COMPARE_FUNCTION_LESS;
    for group in 0..resident_draw_count {
        let secondary_index = group + 1 + transform_secondary_count;
        let (state_warm, state_gpu) = resident_scene_state_warm(state, warm, secondary_index)?;
        match transform_handoff {
            Some(RetainedGraphicsHandoff::NativeMatrices) | None => {
                let draw = prepare_resident_churn_forward_draw(
                    state_warm,
                    resident,
                    sampled_material,
                    group,
                    render_target_gpu,
                    render_target_pitch,
                    target_width,
                    target_height,
                )
                .ok_or("churn-native-draw-resources")?
                .with_rt_surface_format(render_target_surface_format);
                stage_resident_churn_forward_secondary(
                    warm,
                    state_warm,
                    state_gpu,
                    draw,
                    draw_depth,
                    resident,
                    secondary_index,
                    result_ggtt_gpu,
                )?;
            }
            Some(RetainedGraphicsHandoff::ExpandedPositions) => {
                if sampled_material.is_some() {
                    return Err("picasso-retained-texture-native-handoff-required");
                }
                let draw = prepare_resident_churn_expanded_draw(
                    state_warm,
                    resident,
                    group,
                    render_target_gpu,
                    render_target_pitch,
                    target_width,
                    target_height,
                )
                .ok_or("churn-expanded-draw-resources")?
                .with_rt_surface_format(render_target_surface_format);
                stage_resident_scene_secondary(
                    warm,
                    state_warm,
                    state_gpu,
                    draw,
                    TriangleBlendProbeMode::MesaZeroedState,
                    Some(draw_depth),
                    resident.material_rgba(group % trueos_helio_runtime::churn::MATERIAL_COUNT),
                    None,
                    ResidentSceneFragmentContract::ConstantRgba,
                    [0.0, 0.0],
                    resident.topology(),
                    secondary_index,
                    result_ggtt_gpu,
                )?;
            }
        }
    }
    for (static_index, scene) in static_draws.iter().enumerate() {
        let secondary_index = resident_draw_count + static_index + 1 + transform_secondary_count;
        let (state_warm, state_gpu) = resident_scene_state_warm(state, warm, secondary_index)?;
        let draw = prepare_triangle_draw_resources_for_scene_resident_mesh(
            state_warm,
            render_target_gpu,
            render_target_pitch,
            target_width,
            target_height,
            scene.mesh,
        )
        .ok_or("retained-static-draw-resources")?
        .with_rt_surface_format(render_target_surface_format);
        stage_resident_scene_secondary(
            warm,
            state_warm,
            state_gpu,
            draw,
            TriangleBlendProbeMode::StraightAlpha,
            Some(draw_depth),
            scene.rgba,
            scene.sampled_texture,
            scene.fragment_contract,
            scene.viewport_translation_px,
            scene.topology,
            secondary_index,
            result_ggtt_gpu,
        )?;
    }

    let primary_bytes = encode_resident_scene_primary_batch(
        warm,
        secondary_count,
        result_ggtt_gpu,
        GPU_VA_RESULT_BASE,
        carrier.is_some(),
    )?;
    crate::intel::dma_flush(warm.batch_virt, primary_bytes);
    if transform_handoff.is_some_and(RetainedGraphicsHandoff::uses_native_matrices) {
        if !RESIDENT_CHURN_FORWARD_GPU_NATIVE_PATH_LOGGED.swap(true, Ordering::AcqRel) {
            let graph = transform_dispatch.and_then(|dispatch| dispatch.hierarchy);
            crate::log_info!(
                target: "render";
                "resident-scene: native retained online path=helioa-churn-forward-v1->retained-transform-simd16(prep+matrix-rows+compaction)->gpu-208b-instance+u32-compacted+20b-indexed-indirect->artifact-native-vs+ps->indexed-indirect-secondaries->one-guc-scene-schedule graphics_groups={} gpu_transform=1 graphics_handoff=native-matrices transform_secondaries=1 retained_graph={} graph_nodes={} dirty_local={} dirty_world={} dirty_rows={} max_depth={} cpu_matrix_expansion=0 cpu_vertex_projection=0 cpu_readback=0 instance_index=starting_instance+instance_id render_submits=1 target={}x{}\n",
                resident_draw_count,
                graph.is_some() as u8,
                graph.map_or(0, |graph| graph.node_count),
                graph.map_or(0, |graph| graph.dirty_local_count),
                graph.map_or(0, |graph| graph.dirty_world_count),
                graph.map_or(0, |graph| graph.dirty_row_count),
                graph.map_or(0, |graph| graph.max_depth),
                target_width,
                target_height,
            );
        }
    } else if transform_handoff == Some(RetainedGraphicsHandoff::ExpandedPositions) {
        if !RESIDENT_CHURN_FORWARD_GPU_EXPANDED_PATH_LOGGED.swap(true, Ordering::AcqRel) {
            crate::log_info!(
                target: "render";
                "resident-scene: native Churn online path=helioa-churn-forward-v1->retained-transform-simd16(prep+rows+expanded-position)->gpu-float3+indexed-indirect->storage-free-pass-through-vs+constant-ps->12-indexed-indirect-secondaries->one-guc-scene-schedule geometry=24-float3+36-indices-per-compacted-slot gpu_transform=1 graphics_handoff=expanded-positions fallback=physical-admission transform_secondaries=1 cpu_matrix_expansion=0 vs_storage_bindings=0 synthetic_instance_id=0 render_submits=1 target={}x{}\n",
                target_width,
                target_height,
            );
        }
    } else if !RESIDENT_CHURN_FORWARD_CPU_PATH_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log_info!(
            target: "render";
            "resident-scene: native Churn online path=helioa-churn-forward-v1->native-cpu-expanded-instance+compacted+indirect->artifact-vs+ps->12-indexed-indirect-secondaries->one-guc-scene-schedule geometry=3x(pos-normal-cube/24v/36i) gpu_transform=0 transform_secondaries=0 cpu_matrix_expansion=1 instance_index=starting_instance+instance_id sgvs=E0024002/B0020002/3 component_packing=0xA77 render_submits=1 target={}x{}\n",
            target_width,
            target_height,
        );
    }
    let prepare_us = crate::chronos::monotonic_nanos().saturating_sub(prepare_started_ns) / 1_000;
    if let Some(material) = sampled_material
        && !PICASSO_RETAINED_TEXTURED_SUBMIT_LOGGED.swap(true, Ordering::AcqRel)
    {
        crate::log_important!(target: "render";
            "picasso-material: proof=retained-texture-submit-armed accepted=1 contract=pos-normal-uv+texture-id graphics_handoff=native-matrices vertex_shader=gpu-instance-transform+authored-uv vertex_fetch=pos3+uv2+sgvs3 component_packing=0xA37 pixel_shader=filtered-base-color-simd16 interpolation=perspective-authored-uv lighting=none ps_binding_table_alignment=32 ps_bti=2 sampler=0 texture={}x{} stride={} cpu_texture_sampling=0 cpu_vertex_projection=0 render_submits=1\n",
            material.base_color.width,
            material.base_color.height,
            material.base_color.pitch,
        );
    }
    let completed = match carrier {
        Some(lease) => submit_picasso_render1_batch(
            lease,
            GPU_VA_BATCH_BASE,
            RCS_EXEC_RESULT_SCENE_RCS_RELEASE_DONE_LO,
            RESULT_SLOT_SCENE_FRAME_DWORD,
        )
        .is_ok(),
        None => submit_warm_render_batch(
            render_lease.as_ref().expect("render0 storage lease"),
            dev,
            warm,
            RCS_EXEC_RESULT_SCENE_RCS_RELEASE_DONE_LO,
            RESULT_SLOT_SCENE_FRAME_DWORD,
            "helio-churn-forward",
        ),
    };
    if completed
        && let Some(material) = sampled_material
        && !PICASSO_RETAINED_TEXTURED_PATH_LOGGED.swap(true, Ordering::AcqRel)
    {
        crate::log_important!(target: "render";
            "picasso-material: proof=retained-texture-sampled-and-retired accepted=1 coordinates=authored-uv contract=pos-normal-uv+texture-id graphics_handoff=native-matrices vertex_shader=gpu-instance-transform+authored-uv pixel_shader=filtered-base-color-simd16 interpolation=perspective-authored-uv lighting=none texture={}x{} stride={} cpu_texture_sampling=0 cpu_vertex_projection=0 render_submits=1\n",
            material.base_color.width,
            material.base_color.height,
            material.base_color.pitch,
        );
    }
    if !completed && carrier.is_none() {
        record_render_engine_after_nonretired_submit(dev, "helio-churn-forward");
    }
    let (gpu_poll_us, gpu_poll_iters) = resident_scene_last_gpu_poll_profile();
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
    submit_resident_scene_capture_inner(
        draws,
        None,
        None,
        coverage_draws,
        clear_rgba,
        diagnostic_logs,
        straight_alpha_output,
        opaque_depth_enabled,
        raster_quality,
        target_width,
        target_height,
        frame_output,
    )
}

fn submit_resident_scene_capture_inner(
    draws: &[ResidentSceneDraw<'_>],
    native_churn: Option<&ResidentChurnForward>,
    native_sampled_material: Option<ResidentRetainedMaterial<'_>>,
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
    submit_resident_scene_capture_inner_for_carrier(
        draws,
        native_churn,
        native_sampled_material,
        coverage_draws,
        clear_rgba,
        diagnostic_logs,
        straight_alpha_output,
        opaque_depth_enabled,
        raster_quality,
        target_width,
        target_height,
        frame_output,
        None,
    )
}

fn submit_resident_scene_capture_inner_for_carrier(
    draws: &[ResidentSceneDraw<'_>],
    native_churn: Option<&ResidentChurnForward>,
    native_sampled_material: Option<ResidentRetainedMaterial<'_>>,
    coverage_draws: &[ResidentSceneCoverageDraw],
    clear_rgba: Option<[u8; 4]>,
    diagnostic_logs: bool,
    straight_alpha_output: bool,
    opaque_depth_enabled: bool,
    raster_quality: ResidentSceneRasterQuality,
    target_width: usize,
    target_height: usize,
    frame_output: ResidentSceneFrameOutput,
    carrier: Option<PicassoCarrierLease>,
) -> Result<ResidentSceneFrameResult, &'static str> {
    let geometry_draw_count =
        native_churn.map_or(draws.len(), |resident| resident.draw_group_count() + draws.len());
    if target_width == 0
        || target_height == 0
        || target_width > RESIDENT_SCENE_TARGET_WIDTH
        || target_height > RESIDENT_SCENE_TARGET_HEIGHT
    {
        return Err("resident-scene-capture-shape");
    }
    if let ResidentSceneFrameOutput::GpuSurface(destination)
    | ResidentSceneFrameOutput::GpuSurfaceDeferredRelease(destination)
    | ResidentSceneFrameOutput::DirectGpuSurface(destination)
    | ResidentSceneFrameOutput::DirectGpuSurfaceDeferredRelease(destination) = frame_output
        && (!destination.is_valid()
            || destination.width as usize != target_width
            || destination.height as usize != target_height)
    {
        return Err("resident-scene-output-surface-shape");
    }
    if let ResidentSceneFrameOutput::GpuSurface(destination)
    | ResidentSceneFrameOutput::GpuSurfaceDeferredRelease(destination) = frame_output
        && destination.storage_order != crate::intel::gpgpu::GpgpuRgba8StorageOrder::Rgba
    {
        // Resolve/coverage compute kernels currently expose raw linear RGBA.
        // BGRA is supported only by the direct 3D render-target surface state.
        return Err("resident-scene-output-storage-order");
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
        .ok_or("resident-scene-capture-shape")?;
    let target_bytes = target_pitch
        .checked_mul(target_height)
        .ok_or("resident-scene-capture-shape")?;
    // Mapping mutation and submission are mutually exclusive for Render1.
    // Install every stable per-carrier leaf before admitting the frame, so
    // packet encoding/submission only reads already-owned translations.
    if let Some(lease) = carrier {
        let physical = crate::gpu::physical::physical_device().ok_or("picasso-physical-gpu")?;
        prepare_picasso_render1_scene_storage(lease, physical).ok_or("picasso-scene-storage")?;
        if let ResidentSceneFrameOutput::DirectGpuSurface(destination)
        | ResidentSceneFrameOutput::DirectGpuSurfaceDeferredRelease(destination) = frame_output
        {
            prepare_picasso_render1_ui4_target(
                lease,
                physical,
                destination.phys,
                destination.bytes,
            )
            .ok_or("picasso-direct-ui4-target")?;
        }
    }
    let frame_started_ns = crate::chronos::monotonic_nanos();

    let acquired = match carrier {
        Some(lease) => try_begin_picasso_render1_frame(lease),
        None => PRIMARY_PROBE_IN_FLIGHT
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok(),
    };
    if !acquired {
        // Render0 is an ordered GuC lane, not a reason to pin and spin an
        // Embassy carrier. Producers classify this as ordinary backpressure
        // and retry on their next cadence without restarting the task.
        return Err("render-busy");
    }

    // Resident-scene reports one scene-level result. Keep the renderer's proof
    // transcript available for a deliberate stalled-frame diagnostic retry,
    // but do not repeat it for every mesh in ordinary scene updates.
    let _summary_only = (!diagnostic_logs).then(RenderSummaryOnlyGuard::enter);

    let result = (|| {
        let Some(dev) = crate::intel::claimed_device() else {
            return Err("no-device");
        };
        let warm = match carrier {
            Some(lease) => picasso_render1_warm_state(lease).ok_or("picasso-carrier-not-ready")?,
            None => warm_state().ok_or("render-boot-not-ready")?,
        };
        if !forcewake_render_acquire(warm) {
            return Err("forcewake");
        }
        if carrier.is_none() && !ensure_smoke_buffers_mapped(dev, warm) {
            return Err("render-map");
        }
        let raster_quality = if carrier.is_some()
            && raster_quality == ResidentSceneRasterQuality::Multisample4x
        {
            // Phase 1 owns a separate depth/state/target cache; its MSAA
            // color/depth cache is not admitted until it too is carrier-local.
            ResidentSceneRasterQuality::SingleSample
        } else if raster_quality == ResidentSceneRasterQuality::Multisample4x
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
        let msaa_color = if carrier.is_none()
            && raster_quality == ResidentSceneRasterQuality::Multisample4x
        {
            Some(prepare_resident_scene_msaa_color(warm.device_id, target_width, target_height)?)
        } else {
            None
        };
        let direct_output = match frame_output {
            ResidentSceneFrameOutput::DirectGpuSurface(destination)
            | ResidentSceneFrameOutput::DirectGpuSurfaceDeferredRelease(destination) => {
                Some(destination)
            }
            _ => None,
        };
        let render_target_surface_format = if msaa_color.is_some() {
            SURFACE_FORMAT_R8G8B8A8_UNORM
        } else if direct_output.is_some_and(|destination| {
            destination.storage_order == crate::intel::gpgpu::GpgpuRgba8StorageOrder::Bgra
        }) {
            SURFACE_FORMAT_B8G8R8A8_UNORM
        } else {
            SURFACE_FORMAT_R8G8B8A8_UNORM
        };
        let (render_target_gpu, render_target_pitch) = if let Some(target) = msaa_color {
            (target.surface.gpu, target.surface.pitch_bytes as usize)
        } else if let Some(destination) = direct_output {
            (
                match carrier {
                    Some(lease) => prepare_picasso_render1_ui4_target(
                        lease,
                        crate::gpu::physical::physical_device().ok_or("picasso-physical-gpu")?,
                        destination.phys,
                        destination.bytes,
                    )
                    .ok_or("picasso-direct-ui4-target")?,
                    None => prepare_resident_scene_direct_ui4_target(destination)?,
                },
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
                match carrier {
                    Some(lease) => prepare_picasso_resident_scene_depth(
                        lease,
                        warm.device_id,
                        target_width,
                        target_height,
                    )?,
                    None => {
                        prepare_resident_scene_depth(warm.device_id, target_width, target_height)?
                    }
                }
            })
        } else {
            None
        };

        if opaque_depth_enabled
            && !RESIDENT_SCENE_DEPTH_CONTRACT_LOGGED.swap(true, Ordering::AcqRel)
        {
            let opaque = native_churn.map_or_else(
                || draws.iter().filter(|draw| draw.rgba[3] == u8::MAX).count(),
                ResidentChurnForward::draw_group_count,
            );
            let blended = native_churn.map_or_else(
                || {
                    draws
                        .iter()
                        .filter(|draw| draw.rgba[3] != 0 && draw.rgba[3] != u8::MAX)
                        .count()
                },
                |_| 0,
            );
            let skipped = native_churn
                .map_or_else(|| draws.iter().filter(|draw| draw.rgba[3] == 0).count(), |_| 0);
            crate::log_info!(
                target: "render";
                "resident-scene-depth: contract enabled opaque={} blended={} skipped={} clear=fullscreen-color+depth opaque_order=front-to-back opaque_state=depth-test+write+blend-off transparent_order=back-to-front transparent_state=depth-test+write-off+straight-alpha compare=lequal hiz=off\n",
                opaque,
                blended,
                skipped,
            );
        }

        // Resident-scene uses straight-alpha blending.  The GPU must see the real
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
        let geometry = if let Some(resident) = native_churn {
            submit_resident_churn_forward_geometry_batched(
                dev,
                warm,
                resident,
                native_sampled_material,
                draws,
                clear,
                depth_config.ok_or("churn-native-depth")?,
                render_target_gpu,
                render_target_surface_format,
                render_target_pitch,
                target_width,
                target_height,
                carrier,
            )?
        } else {
            submit_resident_scene_geometry_batched(
                dev,
                warm,
                draws,
                clear,
                opaque_depth_enabled,
                depth_config,
                render_target_gpu,
                render_target_surface_format,
                render_target_pitch,
                target_width,
                target_height,
                carrier,
            )?
        };
        let geometry_complete = geometry.completed;
        let mut completed_draws = if geometry_complete {
            geometry_draw_count
        } else {
            0
        };

        // A scene is one atomic visual result.  A timed-out draw leaves the
        // shared target partially updated, so never expose it to either the
        // display or request-render cache.  The caller will retry the same
        // revision on the next scene tick while the last complete frame stays
        // visible.
        let geometry_finished_ns = crate::chronos::monotonic_nanos();
        let needs_scratch_output = matches!(frame_output, ResidentSceneFrameOutput::Readback)
            || (direct_output.is_none() && msaa_color.is_none());
        let scratch_output = if needs_scratch_output {
            if warm.streamout_virt.is_null() || warm.streamout_len < target_bytes {
                return Err("warm-scratch");
            }
            Some(
                crate::intel::gpgpu::GpgpuRgba8Surface::new(
                    warm.streamout_phys,
                    GPU_VA_STREAMOUT_BASE,
                    warm.streamout_len,
                    target_width as u32,
                    target_height as u32,
                    target_pitch as u32,
                )
                .ok_or("resident-scene-resolve-surface")?,
            )
        } else {
            None
        };
        // On the native 4x path, resolve directly into the UI4 producer back
        // buffer. The scratch surface remains the compatibility target for
        // single-sample hardware and for CPU readback consumers.
        let output = match (frame_output, msaa_color) {
            (ResidentSceneFrameOutput::GpuSurface(destination), Some(_)) => destination,
            (ResidentSceneFrameOutput::GpuSurfaceDeferredRelease(destination), Some(_)) => {
                destination
            }
            (ResidentSceneFrameOutput::DirectGpuSurface(destination), _) => destination,
            (ResidentSceneFrameOutput::DirectGpuSurfaceDeferredRelease(destination), _) => {
                destination
            }
            _ => scratch_output.ok_or("resident-scene-resolve-surface")?,
        };
        let direct_scanout_output = match frame_output {
            ResidentSceneFrameOutput::GpuSurface(destination)
            | ResidentSceneFrameOutput::GpuSurfaceDeferredRelease(destination)
            | ResidentSceneFrameOutput::DirectGpuSurface(destination)
            | ResidentSceneFrameOutput::DirectGpuSurfaceDeferredRelease(destination) => {
                output.gpu == destination.gpu
            }
            ResidentSceneFrameOutput::Readback => false,
        };
        let resolved = geometry_complete && msaa_color.is_none();
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
            | ResidentSceneFrameOutput::DirectGpuSurface(destination)
            | ResidentSceneFrameOutput::DirectGpuSurfaceDeferredRelease(destination) =
                frame_output
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
        let incomplete_stage = if !geometry_complete {
            Some(ResidentSceneIncompleteStage::Geometry)
        } else if !resolved {
            Some(ResidentSceneIncompleteStage::Resolve)
        } else if completed_coverage_draws != coverage_draws.len() {
            Some(ResidentSceneIncompleteStage::Coverage)
        } else if present_copy_performed && !frame_complete {
            Some(ResidentSceneIncompleteStage::PresentCopy)
        } else if !frame_complete {
            Some(ResidentSceneIncompleteStage::Unknown)
        } else {
            None
        };
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
                let finalizer = crate::intel::gpgpu::release_rgba8_surface_for_scanout(destination);
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
                "resident-scene-perf: seq={} draws={} frame_us={} geometry_us={} prepare_us={} gpu_poll_us={} gpu_poll_iters={} geometry_other_us={} note=geometry_other_includes_lock-forcewake-lrc-guc-submit-result-handoff\n",
                perf_sequence,
                geometry_draw_count,
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
            requested_draws: geometry_draw_count.saturating_add(coverage_draws.len()),
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
            incomplete_stage,
            release_fence,
        })
    })();
    match carrier {
        Some(lease) => finish_picasso_render1_frame(lease),
        None => PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release),
    }
    result
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn submit_resident_font_mesh_readback_once(
    mesh: &ResidentFontMesh,
    native_scale: u32,
    rgba: crate::intel::gpu_font::GpuFontRgba,
) -> Result<(RenderJokerResult, Option<FontRenderTargetReadback>), &'static str> {
    let mut readback = None;
    let render = submit_resident_font_mesh_inner(mesh, native_scale, rgba, Some(&mut readback))?;
    Ok((render, readback))
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct RenderOaControlResult {
    pub(crate) action: &'static str,
    pub(crate) oactx: u32,
    pub(crate) oar: u32,
    pub(crate) ctx_ctrl: u32,
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct RenderArtificialFragmentResult {
    pub(crate) mode: &'static str,
    pub(crate) ok: bool,
    pub(crate) descs: usize,
    pub(crate) before: u32,
    pub(crate) after: u32,
    pub(crate) rt_gpu: u64,
    pub(crate) remapped_render: bool,
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn render_joker_variant_names() -> &'static [&'static str] {
    RENDER_JOKER_VARIANTS
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn render_oa_control_action(
    action: &str,
) -> Result<RenderOaControlResult, &'static str> {
    let Some(dev) = crate::intel::claimed_device() else {
        return Err("no-device");
    };
    let warm = warm_state().ok_or("render-boot-not-ready")?;
    if !forcewake_render_acquire(warm) {
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
enum RenderJokerTarget {
    ScratchRt,
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn parse_render_joker_spec(name: &str) -> Option<RenderJokerSpec> {
    let scratch = RenderJokerTarget::ScratchRt;
    // Historical variants that once targeted the live primary are retained as
    // offscreen diagnostics. Render probes never acquire a display surface.
    let offscreen = scratch;
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
            target: offscreen,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mesa") || name.eq_ignore_ascii_case("big") {
        RenderJokerSpec {
            variant: "mesa",
            submit_name: "ps-launch-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mesa-retire") {
        RenderJokerSpec {
            variant: "mesa-retire",
            submit_name: "ps-launch-big-primitive-retire",
            target: offscreen,
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
            target: offscreen,
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
            target: offscreen,
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
            target: offscreen,
            blend: explicit,
            geometry: point,
            backend: BackendProbeMode::PsBindingTableCountOne,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-slot0") {
        RenderJokerSpec {
            variant: "point-slot0",
            submit_name: "point-vf-giant-slot0",
            target: offscreen,
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
            target: offscreen,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("so-vf-header") {
        RenderJokerSpec {
            variant: "so-vf-header",
            submit_name: "joker-vf-streamout-header",
            target: offscreen,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("so-vs") {
        RenderJokerSpec {
            variant: "so-vs",
            submit_name: "joker-vs-streamout",
            target: offscreen,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("so-vs-header") {
        RenderJokerSpec {
            variant: "so-vs-header",
            submit_name: "joker-vs-streamout-header",
            target: offscreen,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("bt1") {
        RenderJokerSpec {
            variant: "bt1",
            submit_name: "ps-bt1-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsBindingTableCountOne,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("wm-normal") || name.eq_ignore_ascii_case("wm") {
        RenderJokerSpec {
            variant: "wm-normal",
            submit_name: "ps-wm-normal-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmNormalDispatch,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("slot0") {
        RenderJokerSpec {
            variant: "slot0",
            submit_name: "ps-dispatch-slot0-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchSlot0,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("slot1") {
        RenderJokerSpec {
            variant: "slot1",
            submit_name: "ps-dispatch-slot1-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchSlot1,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("slot2") {
        RenderJokerSpec {
            variant: "slot2",
            submit_name: "ps-dispatch-slot2-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchSlot2,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("all") || name.eq_ignore_ascii_case("slots-all") {
        RenderJokerSpec {
            variant: "all",
            submit_name: "ps-dispatch-all-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchAllKspSlots,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("simd16") {
        RenderJokerSpec {
            variant: "simd16",
            submit_name: "ps-simd16-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsSimd16,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("simd16-retire") {
        RenderJokerSpec {
            variant: "simd16-retire",
            submit_name: "ps-simd16-big-primitive-retire",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsSimd16,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("eot") {
        RenderJokerSpec {
            variant: "eot",
            submit_name: "ps-eot-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsEotOnly,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("eot-retire") {
        RenderJokerSpec {
            variant: "eot-retire",
            submit_name: "ps-eot-big-primitive-retire",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsEotOnly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("cps") || name.eq_ignore_ascii_case("cps-disabled") {
        RenderJokerSpec {
            variant: "cps",
            submit_name: "ps-cps-disabled-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsCpsDisabled,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("cps-retire") {
        RenderJokerSpec {
            variant: "cps-retire",
            submit_name: "ps-cps-disabled-big-primitive-retire",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsCpsDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("hz") || name.eq_ignore_ascii_case("wm-hz") {
        RenderJokerSpec {
            variant: "hz",
            submit_name: "wm-hz-sample-mask-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmHzSampleMask,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("hz-retire") || name.eq_ignore_ascii_case("wm-hz-retire") {
        RenderJokerSpec {
            variant: "hz-retire",
            submit_name: "wm-hz-sample-mask-big-primitive-retire",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmHzSampleMask,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit") || name.eq_ignore_ascii_case("late-reemit") {
        RenderJokerSpec {
            variant: "reemit",
            submit_name: "wm-late-reemit-big-primitive",
            target: offscreen,
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
            target: offscreen,
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
            target: offscreen,
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
            target: offscreen,
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
            target: offscreen,
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
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("payload-push") {
        RenderJokerSpec {
            variant: "payload-push",
            submit_name: "ps-payload-push-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadPushConstant,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-attr") {
        RenderJokerSpec {
            variant: "payload-attr",
            submit_name: "ps-payload-attr-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadAttributeEnable,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-simple") {
        RenderJokerSpec {
            variant: "payload-simple",
            submit_name: "ps-payload-simple-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadSimpleHint,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-depthw") {
        RenderJokerSpec {
            variant: "payload-depthw",
            submit_name: "ps-payload-source-depth-w-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadSourceDepthW,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-bary") || name.eq_ignore_ascii_case("bary") {
        RenderJokerSpec {
            variant: "payload-bary",
            submit_name: "ps-payload-bary-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadBaryPlanes,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("grf1") {
        RenderJokerSpec {
            variant: "grf1",
            submit_name: "ps-grf-start-r1-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfStartR1,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("grf2") {
        RenderJokerSpec {
            variant: "grf2",
            submit_name: "ps-grf-start-r2-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfStartR2,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("grf4") {
        RenderJokerSpec {
            variant: "grf4",
            submit_name: "ps-grf-start-r4-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfStartR4,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mt31") {
        RenderJokerSpec {
            variant: "mt31",
            submit_name: "ps-grf-maxthreads-31-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfMaxThreads31,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mt15") {
        RenderJokerSpec {
            variant: "mt15",
            submit_name: "ps-grf-maxthreads-15-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfMaxThreads15,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("sync-light") {
        RenderJokerSpec {
            variant: "sync-light",
            submit_name: "postdraw-light-only-retire",
            target: offscreen,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: PostDrawSyncVariant::LightOnlyRetire,
        }
    } else if name.eq_ignore_ascii_case("sync-post-no-cs") {
        RenderJokerSpec {
            variant: "sync-post-no-cs",
            submit_name: "postdraw-pc-postsync-no-cs",
            target: offscreen,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("sync-cs-no-post") {
        RenderJokerSpec {
            variant: "sync-cs-no-post",
            submit_name: "postdraw-pc-cs-no-postsync",
            target: offscreen,
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn render_joker_streamout_kind(variant: &str) -> Option<&'static str> {
    match variant {
        "so-vf" | "so-vf-header" => Some("vf"),
        "so-vs" | "so-vs-header" => Some("vs"),
        _ => None,
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const FONT_PS_ADMISSION_ACTIVE_CASE: u8 = 6;

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    let warm = warm_state().ok_or("render-boot-not-ready")?;
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn submit_render_artificial_fragment_sentinel_locked()
-> Result<RenderArtificialFragmentResult, &'static str> {
    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("artificial-fragment-sentinel skipped reason=no-device\n");
        return Err("no-device");
    };
    let warm = warm_state().ok_or("render-boot-not-ready")?;
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
    let render_lease =
        reserve_warm_render_storage("artificial-fragment-sentinel").ok_or("render-storage-busy")?;

    const SENTINEL_COLOR: u32 = 0xA17F_F00D;
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
        core::ptr::write_volatile(warm.streamout_virt as *mut u32, 0xDEAD_BEEF);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.batch_virt, warm.batch_len);
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
        &render_lease,
        dev,
        warm,
        RCS_EXEC_RESULT_MI_PROBE_DONE,
        RESULT_SLOT_PRE3D_DWORD,
        "artificial-fragment-sentinel",
    );
    if !completed {
        record_render_engine_after_nonretired_submit(dev, "artificial-fragment-sentinel");
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    let warm = warm_state().ok_or("render-boot-not-ready")?;
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

    let RenderJokerTarget::ScratchRt = spec.target;
    unsafe {
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
        core::ptr::write_volatile(warm.streamout_virt as *mut u32, 0xDEAD_BEEF);
    }
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
    let (target_gpu, target_pitch, target_w, target_h, target_label) =
        (GPU_VA_STREAMOUT_BASE, 8 * core::mem::size_of::<u32>(), 8, 8, "scratch");

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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn seed_render_scratch_rt(warm: RenderWarmState) {
    unsafe {
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
        core::ptr::write_volatile(warm.streamout_virt as *mut u32, 0xDEAD_BEEF);
    }
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
            isolate_render_context_after_completed_probe(dev, submit_name);
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
            isolate_render_context_after_completed_probe(dev, submit_name);
        }
        if observed {
            return true;
        }
    }
    false
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
            isolate_render_context_after_completed_probe(dev, submit_name);
        }
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn submit_triangle_vf_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(render_lease) = reserve_warm_render_storage("vf-streamout-proof") else {
        return false;
    };
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
        &render_lease,
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
        record_render_engine_after_nonretired_submit(dev, "vf-streamout-proof");
    }
    accepted
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    let Some(render_lease) = reserve_warm_render_storage(submit_name) else {
        return false;
    };
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
        &render_lease,
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
        // Render0 now keeps one persistent GuC context. These engine-global
        // counters remain diagnostic-only; saved-HWLRCA HEAD is the actual
        // identity-specific retirement proof.
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
        record_render_engine_after_nonretired_submit(dev, submit_name);
    }
    completed
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn oa_counter_delta(before: u64, after: u64, bits: u32) -> u64 {
    if after >= before {
        after - before
    } else {
        (1u64 << bits).saturating_add(after).saturating_sub(before)
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn submit_triangle_vs_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(render_lease) = reserve_warm_render_storage("vs-streamout-proof") else {
        return false;
    };
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
        &render_lease,
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
        record_render_engine_after_nonretired_submit(dev, "vs-streamout-proof");
    }
    accepted
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn submit_triangle_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(render_lease) = reserve_warm_render_storage("streamout-proof") else {
        return false;
    };
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
        &render_lease,
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
        record_render_engine_after_nonretired_submit(dev, "streamout-proof");
    }
    accepted
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    // Vertex fetch uses the Render0 PPGTT installed by init_warm_state_for_boot.
    // A stale GGTT mirror here claimed Pipe-A/Slot-3/buffer-0's numeric VA;
    // display later replaced it when that surface was mapped.
    let ok_result = super::map_ggtt(dev, warm.result_phys, warm.result_len, GPU_VA_RESULT_BASE);
    let ok_streamout =
        super::map_ggtt(dev, warm.streamout_phys, warm.streamout_len, GPU_VA_STREAMOUT_BASE);
    if ok_ring && ok_context && ok_batch && ok_draw_state && ok_result && ok_streamout {
        super::ggtt_invalidate(dev);
        true
    } else {
        false
    }
}

static FIXED_RENDER_GGTT_BOOT_RESULT: spin::Once<bool> = spin::Once::new();

pub(crate) fn init_fixed_render_ggtt_for_boot(dev: crate::intel::Dev) -> bool {
    *FIXED_RENDER_GGTT_BOOT_RESULT.call_once(|| {
        if !crate::intel::physical_gt_ready(dev) {
            return false;
        }
        let warm = init_warm_state_for_boot(dev);
        let complete = warm.ring_len != 0
            && warm.context_len != 0
            && warm.batch_len != 0
            && warm.draw_state_len != 0
            && warm.vertex_len != 0
            && warm.result_len != 0
            && warm.streamout_len != 0
            && render_ppgtt_pml4_phys() != 0;
        let picasso_carriers_ready = prewarm_picasso_carrier_control_ggtt_for_boot(dev);
        if !complete || !map_smoke_buffers(dev, warm) {
            WARM_BUFFERS_MAPPED.store(false, Ordering::Release);
            return false;
        }
        log_boot_render_memory_proof(warm);
        MEMORY_PROOF_LOGGED.store(true, Ordering::Release);
        WARM_BUFFERS_MAPPED.store(true, Ordering::Release);
        let picasso_carriers_ready_count =
            usize::from(picasso_carriers_ready) * picasso_carrier_capacity();
        if !picasso_carriers_ready {
            crate::log_error!(target: "render";
                "picasso-carrier boot-gate accepted=0 ready=0 required={} render0_continues=1 vmx_claims=fail-closed\n",
                picasso_carrier_capacity(),
            );
        }
        crate::log_info!(target: "render";
            "intel/render boot-gate render0={} picasso_carriers_ready={}/{} picasso_runtime_ggtt_remap=forbidden max_vmx_domains={}\n",
            1,
            picasso_carriers_ready_count,
            picasso_carrier_capacity(),
            picasso_carrier_capacity(),
        );
        true
    })
}

fn read_first_dword(virt: *mut u8, len: usize) -> u32 {
    if virt.is_null() || len < core::mem::size_of::<u32>() {
        return 0;
    }
    unsafe { core::ptr::read_volatile(virt as *const u32) }
}

fn log_boot_render_memory_proof(warm: RenderWarmState) {
    // This runs only inside FIXED_RENDER_GGTT_BOOT_RESULT before
    // WARM_BUFFERS_MAPPED is published and before Render0 can register its
    // HWLRCA. Ring and context contain GPU-written saved state after that
    // boundary, so proof logging must never flush either allocation. Their
    // exact initialization/append paths publish only the bytes they own.
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
        "memory-proof accepted=1 map=1 ggtt_invalidated=1 flush=client-data-only ring_context_flush=0 phase=boot-before-render0-registration readback=cpu-first-dword ring[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] context[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] batch[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] state[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] vertex[phys=0x{:X} ppgtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] result[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] streamout[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] does_not_prove=fragment_ps_rt_progress\n",
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
    // GGTT PTEs and their invalidate are physical-GT state.  Client launch is
    // only allowed to consume the immutable boot mapping; it cannot remap the
    // same fixed addresses while another GuC context is executing.
    WARM_BUFFERS_MAPPED.load(Ordering::Acquire)
        && FIXED_RENDER_GGTT_BOOT_RESULT.get().copied() == Some(true)
        && warm.device_id == dev.device_id
        && warm.revision_id == dev.revision_id
        && warm.mmio_base == dev.mmio as usize
        && warm.mmio_len == dev.mmio_len
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    let Some(render_lease) = reserve_warm_render_storage(submit_name) else {
        return false;
    };
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
        &render_lease,
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        submit_name,
    );
    if !completed {
        record_render_engine_after_nonretired_submit(dev, submit_name);
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
    let Some(render_lease) = reserve_warm_render_storage(submit_name) else {
        return false;
    };
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
    } else if sf_viewport_transform {
        "font-lyon-clip-field-viewport-transform"
    } else {
        "font-lyon-clip-field-screen-space"
    };
    let unique_vertex_count = resident_mesh
        .map(|mesh| mesh.vertex_count as usize)
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
        "resident-scene" | "font-tessel-3d-once" | "font-outline-gpu-mesh-3d" | "font-resident-3d"
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
    let completed = submit_warm_render_batch(
        &render_lease,
        dev,
        warm,
        completion_value,
        completion_slot,
        submit_name,
    );
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
            // A font job produces an offscreen result only. Visibility belongs
            // to a UI4 frame/window; this render service may return pixels to
            // such a producer but cannot write an application plane directly.
            let captured = if let Some(output) = readback.as_deref_mut() {
                let target_bytes = pixel_count.saturating_mul(4);
                let mut visible_rgba = output
                    .take()
                    .map(|previous| previous.pixels)
                    .unwrap_or_default();
                visible_rgba.clear();
                visible_rgba.reserve_exact(target_bytes);
                for y in 0..target_height {
                    let row_offset = y.saturating_mul(draw.rt_pitch as usize);
                    for x in 0..target_width {
                        let after =
                            read_scratch_dword(row_offset.saturating_add(x.saturating_mul(4)));
                        if after == 0xDEAD_BEEF {
                            visible_rgba.extend_from_slice(&[0, 0, 0, 0]);
                        } else {
                            visible_rgba.extend_from_slice(&after.to_le_bytes());
                        }
                    }
                }
                *output = Some(FontRenderTargetReadback {
                    width: target_width as u32,
                    height: target_height as u32,
                    pixels: visible_rgba,
                });
                true
            } else {
                false
            };
            intel_render_focus_log!(
                "{} offscreen-result captured={} presented=0 source_size={}x{} changed_pixels={} readback_buffers={} visibility=ui4-frame-required source=whole-linear-rgba8-readback\n",
                submit_name,
                captured as u8,
                draw.target_w,
                draw.target_h,
                changed_pixels,
                captured as u8,
            );
        }
    }
    if !completed {
        record_render_engine_after_nonretired_submit(dev, submit_name);
    }
    completed
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn submit_result_store_probe(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    let Some(render_lease) = reserve_warm_render_storage("mi-store-probe") else {
        return false;
    };
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
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
        &render_lease,
        dev,
        warm,
        RCS_EXEC_RESULT_MI_PROBE_DONE,
        RESULT_SLOT_PRE3D_DWORD,
        "mi-store-probe",
    )
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn submit_3d_no_draw_probe(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    let Some(render_lease) = reserve_warm_render_storage("3d-no-draw") else {
        return false;
    };
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
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
        &render_lease,
        dev,
        warm,
        RCS_EXEC_RESULT_3D_NO_DRAW_DONE,
        RESULT_SLOT_POST3D_DWORD,
        "3d-no-draw",
    )
}
