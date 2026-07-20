pub(crate) fn fill_rect_rgba8_stats(
    dst: GpgpuRgba8Surface,
    rect: GpgpuRect,
    color_rgba: u32,
) -> GpgpuSubmitStats {
    let Some(params) = lower_fill_rect(dst, rect, color_rgba) else {
        return GpgpuSubmitStats::default();
    };
    submit_fill_rect_2d_with_stats(dst, params)
}

/// Copy one rectangle with one two-dimensional submission and report success
/// only after that dispatch retired.
pub(crate) fn copy_rect_rgba8_complete(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
) -> bool {
    copy_rect_rgba8_complete_mode(src, src_rect, dst, dst_xy, false)
}

pub(crate) fn copy_rect_rgba8_complete_mode(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
    direct_scanout: bool,
) -> bool {
    let Some(params) = lower_copy_rect(src, src_rect, dst, dst_xy) else {
        return false;
    };
    submit_copy_rect_2d(src, dst, params, direct_scanout)
}

/// Resolve one gfx12.5 Tile64 R8G8B8A8 4x-MSAA surface into linear RGBA8.
///
/// This is deliberately a single two-dimensional SIMD16 dispatch: resident
/// scenes pay one GPU resolve per complete frame rather than one submission
/// per scanline/span.
pub(crate) fn resolve_tile64_msaa4_rgba8(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    width: u32,
    height: u32,
) -> bool {
    resolve_tile64_msaa4_rgba8_mode(src, dst, width, height, false)
}

pub(crate) fn resolve_tile64_msaa4_rgba8_mode(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    width: u32,
    height: u32,
    direct_scanout: bool,
) -> bool {
    let Some(params) =
        lower_copy_rect(src, GpgpuRect::new(0, 0, width, height), dst, GpgpuPoint::new(0, 0))
    else {
        return false;
    };
    submit_resolve_tile64_msaa4_2d(src, dst, params, direct_scanout)
}

