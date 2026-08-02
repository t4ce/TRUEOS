pub(crate) fn sprite_quad_worklist_rgba8_runs_over_result(
    dst: GpgpuRgba8Surface,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
) -> GpgpuWorklistSubmitResult {
    if !sprite_quad_worklist_ready() {
        return GpgpuWorklistSubmitResult::default();
    }
    let Some(desc_buffer) = sprite_quad_worklist_desc_buffer_once() else {
        return GpgpuWorklistSubmitResult::default();
    };
    let total_descs = runs
        .iter()
        .try_fold(0usize, |total, run| total.checked_add(run.descs.len()));
    let Some(total_descs) = total_descs else {
        return GpgpuWorklistSubmitResult::default();
    };
    if total_descs == 0 || total_descs > SPRITE_QUAD_WORKLIST_MAX_DESCS {
        return GpgpuWorklistSubmitResult::default();
    }
    if runs.iter().any(|run| run.descs.is_empty()) {
        return GpgpuWorklistSubmitResult::default();
    }

    let mut stats = GpgpuWorklistSubmitStats::default();
    let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
    if direct_rcs_context_is_quarantined() {
        return GpgpuWorklistSubmitResult::default();
    }
    unsafe {
        core::ptr::write_bytes(desc_buffer.virt, 0, desc_buffer.bytes);
        let out = desc_buffer.virt as *mut GpgpuSpriteQuadWorklistDesc;
        let mut index = 0usize;
        for run in runs {
            for desc in run.descs.iter().copied() {
                core::ptr::write_volatile(out.add(index), desc);
                index = index.saturating_add(1);
            }
        }
    }
    super::dma_flush(desc_buffer.virt, desc_buffer.bytes);

    let submit_start_tick = direct_rcs_now_tick();
    let outcome = submit_sprite_quad_worklist_runs(dst, desc_buffer, runs);
    if outcome != GpgpuSubmissionOutcome::Complete {
        return GpgpuWorklistSubmitResult { stats, outcome };
    }
    stats.submit_ms = stats
        .submit_ms
        .saturating_add(direct_rcs_elapsed_ms_since(submit_start_tick));
    stats.descs = total_descs;
    stats.walkers = runs.iter().fold(0usize, |total, run| {
        total.saturating_add(sprite_quad_worklist_walker_count(run.descs.len()))
    });
    stats.submits = 1;
    GpgpuWorklistSubmitResult { stats, outcome }
}

fn font_sprite_quad_worklist_desc_buffer_once() -> Option<GpgpuRectWorklistDescBuffer> {
    let mut guard = FONT_SPRITE_QUAD_WORKLIST_DESC.lock();
    if let Some(buffer) = *guard {
        return Some(buffer);
    }

    let bytes = align_up(SPRITE_QUAD_WORKLIST_DESC_BYTES, super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);

    // The numeric VA is intentionally shared with the other sprite descriptor
    // pages. Font owns a distinct PPGTT root and a distinct physical page.
    let buffer = GpgpuRectWorklistDescBuffer {
        phys,
        gpu: SPRITE_QUAD_WORKLIST_DESC_GPU,
        virt,
        bytes,
    };
    *guard = Some(buffer);
    Some(buffer)
}

fn font_sprite_quad_descriptor_is_valid(desc: GpgpuSpriteQuadWorklistDesc) -> bool {
    const VALID_FLAGS: u32 =
        SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER | SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC;

    let coordinates = [
        desc.c0_x, desc.c0_y, desc.c0_u, desc.c0_v, desc.c1_x, desc.c1_y, desc.c1_u, desc.c1_v,
        desc.c2_x, desc.c2_y, desc.c2_u, desc.c2_v, desc.c3_x, desc.c3_y, desc.c3_u, desc.c3_v,
    ];
    if coordinates.iter().any(|coordinate| !coordinate.is_finite())
        || desc.flags & !VALID_FLAGS != 0
        || (desc.flags & SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC != 0
            && desc.flags & SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER == 0)
    {
        return false;
    }

    let edge_x = [desc.c1_x - desc.c0_x, desc.c1_y - desc.c0_y];
    let edge_y = [desc.c3_x - desc.c0_x, desc.c3_y - desc.c0_y];
    let determinant = edge_x[0] * edge_y[1] - edge_x[1] * edge_y[0];
    determinant.is_finite() && determinant.abs() >= 0.00001
}

