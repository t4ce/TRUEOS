// Retained, color-independent R8 render target. No UI4 frame or scanout
// ownership: the font service owns the mask and all reader lifetimes. Extents
// do not depend on the scene scratch/depth surfaces (this path has no depth).
const RESIDENT_COVERAGE_MASK_MAX_WIDTH: u32 = 4096;
const RESIDENT_COVERAGE_MASK_MAX_HEIGHT: u32 = 4096;

// Logical atlas limit; each retired hardware submission retains the renderer's
// existing bounded batch/state allocation and audited GPU address ranges.
const RESIDENT_COVERAGE_MASK_MAX_DRAWS: usize = 3000;

fn sum_coverage_gpu_intervals(
    a: Option<ResidentSceneGpuTimestamp>,
    b: Option<ResidentSceneGpuTimestamp>,
) -> Option<ResidentSceneGpuTimestamp> {
    let (a, b) = (a?, b?);
    (a.hz == b.hz).then_some(ResidentSceneGpuTimestamp {
        ticks: a.ticks.saturating_add(b.ticks), hz: a.hz,
        ns: a.ns.saturating_add(b.ns),
    })
}

fn submit_coverage_mask_batches<T>(
    draws: &[T],
    mut submit: impl FnMut(&[T], Option<[u8; 4]>) -> Result<ResidentSceneGeometryResult, &'static str>,
) -> Result<ResidentSceneGeometryResult, &'static str> {
    if draws.is_empty() { return Err("microfont-coverage-mask-mesh"); }
    if draws.len() > RESIDENT_COVERAGE_MASK_MAX_DRAWS {
        return Err("microfont-coverage-draw-limit");
    }
    let mut chunks = draws.chunks(RESIDENT_SCENE_MAX_DRAWS);
    let mut total = submit(chunks.next().unwrap(), Some([0; 4]))?;
    if !total.completed { return Ok(total); }
    for chunk in chunks {
        // The preceding chunk has retired. Preserve its pixels and reuse the
        // same bounded command/state storage; clear only on the first chunk.
        let next = submit(chunk, None)?;
        total.completed = next.completed;
        total.prepare_us = total.prepare_us.saturating_add(next.prepare_us);
        total.gpu_poll_us = total.gpu_poll_us.saturating_add(next.gpu_poll_us);
        total.gpu_poll_iters = total.gpu_poll_iters.saturating_add(next.gpu_poll_iters);
        // GPU intervals sum active command-stream time. The frame's wall time
        // below also includes CPU preparation and gaps between submissions.
        total.gpu_timestamp = sum_coverage_gpu_intervals(total.gpu_timestamp, next.gpu_timestamp);
        total.gpu_target_ready_timestamp = sum_coverage_gpu_intervals(total.gpu_target_ready_timestamp, next.gpu_target_ready_timestamp);
        total.gpu_clear_setup_timestamp = sum_coverage_gpu_intervals(total.gpu_clear_setup_timestamp, next.gpu_clear_setup_timestamp);
        total.gpu_writeback_timestamp = sum_coverage_gpu_intervals(total.gpu_writeback_timestamp, next.gpu_writeback_timestamp);
        total.pipeline_stats = None;
        total.vue_capture |= next.vue_capture;
        if !total.completed { break; }
    }
    Ok(total)
}

pub(crate) struct ResidentCoverageMask {
    backing: crate::intel::gpgpu::GpgpuOwnedMask8Surface,
    render_gpu: u64,
    retired: bool,
}

impl ResidentCoverageMask {
    pub(crate) fn surface(&self) -> crate::intel::gpgpu::GpgpuMask8Surface {
        self.backing.surface()
    }