fn reserve_font_coverage_gpu_va(bytes: usize) -> Option<u64> {
    let bytes = align_up(bytes, super::WARM_ALIGN)? as u64;
    {
        let mut free = FONT_COVERAGE_GPU_VA_FREE.lock();
        if let Some(index) = free
            .iter()
            .position(|(start, end)| end.saturating_sub(*start) >= bytes)
        {
            let (start, end) = free[index];
            let next = start.checked_add(bytes)?;
            if next == end {
                free.swap_remove(index);
            } else {
                free[index].0 = next;
            }
            return Some(start);
        }
    }
    loop {
        let current = FONT_COVERAGE_GPU_VA_CURSOR.load(Ordering::Acquire);
        let aligned = current.checked_add((super::WARM_ALIGN - 1) as u64)?
            & !((super::WARM_ALIGN - 1) as u64);
        let next = aligned.checked_add(bytes)?;
        if aligned < DIRECT_RCS_GPU_VA_FONT_COVERAGE_PRIMARY_LIMIT
            && next > DIRECT_RCS_GPU_VA_FONT_COVERAGE_PRIMARY_LIMIT
        {
            let _ = FONT_COVERAGE_GPU_VA_CURSOR.compare_exchange(
                current,
                DIRECT_RCS_GPU_VA_FONT_COVERAGE_SECONDARY_BASE,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            continue;
        }
        if (DIRECT_RCS_GPU_VA_FONT_COVERAGE_PRIMARY_LIMIT
            ..DIRECT_RCS_GPU_VA_FONT_COVERAGE_SECONDARY_BASE)
            .contains(&aligned)
        {
            let _ = FONT_COVERAGE_GPU_VA_CURSOR.compare_exchange(
                current,
                DIRECT_RCS_GPU_VA_FONT_COVERAGE_SECONDARY_BASE,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            continue;
        }
        if next > DIRECT_RCS_GPU_VA_FONT_COVERAGE_LIMIT {
            return None;
        }
        if FONT_COVERAGE_GPU_VA_CURSOR
            .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Some(aligned);
        }
    }
}

fn recycle_font_coverage_gpu_va(gpu: u64, bytes: usize) {
    let Some(bytes) = align_up(bytes, super::WARM_ALIGN).map(|value| value as u64) else {
        return;
    };
    let Some(end) = gpu.checked_add(bytes) else {
        return;
    };
    let in_primary = gpu >= DIRECT_RCS_GPU_VA_FONT_COVERAGE_BASE
        && end <= DIRECT_RCS_GPU_VA_FONT_COVERAGE_PRIMARY_LIMIT;
    let in_secondary = gpu >= DIRECT_RCS_GPU_VA_FONT_COVERAGE_SECONDARY_BASE
        && end <= DIRECT_RCS_GPU_VA_FONT_COVERAGE_LIMIT;
    if !in_primary && !in_secondary {
        return;
    }
    let mut free = FONT_COVERAGE_GPU_VA_FREE.lock();
    free.push((gpu, end));
    free.sort_unstable_by_key(|range| range.0);
    let mut write = 0usize;
    for read in 0..free.len() {
        let range = free[read];
        if write != 0 && range.0 <= free[write - 1].1 {
            free[write - 1].1 = free[write - 1].1.max(range.1);
        } else {
            free[write] = range;
            write += 1;
        }
    }
    free.truncate(write);
}

/// Allocate one persistent linear R8 mask with its own PPGTT virtual range.
/// Distinct simultaneously-live masks are never remapped over one another.
pub(crate) fn allocate_font_coverage_mask(
    width: u32,
    height: u32,
) -> Option<GpgpuOwnedMask8Surface> {
    if width == 0 || height == 0 {
        return None;
    }
    let pitch_bytes = u32::try_from(align_up(width as usize, 64)?).ok()?;
    let raw_bytes = (pitch_bytes as usize).checked_mul(height as usize)?;
    let bytes = align_up(raw_bytes, super::WARM_ALIGN)?;
    if bytes > DIRECT_RCS_FONT_COVERAGE_MASK_MAX_BYTES {
        return None;
    }
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    let Some(gpu) = reserve_font_coverage_gpu_va(bytes) else {
        crate::dma::dealloc(virt, bytes);
        return None;
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);
    let Some(surface) = GpgpuMask8Surface::new(phys, gpu, bytes, width, height, pitch_bytes) else {
        crate::dma::dealloc(virt, bytes);
        recycle_font_coverage_gpu_va(gpu, bytes);
        return None;
    };
    Some(GpgpuOwnedMask8Surface { surface, virt })
}

fn run_font_outline_coverage_r8_self_test() -> bool {
    const WIDTH: u32 = 19;
    const HEIGHT: u32 = 11;
    const OPS: [[u32; 8]; 5] = [
        [0, 2.0f32.to_bits(), 2.0f32.to_bits(), 0, 0, 0, 0, 0],
        [1, 17.0f32.to_bits(), 2.0f32.to_bits(), 0, 0, 0, 0, 0],
        [1, 17.0f32.to_bits(), 9.0f32.to_bits(), 0, 0, 0, 0, 0],
        [1, 2.0f32.to_bits(), 9.0f32.to_bits(), 0, 0, 0, 0, 0],
        [4, 0, 0, 0, 0, 0, 0, 0],
    ];
    let Some(mask) = allocate_font_coverage_mask(WIDTH, HEIGHT) else {
        return false;
    };
    let input_bytes = OPS.len() * core::mem::size_of::<[u32; 8]>();
    let Some(mapped_bytes) = align_up(input_bytes, super::WARM_ALIGN) else {
        return false;
    };
    let Some((ops_phys, ops_virt)) = crate::dma::alloc(mapped_bytes, super::WARM_ALIGN) else {
        return false;
    };
    unsafe {
        core::ptr::write_bytes(ops_virt, 0, mapped_bytes);
        core::ptr::copy_nonoverlapping(OPS.as_ptr().cast::<u8>(), ops_virt, input_bytes);
    }
    super::dma_flush(ops_virt, mapped_bytes);
    let surface = mask.surface();
    let params = FontOutlineCoverageR8Params {
        ops_gpu: DIRECT_RCS_GPU_VA_FONT_COVERAGE_OPS_BASE,
        mask_gpu: surface.gpu,
        op_count: OPS.len() as u32,
        subdivisions: 1,
        mask_pitch_bytes: surface.pitch_bytes,
        mask_width: WIDTH,
        mask_height: HEIGHT,
        rect_x: 0,
        rect_y: 0,
        rect_width: WIDTH,
        rect_height: HEIGHT,
        optical_bias_px: 0.0,
    };
    let submitted = submit_font_outline_coverage_r8_2d(ops_phys, mapped_bytes, surface, params);
    crate::dma::dealloc(ops_virt, mapped_bytes);
    if !submitted {
        return false;
    }
    let Some(audit) = mask.nonzero_audit() else {
        return false;
    };
    let mut solid_interior = true;
    for y in 3..8usize {
        // Include x=16 from the odd-width tail workgroup.  This catches a
        // walker that incorrectly applies a three-lane tail mask to every
        // SIMD16 group while still appearing to complete successfully.
        for x in 3..17usize {
            let offset = y * surface.pitch_bytes as usize + x;
            let coverage = unsafe { core::ptr::read_volatile(mask.virt.add(offset)) };
            solid_interior &= coverage == u8::MAX;
        }
    }
    let corner = unsafe { core::ptr::read_volatile(mask.virt) };
    let ok = solid_interior && corner == 0 && audit.nonzero_pixels >= 65;
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: font-outline-coverage-r8 self-test={} mask_gpu=0x{:X} nonzero={} bounds={},{},{}x{} tail_width={} right_mask=full-simd16 invariant=solid-interior-including-tail+empty-corner+unique-va\n",
        if ok { "pass" } else { "fail" },
        surface.gpu,
        audit.nonzero_pixels,
        audit.bounds.x,
        audit.bounds.y,
        audit.bounds.width,
        audit.bounds.height,
        WIDTH % FILL_RECT_PIXELS_PER_GROUP_X,
    );
    ok
}

