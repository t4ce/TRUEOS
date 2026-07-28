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

/// Allocate the persistent RGBA base consumed by the font-instance engine.
///
/// It shares the font residency VA allocator so every live page owns a unique
/// PPGTT range and can be copied without remapping another page's storage.
pub(crate) fn allocate_font_instance_rgba8_surface(
    width: u32,
    height: u32,
) -> Option<GpgpuOwnedRgba8Surface> {
    allocate_font_instance_rgba8_surface_cleared(width, height, 0)
}

/// Allocate a persistent font RGBA surface with every pixel initialized to one
/// premultiplied native RGBA value. This folds an offscreen canvas background
/// into allocation initialization instead of spending another RCS submission.
pub(crate) fn allocate_font_instance_rgba8_surface_cleared(
    width: u32,
    height: u32,
    clear_rgba: u32,
) -> Option<GpgpuOwnedRgba8Surface> {
    if width == 0 || height == 0 {
        return None;
    }
    let row_bytes = (width as usize).checked_mul(core::mem::size_of::<u32>())?;
    let pitch_bytes = u32::try_from(align_up(row_bytes, 64)?).ok()?;
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
    let clear = clear_rgba.to_le_bytes();
    let allocation = unsafe { core::slice::from_raw_parts_mut(virt, bytes) };
    for pixel in allocation.chunks_exact_mut(core::mem::size_of::<u32>()) {
        pixel.copy_from_slice(&clear);
    }
    super::dma_flush(virt, bytes);
    let Some(surface) = GpgpuRgba8Surface::new(phys, gpu, bytes, width, height, pitch_bytes) else {
        crate::dma::dealloc(virt, bytes);
        recycle_font_coverage_gpu_va(gpu, bytes);
        return None;
    };
    Some(GpgpuOwnedRgba8Surface { surface, virt })
}

pub(crate) fn allocate_font_instance_state(capacity: usize) -> Option<GpgpuOwnedFontInstanceState> {
    if capacity == 0 || capacity > GPGPU_FONT_INSTANCE_MAX_LAYERS {
        return None;
    }
    let raw_bytes = capacity.checked_mul(GPGPU_FONT_INSTANCE_DESCRIPTOR_BYTES)?;
    let bytes = align_up(raw_bytes, super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    let Some(gpu) = reserve_font_coverage_gpu_va(bytes) else {
        crate::dma::dealloc(virt, bytes);
        return None;
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);
    Some(GpgpuOwnedFontInstanceState {
        phys,
        gpu,
        bytes,
        virt,
        capacity,
    })
}

/// Submission ownership state for a direct-RCS operation. A submitted command
/// that missed its retirement marker is not an ordinary `false`: its mapped
/// inputs and outputs must remain alive and the shared direct context must not
/// accept another batch.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuDispatchRetirement {
    NotSubmitted,
    Complete,
    SubmittedIncomplete,
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
) -> GpgpuDispatchRetirement {
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
        return GpgpuDispatchRetirement::NotSubmitted;
    }
    let input_bytes = match outline_ops
        .len()
        .checked_mul(core::mem::size_of::<[u32; 8]>())
    {
        Some(bytes) => bytes,
        None => return GpgpuDispatchRetirement::NotSubmitted,
    };
    let mapped_bytes = match align_up(input_bytes, super::WARM_ALIGN) {
        Some(bytes) => bytes,
        None => return GpgpuDispatchRetirement::NotSubmitted,
    };
    if mapped_bytes > DIRECT_RCS_FONT_COVERAGE_OPS_WINDOW_BYTES {
        return GpgpuDispatchRetirement::NotSubmitted;
    }
    let Some((ops_phys, ops_virt)) = crate::dma::alloc(mapped_bytes, super::WARM_ALIGN) else {
        return GpgpuDispatchRetirement::NotSubmitted;
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
    let retirement = submit_font_outline_coverage_r8_2d(ops_phys, mapped_bytes, surface, params);
    if retirement == GpgpuDispatchRetirement::SubmittedIncomplete {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: font-outline input quarantined phys=0x{:X} bytes={} reason=retirement-uncertain action=no-unmap-no-free\n",
            ops_phys,
            mapped_bytes,
        );
    } else {
        crate::dma::dealloc(ops_virt, mapped_bytes);
    }
    retirement
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

fn lower_font_instance_layer(
    layer: GpgpuFontInstanceLayer,
    state: &GpgpuOwnedFontInstanceState,
    dst: GpgpuRgba8Surface,
) -> Option<GpgpuRect> {
    if !layer.mask.is_valid()
        || !dst.is_valid()
        || !rect_is_inside_mask(layer.mask, layer.mask_rect)
        || layer.descriptor_index >= state.capacity()
        || !layer.dst_center[0].is_finite()
        || !layer.dst_center[1].is_finite()
        || layer.dispatch_rect.width == 0
        || layer.dispatch_rect.height == 0
    {
        return None;
    }
    let left = i64::from(layer.dispatch_rect.x).max(0);
    let top = i64::from(layer.dispatch_rect.y).max(0);
    let right = (i64::from(layer.dispatch_rect.x) + i64::from(layer.dispatch_rect.width))
        .min(i64::from(dst.width));
    let bottom = (i64::from(layer.dispatch_rect.y) + i64::from(layer.dispatch_rect.height))
        .min(i64::from(dst.height));
    if right <= left || bottom <= top {
        return Some(GpgpuRect::default());
    }
    Some(GpgpuRect::new(left as i32, top as i32, (right - left) as u32, (bottom - top) as u32))
}

/// Composite persistent Skrifa R8 layers through the C++ font-instance
/// engine. Descriptor storage remains resident across frames; this call
/// changes only scene centers and monotonic animation time.
pub(crate) fn font_instance_layers_rgba8_2d_mode(
    layers: &[GpgpuFontInstanceLayer],
    state: &GpgpuOwnedFontInstanceState,
    dst: GpgpuRgba8Surface,
    direct_scanout: bool,
    time_seconds: f32,
) -> GpgpuFontInstanceBatchResult {
    let mut result = GpgpuFontInstanceBatchResult {
        requested_layers: layers.len(),
        ..GpgpuFontInstanceBatchResult::default()
    };
    if !dst.is_valid()
        || layers.len() > FONT_INSTANCE_BATCH_MAX_LAYERS
        || state.capacity() < layers.len()
        || !time_seconds.is_finite()
    {
        return result;
    }
    for &layer in layers {
        let Some(rect) = lower_font_instance_layer(layer, state, dst) else {
            return result;
        };
        if rect.width != 0 && rect.height != 0 {
            result.active_walkers += 1;
        }
    }
    if result.active_walkers == 0 {
        result.ok = true;
        return result;
    }
    let (submitted, completed) =
        submit_font_instance_layers_2d(layers, state, dst, direct_scanout, time_seconds);
    result.submitted = submitted;
    result.ok = completed;
    result.submits = usize::from(submitted);
    result
}
