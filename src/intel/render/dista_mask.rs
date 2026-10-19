// A color-independent offscreen target for the Dista experiment. This does
// not participate in UI4 frame ownership or extend display scanout formats.

// A 2x printable Basic sheet enables a one-time CPU box resolve for the
// visible quality preview. The direct mask path owns its target and never
// uses the scene scratch/depth allocations whose limits remain 2560x1440.
const RESIDENT_DISTA_MASK_MAX_WIDTH: u32 = 3072;
const RESIDENT_DISTA_MASK_MAX_HEIGHT: u32 = 2112;

pub(crate) struct ResidentDistaMask {
    backing: crate::intel::gpgpu::GpgpuOwnedMask8Surface,
    render_gpu: u64,
    retired: bool,
}

impl ResidentDistaMask {
    pub(crate) fn surface(&self) -> crate::intel::gpgpu::GpgpuMask8Surface {
        self.backing.surface()
    }

    fn retire_alias(&mut self) -> bool {
        if self.render_gpu == 0 { return true; }
        if !self.retired || self.backing.backing_is_quarantined() {
            self.backing.quarantine_backing();
            return false;
        }
        let surface = self.surface();
        if !unmap_render_ppgtt_range(self.render_gpu, surface.bytes) {
            self.backing.quarantine_backing();
            return false;
        }
        recycle_persistent_render_gpu_va(self.render_gpu, surface.bytes);
        self.render_gpu = 0;
        true
    }
}

impl Drop for ResidentDistaMask {
    fn drop(&mut self) {
        if !self.retire_alias() {
            crate::log_error!(target: "render";
                "microfont-dista-mask: retirement uncertain render_gpu=0x{:X} bytes={} action=pin-alias-and-backing\n",
                self.render_gpu, self.surface().bytes);
        }
        // The owned mask's Drop retires its separate font RCS mapping and
        // backing only after our Render0 alias has been removed successfully.
    }
}

pub(crate) fn create_resident_dista_mask(width: u32, height: u32)
    -> Result<ResidentDistaMask, &'static str>
{
    if !resident_dista_mask_shape_is_valid(width, height) {
        return Err("microfont-dista-mask-shape");
    }
    if warm_state().is_none() { return Err("render-boot-not-ready"); }
    let backing = crate::intel::gpgpu::allocate_font_coverage_mask(width, height)
        .ok_or("microfont-dista-mask-alloc")?;
    let surface = backing.surface();
    let render_gpu = reserve_persistent_render_gpu_va(surface.bytes)
        .ok_or("microfont-dista-mask-va")?;
    // Match the existing RGBA direct target's PAT3/UC alias so the measured
    // comparison changes format rather than simultaneously changing cache
    // policy. No display plane can consume this R8 allocation.
    if !map_render_ppgtt_scanout_range(render_gpu, surface.phys, surface.bytes) {
        recycle_persistent_render_gpu_va(render_gpu, surface.bytes);
        return Err("microfont-dista-mask-map");
    }
    Ok(ResidentDistaMask { backing, render_gpu, retired: true })
}

pub(crate) fn release_resident_dista_mask(mut mask: ResidentDistaMask) -> bool {
    mask.retire_alias() && mask.backing.try_release_backing()
}

fn resident_dista_mask_shape_is_valid(width: u32, height: u32) -> bool {
    width != 0 && height != 0
        && width <= RESIDENT_DISTA_MASK_MAX_WIDTH
        && height <= RESIDENT_DISTA_MASK_MAX_HEIGHT
}

fn resident_dista_mask_draw_is_valid(draw: &ResidentSceneDraw<'_>) -> bool {
    matches!(draw.fragment_contract, ResidentSceneFragmentContract::MicrofontDista)
        && draw.rgba == [255; 4] && draw.depth_flags.is_none()
        && draw.sampled_texture.is_none() && draw.viewport_translation_px == [0.0; 2]
        && draw.topology == ResidentScenePrimitiveTopology::TriangleList
        && draw.point_width_px == 0 && draw.mesh.carrier.is_none()
        && draw.mesh.vertex_format == TriangleVertexFormat::Float3
        && draw.mesh.vertex_stride == 12
}

