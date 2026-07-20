pub(crate) fn fill_rect_worklist_rgba8_stats(
    dst: GpgpuRgba8Surface,
    descs: &[FillRectWorklistRgba8Desc],
) -> GpgpuWorklistSubmitStats {
    fill_rect_worklist_rgba8_stats_mode(dst, descs, false)
}

fn fill_rect_worklist_rgba8_stats_mode(
    dst: GpgpuRgba8Surface,
    descs: &[FillRectWorklistRgba8Desc],
    direct_scanout: bool,
) -> GpgpuWorklistSubmitStats {
    let Some(desc_buffer) = rect_worklist_desc_buffer_once() else {
        return GpgpuWorklistSubmitStats::default();
    };
    let mut stats = GpgpuWorklistSubmitStats::default();
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
        if !submit_fill_rect_worklist(dst, desc_buffer, params, direct_scanout) {
            break;
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
    stats
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
    let stats = fill_rect_worklist_rgba8_stats_mode(dst, &descs[..desc_count], direct_scanout);
    stats.descs == desc_count && stats.submits == 1
}
