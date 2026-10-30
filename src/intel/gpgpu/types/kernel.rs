#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuActivitySnapshot {
    pub(crate) available: bool,
    pub(crate) direct_rcs_enabled: bool,
    pub(crate) submit_seq: u32,
    pub(crate) ring_head: u32,
    pub(crate) ring_tail: u32,
    pub(crate) ring_start: u32,
    pub(crate) ring_ctl: u32,
    pub(crate) acthd: u32,
    pub(crate) mi_mode: u32,
    pub(crate) mode: u32,
    pub(crate) context_control: u32,
    pub(crate) execlist_control: u32,
    pub(crate) execlist_status_lo: u32,
    pub(crate) execlist_status_hi: u32,
    pub(crate) ipeir: u32,
    pub(crate) ipehr: u32,
    pub(crate) eir: u32,
    pub(crate) instdone: u32,
    pub(crate) instps: u32,
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
    /// H.264 matrix_coefficients in bits 0..7; video_full_range_flag in bit 8.
    pub(crate) colorimetry: u32,
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
    pub(crate) arc: u16,
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
    arc: u32,
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
/// Slot0 owns rectangular window visibility, not source-alpha blending between
/// windows.  The compositor chooses the topmost covering frame and writes its
/// already-authored RGBA pixel once; transparent pixels then belong to the
/// Slot0 plane itself and are blended only by display hardware over Pipe A.
#[expect(dead_code, reason = "Optional compositor ABI mode; Slot0 currently uses source-over blending")]
pub(crate) const UI4_COMPOSE_FLAG_OPAQUE_RECTS: u32 = 1 << 2;

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





pub(crate) const CPP_DEMO_MODE_GALLERY: u32 = 0;
pub(crate) const CPP_DEMO_MODE_CLOUD_HIGH_WISPS: u32 = 6;
pub(crate) const CPP_DEMO_MODE_COUNT: u32 = 7;
pub(crate) const CPP_DEMO_FLAG_VIGNETTE: u32 = 1 << 0;
pub(crate) const CPP_CLOUD_BRUSH_POINT_CAPACITY: usize = 32;

#[derive(Copy, Clone, Debug)]
pub(crate) struct CppDemoRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    pub(crate) rect_x: u32,
    pub(crate) rect_y: u32,
    pub(crate) rect_width: u32,
    pub(crate) rect_height: u32,
    pub(crate) time_seconds: f32,
    pub(crate) demo_mode: u32,
    pub(crate) seed: u32,
    pub(crate) flags: u32,
    pub(crate) brush_points: [u32; CPP_CLOUD_BRUSH_POINT_CAPACITY],
    pub(crate) brush_point_count: u32,
}

impl CppDemoRgba8Params {
    pub(crate) const fn new(time_seconds: f32, demo_mode: u32, seed: u32) -> Self {
        Self {
            dst_gpu: 0,
            dst_pitch_bytes: 0,
            dst_width: 0,
            dst_height: 0,
            rect_x: 0,
            rect_y: 0,
            rect_width: 0,
            rect_height: 0,
            time_seconds,
            demo_mode,
            seed,
            flags: CPP_DEMO_FLAG_VIGNETTE,
            brush_points: [0; CPP_CLOUD_BRUSH_POINT_CAPACITY],
            brush_point_count: 0,
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct CppAudioVisualizerRgba8Params {
    audio_gpu: u64,
    dst_gpu: u64,
    dst_pitch_bytes: u32,
    dst_width: u32,
    dst_height: u32,
    time_seconds: f32,
    frame: u32,
    flags: u32,
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

};
