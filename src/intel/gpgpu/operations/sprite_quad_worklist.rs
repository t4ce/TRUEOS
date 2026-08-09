/// Upload one immutable tightly-packed Helio RGBA atlas into its dedicated
/// system-service PPGTT range. The allocation intentionally lives for the
/// kernel lifetime; all Sprite Dig instances share the exact same pixels.
pub(crate) fn prepare_helio_sprite_atlas_rgba8(
    width: u32,
    height: u32,
    pitch_bytes: u32,
    pixels: &[u8],
) -> Option<GpgpuRgba8Surface> {
    if let Some(surface) = HELIO_SPRITE_ATLAS_SURFACE.get().copied() {
        let matches = surface.width == width
            && surface.height == height
            && surface.pitch_bytes == pitch_bytes;
        if !matches {
            crate::log_warn!(target: "gpgpu";
                "intel/gpgpu: helio-sprite-atlas rejected reason=resident-contract-mismatch requested={}x{} pitch={} resident={}x{} pitch={}\n",
                width,
                height,
                pitch_bytes,
                surface.width,
                surface.height,
                surface.pitch_bytes,
            );
        }
        return matches.then_some(surface);
    }
    if width == 0
        || height == 0
        || pitch_bytes != width.checked_mul(core::mem::size_of::<u32>() as u32)?
    {
        crate::log_warn!(target: "gpgpu";
            "intel/gpgpu: helio-sprite-atlas rejected reason=invalid-extent width={} height={} pitch={} expected_pitch={}\n",
            width,
            height,
            pitch_bytes,
            width.saturating_mul(core::mem::size_of::<u32>() as u32),
        );
        return None;
    }
    let raw_bytes = (pitch_bytes as usize).checked_mul(height as usize)?;
    if pixels.len() != raw_bytes || raw_bytes > HELIO_SPRITE_ATLAS_MAX_BYTES {
        crate::log_warn!(target: "gpgpu";
            "intel/gpgpu: helio-sprite-atlas rejected reason=payload-bounds payload={} expected={} capacity={}\n",
            pixels.len(),
            raw_bytes,
            HELIO_SPRITE_ATLAS_MAX_BYTES,
        );
        return None;
    }
    let bytes = align_up(raw_bytes, super::WARM_ALIGN)?;
    let Some((phys, virt)) = crate::dma::alloc(bytes, super::WARM_ALIGN) else {
        crate::log_warn!(target: "gpgpu";
            "intel/gpgpu: helio-sprite-atlas rejected reason=dma-allocation bytes={} alignment={} capacity={}\n",
            bytes,
            super::WARM_ALIGN,
            HELIO_SPRITE_ATLAS_MAX_BYTES,
        );
        return None;
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
        core::ptr::copy_nonoverlapping(pixels.as_ptr(), virt, pixels.len());
    }
    super::dma_flush(virt, bytes);
    let Some(surface) =
        GpgpuRgba8Surface::new(phys, HELIO_SPRITE_ATLAS_GPU, bytes, width, height, pitch_bytes)
    else {
        crate::dma::dealloc(virt, bytes);
        return None;
    };
    let published = *HELIO_SPRITE_ATLAS_SURFACE.call_once(|| surface);
    if published.phys != surface.phys {
        // Another caller won publication. Its immutable allocation is the
        // canonical one; reclaim this never-mapped duplicate immediately.
        crate::dma::dealloc(virt, bytes);
    }
    crate::log_info!(target: "gpgpu";
        "intel/gpgpu: helio-sprite-atlas resident=1 extent={}x{} pitch={} payload_bytes={} allocation_bytes={} gpu=0x{:X} phys=0x{:X} publication={}\n",
        published.width,
        published.height,
        published.pitch_bytes,
        raw_bytes,
        published.bytes,
        published.gpu,
        published.phys,
        if published.phys == surface.phys { "installed" } else { "reused-race-winner" },
    );
    Some(published)
}

