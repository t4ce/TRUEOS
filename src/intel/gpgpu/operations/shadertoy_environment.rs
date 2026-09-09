// Program 16: one window-owned, resident Chroma cubemap. The expensive bake
// depends on the world generation/palette/preset, never camera or scanout size.
const SHADERTOY_ENVIRONMENT_FACE: u32 = 1024;
const SHADERTOY_ENVIRONMENT_STRIDE: u32 = SHADERTOY_ENVIRONMENT_FACE + 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ShaderToyEnvironmentKey {
    generation: u32,
    colors: [u32; 3],
    count: u32,
    cathedral: bool,
}

impl ShaderToyEnvironmentKey {
    fn from_params(params: ShaderToyFrameParams) -> Option<Self> {
        let colors = [params.date_year, params.date_month, params.date_day];
        if colors.iter().any(|c| *c < 0.0 || *c > 0xffffff as f32 || *c != (*c as u32) as f32)
            || !matches!(params.sample_rate, 1.0 | 2.0 | 3.0)
            || !matches!(params.date_seconds, 0.0 | 1.0)
        {
            return None;
        }
        Some(Self {
            generation: params.frame,
            colors: colors.map(|c| c as u32),
            count: params.sample_rate as u32,
            cathedral: params.date_seconds == 1.0,
        })
    }
}

struct ShaderToyEnvironment {
    allocation: Option<GpgpuOwnedRgba8Surface>,
    ready: Option<ShaderToyEnvironmentKey>,
}

impl ShaderToyEnvironment {
    const fn new() -> Self {
        Self { allocation: None, ready: None }
    }

    fn render(&mut self, dst: GpgpuRgba8Surface, mut params: ShaderToyFrameParams) -> GpgpuRgba8KernelResult {
        if params.time_seconds < 0.5 {
            // Leaving Key 5 retains the allocation, but does no bake or lookup.
            return shadertoy_rgba8_surface_full(dst, params);
        }
        let Some(key) = ShaderToyEnvironmentKey::from_params(params) else {
            return GpgpuRgba8KernelResult::default();
        };
        let q = [params.mouse_x, params.mouse_y, params.click_x, params.click_y];
        let length = libm::sqrtf(q.iter().map(|x| x * x).sum());
        if !length.is_finite() || length < 0.0001 || !(0.05..=4.0).contains(&params.delta_seconds) {
            return GpgpuRgba8KernelResult::default();
        }
        [params.mouse_x, params.mouse_y, params.click_x, params.click_y] = q.map(|x| x / length);
        if self.allocation.is_none() {
            let Some(mut allocation) = allocate_font_instance_rgba8_surface(
                SHADERTOY_ENVIRONMENT_STRIDE * 3, SHADERTOY_ENVIRONMENT_STRIDE * 2,
            ) else { return GpgpuRgba8KernelResult::default(); };
            // Unique persistent VA, mapped/retired only by SystemService RCS.
            allocation.system_service = true;
            self.allocation = Some(allocation);
        }
        let atlas = self.allocation.as_ref().unwrap().surface();
        if self.ready != Some(key) {
            self.ready = None;
            // Every previous lookup retired before returning. Reuse the one
            // allocation while UI4 keeps showing its old published background.
            // No partially baked face is ever sampled or published.
            let result = shadertoy_render_pass(atlas, params, ShaderToyPass {
                phase: 1, ..ShaderToyPass::native(atlas)
            });
            if !result.ok { return result; }
            self.ready = Some(key);
            crate::log_info!(target: "ui4/blueprint-frame";
                "Chroma environment ready generation={} preset={} colors={} faces=6 face_size={} bake_ms={} resident_bytes={}\n",
                key.generation, key.cathedral as u8, key.count,
                SHADERTOY_ENVIRONMENT_FACE, result.submit_ms, atlas.bytes,
            );
        }
        let mut result = shadertoy_render_pass(dst, params, ShaderToyPass {
            phase: 2, source: atlas, ..ShaderToyPass::native(dst)
        });
        if result.ok { result.release = Some(gpgpu_rgba8_release(dst)); }
        result
    }
}