/// Clear and draw an opaque Dista union into a retained R8 coverage mask.
/// White ink writes 255 and the background is zero. Color/opacity application
/// belongs to a later consumer. There is no pixel readback or copy in this
/// operation, and its four GPU markers match the RGBA benchmark path.
pub(crate) fn render_resident_dista_mask(
    mesh: &ResidentTriangleMesh,
    destination: &mut ResidentDistaMask,
) -> Result<ResidentSceneFrameResult, &'static str> {
    let surface = destination.surface();
    if !surface.is_valid() || !resident_dista_mask_shape_is_valid(surface.width, surface.height)
        || destination.render_gpu == 0 || !destination.retired
        || destination.backing.backing_is_quarantined()
    {
        return Err("microfont-dista-mask-unavailable");
    }
    let draw = ResidentSceneDraw {
        mesh, depth_flags: None, rgba: [255; 4], sampled_texture: None,
        fragment_contract: ResidentSceneFragmentContract::MicrofontDista,
        viewport_translation_px: [0.0; 2],
        topology: ResidentScenePrimitiveTopology::TriangleList, point_width_px: 0,
    };
    if !resident_dista_mask_draw_is_valid(&draw) { return Err("microfont-dista-mask-mesh"); }
    if PRIMARY_PROBE_IN_FLIGHT.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return Err("render-busy");
    }
    let _summary_only = RenderSummaryOnlyGuard::enter();
    let started_ns = crate::chronos::monotonic_nanos();
    let result = (|| {
        let dev = crate::intel::claimed_device().ok_or("no-device")?;
        let warm = warm_state().ok_or("render-boot-not-ready")?;
        if !matches!(warm.device_id, 0x4680 | 0xa780) { return Err("microfont-dista-device"); }
        if !forcewake_render_acquire(warm) { return Err("forcewake"); }
        if !ensure_smoke_buffers_mapped(dev, warm) { return Err("render-map"); }
        // All Err exits from the batched encoder occur before submission.
        // Accepted submissions without retirement return completed=false and
        // pin the destination here before it can be reused or dropped.
        let geometry = submit_resident_scene_geometry_batched(
            dev, warm, core::slice::from_ref(&draw), Some([0; 4]), false, None,
            destination.render_gpu, SURFACE_FORMAT_R8_UNORM, surface.pitch_bytes as usize,
            surface.width as usize, surface.height as usize, None, None,
        )?;
        destination.retired = geometry.completed;
        if !geometry.completed { destination.backing.quarantine_backing(); }
        let frame_us = crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000;
        Ok(ResidentSceneFrameResult {
            completed_draws: usize::from(geometry.completed), requested_draws: 1,
            changed_pixels: if geometry.completed { surface.width as usize * surface.height as usize } else { 0 },
            presented: false, width: surface.width, height: surface.height,
            frame_us, geometry_us: frame_us, geometry_prepare_us: geometry.prepare_us,
            gpu_poll_us: geometry.gpu_poll_us, gpu_poll_iters: geometry.gpu_poll_iters,
            gpu_timestamp: geometry.gpu_timestamp,
            gpu_target_ready_timestamp: geometry.gpu_target_ready_timestamp,
            gpu_clear_setup_timestamp: geometry.gpu_clear_setup_timestamp,
            gpu_writeback_timestamp: geometry.gpu_writeback_timestamp,
            resolve_us: 0, coverage_us: 0, present_copy_us: 0,
            present_copy_performed: false, coverage_submits: 0, coverage_walkers: 0, rgba: None,
            frame_complete: geometry.completed,
            incomplete_stage: (!geometry.completed).then_some(ResidentSceneIncompleteStage::Geometry),
            release_fence: geometry.completed.then(|| ResidentSceneReleaseFence {
                phys: surface.phys, byte_len: surface.bytes,
                sequence: RESIDENT_SCENE_RELEASE_SEQUENCE.fetch_add(1, Ordering::AcqRel).saturating_add(1),
            }),
        })
    })();
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    result
}

#[cfg(test)]
mod resident_dista_mask_tests {
    use super::*;

    #[test]
    fn mask_extent_supports_bounded_two_by_two_supersampling() {
        assert!(resident_dista_mask_shape_is_valid(1536, 1056));
        assert!(resident_dista_mask_shape_is_valid(3072, 2112));
        assert_eq!(RESIDENT_DISTA_MASK_MAX_WIDTH as usize * RESIDENT_DISTA_MASK_MAX_HEIGHT as usize,
            6_488_064);
        for (width, height) in [(0, 1), (1, 0), (RESIDENT_DISTA_MASK_MAX_WIDTH + 1, 1),
            (1, RESIDENT_DISTA_MASK_MAX_HEIGHT + 1)] {
            assert!(!resident_dista_mask_shape_is_valid(width, height));
        }
    }