fn font_sprite_quad_worklist_request_descs(
    dst: GpgpuRgba8Surface,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
) -> Option<usize> {
    if !dst.is_valid() || dst.storage_order != GpgpuRgba8StorageOrder::Rgba || runs.is_empty() {
        return None;
    }
    let total_descs = runs
        .iter()
        .try_fold(0usize, |total, run| total.checked_add(run.descs.len()))?;
    if total_descs == 0 || total_descs > SPRITE_QUAD_WORKLIST_MAX_DESCS {
        return None;
    }
    for run in runs {
        if !run.src.is_valid()
            || run.src.storage_order != GpgpuRgba8StorageOrder::Rgba
            || run.descs.is_empty()
            || gpu_ranges_overlap(run.src.gpu, run.src.bytes, dst.gpu, dst.bytes)
            || gpu_ranges_overlap(run.src.phys, run.src.bytes, dst.phys, dst.bytes)
            || run
                .descs
                .iter()
                .copied()
                .any(|desc| !font_sprite_quad_descriptor_is_valid(desc))
        {
            return None;
        }
    }
    Some(total_descs)
}

/// Stamp one ordered set of immutable linear-RGBA tiles over an exact UI4
/// destination through the Font GuC context.
///
/// No clear is encoded. A descriptor with no flags performs a raw overwrite;
/// transparent glyph tiles use `SRC_OVER`, plus `PREMUL_SRC` when their RGB is
/// already multiplied by alpha. One walker and an ordering flush are emitted
/// per descriptor, so overlapping glyphs retain slice/run order.
///
/// The caller owns every source and the destination until `outcome` is
/// `Complete`. `SubmittedIncomplete` is irreversible: the Font context and its
/// descriptor bytes are quarantined, and all referenced storage must remain
/// pinned because late GPU reads or writes are still possible.
pub(crate) fn font_sprite_quad_worklist_rgba8_runs_over_result(
    dst: GpgpuRgba8Surface,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
) -> GpgpuSpriteQuadWorklistResult {
    let Some(total_descs) = font_sprite_quad_worklist_request_descs(dst, runs) else {
        return GpgpuSpriteQuadWorklistResult::default();
    };
    let Some(desc_buffer) = font_sprite_quad_worklist_desc_buffer_once() else {
        return GpgpuSpriteQuadWorklistResult::default();
    };

    // Font owns both this descriptor page and the encoder state. Keep the lane
    // lock through the marker/context-save proof so a later request can never
    // overwrite bytes which an ambiguous batch may still fetch.
    let _font_guard = FONT_RCS_SUBMIT_LOCK.lock();
    if font_rcs_context_is_quarantined()
        || gpu_ranges_overlap(desc_buffer.gpu, desc_buffer.bytes, dst.gpu, dst.bytes)
        || runs.iter().any(|run| {
            gpu_ranges_overlap(desc_buffer.gpu, desc_buffer.bytes, run.src.gpu, run.src.bytes)
        })
    {
        return GpgpuSpriteQuadWorklistResult::default();
    }

    unsafe {
        core::ptr::write_bytes(desc_buffer.virt, 0, desc_buffer.bytes);
        let out = desc_buffer.virt as *mut GpgpuSpriteQuadWorklistDesc;
        let mut index = 0usize;
        for run in runs {
            for descriptor in run.descs.iter().copied() {
                core::ptr::write_volatile(out.add(index), descriptor);
                index = index.saturating_add(1);
            }
        }
    }
    super::dma_flush(desc_buffer.virt, desc_buffer.bytes);

    let submit_started = direct_rcs_now_tick();
    let outcome = submit_font_sprite_quad_worklist_runs(dst, desc_buffer, runs);
    let mut stats = GpgpuWorklistSubmitStats::default();
    let release = if outcome == GpgpuSubmissionOutcome::Complete {
        stats.descs = total_descs;
        stats.walkers = total_descs;
        stats.submits = 1;
        stats.submit_ms = direct_rcs_elapsed_ms_since(submit_started);
        Some(gpgpu_rgba8_release(dst))
    } else {
        None
    };
    GpgpuSpriteQuadWorklistResult {
        stats,
        outcome,
        release,
    }
}

