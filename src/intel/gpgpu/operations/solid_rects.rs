fn lower_solid_rects(
    dst: GpgpuRgba8Surface,
    rects: &[GpgpuSolidRect],
) -> Option<Vec<GpgpuAlphaBlendWorklistDesc>> {
    if !dst.is_valid() {
        return None;
    }
    let mut descriptors = Vec::with_capacity(rects.len());
    for solid in rects {
        let Some(rect) = clip_gpgpu_rect_to_surface(solid.rect, dst.width, dst.height) else {
            continue;
        };
        let dst_x = i16::try_from(rect.x).ok()?;
        let dst_y = i16::try_from(rect.y).ok()?;
        if rect.width > u16::MAX as u32 || rect.height > u16::MAX as u32 {
            return None;
        }
        descriptors.push(GpgpuAlphaBlendWorklistDesc {
            src_xy: 0,
            dst_xy: pack_i16_pair_u32(dst_x, dst_y),
            size: pack_u16_pair_u32(rect.width as u16, rect.height as u16),
            flags: ALPHA_BLEND_WORKLIST_FLAG_COPY | ALPHA_BLEND_WORKLIST_FLAG_SOLID,
            color_rgba: solid.color_rgba,
        });
    }
    Some(descriptors)
}

fn solid_rects_rgba8_result_mode(
    dst: GpgpuRgba8Surface,
    rects: &[GpgpuSolidRect],
    direct_scanout: bool,
) -> GpgpuWorklistSubmitResult {
    let Some(descriptors) = lower_solid_rects(dst, rects) else {
        return GpgpuWorklistSubmitResult::default();
    };
    if descriptors.is_empty() {
        return GpgpuWorklistSubmitResult {
            outcome: GpgpuSubmissionOutcome::Complete,
            ..GpgpuWorklistSubmitResult::default()
        };
    }
    let Some(desc_buffer) = rect_worklist_desc_buffer_once() else {
        return GpgpuWorklistSubmitResult::default();
    };
    let mut stats = GpgpuWorklistSubmitStats::default();
    let mut outcome = GpgpuSubmissionOutcome::Unavailable;
    for chunk in descriptors.chunks(ALPHA_BLEND_WORKLIST_MAX_DESCS) {
        let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
        // Preserve the descriptor bytes after an ambiguous submission; the
        // quarantined system lane may still fetch them while retiring late.
        if direct_rcs_context_is_quarantined() {
            return GpgpuWorklistSubmitResult { stats, outcome };
        }
        unsafe {
            core::ptr::write_bytes(desc_buffer.virt, 0, desc_buffer.bytes);
            let out = desc_buffer.virt as *mut GpgpuAlphaBlendWorklistDesc;
            for (index, descriptor) in chunk.iter().copied().enumerate() {
                core::ptr::write_volatile(out.add(index), descriptor);
            }
        }
        super::dma_flush(desc_buffer.virt, desc_buffer.bytes);

        // SOLID descriptors do not logically dereference the source. Bind the
        // already-mapped destination at both surface slots so even compiler
        // speculation remains inside a valid, identically cached allocation.
        let params = AlphaBlendWorklistRgba8Params {
            src_gpu: dst.gpu,
            dst_gpu: dst.gpu,
            desc_gpu: desc_buffer.gpu,
            src_pitch_bytes: dst.pitch_bytes,
            dst_pitch_bytes: dst.pitch_bytes,
            desc_base: 0,
            desc_count: chunk.len() as u32,
        };
        let submit_started = direct_rcs_now_tick();
        outcome = submit_solid_rect_worklist(dst, desc_buffer, params, direct_scanout);
        if outcome != GpgpuSubmissionOutcome::Complete {
            return GpgpuWorklistSubmitResult { stats, outcome };
        }
        stats.submit_ms = stats
            .submit_ms
            .saturating_add(direct_rcs_elapsed_ms_since(submit_started));
        stats.descs = stats.descs.saturating_add(chunk.len());
        stats.walkers = stats
            .walkers
            .saturating_add(rect_worklist_walker_count(chunk.len()));
        stats.submits = stats.submits.saturating_add(1);
    }
    GpgpuWorklistSubmitResult { stats, outcome }
}

/// Fill retained offscreen rectangles through the consolidated alpha
/// compositor's source-free SOLID mode.
pub(crate) fn fill_solid_rects_rgba8_result(
    dst: GpgpuRgba8Surface,
    rects: &[GpgpuSolidRect],
) -> GpgpuWorklistSubmitResult {
    solid_rects_rgba8_result_mode(dst, rects, false)
}

/// Fill direct-scanout rectangles while preserving the submitted/incomplete
/// distinction required by UI4 lease quarantine.
pub(crate) fn fill_solid_rects_rgba8_scanout_result(
    dst: GpgpuRgba8Surface,
    rects: &[GpgpuSolidRect],
) -> GpgpuWorklistSubmitResult {
    solid_rects_rgba8_result_mode(dst, rects, true)
}

pub(crate) fn fill_solid_rects_rgba8(dst: GpgpuRgba8Surface, rects: &[GpgpuSolidRect]) -> bool {
    fill_solid_rects_rgba8_mode(dst, rects, false)
}

pub(crate) fn fill_solid_rects_rgba8_scanout(
    dst: GpgpuRgba8Surface,
    rects: &[GpgpuSolidRect],
) -> bool {
    fill_solid_rects_rgba8_mode(dst, rects, true)
}

fn fill_solid_rects_rgba8_mode(
    dst: GpgpuRgba8Surface,
    rects: &[GpgpuSolidRect],
    direct_scanout: bool,
) -> bool {
    const INLINE_RECTS: usize = 16;
    if rects.len() > INLINE_RECTS {
        return false;
    }
    let Some(descriptors) = lower_solid_rects(dst, rects) else {
        return false;
    };
    if descriptors.is_empty() {
        return true;
    }
    let result = solid_rects_rgba8_result_mode(dst, rects, direct_scanout);
    result.outcome == GpgpuSubmissionOutcome::Complete
        && result.stats.descs == descriptors.len()
        && result.stats.submits == 1
}