fn log_helio_sprite_tilemap_rejection(
    reason: &'static str,
    dst: GpgpuRgba8Surface,
    atlas: GpgpuRgba8Surface,
    desc_count: usize,
    state_dwords: usize,
) {
    let sequence = HELIO_SPRITE_TILEMAP_REJECTION_LOGS
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    if sequence > 16 && !sequence.is_power_of_two() {
        return;
    }
    crate::log_warn!(target: "gpgpu";
        "intel/gpgpu: helio-sprite-tilemap unavailable seq={} reason={} direct_rcs_enabled={} claimed_device={} probe_ran={} probe_ok={} kernel_uploaded={} atlas_resident={} direct_state={} desc_resident={} quarantined={} dst_valid={} atlas_valid={} dst={}x{}:pitch{}:bytes{} atlas={}x{}:pitch{}:bytes{} descs={}/{} state_dwords={}/{}\n",
        sequence,
        reason,
        DIRECT_RCS_ENABLED as u8,
        super::claimed_device().is_some() as u8,
        SPRITE_QUAD_WORKLIST_RAN.load(Ordering::Acquire) as u8,
        SPRITE_QUAD_WORKLIST_OK.load(Ordering::Acquire) as u8,
        SPRITE_QUAD_WORKLIST_RGBA8_UPLOAD.lock().is_some() as u8,
        HELIO_SPRITE_ATLAS_SURFACE.get().is_some() as u8,
        DIRECT_RCS_STATE.lock().is_some() as u8,
        GPGPU_SPRITE_QUAD_WORKLIST_DESC.lock().is_some() as u8,
        direct_rcs_context_is_quarantined() as u8,
        dst.is_valid() as u8,
        atlas.is_valid() as u8,
        dst.width,
        dst.height,
        dst.pitch_bytes,
        dst.bytes,
        atlas.width,
        atlas.height,
        atlas.pitch_bytes,
        atlas.bytes,
        desc_count,
        SPRITE_QUAD_WORKLIST_MAX_DESCS,
        state_dwords,
        SPRITE_QUAD_TILEMAP_STATE_MAX_DWORDS,
    );
}

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

