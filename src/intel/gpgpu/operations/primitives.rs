/// Compatibility API for the dormant WebGPU clear path. SOLID shares the
/// existing compositor artifact; there is no dedicated fill kernel.
pub(crate) fn fill_rect_rgba8_stats(
    dst: GpgpuRgba8Surface,
    rect: GpgpuRect,
    color_rgba: u32,
) -> GpgpuSubmitStats {
    if !dst.is_valid() || clip_gpgpu_rect_to_surface(rect, dst.width, dst.height).is_none() {
        return GpgpuSubmitStats::default();
    }
    let result = fill_solid_rects_rgba8_result(dst, &[GpgpuSolidRect { rect, color_rgba }]);
    GpgpuSubmitStats {
        submits: result.stats.submits,
        ..Default::default()
    }
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
    Some(GpgpuOwnedMask8Surface {
        surface,
        virt,
        quarantined: AtomicBool::new(false),
    })
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
    // This allocation is an ordinary Font-RCS PPGTT resource, not a legacy
    // 32-bit device DMA or direct-display surface. Prefer low physical memory
    // for continuity with the other small font resources, but do not reject a
    // large document canvas merely because the sub-4-GiB PMM ranges became
    // fragmented late in boot. Gen12 PPGTT leaves carry high physical page
    // addresses and the HHDM maps the backing for CPU initialization. Keep
    // the fallback within XeLP's conservative 39-bit physical-address range.
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN).or_else(|| {
        let allocation =
            crate::dma::alloc_with_max(bytes, super::WARM_ALIGN, Some(1u64 << 39))?;
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: font RGBA backing admitted from high physical memory phys=0x{:X} bytes=0x{:X} reason=sub4g-fragmented ownership=font-ppgtt-only\n",
            allocation.0,
            bytes,
        );
        Some(allocation)
    })?;
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
    Some(GpgpuOwnedRgba8Surface {
        surface,
        virt,
        system_service: false,
        quarantined: AtomicBool::new(false),
    })
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
    result.release = (completed && direct_scanout).then(|| gpgpu_rgba8_release(dst));
    result
}



