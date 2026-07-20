#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuActivitySnapshot {
    pub(crate) available: bool,
    pub(crate) direct_rcs_enabled: bool,
    pub(crate) submit_seq: u32,
    pub(crate) ring_head: u32,
    pub(crate) ring_tail: u32,
    pub(crate) acthd: u32,
    pub(crate) ipeir: u32,
    pub(crate) ipehr: u32,
    pub(crate) eir: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct CopyRectRgba8Params {
    pub(crate) src_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) src_pitch_bytes: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) src_x: u32,
    pub(crate) src_y: u32,
    pub(crate) dst_x: u32,
    pub(crate) dst_y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Ui4Nv12Tile64ToRgba8FrameParams {
    pub(crate) nv12_gpu: u64,
    pub(crate) base_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) src_pitch_bytes: u32,
    pub(crate) src_uv_offset: u32,
    pub(crate) base_pitch_bytes: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) output_width: u32,
    pub(crate) output_height: u32,
    pub(crate) content_dst_x: u32,
    pub(crate) content_dst_y: u32,
    pub(crate) content_width: u32,
    pub(crate) content_height: u32,
    pub(crate) source_x: u32,
    pub(crate) source_y: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Mandel64WorklistRgba8Desc {
    pub(crate) src_xy: u32,
    pub(crate) dst_xy: u32,
    pub(crate) flags: u32,
    pub(crate) color_rgba: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Mandel64WorklistRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) desc_base: u32,
    pub(crate) desc_count: u32,
}

/// One axis-aligned premultiplied RGBA source in the stable UI4 compositor
/// contract.  Unlike the exploratory sprite worklist, every layer in a frame
/// is consumed by one kernel invocation and one walker.
#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuUi4ComposeLayer {
    pub(crate) src: GpgpuRgba8Surface,
    /// The source was produced through the stable PAT3/UC scanout mapping and
    /// must retain that policy when the compositor samples it.
    pub(crate) src_scanout_cache: bool,
    pub(crate) dst_x: i32,
    pub(crate) dst_y: i32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    pub(crate) opacity: u8,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
struct GpgpuUi4ComposeLayerDesc {
    src_gpu_lo: u32,
    src_gpu_hi: u32,
    src_pitch_bytes: u32,
    src_width: u32,
    src_height: u32,
    dst_x: i32,
    dst_y: i32,
    dst_width: u32,
    dst_height: u32,
    opacity: u32,
    flags: u32,
    reserved: u32,
}

#[derive(Copy, Clone, Debug)]
struct Ui4ComposeLayersParams {
    base_gpu: u64,
    dst_gpu: u64,
    layers_gpu: u64,
    base_pitch_bytes: u32,
    dst_pitch_bytes: u32,
    dst_width: u32,
    dst_height: u32,
    damage_x: u32,
    damage_y: u32,
    damage_width: u32,
    damage_height: u32,
    layer_count: u32,
    flags: u32,
}

pub(crate) const UI4_COMPOSE_FLAG_BASE_XRGB: u32 = 1 << 0;
pub(crate) const UI4_COMPOSE_FLAG_DEST_XRGB: u32 = 1 << 1;

#[derive(Copy, Clone, Debug)]
pub(crate) struct SkyboxSampleRgb565Params {
    pub(crate) sky_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) sky_pitch_bytes: u32,
    pub(crate) sky_width: u32,
    pub(crate) sky_height: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    pub(crate) rect_x: u32,
    pub(crate) rect_y: u32,
    pub(crate) rect_width: u32,
    pub(crate) rect_height: u32,
    pub(crate) right_x: f32,
    pub(crate) right_y: f32,
    pub(crate) right_z: f32,
    pub(crate) up_x: f32,
    pub(crate) up_y: f32,
    pub(crate) up_z: f32,
    pub(crate) forward_x: f32,
    pub(crate) forward_y: f32,
    pub(crate) forward_z: f32,
    pub(crate) aspect_tan_half_fov_y: f32,
    pub(crate) tan_half_fov_y: f32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ChartSineRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    pub(crate) rect_x: u32,
    pub(crate) rect_y: u32,
    pub(crate) rect_width: u32,
    pub(crate) rect_height: u32,
    pub(crate) phase: f32,
    pub(crate) cycles: f32,
    pub(crate) amplitude: f32,
    pub(crate) line_width_px: f32,
    pub(crate) background_rgba: u32,
    pub(crate) minor_grid_rgba: u32,
    pub(crate) major_grid_rgba: u32,
    pub(crate) axis_rgba: u32,
    pub(crate) line_rgba: u32,
    pub(crate) glow_rgba: u32,
    pub(crate) flags: u32,
}

