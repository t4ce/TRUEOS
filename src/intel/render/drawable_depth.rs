// Persistent, owner-scoped D32 targets for immediate OpenGL submissions.
// Uses the existing resident allocator/PPGTT and hardware depth stage; no CPU
// depth clear, pixel loop or global scene-depth alias is involved.
pub(crate) struct DrawableDepth {
    storage: ResidentRenderBuffer,
    config: TriangleDepthConfig,
    initialized: AtomicBool,
}

pub(crate) fn drawable_depth_bytes(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 || width as usize > RESIDENT_SCENE_TARGET_WIDTH
        || height as usize > RESIDENT_SCENE_TARGET_HEIGHT { return None; }
    let pitch = crate::intel::align_up((width as usize).checked_mul(4)?, 128)?;
    let rows = crate::intel::align_up(height as usize, 32)?;
    crate::intel::align_up(pitch.checked_mul(rows)?, 4096)
}

pub(crate) fn create_drawable_depth(width: u32, height: u32) -> Result<DrawableDepth, &'static str> {
    let bytes = drawable_depth_bytes(width, height).ok_or("drawable-depth-shape")?;
    let dev = crate::intel::claimed_device().ok_or("no-device")?;
    if !device_is_gfx12(dev.device_id) { return Err("drawable-depth-device"); }
    let storage = allocate_resident_render_buffer(bytes)?;
    let config = TriangleDepthConfig {
        gpu_addr: storage.gpu_base(), hiz: None, hiz_clear: false,
        pitch_bytes: crate::intel::align_up(width as usize * 4, 128).unwrap() as u32,
        width, height,
        qpitch_rows_div4: (crate::intel::align_up(height as usize, 32).unwrap() / 4) as u32,
        write_enabled: false, compare_function: COMPARE_FUNCTION_LESS,
    };
    Ok(DrawableDepth { storage, config, initialized: AtomicBool::new(false) })
}

impl DrawableDepth {
    pub(crate) fn matches(&self, width: u32, height: u32) -> bool {
        self.config.width == width && self.config.height == height
    }
    pub(crate) fn bytes(&self) -> usize { self.storage.storage_bytes() }
    pub(crate) fn initialized(&self) -> bool { self.initialized.load(Ordering::Acquire) }
    pub(crate) fn mark_initialized(&self) { self.initialized.store(true, Ordering::Release); }
}

pub(crate) fn release_drawable_depth(depth: &DrawableDepth) -> bool {
    release_resident_render_buffer(&depth.storage)
}

// Intel COMPAREFUNCTION: ALWAYS=0, NEVER=1, LESS=2, EQUAL=3,
// LEQUAL=4, GREATER=5, NOTEQUAL=6, GEQUAL=7.
fn drawable_depth_compare(gl_ordinal: u32) -> u8 {
    [1, 2, 3, 4, 5, 6, 7, 0][(gl_ordinal & 7) as usize]
}

// The initial contents are established by a GPU clear before the first draw.
// An initialized attachment survives subsequent draws and color-only clears.
fn drawable_depth_needs_clear(initialized: bool, flags: u32) -> bool {
    !initialized || flags & v::vgpu::INDEXED_DRAW_CLEAR_DEPTH != 0
}

#[derive(Clone, Copy)]
struct DrawableDepthSubmission {
    config: Option<TriangleDepthConfig>,
    geometry_clear: bool,
    preserve_color: bool,
    test: bool,
    clear: bool,
}

pub(crate) fn render_drawable_depth_scene(
    draws: &[ResidentSceneDraw<'_>], clear_color: Option<[u8; 4]>,
    target: crate::intel::gpgpu::GpgpuRgba8Surface,
    depth: Option<&DrawableDepth>, flags: u32, diagnostics: bool,
) -> Result<ResidentSceneFrameResult, &'static str> {
    use v::vgpu::*;
    let geometry_clear = flags & INDEXED_DRAW_GEOMETRY_CLEAR != 0;
    let clear = drawable_depth_needs_clear(depth.is_none_or(|d| d.initialized()), flags);
    let config = if let Some(depth) = depth {
        if !depth.matches(target.width, target.height) { return Err("drawable-depth-extent"); }
        let mut config = depth.config;
        config.write_enabled = clear && geometry_clear || flags & INDEXED_DRAW_DEPTH_WRITE != 0;
        config.compare_function = if clear && geometry_clear { COMPARE_FUNCTION_ALWAYS }
            else { drawable_depth_compare(flags >> INDEXED_DRAW_DEPTH_COMPARE_SHIFT) };
        Some(config)
    } else {
        if clear || flags & INDEXED_DRAW_DEPTH_TEST != 0 { return Err("drawable-depth-missing"); }
        None
    };
    let submission = DrawableDepthSubmission {
        config, geometry_clear, preserve_color: clear_color.is_none(),
        clear: clear && !geometry_clear,
        test: clear && geometry_clear || flags & INDEXED_DRAW_DEPTH_TEST != 0,
    };
    // Geometry clears always load the target; only covered fragments are replaced.
    let clear_color = if geometry_clear { None } else { clear_color };
    submit_resident_scene_capture_inner_for_carrier(
        draws, None, None, &[], clear_color, diagnostics, false, false,
        ResidentSceneRasterQuality::SingleSample, target.width as usize, target.height as usize,
        ResidentSceneFrameOutput::DirectGpuSurface(target), None, clear_color.is_none(), None,
        Some(submission),
    )
}

#[cfg(test)]
mod drawable_depth_tests {
    use super::*;
    #[test]
    fn first_draw_initializes_depth_but_later_draws_preserve_it() {
        assert!(drawable_depth_needs_clear(false, 0));
        assert!(!drawable_depth_needs_clear(true, 0));
        assert!(drawable_depth_needs_clear(true, v::vgpu::INDEXED_DRAW_CLEAR_DEPTH));
    }
    #[test]
    fn depth_layout_is_tiled_and_bounded() {
        assert_eq!(drawable_depth_bytes(2560, 1440), Some(14_745_600));
        assert_eq!(drawable_depth_bytes(1, 1), Some(4096));
        assert_eq!(drawable_depth_bytes(0, 1), None);
        assert_eq!(drawable_depth_bytes(u32::MAX, 1), None);
    }
    #[test]
    fn gl_comparisons_translate_without_changing_ordering() {
        assert_eq!((0..8).map(drawable_depth_compare).collect::<alloc::vec::Vec<_>>(), [1,2,3,4,5,6,7,0]);
    }
}