fn font_outline_coverage_r8_self_test() -> bool {
    *FONT_OUTLINE_COVERAGE_R8_SELF_TEST.call_once(run_font_outline_coverage_r8_self_test)
}

/// Add one positioned Skrifa outline stream into a persistent R8 mask.
/// Existing coverage is retained with `max`, allowing bold duplicate runs and
/// multiple glyphs to share one color-layer mask without CPU mask blending.
pub(crate) fn font_outline_coverage_r8(
    mask: &GpgpuOwnedMask8Surface,
    outline_ops: &[[u32; 8]],
    rect: GpgpuRect,
    subdivisions: u32,
    optical_bias_px: f32,
) -> bool {
    let surface = mask.surface();
    if outline_ops.is_empty()
        || outline_ops.len() > u32::MAX as usize
        || rect.x < 0
        || rect.y < 0
        || !rect_is_inside_mask(surface, rect)
        || !(1..=16).contains(&subdivisions)
        || !optical_bias_px.is_finite()
        || !(0.0..=0.35).contains(&optical_bias_px)
    {
        return false;
    }
    if !font_outline_coverage_r8_self_test() {
        return false;
    }
    let input_bytes = match outline_ops
        .len()
        .checked_mul(core::mem::size_of::<[u32; 8]>())
    {
        Some(bytes) => bytes,
        None => return false,
    };
    let mapped_bytes = match align_up(input_bytes, super::WARM_ALIGN) {
        Some(bytes) => bytes,
        None => return false,
    };
    if mapped_bytes > DIRECT_RCS_FONT_COVERAGE_OPS_WINDOW_BYTES {
        return false;
    }
    let Some((ops_phys, ops_virt)) = crate::dma::alloc(mapped_bytes, super::WARM_ALIGN) else {
        return false;
    };
    unsafe {
        core::ptr::write_bytes(ops_virt, 0, mapped_bytes);
        core::ptr::copy_nonoverlapping(outline_ops.as_ptr().cast::<u8>(), ops_virt, input_bytes);
    }
    super::dma_flush(ops_virt, mapped_bytes);
    let params = FontOutlineCoverageR8Params {
        ops_gpu: DIRECT_RCS_GPU_VA_FONT_COVERAGE_OPS_BASE,
        mask_gpu: surface.gpu,
        op_count: outline_ops.len() as u32,
        subdivisions,
        mask_pitch_bytes: surface.pitch_bytes,
        mask_width: surface.width,
        mask_height: surface.height,
        rect_x: rect.x as u32,
        rect_y: rect.y as u32,
        rect_width: rect.width,
        rect_height: rect.height,
        optical_bias_px,
    };
    let completed = submit_font_outline_coverage_r8_2d(ops_phys, mapped_bytes, surface, params);
    crate::dma::dealloc(ops_virt, mapped_bytes);
    completed
}