/// Render one bounded atlas tilemap followed by ordinary ordered sprite
/// descriptors directly into a UI4 producer allocation.
///
/// The C++/SPIR-V kernel reads `tilemap_state` after its fixed descriptor
/// array. A complete result includes the exact compute release required by
/// UI4; an ambiguous submitted result quarantines the system-service RCS lane
/// and deliberately returns no release.
pub(crate) fn sprite_quad_tilemap_rgba8_direct_result(
    dst: GpgpuRgba8Surface,
    atlas: GpgpuRgba8Surface,
    descs: &[GpgpuSpriteQuadWorklistDesc],
    tilemap_state: &[u32],
) -> GpgpuSpriteQuadWorklistResult {
    let reject = |reason| {
        log_helio_sprite_tilemap_rejection(reason, dst, atlas, descs.len(), tilemap_state.len());
        GpgpuSpriteQuadWorklistResult::default()
    };
    if !sprite_quad_worklist_ready() {
        return reject("sprite-kernel-probe-not-ready");
    }
    if !dst.is_valid() {
        return reject("destination-surface-invalid");
    }
    if !atlas.is_valid() {
        return reject("atlas-surface-invalid");
    }
    if dst.storage_order != GpgpuRgba8StorageOrder::Rgba
        || atlas.storage_order != GpgpuRgba8StorageOrder::Rgba
    {
        return reject("storage-order-not-rgba");
    }
    if descs.is_empty() || descs.len() > SPRITE_QUAD_WORKLIST_MAX_DESCS {
        return reject("descriptor-count");
    }
    if descs
        .iter()
        .filter(|desc| desc.flags & SPRITE_QUAD_WORKLIST_FLAG_TILEMAP != 0)
        .count()
        != 1
    {
        return reject("tilemap-descriptor-count");
    }
    if tilemap_state.len() > SPRITE_QUAD_TILEMAP_STATE_MAX_DWORDS {
        return reject("tilemap-state-capacity");
    }
    if tilemap_state.get(0) != Some(&SPRITE_QUAD_TILEMAP_MAGIC)
        || tilemap_state.get(1) != Some(&SPRITE_QUAD_TILEMAP_VERSION)
    {
        return reject("tilemap-state-header");
    }
    if gpu_ranges_overlap(atlas.gpu, atlas.bytes, dst.gpu, dst.bytes) {
        return reject("gpu-range-overlap");
    }
    if gpu_ranges_overlap(atlas.phys, atlas.bytes, dst.phys, dst.bytes) {
        return reject("physical-range-overlap");
    }
    let Some(desc_buffer) = sprite_quad_worklist_desc_buffer_once() else {
        return reject("descriptor-buffer-allocation");
    };
    let state_bytes = tilemap_state.len() * core::mem::size_of::<u32>();
    if SPRITE_QUAD_TILEMAP_STATE_OFFSET_BYTES.saturating_add(state_bytes) > desc_buffer.bytes {
        return reject("descriptor-buffer-state-capacity");
    }

    let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
    if direct_rcs_context_is_quarantined() {
        return reject("system-service-lane-quarantined");
    }
    unsafe {
        core::ptr::write_bytes(desc_buffer.virt, 0, desc_buffer.bytes);
        core::ptr::copy_nonoverlapping(
            descs.as_ptr() as *const u8,
            desc_buffer.virt,
            core::mem::size_of_val(descs),
        );
        core::ptr::copy_nonoverlapping(
            tilemap_state.as_ptr() as *const u8,
            desc_buffer.virt.add(SPRITE_QUAD_TILEMAP_STATE_OFFSET_BYTES),
            state_bytes,
        );
    }
    super::dma_flush(desc_buffer.virt, desc_buffer.bytes);

    let run = GpgpuSpriteQuadWorklistRun { src: atlas, descs };
    let started = direct_rcs_now_tick();
    let outcome = submit_sprite_quad_worklist_runs(dst, desc_buffer, core::slice::from_ref(&run));
    if outcome == GpgpuSubmissionOutcome::Unavailable {
        log_helio_sprite_tilemap_rejection(
            "direct-submit-unavailable",
            dst,
            atlas,
            descs.len(),
            tilemap_state.len(),
        );
    }
    if outcome == GpgpuSubmissionOutcome::SubmittedIncomplete {
        quarantine_direct_rcs_context("helio-sprite-tilemap-marker-timeout");
    }
    let complete = outcome == GpgpuSubmissionOutcome::Complete;
    GpgpuSpriteQuadWorklistResult {
        stats: GpgpuWorklistSubmitStats {
            descs: complete.then_some(descs.len()).unwrap_or(0),
            walkers: complete.then_some(descs.len()).unwrap_or(0),
            submits: usize::from(complete),
            submit_ms: direct_rcs_elapsed_ms_since(started),
            ..GpgpuWorklistSubmitStats::default()
        },
        outcome,
        release: complete.then(|| gpgpu_rgba8_release(dst)),
    }
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

fn font_sprite_quad_same_source(left: GpgpuRgba8Surface, right: GpgpuRgba8Surface) -> bool {
    left.phys == right.phys
        && left.gpu == right.gpu
        && left.bytes == right.bytes
        && left.width == right.width
        && left.height == right.height
        && left.pitch_bytes == right.pitch_bytes
        && left.storage_order == right.storage_order
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
    for (run_index, run) in runs.iter().enumerate() {
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

        // Reusing the exact immutable source in multiple ordered runs is safe.
        // Two distinct allocations may not occupy intersecting Font PPGTT VAs:
        // the later map would silently redirect the earlier run's binding.
        if runs[..run_index].iter().any(|previous| {
            gpu_ranges_overlap(previous.src.gpu, previous.src.bytes, run.src.gpu, run.src.bytes)
                && !font_sprite_quad_same_source(previous.src, run.src)
        }) {
            return None;
        }
    }
    Some(total_descs)
}

/// Prepare every Font-owned immutable object needed by the cached sprite path.
///
/// Font Rush calls this while its blank cache-charge interval is active. The
/// terminal wave therefore cannot be the first allocation/upload/map of the
/// descriptor page or sprite kernel. No batch is submitted and no destination
/// is touched here.
pub(crate) fn prepare_font_sprite_quad_worklist_rgba8() -> bool {
    let Some(desc_buffer) = font_sprite_quad_worklist_desc_buffer_once() else {
        return false;
    };
    let _font_guard = FONT_RCS_SUBMIT_LOCK.lock();
    if font_rcs_context_is_quarantined() {
        return false;
    }
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_sprite_quad_worklist_rgba8_kernel() else {
        return false;
    };
    let Some(state) = font_rcs_state_once(dev) else {
        return false;
    };
    direct_rcs_forcewake(dev)
        && direct_rcs_map_state(dev, state)
        && font_rcs_init_ppgtt_once(state)
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes)
        && direct_rcs_map_ppgtt_kernel(state, desc_buffer.gpu, desc_buffer.phys, desc_buffer.bytes)
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
        || gpu_ranges_overlap(desc_buffer.phys, desc_buffer.bytes, dst.phys, dst.bytes)
        || runs.iter().any(|run| {
            gpu_ranges_overlap(desc_buffer.gpu, desc_buffer.bytes, run.src.gpu, run.src.bytes)
                || gpu_ranges_overlap(
                    desc_buffer.phys,
                    desc_buffer.bytes,
                    run.src.phys,
                    run.src.bytes,
                )
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