#[cfg(test)]
mod font_sprite_quad_worklist_tests {
    use super::*;

    fn surface(phys: u64, gpu: u64) -> GpgpuRgba8Surface {
        GpgpuRgba8Surface::new(phys, gpu, 4096, 16, 16, 64).unwrap()
    }

    fn descriptor(flags: u32) -> GpgpuSpriteQuadWorklistDesc {
        GpgpuSpriteQuadWorklistDesc {
            c0_x: 4.0,
            c0_y: 4.0,
            c0_u: 0.0,
            c0_v: 0.0,
            c1_x: 12.0,
            c1_y: 4.0,
            c1_u: 1.0,
            c1_v: 0.0,
            c2_x: 12.0,
            c2_y: 12.0,
            c2_u: 1.0,
            c2_v: 1.0,
            c3_x: 4.0,
            c3_y: 12.0,
            c3_u: 0.0,
            c3_v: 1.0,
            color_rgba: u32::MAX,
            flags,
        }
    }

    #[test]
    fn font_sprite_request_accepts_raw_straight_and_premultiplied_rgba() {
        let src = surface(0x1000, 0x0A00_0000);
        let dst = surface(0x3000, 0x0A01_0000);
        for flags in [
            0,
            SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER,
            SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER | SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC,
        ] {
            let descriptors = [descriptor(flags)];
            let runs = [GpgpuSpriteQuadWorklistRun {
                src,
                descs: &descriptors,
            }];
            assert_eq!(font_sprite_quad_worklist_request_descs(dst, &runs), Some(1));
        }
    }

    #[test]
    fn font_sprite_request_rejects_clear_xrgb_and_unpaired_premul_flags() {
        let src = surface(0x1000, 0x0A00_0000);
        let dst = surface(0x3000, 0x0A01_0000);
        for flags in [
            SPRITE_QUAD_WORKLIST_FLAG_CLEAR,
            SPRITE_QUAD_WORKLIST_FLAG_SOURCE_XRGB,
            SPRITE_QUAD_WORKLIST_FLAG_DEST_XRGB,
            SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC,
        ] {
            let descriptors = [descriptor(flags)];
            let runs = [GpgpuSpriteQuadWorklistRun {
                src,
                descs: &descriptors,
            }];
            assert_eq!(font_sprite_quad_worklist_request_descs(dst, &runs), None);
        }
    }

    #[test]
    fn font_sprite_request_rejects_destination_alias_and_descriptor_overflow() {
        let src = surface(0x1000, 0x0A00_0000);
        let descriptors = [descriptor(SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER)];
        let aliased = [GpgpuSpriteQuadWorklistRun {
            src,
            descs: &descriptors,
        }];
        assert_eq!(font_sprite_quad_worklist_request_descs(src, &aliased), None);

        let too_many =
            [descriptor(SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER); SPRITE_QUAD_WORKLIST_MAX_DESCS + 1];
        let runs = [GpgpuSpriteQuadWorklistRun {
            src,
            descs: &too_many,
        }];
        let dst = surface(0x3000, 0x0A01_0000);
        assert_eq!(font_sprite_quad_worklist_request_descs(dst, &runs), None);
    }
}