impl ChartSineRgba8Params {
    pub(crate) const fn scope_defaults(phase: f32, flags: u32) -> Self {
        Self {
            dst_gpu: 0,
            dst_pitch_bytes: 0,
            dst_width: 0,
            dst_height: 0,
            rect_x: 0,
            rect_y: 0,
            rect_width: 0,
            rect_height: 0,
            phase,
            cycles: 3.0,
            amplitude: 0.34,
            line_width_px: 2.25,
            background_rgba: 0xFF1F_1107,
            minor_grid_rgba: 0xFF3C_2610,
            major_grid_rgba: 0xFF63_451E,
            axis_rgba: 0xFF98_7D62,
            line_rgba: 0xFFE3_FF86,
            glow_rgba: 0xFFE8_D718,
            flags,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct PixelPlasmaRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    pub(crate) rect_x: u32,
    pub(crate) rect_y: u32,
    pub(crate) rect_width: u32,
    pub(crate) rect_height: u32,
    pub(crate) time: f32,
    pub(crate) spatial_scale: f32,
    pub(crate) intensity: f32,
    pub(crate) low_rgba: u32,
    pub(crate) mid_rgba: u32,
    pub(crate) high_rgba: u32,
    pub(crate) flags: u32,
}

impl PixelPlasmaRgba8Params {
    pub(crate) const fn demo_defaults(time: f32, flags: u32) -> Self {
        Self {
            dst_gpu: 0,
            dst_pitch_bytes: 0,
            dst_width: 0,
            dst_height: 0,
            rect_x: 0,
            rect_y: 0,
            rect_width: 0,
            rect_height: 0,
            time,
            spatial_scale: 1.0,
            intensity: 1.0,
            low_rgba: 0xFF24_0A08,
            mid_rgba: 0xFFE6_D214,
            high_rgba: 0xFF2D_55FF,
            flags,
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct FontOutlineMeshParams {
    src_gpu: u64,
    dst_gpu: u64,
    op_count: u32,
    stage: u32,
    subdivisions: u32,
    max_vertices: u32,
    max_indices: u32,
    scale: f32,
    origin_x: f32,
    origin_y: f32,
    stroke_half_width: f32,
}

#[derive(Copy, Clone, Debug)]
struct FontOutlineCoverageR8Params {
    ops_gpu: u64,
    mask_gpu: u64,
    op_count: u32,
    subdivisions: u32,
    mask_pitch_bytes: u32,
    mask_width: u32,
    mask_height: u32,
    rect_x: u32,
    rect_y: u32,
    rect_width: u32,
    rect_height: u32,
    optical_bias_px: f32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuFontOutlineMesh {
    pub(crate) storage_phys: u64,
    pub(crate) storage_bytes: usize,
    pub(crate) vertex_offset_bytes: u32,
    pub(crate) vertex_count: u32,
    pub(crate) vertex_stride: u32,
    pub(crate) index_offset_bytes: u32,
    pub(crate) index_count: u32,
    pub(crate) min_x: f32,
    pub(crate) min_y: f32,
    pub(crate) max_x: f32,
    pub(crate) max_y: f32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuFontOutlineProbeResult {
    pub(crate) available: bool,
    pub(crate) forcewake_ok: bool,
    pub(crate) mapped_ok: bool,
    pub(crate) ppgtt_ok: bool,
    pub(crate) kernel_ppgtt_ok: bool,
    pub(crate) src_ppgtt_ok: bool,
    pub(crate) dst_ppgtt_ok: bool,
    pub(crate) batch_ok: bool,
    pub(crate) submitted: bool,
    pub(crate) retired: bool,
    pub(crate) kernel_done: bool,
    pub(crate) ok: bool,
    pub(crate) retire_ms: u64,
    pub(crate) op_count: u32,
    pub(crate) move_count: u32,
    pub(crate) line_count: u32,
    pub(crate) quad_count: u32,
    pub(crate) cubic_count: u32,
    pub(crate) close_count: u32,
    pub(crate) vertices: u32,
    pub(crate) segments: u32,
    pub(crate) indices: u32,
    pub(crate) generated_mesh: Option<GpgpuFontOutlineMesh>,
    pub(crate) checksum: u32,
    pub(crate) expected_checksum: u32,
    pub(crate) invalid: u32,
    pub(crate) truncated: bool,
    pub(crate) indices_in_range: bool,
    pub(crate) min_x: f32,
    pub(crate) min_y: f32,
    pub(crate) max_x: f32,
    pub(crate) max_y: f32,
    pub(crate) pre_marker: u32,
    pub(crate) post_marker: u32,
    pub(crate) report_marker: u32,
    pub(crate) done_marker: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct FillRectRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) dst_x: u32,
    pub(crate) dst_y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) color_rgba: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct FillRect2dDispatch {
    group_x: u32,
    group_y: u32,
    right_mask: u32,
}

const fn fill_rect_2d_dispatch(width: u32, height: u32) -> Option<FillRect2dDispatch> {
    if width == 0 || height == 0 {
        return None;
    }
    let full_groups = width / FILL_RECT_PIXELS_PER_GROUP_X;
    let tail_pixels = width % FILL_RECT_PIXELS_PER_GROUP_X;
    let group_x = full_groups + if tail_pixels == 0 { 0 } else { 1 };
    // GPGPU_WALKER's RightExecutionMask applies to every SIMD hardware
    // thread, not just the final X workgroup.  A tail-derived mask therefore
    // removes the same lanes from every 16-pixel block and turns glyphs into
    // periodic vertical fragments.  All callers using this dispatch have an
    // explicit x/width guard, so run every group with all lanes enabled and
    // let the final padded lanes return without touching the surface.
    let right_mask = GPGPU_WALKER_SIMD16_MASK;
    Some(FillRect2dDispatch {
        group_x,
        group_y: height,
        right_mask,
    })
}

const fn copy_rect_2d_dispatch(width: u32, height: u32) -> Option<FillRect2dDispatch> {
    if width == 0 || height == 0 {
        return None;
    }
    // copy_rect_rgba8 handles two adjacent pixels per SIMD lane, whereas the
    // other 2D kernels handle one. Dispatch in work items, not pixels.
    let work_item_width = width.div_ceil(COPY_RECT_PIXELS_PER_LANE);
    fill_rect_2d_dispatch(work_item_width, height)
}

const fn sprite_quad_2d_dispatch(width: u32, height: u32) -> Option<FillRect2dDispatch> {
    let Some(mut dispatch) = fill_rect_2d_dispatch(width, height) else {
        return None;
    };
    dispatch.group_y = height.div_ceil(SPRITE_QUAD_WORKLIST_TILE_ROWS);
    Some(dispatch)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct SpriteQuadDescriptorDispatch {
    walker: FillRect2dDispatch,
    global_x: u32,
    global_tile_y: u32,
}

fn sprite_quad_descriptor_dispatch(
    desc: GpgpuSpriteQuadWorklistDesc,
    dst_width: u32,
    dst_height: u32,
) -> Option<SpriteQuadDescriptorDispatch> {
    if dst_width == 0 || dst_height == 0 {
        return None;
    }
    let xs = [desc.c0_x, desc.c1_x, desc.c2_x, desc.c3_x];
    let ys = [desc.c0_y, desc.c1_y, desc.c2_y, desc.c3_y];
    if xs.iter().chain(ys.iter()).any(|value| !value.is_finite()) {
        return None;
    }
    let mut left = xs[0];
    let mut right = xs[0];
    let mut top = ys[0];
    let mut bottom = ys[0];
    for value in xs.into_iter().skip(1) {
        left = left.min(value);
        right = right.max(value);
    }
    for value in ys.into_iter().skip(1) {
        top = top.min(value);
        bottom = bottom.max(value);
    }

    let min_x = (libm::floorf(left).max(0.0) as u32).min(dst_width - 1);
    let max_x = (libm::ceilf(right).max(0.0) as u32).min(dst_width - 1);
    let min_y = (libm::floorf(top).max(0.0) as u32).min(dst_height - 1);
    let max_y = (libm::ceilf(bottom).max(0.0) as u32).min(dst_height - 1);
    if max_x < min_x || max_y < min_y || right < 0.0 || bottom < 0.0 {
        return None;
    }

    let global_tile_y = min_y / SPRITE_QUAD_WORKLIST_TILE_ROWS;
    let final_tile_y = max_y / SPRITE_QUAD_WORKLIST_TILE_ROWS;
    Some(SpriteQuadDescriptorDispatch {
        walker: FillRect2dDispatch {
            group_x: max_x.saturating_sub(min_x).saturating_add(1).div_ceil(16),
            group_y: final_tile_y.saturating_sub(global_tile_y).saturating_add(1),
            right_mask: GPGPU_WALKER_SIMD16_MASK,
        },
        global_x: min_x,
        global_tile_y,
    })
}

const _: () = {
    let exact = fill_rect_2d_dispatch(16, 1).unwrap();
    assert!(exact.group_x == 1);
    assert!(exact.group_y == 1);
    assert!(exact.right_mask == GPGPU_WALKER_SIMD16_MASK);
    let tail = fill_rect_2d_dispatch(17, 3).unwrap();
    assert!(tail.group_x == 2);
    assert!(tail.group_y == 3);
    assert!(tail.right_mask == GPGPU_WALKER_SIMD16_MASK);
    let scanout = fill_rect_2d_dispatch(2560, 1440).unwrap();
    assert!(scanout.group_x == 160);
    assert!(scanout.group_y == 1440);
    assert!(scanout.right_mask == GPGPU_WALKER_SIMD16_MASK);
    let copy_exact = copy_rect_2d_dispatch(32, 1).unwrap();
    assert!(copy_exact.group_x == 1);
    let copy_tail = copy_rect_2d_dispatch(33, 3).unwrap();
    assert!(copy_tail.group_x == 2);
    assert!(copy_tail.group_y == 3);
    let sprite_scanout = sprite_quad_2d_dispatch(2560, 1440).unwrap();
    assert!(sprite_scanout.group_x == 160);
    assert!(sprite_scanout.group_y == 1440);
};
