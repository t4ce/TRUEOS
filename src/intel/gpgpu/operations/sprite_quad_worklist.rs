



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

    #[test]
    fn font_sprite_request_rejects_distinct_sources_with_overlapping_gpu_va() {
        let first = surface(0x1000, 0x0A00_0000);
        let second = surface(0x3000, 0x0A00_0000);
        let dst = surface(0x5000, 0x0A01_0000);
        let descriptors = [descriptor(SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER)];
        let runs = [
            GpgpuSpriteQuadWorklistRun {
                src: first,
                descs: &descriptors,
            },
            GpgpuSpriteQuadWorklistRun {
                src: second,
                descs: &descriptors,
            },
        ];
        assert_eq!(font_sprite_quad_worklist_request_descs(dst, &runs), None);

        let same_source_runs = [
            GpgpuSpriteQuadWorklistRun {
                src: first,
                descs: &descriptors,
            },
            GpgpuSpriteQuadWorklistRun {
                src: first,
                descs: &descriptors,
            },
        ];
        assert_eq!(font_sprite_quad_worklist_request_descs(dst, &same_source_runs), Some(2));
    }
}
