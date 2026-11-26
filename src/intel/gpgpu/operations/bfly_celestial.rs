const BFLY_CELESTIAL_ATLAS_PACKAGE: u32 = 40;
const BFLY_CELESTIAL_MASK_EDGE: u32 = 512;
const BFLY_CELESTIAL_MASK_BYTES: usize =
    (BFLY_CELESTIAL_MASK_EDGE * BFLY_CELESTIAL_MASK_EDGE) as usize;
const BFLY_SUN_MASK_BYTES: usize = 1000 * 560;
const BFLY_CELESTIAL_ATLAS_BYTES: usize = BFLY_CELESTIAL_MASK_BYTES * 2 + BFLY_SUN_MASK_BYTES;
const BFLY_CELESTIAL_INIT_PHASE: u32 = 3;
static BFLY_CELESTIAL_ATLAS: Mutex<Option<GpgpuOwnedRgba8Surface>> = Mutex::new(None);

fn upload_bfly_celestial_atlas() -> Option<GpgpuRgba8Surface> {
    let mut resident = BFLY_CELESTIAL_ATLAS.lock();
    if let Some(allocation) = resident.as_ref() {
        return Some(allocation.surface());
    }
    let moon = crate::gpu::app_shaders::bytes(BFLY_CELESTIAL_ATLAS_PACKAGE, 0)?;
    let earth = crate::gpu::app_shaders::bytes(BFLY_CELESTIAL_ATLAS_PACKAGE, 1)?;
    let sun = crate::gpu::app_shaders::bytes(BFLY_CELESTIAL_ATLAS_PACKAGE, 2)?;
    if moon.len() != BFLY_CELESTIAL_MASK_BYTES
        || earth.len() != BFLY_CELESTIAL_MASK_BYTES
        || sun.len() != BFLY_SUN_MASK_BYTES
    {
        return None;
    }
    // The existing read-only storage buffer binding uses an RGBA surface
    // descriptor. The kernel sees three consecutive raw one-byte mask planes.
    let rows = BFLY_CELESTIAL_ATLAS_BYTES.div_ceil(BFLY_CELESTIAL_MASK_EDGE as usize);
    let mut allocation =
        allocate_font_instance_rgba8_surface(BFLY_CELESTIAL_MASK_EDGE / 4, rows as u32)?;
    let surface = allocation.surface();
    if surface.pitch_bytes != BFLY_CELESTIAL_MASK_EDGE || surface.bytes < BFLY_CELESTIAL_ATLAS_BYTES
    {
        return None;
    }
    allocation.owner = Rgba8SurfaceOwner::SystemService;
    // The allocation is private until this immutable atlas enters the cache.
    unsafe {
        core::ptr::copy_nonoverlapping(moon.as_ptr(), allocation.virt, moon.len());
        core::ptr::copy_nonoverlapping(
            earth.as_ptr(),
            allocation.virt.add(moon.len()),
            earth.len(),
        );
        core::ptr::copy_nonoverlapping(
            sun.as_ptr(),
            allocation.virt.add(moon.len() + earth.len()),
            sun.len(),
        );
    }
    super::dma_flush(allocation.virt, BFLY_CELESTIAL_ATLAS_BYTES);
    *resident = Some(allocation);
    crate::log_info!(target: "gpgpu";
        "intel/gpgpu: bfly celestial R8 atlas uploaded bytes={} gpu=0x{:X}\n",
        BFLY_CELESTIAL_ATLAS_BYTES, surface.gpu,
    );
    Some(surface)
}

fn bfly_celestial_rect(params: ShaderToyFrameParams) -> Option<GpgpuRect> {
    let x = params.mouse_x as i32;
    let y = params.mouse_y as i32;
    let width = params.click_x as i32;
    let height = if params.date_seconds == 0.0 {
        width
    } else {
        params.date_seconds as i32
    };
    if x as f32 != params.mouse_x
        || y as f32 != params.mouse_y
        || width as f32 != params.click_x
        || (params.date_seconds != 0.0 && height as f32 != params.date_seconds)
        || width < 0
        || height < 0
        || ((width == 0 || height == 0)
            && !(params.time_seconds == 0.0 && width == 0 && height == 0))
        || !(0.0..=1.0).contains(&params.click_y)
        || !(0.0..=1.0).contains(&params.date_year)
        || !matches!(params.time_seconds, 0.0 | 1.0 | 2.0 | 3.0)
    {
        return None;
    }
    x.checked_add(width)?;
    y.checked_add(height)?;
    Some(GpgpuRect::new(x, y, width as u32, height as u32))
}