/// Composite one R8 glyph layer in a single native two-dimensional dispatch.
/// A valid layer that is fully outside the destination is already complete:
/// panning a resident scene must not turn an empty clip into a GPU failure and
/// demote all of its other analytical layers to triangle rendering.
pub(crate) fn glyph_mask_rgba8_2d(blit: GpgpuGlyphMaskBlit) -> bool {
    glyph_mask_rgba8_2d_mode(blit, false)
}

pub(crate) fn glyph_mask_rgba8_2d_mode(blit: GpgpuGlyphMaskBlit, direct_scanout: bool) -> bool {
    if !blit.mask.is_valid()
        || !blit.dst.is_valid()
        || !rect_is_inside_mask(blit.mask, blit.mask_rect)
    {
        return false;
    }
    let Some(params) = lower_glyph_mask_blit(blit) else {
        return true;
    };
    submit_glyph_mask_2d(blit.mask, blit.dst, params, blit.color_rgba, direct_scanout)
}

/// Composite all persistent R8 coverage layers into one RGBA destination with
/// one RCS submission and one retirement marker. Each active layer retains an
/// independent stateless mask address, clip, destination point, and RGBA
/// payload; fully clipped layers are successful no-ops.
pub(crate) fn glyph_mask_layers_rgba8_2d(
    layers: &[GpgpuGlyphMaskLayer],
    dst: GpgpuRgba8Surface,
) -> GpgpuGlyphMaskBatchResult {
    glyph_mask_layers_rgba8_2d_mode(layers, dst, false)
}

pub(crate) fn glyph_mask_layers_rgba8_2d_mode(
    layers: &[GpgpuGlyphMaskLayer],
    dst: GpgpuRgba8Surface,
    direct_scanout: bool,
) -> GpgpuGlyphMaskBatchResult {
    let mut result = GpgpuGlyphMaskBatchResult {
        requested_layers: layers.len(),
        ..GpgpuGlyphMaskBatchResult::default()
    };
    if !dst.is_valid() || layers.len() > GLYPH_MASK_BATCH_MAX_LAYERS {
        return result;
    }
    for layer in layers {
        if !layer.mask.is_valid() || !rect_is_inside_mask(layer.mask, layer.mask_rect) {
            return result;
        }
        let blit = GpgpuGlyphMaskBlit {
            mask: layer.mask,
            mask_rect: layer.mask_rect,
            dst,
            dst_xy: layer.dst_xy,
            color_rgba: layer.color_rgba,
        };
        if lower_glyph_mask_blit(blit).is_some() {
            result.active_walkers += 1;
        }
    }
    if result.active_walkers == 0 {
        result.ok = true;
        return result;
    }
    let (submitted, completed) = submit_glyph_mask_layers_2d(layers, dst, direct_scanout);
    result.submitted = submitted;
    result.ok = completed;
    result.submits = usize::from(submitted);
    result
}