    #[test]
    fn r8_atlas_surface_uses_one_coverage_byte_per_pixel() {
        let surface = crate::intel::gpgpu::GpgpuMask8Surface::new(
            0x1000, 0x2000, 1_622_016, 1536, 1056, 1536).unwrap();
        assert_eq!(surface.pitch_bytes, surface.width);
        assert_eq!(surface.bytes, surface.width as usize * surface.height as usize);
        assert!(crate::intel::gpgpu::GpgpuMask8Surface::new(
            0x1000, 0x2000, surface.bytes - 1, 1536, 1056, 1536).is_none());
        assert_eq!(SURFACE_FORMAT_R8_UNORM, 320);
    }
}

#[cfg(test)]
mod dista_render_target_packet_tests {
    use super::*;

    #[test]
    fn single_channel_native_descriptor_keeps_byte_pitch_and_normalized_path() {
        let target = TriangleRenderTargetSurface {
            format: SURFACE_FORMAT_R8_UNORM, pitch: 1536,
            width: 1536, height: 1056, gpu: 0x1234_5678_9000, msaa4: false,
        };
        let state = triangle_render_target_surface_state(target, 0x4680, 1).unwrap();
        assert_eq!((state[0] >> 18) & 0x1ff, 320);
        assert_eq!((state[0] >> 29) & 7, SURFTYPE_2D);
        assert_eq!(state[1], (RENDER_MOCS << 24) | (1 << 31));
        assert_eq!(state[2], 1535 | (1055 << 16));
        assert_eq!(state[3], 1535);
        assert_eq!(state[8], 0x5678_9000);
        assert_eq!(state[9], 0x1234);
        assert!(state[4..7].iter().chain(&state[10..]).all(|&word| word == 0));
        assert_eq!(state[0] & ((1 << 12) | (1 << 28)), 0);
    }

    #[test]
    fn rgba_descriptor_differs_only_in_format_and_byte_pitch() {
        let target = TriangleRenderTargetSurface {
            format: SURFACE_FORMAT_R8_UNORM, pitch: 1536,
            width: 1536, height: 1056, gpu: 0x1234_5678_9000, msaa4: false,
        };
        for device in [0x4680, 0xa780] {
            let mask = triangle_render_target_surface_state(target, device, 1).unwrap();
            let rgba = triangle_render_target_surface_state(TriangleRenderTargetSurface {
                format: SURFACE_FORMAT_R8G8B8A8_UNORM, pitch: 6144, ..target
            }, device, 1).unwrap();
            assert_eq!(rgba[0] & !(0x1ff << 18), mask[0] & !(0x1ff << 18));
            assert_eq!((rgba[0] >> 18) & 0x1ff, 199);
            assert_eq!(rgba[3], 6143);
            for word in (1..16).filter(|&word| word != 3) { assert_eq!(rgba[word], mask[word]); }
        }
    }

    #[test]
    fn supersampled_mask_descriptor_has_full_extent_without_msaa_storage() {
        let target = TriangleRenderTargetSurface {
            format: SURFACE_FORMAT_R8_UNORM, pitch: 3072,
            width: 3072, height: 2112, gpu: 0x1234_5678_9000, msaa4: false,
        };
        let state = triangle_render_target_surface_state(target, 0x4680, 1).unwrap();
        assert_eq!(state[2], 3071 | (2111 << 16));
        assert_eq!(state[3], 3071);
        assert_eq!(state[4], 0);
        assert_eq!(state[0] & ((1 << 12) | (1 << 28)), 0);
    }

    #[test]
    fn existing_four_sample_rgba_descriptor_retains_tile64_contract() {
        let target = TriangleRenderTargetSurface {
            format: SURFACE_FORMAT_R8G8B8A8_UNORM, pitch: 4096,
            width: 1024, height: 769, gpu: 0x1000, msaa4: true,
        };
        assert_eq!(triangle_render_target_surface_state(target, 0x4680, 3), Err("probe-msaa4-device"));
        let state = triangle_render_target_surface_state(target, 0x5690, 3).unwrap();
        assert_eq!(state[0] & ((1 << 12) | (1 << 28)), (1 << 12) | (1 << 28));
        assert_eq!(state[1], (RENDER_MOCS << 24) | 208);
        assert_eq!(state[4], 2 << 3);
    }
}
