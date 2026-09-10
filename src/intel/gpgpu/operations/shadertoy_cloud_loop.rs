// Optional, per-window Protean frame cache for low-cost background playback.
// Only the existing authenticated Protean executable is dispatched.
const SHADERTOY_CLOUD_FRAMES: usize = 96;
const SHADERTOY_FLAG_CACHED_CLOUD_LOOP: u32 = 4;

fn shadertoy_cloud_frame(frame: u32) -> usize {
    // Reflect at the endpoints instead of jumping between unrelated clouds.
    let period = (SHADERTOY_CLOUD_FRAMES as u32 - 1) * 2;
    let phase = frame % period;
    phase.min(period - phase) as usize
}
fn shadertoy_cloud_extent(width: u32, height: u32) -> (u32, u32) {
    let scale = libm::sqrtf(57_600. / (width as f32 * height as f32)).min(1.);
    let scale = scale.min(640. / width.max(height) as f32);
    (
        (libm::ceilf(width as f32 * scale) as u32).max(1),
        (libm::ceilf(height as f32 * scale) as u32).max(1),
    )
}
struct ShaderToyCloudLoop {
    frames: alloc::vec::Vec<Option<GpgpuOwnedRgba8Surface>>,
    extent: (u32, u32),
    quarantined: bool,
}
impl ShaderToyCloudLoop {
    const fn new() -> Self {
        Self {
            frames: alloc::vec::Vec::new(),
            extent: (0, 0),
            quarantined: false,
        }
    }
    fn render(
        &mut self,
        dst: GpgpuRgba8Surface,
        mut params: ShaderToyFrameParams,
    ) -> GpgpuRgba8KernelResult {
        if self.quarantined {
            return GpgpuRgba8KernelResult::default();
        }
        let extent = shadertoy_cloud_extent(dst.width, dst.height);
        if self.extent != extent {
            self.frames.clear();
            self.frames.resize_with(SHADERTOY_CLOUD_FRAMES, || None);
            self.extent = extent;
        }
        let index = shadertoy_cloud_frame(params.frame);
        params.flags = 0;
        params.time_seconds = 8. + index as f32 / 24.;
        params.mouse_x = extent.0 as f32 * 0.5;
        params.mouse_y = extent.1 as f32 * 0.5;
        params.click_x = 0.;
        params.click_y = 0.;
        if self.frames[index].is_none() {
            let Some(mut allocation) = allocate_font_instance_rgba8_surface(extent.0, extent.1)
            else {
                return GpgpuRgba8KernelResult::default();
            };
            allocation.system_service = true;
            self.frames[index] = Some(allocation);
            let source = self.frames[index].as_ref().unwrap().surface();
            let result = shadertoy_render_pass(source, params, ShaderToyPass::native(source));
            if !result.ok {
                // A submitted failure may still reference this allocation. Retain
                // it and refuse reuse until the owning runtime is torn down safely.
                if result.submitted {
                    self.quarantined = true;
                } else {
                    self.frames[index] = None;
                }
                return result;
            }
        }
        let source = self.frames[index].as_ref().unwrap().surface();
        let mut result = shadertoy_render_pass(
            dst,
            params,
            ShaderToyPass {
                phase: 2,
                source,
                ..ShaderToyPass::native(dst)
            },
        );
        if result.ok {
            result.release = Some(gpgpu_rgba8_release(dst));
        } else if result.submitted {
            self.quarantined = true;
        }
        result
    }
}
