/// Render a complete Mandelbrot image into a trusted UI4 direct-scanout
/// surface. Parameters 1..=512 retain the existing descriptor worklist and
/// GuC submission; only the destination mapping and ownership result use the
/// display handoff contract.
pub(crate) fn mandel64_worklist_surface_full(
    dst: GpgpuRgba8Surface,
    iterations: u32,
) -> Option<GpgpuShellMandel64WorklistResult> {
    mandel64_worklist_surface_view_mode(dst, dst.bounds(), iterations, true, true)
}

/// Render the analytical chart node into an arbitrary trusted RGBA surface.
/// This is compute-only: the caller owns frame publication and cadence.
pub(crate) fn chart_sine_rgba8_surface_full(
    dst: GpgpuRgba8Surface,
    phase: f32,
    flags: u32,
) -> GpgpuRgba8KernelResult {
    let start_tick = direct_rcs_now_tick();
    let mut params = ChartSineRgba8Params::scope_defaults(phase, flags);
    params.rect_width = dst.width;
    params.rect_height = dst.height;
    let outcome = submit_chart_sine_rgba8(dst, params);
    let ok = outcome.observed == CHART_SINE_POST_MARKER;
    GpgpuRgba8KernelResult {
        ok,
        submitted: outcome.submitted,
        marker: outcome.observed,
        submit_ms: direct_rcs_elapsed_ms_since(start_tick),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}

/// Render the procedural plasma node into an arbitrary trusted RGBA surface.
/// This is compute-only: the caller owns frame publication and cadence.
pub(crate) fn pixel_plasma_rgba8_surface_full(
    dst: GpgpuRgba8Surface,
    time: f32,
    flags: u32,
) -> GpgpuRgba8KernelResult {
    let start_tick = direct_rcs_now_tick();
    let mut params = PixelPlasmaRgba8Params::demo_defaults(time, flags);
    params.rect_width = dst.width;
    params.rect_height = dst.height;
    let outcome = submit_pixel_plasma_rgba8(dst, params);
    let ok = outcome.observed == PIXEL_PLASMA_POST_MARKER;
    GpgpuRgba8KernelResult {
        ok,
        submitted: outcome.submitted,
        marker: outcome.observed,
        submit_ms: direct_rcs_elapsed_ms_since(start_tick),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}

fn gpgpu_rgba8_release(dst: GpgpuRgba8Surface) -> GpgpuRgba8ReleaseFence {
    let sequence = GPGPU_RGBA8_RELEASE_SEQUENCE
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1)
        .max(1);
    GpgpuRgba8ReleaseFence {
        phys: dst.phys,
        byte_len: dst.bytes,
        sequence,
    }
}

fn mandel64_worklist_surface_view_mode(
    dst: GpgpuRgba8Surface,
    view: GpgpuRect,
    iterations: u32,
    mirror_at_center: bool,
    direct_scanout: bool,
) -> Option<GpgpuShellMandel64WorklistResult> {
    if !dst.is_valid()
        || dst.width < MANDEL64_WORKLIST_CELL_PIXELS
        || dst.height < MANDEL64_WORKLIST_CELL_PIXELS
        || view.is_empty()
    {
        return None;
    }

    if iterations == 0 && direct_scanout {
        // Direct publication requires an exact producer-release token. The
        // preview never requests zero iterations, and the ordinary fill path
        // deliberately does not manufacture one.
        return None;
    }

    if iterations == 0 {
        let stats = fill_rect_rgba8_stats(dst, dst.bounds(), 0xFF00_0000);
        let submitted = stats.submits != 0;
        return Some(GpgpuShellMandel64WorklistResult {
            ok: submitted,
            submitted,
            requested: 0,
            descriptors: 0,
            walkers: 0,
            pixels: (dst.width as usize).saturating_mul(dst.height as usize),
            submit_ms: stats.submit_ms,
            ..GpgpuShellMandel64WorklistResult::default()
        });
    }

    let render_width = view.width.min(dst.width);
    let view_height = view.height.min(dst.height);
    let columns = render_width.div_ceil(MANDEL64_WORKLIST_CELL_PIXELS).max(1);
    let render_height = if mirror_at_center {
        view_height.div_ceil(2)
    } else {
        view_height
    }
    .max(1);
    let rows = render_height.div_ceil(MANDEL64_WORKLIST_CELL_PIXELS).max(1);
    let count = columns.saturating_mul(rows) as usize;
    if count == 0 {
        return None;
    }
    let iterations = iterations.clamp(1, MANDEL64_WORKLIST_MAX_ITERATIONS);
    let mut placements = Vec::new();
    let mut submitted = true;
    let mut descriptors = 0usize;
    let mut walkers = 0usize;
    let mut pixels = 0usize;
    let mut submit_ms = 0u64;
    let mut desc_gpu = 0u64;
    let mut last_src_xy = GpgpuPoint::new(0, 0);
    let mut last_dst_xy = GpgpuPoint::new(0, 0);
    let mut last_marker = 0u32;
    let mut submitted_tiles = 0usize;
    let mut index = 0usize;
    while index < count {
        let tile_batch = MANDEL64_WORKLIST_MAX_DESCS / MANDEL64_WORKLIST_BANDS_PER_TILE;
        let end = index.saturating_add(tile_batch).min(count);
        placements.clear();
        for tile_index in index..end {
            let tile_x = (tile_index as u32) % columns;
            let tile_y = (tile_index as u32) / columns;
            let dst_x = tile_x.saturating_mul(MANDEL64_WORKLIST_CELL_PIXELS);
            let dst_y = tile_y.saturating_mul(MANDEL64_WORKLIST_CELL_PIXELS);
            let width = render_width
                .saturating_sub(dst_x)
                .min(MANDEL64_WORKLIST_CELL_PIXELS);
            let height = render_height
                .saturating_sub(dst_y)
                .min(MANDEL64_WORKLIST_CELL_PIXELS);
            placements.push(GpgpuMandel64Placement {
                src_x: view.x.saturating_add(dst_x as i32),
                src_y: view.y.saturating_add(dst_y as i32),
                dst_x: dst_x as i32,
                dst_y: dst_y as i32,
                width,
                height,
                view_height,
                mirror_at_center,
                iterations,
            });
        }

        let result =
            mandel64_worklist_surface_with_policy(dst, placements.as_slice(), direct_scanout)?;
        submitted &= result.submitted;
        submitted_tiles = submitted_tiles.saturating_add(result.requested);
        descriptors = descriptors.saturating_add(result.descriptors);
        walkers = walkers.saturating_add(result.walkers);
        pixels = pixels.saturating_add(result.pixels);
        submit_ms = submit_ms.saturating_add(result.submit_ms);
        desc_gpu = result.desc_gpu;
        last_src_xy = result.last_src_xy;
        last_dst_xy = result.last_dst_xy;
        last_marker = result.marker;
        if !result.ok {
            break;
        }
        index = end;
    }

    let ok = submitted && submitted_tiles == count && last_marker == MANDEL64_WORKLIST_POST_MARKER;
    Some(GpgpuShellMandel64WorklistResult {
        ok,
        submitted,
        marker: last_marker,
        requested: count,
        descriptors,
        walkers,
        pixels,
        submit_ms,
        desc_gpu,
        last_src_xy,
        last_dst_xy,
        release: (ok && direct_scanout).then(|| gpgpu_rgba8_release(dst)),
    })
}

pub(crate) fn mandel64_worklist_surface(
    dst: GpgpuRgba8Surface,
    placements: &[GpgpuMandel64Placement],
) -> Option<GpgpuShellMandel64WorklistResult> {
    mandel64_worklist_surface_with_policy(dst, placements, false)
}

fn mandel64_worklist_surface_with_policy(
    dst: GpgpuRgba8Surface,
    placements: &[GpgpuMandel64Placement],
    direct_scanout: bool,
) -> Option<GpgpuShellMandel64WorklistResult> {
    if !dst.is_valid()
        || dst.width < MANDEL64_WORKLIST_CELL_PIXELS
        || dst.height < MANDEL64_WORKLIST_CELL_PIXELS
        || placements.is_empty()
    {
        return None;
    }
    let desc = mandel64_worklist_desc_buffer_once()?;
    let max_placements = MANDEL64_WORKLIST_MAX_DESCS / MANDEL64_WORKLIST_BANDS_PER_TILE;
    let count = placements.len().min(max_placements);
    if count == 0 {
        return None;
    }

    let mut last_src_xy = GpgpuPoint::new(0, 0);
    let mut last_dst_xy = GpgpuPoint::new(0, 0);
    let mut desc_count = 0usize;
    let mut drawn_pixels = 0usize;
    let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
    unsafe {
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let descs = desc.virt as *mut Mandel64WorklistRgba8Desc;
        for placement in placements.iter().take(count) {
            let src_x = placement.src_x.clamp(i16::MIN as i32, i16::MAX as i32);
            let src_y = placement.src_y.clamp(i16::MIN as i32, i16::MAX as i32);
            let dst_x = placement.dst_x.clamp(0, dst.width.saturating_sub(1) as i32);
            let dst_y = placement
                .dst_y
                .clamp(0, dst.height.saturating_sub(1) as i32);
            let requested_width = if placement.width == 0 {
                MANDEL64_WORKLIST_CELL_PIXELS
            } else {
                placement.width
            };
            let requested_height = if placement.height == 0 {
                MANDEL64_WORKLIST_CELL_PIXELS
            } else {
                placement.height
            };
            let width = requested_width
                .min(MANDEL64_WORKLIST_CELL_PIXELS)
                .min(dst.width.saturating_sub(dst_x as u32));
            let height = requested_height
                .min(MANDEL64_WORKLIST_CELL_PIXELS)
                .min(dst.height.saturating_sub(dst_y as u32));
            let iterations = placement
                .iterations
                .clamp(1, MANDEL64_WORKLIST_MAX_ITERATIONS);
            let iteration_payload = pack_mandel64_iterations(iterations);
            if width == 0 || height == 0 {
                continue;
            }
            let bands = height
                .div_ceil(MANDEL64_WORKLIST_BAND_ROWS)
                .min(MANDEL64_WORKLIST_BANDS_PER_TILE as u32);
            for band in 0..bands {
                if desc_count >= MANDEL64_WORKLIST_MAX_DESCS {
                    break;
                }
                let band_y = (band as i32).saturating_mul(MANDEL64_WORKLIST_BAND_ROWS as i32);
                let band_rows = height
                    .saturating_sub(band.saturating_mul(MANDEL64_WORKLIST_BAND_ROWS))
                    .min(MANDEL64_WORKLIST_BAND_ROWS);
                let flags = (band_rows & MANDEL64_WORKLIST_FLAG_ROWS_MASK)
                    | if placement.mirror_at_center {
                        0
                    } else {
                        MANDEL64_WORKLIST_FLAG_NO_MIRROR
                    }
                    | (width << MANDEL64_WORKLIST_FLAG_COLS_SHIFT)
                    | (placement.view_height.min(u16::MAX as u32)
                        << MANDEL64_WORKLIST_FLAG_VIEW_HEIGHT_SHIFT);
                let desc_value = Mandel64WorklistRgba8Desc {
                    src_xy: pack_i16_pair_u32(
                        src_x as i16,
                        src_y
                            .saturating_add(band_y)
                            .clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                    ),
                    dst_xy: pack_i16_pair_u32(
                        dst_x as i16,
                        dst_y.saturating_add(band_y).clamp(0, dst.height as i32 - 1) as i16,
                    ),
                    flags,
                    color_rgba: iteration_payload,
                };
                core::ptr::write_volatile(descs.add(desc_count), desc_value);
                desc_count = desc_count.saturating_add(1);
            }
            let computed_pixels = (width as usize).saturating_mul(height as usize);
            let output_pixels = if !placement.mirror_at_center || placement.view_height == 0 {
                computed_pixels
            } else {
                computed_pixels.saturating_mul(2)
            };
            drawn_pixels = drawn_pixels.saturating_add(output_pixels);
            last_src_xy = GpgpuPoint::new(src_x, src_y);
            last_dst_xy = GpgpuPoint::new(dst_x, dst_y);
        }
    }
    if desc_count == 0 {
        return None;
    }
    super::dma_flush(desc.virt, desc.bytes);

    let params = Mandel64WorklistRgba8Params {
        dst_gpu: dst.gpu,
        desc_gpu: desc.gpu,
        dst_pitch_bytes: dst.pitch_bytes,
        desc_base: 0,
        desc_count: desc_count as u32,
    };
    let walkers = mandel64_worklist_walker_count(desc_count);

    let submit_start_tick = direct_rcs_now_tick();
    let outcome = submit_mandel64_worklist(dst, desc, params, direct_scanout);
    let submit_ms = direct_rcs_elapsed_ms_since(submit_start_tick);
    let ok = outcome.observed == MANDEL64_WORKLIST_POST_MARKER;

    Some(GpgpuShellMandel64WorklistResult {
        ok,
        submitted: outcome.submitted,
        marker: outcome.observed,
        requested: count,
        descriptors: desc_count,
        walkers,
        pixels: drawn_pixels,
        submit_ms,
        desc_gpu: desc.gpu,
        last_src_xy,
        last_dst_xy,
        release: None,
    })
}
