/// State belongs to a window, not to the immutable artifact cache. The UI4
/// owner takes this value for the entire render and restores it after retirement.
pub(crate) struct ShaderToyRuntimeState {
    selected: u32,
    seed: u32,
    extent: (u32, u32),
    audio: Option<crate::aud::audio_visualizer::AudioVisualizerSubscription>,
    particles: Option<GpgpuOwnedParticleCraftState>,
    brush: CppCloudBrush,
}

impl ShaderToyRuntimeState {
    pub(crate) fn stop_audio(&mut self) {
        self.audio = None;
    }

    pub(crate) const fn new(seed: u32) -> Self {
        Self {
            selected: 0,
            seed,
            extent: (0, 0),
            audio: None,
            particles: None,
            brush: CppCloudBrush::new(),
        }
    }

    pub(crate) fn render(
        &mut self,
        dst: GpgpuRgba8Surface,
        params: ShaderToyFrameParams,
    ) -> GpgpuRgba8KernelResult {
        if !dst.is_valid() || !params.is_valid() || direct_rcs_context_is_quarantined() {
            return GpgpuRgba8KernelResult::default();
        }
        let Some(program) = shadertoy_package::program_id(params.shader_id) else {
            return GpgpuRgba8KernelResult::default();
        };
        // UI4 separately requires this window's successful package registration.
        // A resident embedded artifact from another consumer is not admission.
        if upload_shadertoy_kernel(program).is_none() {
            return GpgpuRgba8KernelResult::default();
        }
        if self.selected != params.shader_id {
            self.audio = None;
            self.particles = None;
            self.brush = CppCloudBrush::new();
            self.selected = params.shader_id;
        }
        if self.extent != (dst.width, dst.height) {
            self.brush.last = None;
            self.extent = (dst.width, dst.height);
        }
        match params.shader_id {
            1..=6 => shadertoy_rgba8_surface_full(dst, params),
            7 => {
                self.audio.get_or_insert_with(
                    crate::aud::audio_visualizer::AudioVisualizerSubscription::acquire,
                );
                let snapshot = crate::aud::audio_visualizer::snapshot();
                cpp_audio_visualizer_rgba8_surface_full(
                    dst,
                    params.time_seconds,
                    params.frame,
                    &snapshot,
                )
            }
            8..=14 => {
                if params.shader_id == 14 && params.flags & 2 != 0 {
                    let x = params
                        .mouse_x
                        .clamp(0.0, dst.width.saturating_sub(1) as f32)
                        as i32;
                    let y = (dst.height as f32 - params.mouse_y)
                        .clamp(0.0, dst.height.saturating_sub(1) as f32)
                        as i32;
                    // A held stationary pointer must not evict the entire history.
                    if self.brush.last != Some((x, y)) {
                        self.brush.drag_to(x, y, dst.width, dst.height);
                    }
                } else {
                    self.brush.last = None;
                }
                cpp_demo_rgba8_surface_full(
                    dst,
                    params.time_seconds,
                    params.shader_id - 8,
                    self.seed.rotate_left(13).wrapping_add(0xC0DE_C901),
                    &self.brush.points[..self.brush.count],
                )
            }
            15 => {
                let reset = self.particles.is_none();
                if reset {
                    self.particles = GpgpuOwnedParticleCraftState::allocate();
                }
                let Some(state) = self.particles.as_mut() else {
                    return GpgpuRgba8KernelResult::default();
                };
                let mut craft = ParticleCraftParamsV1::arc_forge(
                    params.time_seconds,
                    params.delta_seconds.clamp(0.001, 0.05),
                    self.seed.rotate_left(11).wrapping_add(0xC0FF_EE51),
                );
                if reset {
                    craft.flags |= PARTICLE_CRAFT_FLAG_RESET;
                }
                particle_craft_rgba8_frame_scaled(state, dst, craft,
                    particle_craft_catalog_divisor(dst.width, dst.height))
            }
            _ => GpgpuRgba8KernelResult::default(),
        }
    }
}
