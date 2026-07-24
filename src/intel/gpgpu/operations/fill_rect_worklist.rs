pub(crate) fn fill_rect_worklist_rgba8_stats(
    dst: GpgpuRgba8Surface,
    descs: &[FillRectWorklistRgba8Desc],
) -> GpgpuWorklistSubmitStats {
    fill_rect_worklist_rgba8_result_mode(dst, descs, false).stats
}

fn fill_rect_worklist_rgba8_result_mode(
    dst: GpgpuRgba8Surface,
    descs: &[FillRectWorklistRgba8Desc],
    direct_scanout: bool,
) -> GpgpuWorklistSubmitResult {
    let Some(desc_buffer) = rect_worklist_desc_buffer_once() else {
        return GpgpuWorklistSubmitResult::default();
    };
    let mut stats = GpgpuWorklistSubmitStats::default();
    let mut outcome = GpgpuSubmissionOutcome::Unavailable;
    for chunk in descs.chunks(RECT_WORKLIST_MAX_DESCS) {
        if chunk.is_empty() {
            continue;
        }
        let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
        unsafe {
            core::ptr::write_bytes(desc_buffer.virt, 0, desc_buffer.bytes);
            let out = desc_buffer.virt as *mut FillRectWorklistRgba8Desc;
            for (index, desc) in chunk.iter().copied().enumerate() {
                core::ptr::write_volatile(out.add(index), desc);
            }
        }
        super::dma_flush(desc_buffer.virt, desc_buffer.bytes);

        let params = FillRectWorklistRgba8Params {
            dst_gpu: dst.gpu,
            desc_gpu: desc_buffer.gpu,
            dst_pitch_bytes: dst.pitch_bytes,
            desc_base: 0,
            desc_count: chunk.len() as u32,
        };
        let submit_start_tick = direct_rcs_now_tick();
        outcome = submit_fill_rect_worklist(dst, desc_buffer, params, direct_scanout);
        if outcome != GpgpuSubmissionOutcome::Complete {
            return GpgpuWorklistSubmitResult { stats, outcome };
        }
        stats.submit_ms = stats
            .submit_ms
            .saturating_add(direct_rcs_elapsed_ms_since(submit_start_tick));
        stats.descs = stats.descs.saturating_add(chunk.len());
        stats.walkers = stats
            .walkers
            .saturating_add(rect_worklist_walker_count(chunk.len()));
        stats.submits = stats.submits.saturating_add(1);
    }
    GpgpuWorklistSubmitResult { stats, outcome }
}

fn fill_solid_rects_rgba8_result_mode(
    dst: GpgpuRgba8Surface,
    rects: &[GpgpuSolidRect],
    direct_scanout: bool,
) -> GpgpuWorklistSubmitResult {
    let mut descs = Vec::with_capacity(rects.len());
    for solid in rects {
        let Some(rect) = clip_gpgpu_rect_to_surface(solid.rect, dst.width, dst.height) else {
            continue;
        };
        let Ok(dst_x) = i16::try_from(rect.x) else {
            return GpgpuWorklistSubmitResult::default();
        };
        let Ok(dst_y) = i16::try_from(rect.y) else {
            return GpgpuWorklistSubmitResult::default();
        };
        if rect.width > u16::MAX as u32 || rect.height > u16::MAX as u32 {
            return GpgpuWorklistSubmitResult::default();
        }
        descs.push(FillRectWorklistRgba8Desc {
            dst_xy: pack_i16_pair_u32(dst_x, dst_y),
            size: pack_u16_pair_u32(rect.width as u16, rect.height as u16),
            color_rgba: solid.color_rgba,
        });
    }
    if descs.is_empty() {
        return GpgpuWorklistSubmitResult {
            outcome: GpgpuSubmissionOutcome::Complete,
            ..GpgpuWorklistSubmitResult::default()
        };
    }
    fill_rect_worklist_rgba8_result_mode(dst, descs.as_slice(), direct_scanout)
}

/// Fill an arbitrary retained set of solid rectangles in an ordinary
/// offscreen allocation and preserve the submitted/incomplete distinction.
/// Retained surfaces are read again by the GPU and must therefore remain on
/// the same PAT0/WB contract for both producer and consumer mappings.
pub(crate) fn fill_solid_rects_rgba8_result(
    dst: GpgpuRgba8Surface,
    rects: &[GpgpuSolidRect],
) -> GpgpuWorklistSubmitResult {
    fill_solid_rects_rgba8_result_mode(dst, rects, false)
}

/// Fill an arbitrary retained set of solid rectangles in a scanout allocation
/// and report whether a failed batch crossed the hardware submission boundary.
/// UI4 producers use this distinction to cancel an untouched lease or
/// quarantine an allocation that a late GPU batch may still write.
pub(crate) fn fill_solid_rects_rgba8_scanout_result(
    dst: GpgpuRgba8Surface,
    rects: &[GpgpuSolidRect],
) -> GpgpuWorklistSubmitResult {
    fill_solid_rects_rgba8_result_mode(dst, rects, true)
}

/// Fill a small set of solid rectangles in one worklist submission.
///
/// This is the retained-UI overlay path: callers can add cursors and other
/// simple decorations to a GPU-owned frame without mapping or touching its
/// pixels on the CPU. Rectangles are clipped to `dst`; a fully clipped set is
/// a successful no-op.
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
    if !dst.is_valid() {
        return false;
    }
    if rects.is_empty() {
        return true;
    }
    if rects.len() > INLINE_RECTS {
        return false;
    }
    let mut descs = [FillRectWorklistRgba8Desc::default(); INLINE_RECTS];
    let mut desc_count = 0usize;
    for solid in rects {
        let Some(rect) = clip_gpgpu_rect_to_surface(solid.rect, dst.width, dst.height) else {
            continue;
        };
        let Ok(dst_x) = i16::try_from(rect.x) else {
            return false;
        };
        let Ok(dst_y) = i16::try_from(rect.y) else {
            return false;
        };
        if rect.width > u16::MAX as u32 || rect.height > u16::MAX as u32 {
            return false;
        }
        descs[desc_count] = FillRectWorklistRgba8Desc {
            dst_xy: pack_i16_pair_u32(dst_x, dst_y),
            size: pack_u16_pair_u32(rect.width as u16, rect.height as u16),
            color_rgba: solid.color_rgba,
        };
        desc_count += 1;
    }
    if desc_count == 0 {
        return true;
    }
    let result = fill_rect_worklist_rgba8_result_mode(dst, &descs[..desc_count], direct_scanout);
    result.outcome == GpgpuSubmissionOutcome::Complete
        && result.stats.descs == desc_count
        && result.stats.submits == 1
}
