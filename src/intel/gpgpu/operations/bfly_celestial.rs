const BFLY_CELESTIAL_ATLAS_PACKAGE: u32 = 35;
const BFLY_CELESTIAL_MASK_EDGE: u32 = 512;
const BFLY_CELESTIAL_MASK_BYTES: usize =
    (BFLY_CELESTIAL_MASK_EDGE * BFLY_CELESTIAL_MASK_EDGE) as usize;
const BFLY_CELESTIAL_INIT_PHASE: u32 = 3;
static BFLY_CELESTIAL_ATLAS: Mutex<Option<GpgpuOwnedRgba8Surface>> = Mutex::new(None);

fn upload_bfly_celestial_atlas() -> Option<GpgpuRgba8Surface> {
    let mut resident = BFLY_CELESTIAL_ATLAS.lock();
    if let Some(allocation) = resident.as_ref() {
        return Some(allocation.surface());
    }
    let moon = crate::gpu::app_shaders::bytes(BFLY_CELESTIAL_ATLAS_PACKAGE, 0)?;
    let earth = crate::gpu::app_shaders::bytes(BFLY_CELESTIAL_ATLAS_PACKAGE, 1)?;
    if moon.len() != BFLY_CELESTIAL_MASK_BYTES || earth.len() != BFLY_CELESTIAL_MASK_BYTES {
        return None;
    }
    // The existing read-only storage buffer binding uses an RGBA surface
    // descriptor. The kernel sees two consecutive raw one-byte mask planes.
    let mut allocation = allocate_font_instance_rgba8_surface(
        BFLY_CELESTIAL_MASK_EDGE / 4,
        BFLY_CELESTIAL_MASK_EDGE * 2,
    )?;
    let surface = allocation.surface();
    if surface.pitch_bytes != BFLY_CELESTIAL_MASK_EDGE
        || surface.bytes < BFLY_CELESTIAL_MASK_BYTES * 2
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
    }
    super::dma_flush(allocation.virt, moon.len() + earth.len());
    *resident = Some(allocation);
    crate::log_info!(target: "gpgpu";
        "intel/gpgpu: bfly celestial R8 atlas uploaded bytes={} gpu=0x{:X}\n",
        moon.len() + earth.len(), surface.gpu,
    );
    Some(surface)
}

fn bfly_celestial_square(params: ShaderToyFrameParams) -> Option<GpgpuRect> {
    let x = params.mouse_x as i32;
    let y = params.mouse_y as i32;
    let size = params.click_x as i32;
    if x as f32 != params.mouse_x
        || y as f32 != params.mouse_y
        || size <= 0
        || size as f32 != params.click_x
        || !(0.0..=1.0).contains(&params.click_y)
        || !(params.time_seconds == 0.0 || params.time_seconds == 1.0 || params.time_seconds == 2.0)
    {
        return None;
    }
    x.checked_add(size)?;
    y.checked_add(size)?;
    Some(GpgpuRect::new(x, y, size as u32, size as u32))
}

fn clip_bfly_celestial_square(square: GpgpuRect, dst: GpgpuRgba8Surface) -> Option<GpgpuRect> {
    let x = square.x.max(0);
    let y = square.y.max(0);
    let right = square
        .x
        .checked_add(square.width as i32)?
        .min(dst.width as i32);
    let bottom = square
        .y
        .checked_add(square.height as i32)?
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

#[derive(Copy, Clone, Eq, PartialEq)]
struct BflyCelestialBuffer {
    frame: crate::ui4::FrameHandle,
    phys: u64,
    width: u32,
    height: u32,
    region: GpgpuRect,
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
        let Some(square) = bfly_celestial_square(params) else {
            return GpgpuRgba8KernelResult::default();
        };
        let Some(cached) = self.initialized.get_mut(usize::from(buffer_index)) else {
            return GpgpuRgba8KernelResult::default();
        };
        let Some(region) = clip_bfly_celestial_square(square, dst) else {
            return GpgpuRgba8KernelResult::default();
        };
        let current = BflyCelestialBuffer {
            frame,
            phys: dst.phys,
            width: dst.width,
            height: dst.height,
            region,
        };
        let initialize = *cached != Some(current);
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
                region: (!initialize).then_some(region),
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