    fn retire_alias(&mut self) -> bool {
        if self.render_gpu == 0 {
            return true;
        }
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

impl Drop for ResidentCoverageMask {
    fn drop(&mut self) {
        if !self.retire_alias() {
            crate::log_error!(target: "render";
                "microfont-coverage-mask: retirement uncertain render_gpu=0x{:X} bytes={} action=pin-alias-and-backing\n",
                self.render_gpu, self.surface().bytes);
        }
        // The owned mask's Drop retires its separate font RCS mapping and
        // backing only after our Render0 alias has been removed successfully.
    }
}

pub(crate) fn create_resident_coverage_mask(
    width: u32,
    height: u32,
) -> Result<ResidentCoverageMask, &'static str> {
    if !resident_coverage_mask_shape_is_valid(width, height) {
        return Err("microfont-coverage-mask-shape");
    }
    if warm_state().is_none() {
        return Err("render-boot-not-ready");
    }
    let backing = crate::intel::gpgpu::allocate_font_coverage_mask(width, height)
        .ok_or("microfont-coverage-mask-alloc")?;
    let surface = backing.surface();
    let render_gpu =
        reserve_persistent_render_gpu_va(surface.bytes).ok_or("microfont-coverage-mask-va")?;
    // Use the established PAT3/UC render-target alias. No display plane
    // consumes this R8 allocation directly.
    if !map_render_ppgtt_scanout_range(render_gpu, surface.phys, surface.bytes) {
        recycle_persistent_render_gpu_va(render_gpu, surface.bytes);
        return Err("microfont-coverage-mask-map");
    }
    Ok(ResidentCoverageMask {
        backing,
        render_gpu,
        retired: true,
    })
}

pub(crate) fn release_resident_coverage_mask(mut mask: ResidentCoverageMask) -> bool {
    mask.retire_alias() && mask.backing.try_release_backing()
}

fn resident_coverage_mask_shape_is_valid(width: u32, height: u32) -> bool {
    width != 0
        && height != 0
        && width <= RESIDENT_COVERAGE_MASK_MAX_WIDTH
        && height <= RESIDENT_COVERAGE_MASK_MAX_HEIGHT
}

fn resident_coverage_draw_is_valid(draw: &ResidentSceneDraw<'_>) -> bool {
    matches!(draw.fragment_contract, ResidentSceneFragmentContract::MicrofontMask(_))
        && draw.rgba == [255; 4]
        && draw.depth_flags.is_none()
        && draw.sampled_texture.is_none()
        && draw.viewport_translation_px == [0.0; 2]
        && matches!(
            draw.topology,
            ResidentScenePrimitiveTopology::TriangleList
                | ResidentScenePrimitiveTopology::LineStripAdj
        )
        && match draw.topology {
            ResidentScenePrimitiveTopology::LineStripAdj => {
                draw.clip_adjacency && draw.line_width_px > 0.0
            }
            ResidentScenePrimitiveTopology::TriangleList => {
                !draw.clip_adjacency && draw.line_width_px == 0.0
            }
            _ => false,
        }
        && draw.point_width_px == 0
        && draw.mesh.carrier.is_none()
        && draw.mesh.vertex_format == TriangleVertexFormat::Float3
        && draw.mesh.vertex_stride == 12
}

/// Clear and draw retained quads, native-AA lines and joins into R8 coverage.
/// White ink writes 255 and the background is zero. Color/opacity application
/// belongs to a later consumer. There is no pixel readback or copy in this
/// operation, and its four GPU markers match the RGBA benchmark path.
pub(crate) fn render_resident_coverage_mask(
    draws: &[ResidentSceneDraw<'_>],
    destination: &mut ResidentCoverageMask,
) -> Result<ResidentSceneFrameResult, &'static str> {
    let surface = destination.surface();
    if !surface.is_valid()
        || !resident_coverage_mask_shape_is_valid(surface.width, surface.height)
        || destination.render_gpu == 0
        || !destination.retired
        || destination.backing.backing_is_quarantined()
    {
        return Err("microfont-coverage-mask-unavailable");
    }
    if draws.len() > RESIDENT_COVERAGE_MASK_MAX_DRAWS {
        return Err("microfont-coverage-draw-limit");
    }
    if draws.is_empty()
        || draws
            .iter()
            .any(|draw| !resident_coverage_draw_is_valid(draw))
    {
        return Err("microfont-coverage-mask-mesh");
    }
    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("render-busy");
    }
    let _summary_only = RenderSummaryOnlyGuard::enter();
    let started_ns = crate::chronos::monotonic_nanos();
    let result = (|| {
        let dev = crate::intel::claimed_device().ok_or("no-device")?;
        let warm = warm_state().ok_or("render-boot-not-ready")?;
        if !matches!(warm.device_id, 0x4680 | 0xa780) {
            return Err("microfont-coverage-device");
        }
        if !forcewake_render_acquire(warm) {
            return Err("forcewake");
        }
        if !ensure_smoke_buffers_mapped(dev, warm) {
            return Err("render-map");
        }
        // All Err exits from the batched encoder occur before submission.
        // Accepted submissions without retirement return completed=false and
        // pin the destination here before it can be reused or dropped.
        let geometry = submit_coverage_mask_batches(draws, |chunk, clear| submit_resident_scene_geometry_batched(
            dev,
            warm,
            chunk,
            clear,
            false,
            None,
            destination.render_gpu,
            SURFACE_FORMAT_R8_UNORM,
            surface.pitch_bytes as usize,
            surface.width as usize,
            surface.height as usize,
            None,
            None,
        ))?;
        destination.retired = geometry.completed;
        if !geometry.completed {
            destination.backing.quarantine_backing();
        }
        if geometry.completed && draws.len() > RESIDENT_SCENE_MAX_DRAWS {
            crate::log_important!(target: "render";
                "microfont-coverage-mask: draws={} submissions={} draw_cap={} per_submission={} completed=1\n",
                draws.len(), draws.len().div_ceil(RESIDENT_SCENE_MAX_DRAWS),
                RESIDENT_COVERAGE_MASK_MAX_DRAWS, RESIDENT_SCENE_MAX_DRAWS);
        }
        let frame_us = crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000;
        Ok(ResidentSceneFrameResult {
            completed_draws: if geometry.completed { draws.len() } else { 0 },
            requested_draws: draws.len(),
            changed_pixels: if geometry.completed {
                surface.width as usize * surface.height as usize
            } else {
                0
            },
            presented: false,
            width: surface.width,
            height: surface.height,
            gpu_poll_us: geometry.gpu_poll_us,
            gpu_poll_iters: geometry.gpu_poll_iters,
            gpu_timestamp: geometry.gpu_timestamp,
            gpu_target_ready_timestamp: geometry.gpu_target_ready_timestamp,
            resolve_us: 0,
            coverage_us: 0,
            present_copy_us: 0,
            present_copy_performed: false,
            coverage_submits: 0,
            coverage_walkers: 0,
            frame_complete: geometry.completed,
            incomplete_stage: (!geometry.completed)
                .then_some(ResidentSceneIncompleteStage::Geometry),
            release_fence: geometry.completed.then(|| ResidentSceneReleaseFence {
                phys: surface.phys,
                byte_len: surface.bytes,
                sequence: RESIDENT_SCENE_RELEASE_SEQUENCE
                    .fetch_add(1, Ordering::AcqRel)
                    .saturating_add(1),
            }),
        })
    })();
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    result
}