fn clip_bfly_celestial_rect(rect: GpgpuRect, dst: GpgpuRgba8Surface) -> Option<GpgpuRect> {
    let x = rect.x.max(0);
    let y = rect.y.max(0);
    let right = rect.x.checked_add(rect.width as i32)?.min(dst.width as i32);
    let bottom = rect
        .y
        .checked_add(rect.height as i32)?
        .min(dst.height as i32);
    if x >= right || y >= bottom {
        return None;
    }
    Some(GpgpuRect::new(
        x,
        y,
        (right - x) as u32,
        (bottom - y) as u32,
    ))
}

fn bfly_celestial_damage(old: Option<GpgpuRect>, current: Option<GpgpuRect>) -> GpgpuRect {
    match (old, current) {
        (Some(a), Some(b)) => {
            // Both inputs are clipped to the destination here.
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            let right = (a.x + a.width as i32).max(b.x + b.width as i32);
            let bottom = (a.y + a.height as i32).max(b.y + b.height as i32);
            GpgpuRect::new(x, y, (right - x) as u32, (bottom - y) as u32)
        }
        (Some(rect), None) | (None, Some(rect)) => rect,
        // Keep the normal publish fence even when an already empty frame is acquired.
        (None, None) => GpgpuRect::new(0, 0, 1, 1),
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct BflyCelestialBuffer {
    frame: crate::ui4::FrameHandle,
    phys: u64,
    width: u32,
    height: u32,
    tint_bits: u32,
    region: Option<GpgpuRect>,
}

struct BflyCelestialState {
    initialized: [Option<BflyCelestialBuffer>; crate::ui4::FrameBuffering::Quad.count()],
}

impl BflyCelestialState {
    const fn new() -> Self {
        Self {
            initialized: [None; crate::ui4::FrameBuffering::Quad.count()],
        }
    }

    fn render(
        &mut self,
        dst: GpgpuRgba8Surface,
        params: ShaderToyFrameParams,
        target: (crate::ui4::FrameHandle, u8),
    ) -> GpgpuRgba8KernelResult {
        let (frame, buffer_index) = target;
        let Some(rect) = bfly_celestial_rect(params) else {
            return GpgpuRgba8KernelResult::default();
        };
        let Some(cached) = self.initialized.get_mut(usize::from(buffer_index)) else {
            return GpgpuRgba8KernelResult::default();
        };
        let region = clip_bfly_celestial_rect(rect, dst);
        let current = BflyCelestialBuffer {
            frame,
            phys: dst.phys,
            width: dst.width,
            height: dst.height,
            tint_bits: params.date_year.to_bits(),
            region,
        };
        let initialize =
            cached.is_none_or(|previous| BflyCelestialBuffer { region, ..previous } != current);
        let damage = bfly_celestial_damage(cached.and_then(|state| state.region), region);
        let Some(atlas) = upload_bfly_celestial_atlas() else {
            return GpgpuRgba8KernelResult::default();
        };
        let mut result = shadertoy_render_pass(
            dst,
            params,
            ShaderToyPass {
                phase: if initialize {
                    BFLY_CELESTIAL_INIT_PHASE
                } else {
                    0
                },
                source: atlas,
                region: (!initialize).then_some(damage),
                focus: [
                    damage.x as f32,
                    damage.y as f32,
                    (damage.x as u32 + damage.width) as f32,
                    (damage.y as u32 + damage.height) as f32,
                ],
                ..ShaderToyPass::native(dst)
            },
        );
        if result.ok {
            result.release = Some(gpgpu_rgba8_release(dst));
            *cached = Some(current);
            if initialize {
                crate::log_trace!(target: "gpgpu";
                    "intel/gpgpu: bfly celestial gradient initialized frame={} buffer={} full={}x{}\n",
                    frame.raw(), buffer_index, dst.width, dst.height,
                );
            }
        }
        result
    }
}
