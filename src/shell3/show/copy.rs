//! CPU, BCS0 or virgl copies from Shell3's warmed antialiased glyph cells.

use super::{Ui4Surface, rendered_lines};
use crate::ui4::{
    FrameRgbaView, acquire_frame_buffer, cancel_frame_buffer, publish_frame_buffer,
    writable_rgba_view,
};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use trueos_time::{Duration, Timer};

static REPORTED_VIRGL_COPY: AtomicBool = AtomicBool::new(false);

pub(super) async fn present(
    surface: &mut Ui4Surface,
    lines: &[&[(char, Option<super::super::RgbaColor>)]],
    fallback_segments: &[super::super::SegmentUpdate],
    poisoned: &mut bool,
    pan: &mut Option<super::pan::PanBuffer>,
    budget: &crate::ui4::text_area::RasterBudget,
    area: Option<&super::super::update::MatrixAreaSnapshot>,
    strips: &mut [Option<super::reveal::StripCache>; 2],
    reveals: &[super::super::update::StripReveal],
) -> Result<Option<crate::ui4::DamageRect>, &'static str> {
    crate::ui4::text_blit::benchmark_once().await?;
    let present_started = crate::ui4::text_blit::stamp();
    let lease =
        acquire_frame_buffer(surface.frame).map_err(|_| "shell3-show-copy-destination-busy")?;
    let index = lease.buffer_index as usize;
    let previous = surface.frame_contents[index].clone();
    let current = rendered_lines(lines);
    let updates = super::super::update::diff_rendered_lines(previous.as_deref(), &current);
    let view = match writable_rgba_view(lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            return Err("shell3-show-copy-destination-view");
        }
    };
    let using_pan =
        match super::pan::PanBuffer::prepare(pan, budget, area, surface.scale, poisoned).await {
            Ok(using_pan) => using_pan,
            Err(error) => {
                if !*poisoned {
                    let _ = cancel_frame_buffer(lease);
                }
                return Err(error);
            }
        };
    let mut cached = [false; 2];
    for input in reveals {
        match super::reveal::StripCache::prepare(&mut strips[input.row], input, surface.scale, budget, poisoned).await {
            Ok(ready) => cached[input.row] = ready,
            Err(error) => {
                if !*poisoned { let _ = cancel_frame_buffer(lease); }
                return Err(error);
            }
        }
    }
    let mut glyphs = Vec::new();
    for update in &updates {
        if using_pan && matches!(update.row, super::super::SpecialRows::MatrixRow(_)) {
            continue;
        }
        if let Err(error) = glyphs_for_update(view, update, surface.scale, &mut glyphs, reveals, &cached) {
            let _ = cancel_frame_buffer(lease);
            return Err(error);
        }
    }
    // A later chunk can fail admission after earlier chunks changed pixels.
    // Any retry must then repaint this entire buffer, including erased cells.
    let clearing = surface.clear_buffers[index];
    surface.frame_contents[index] = None;
    // Sparse fresh-frame painting requires a known background. If a later
    // chunk fails, the next attempt must clear any partially painted pixels.
    surface.clear_buffers[index] = true;
    let emulator = crate::virtio_gpu_logo::output_dimensions().is_some();
    if clearing && !emulator {
        crate::intel::dma_cache_flush_range(view.virt, view.byte_len);
    }
    if clearing && emulator {
        if !unsafe { crate::ui4::text_blit::fill(bcs_surface(view), &[
            crate::intel::GucBcs0RgbaFill { x: 0, y: 0, width: view.width, height: view.height,
                color: u32::from_le_bytes(super::BACKGROUND) },
        ]) } {
            let _ = cancel_frame_buffer(lease);
            return Err("shell3-show-emulator-clear");
        }
    } else if clearing {
        let submission = match crate::intel::queue_guc_bcs0_rgba_fill(
            bcs_surface(view),
            u32::from_le_bytes(super::BACKGROUND),
        ) {
            Ok(submission) => submission,
            Err(crate::intel::GucBcs0CopySubmitError::SubmitFailed) => {
                *poisoned = true;
                return Err("shell3-show-scale-clear-uncertain");
            }
            Err(_) => {
                let _ = cancel_frame_buffer(lease);
                return Err("shell3-show-scale-clear-busy");
            }
        };
        *poisoned = true;
        loop {
            match crate::intel::poll_guc_bcs0_rgba_copies(submission) {
                crate::intel::GucBcs0CopyCompletion::Pending => {
                    Timer::after(Duration::from_millis(1)).await
                }
                crate::intel::GucBcs0CopyCompletion::Complete => break,
                _ => return Err("shell3-show-scale-clear-retirement-uncertain"),
            }
        }
        *poisoned = false;
    }
    let paint_started = crate::ui4::text_blit::stamp();
    // The frame lease is exclusively writable; clears above have retired.
    let glyph_count = glyphs.len();
    let terminal_paint = area.is_none() && reveals.is_empty();
    let cpu_paint;
    if terminal_paint {
        let plan = crate::ui4::text_paint::RgbaPaintPlan::new(core::mem::take(&mut glyphs),
            crate::r::services::shell3_font_service::solid_color);
        let stats = match plan.paint(bcs_surface(view), poisoned).await {
            Ok(stats) => stats,
            Err(error) => {
                if !*poisoned { let _ = cancel_frame_buffer(lease); }
                return Err(error);
            }
        };
        cpu_paint = stats.gpu_pixels == 0;
        if crate::allcaps::text_blit::DIAGNOSTICS {
            crate::log_info!(target:"apps";"text-paint: cells={} cpu_pixels={} gpu_pixels={} copy_batches={} fill_batches={} wall_ns={}\n",
                glyph_count, stats.cpu_pixels, stats.gpu_pixels, stats.copy_batches, stats.fill_batches,
                crate::ui4::text_blit::nanos(crate::ui4::text_blit::stamp().wrapping_sub(paint_started)));
        }
    } else {
        // Both sinks copy exactly the same warmed RGBA cells. QEMU stays local
        // until window upload; Intel retains its measured CPU/BLT allowance.
        cpu_paint = !*poisoned && unsafe {
            crate::ui4::text_blit::try_glyph_copy(bcs_surface(view), &glyphs)
        };
        if !cpu_paint && !glyphs.is_empty() {
            crate::intel::dma_cache_flush_range(view.virt, view.byte_len);
        }
        for chunk in glyphs.chunks(128).filter(|_| !cpu_paint) {
            if crate::virtio_gpu_logo::output_dimensions().is_some() {
                if let Err(error) = crate::virtio_gpu_logo::copy_rgba(bcs_surface(view), chunk).await {
                    let _ = cancel_frame_buffer(lease);
                    return Err(error);
                }
                continue;
            }
            let submission = match crate::intel::queue_guc_bcs0_legacy_rgba_copies(bcs_surface(view), chunk) {
                Ok(submission) => submission,
                Err(crate::intel::GucBcs0CopySubmitError::SubmitFailed) => {
                    *poisoned = true;
                    return Err("shell3-show-copy-submit-uncertain");
                }
                Err(_) => {
                    let _ = cancel_frame_buffer(lease);
                    return Err("shell3-show-copy-unavailable");
                }
            };
            *poisoned = true;
            loop {
                match crate::intel::poll_guc_bcs0_rgba_copies(submission) {
                    crate::intel::GucBcs0CopyCompletion::Pending => {
                        Timer::after(Duration::from_millis(1)).await
                    }
                    crate::intel::GucBcs0CopyCompletion::Complete => break,
                    crate::intel::GucBcs0CopyCompletion::Failed
                    | crate::intel::GucBcs0CopyCompletion::InvalidSubmission => {
                        return Err("shell3-show-copy-retirement-uncertain");
                    }
                }
            }
            *poisoned = false;
        }
        crate::ui4::text_blit::report("frame-glyph-copy", crate::ui4::text_blit::copy_pixels(&glyphs),
            cpu_paint, if cpu_paint {0} else {glyphs.len().div_ceil(128)}, paint_started);
    }
    for input in reveals.iter().filter(|input| cached[input.row]) {
        if let Err(error) = strips[input.row].as_mut().ok_or("shell3-strip-cache-missing")?
            .copy(input, bcs_surface(view), poisoned).await {
            if !*poisoned { let _ = cancel_frame_buffer(lease); }
            return Err(error);
        }
    }
    if using_pan
        && (clearing
            || previous.is_none()
            || updates
                .iter()
                .any(|update| matches!(update.row, super::super::SpecialRows::MatrixRow(_))))
    {
        if let Err(error) = pan
            .as_mut()
            .ok_or("shell3-pan-missing")?
            .copy_view(bcs_surface(view), surface.scale, poisoned)
            .await
        {
            if !*poisoned {
                let _ = cancel_frame_buffer(lease);
            }
            return Err(error);
        }
    }
    if !cpu_paint && !emulator {
        crate::intel::dma_cache_flush_range(view.virt, view.byte_len);
    }
    if publish_frame_buffer(lease).is_err() {
        let _ = cancel_frame_buffer(lease);
        return Err("shell3-show-frame-publish");
    }
    surface.frame_contents[index] = Some(current);
    surface.clear_buffers[index] = false;
    if !terminal_paint && !glyphs.is_empty() && !cpu_paint {
        if crate::virtio_gpu_logo::output_dimensions().is_some() {
            if !REPORTED_VIRGL_COPY.swap(true, Ordering::Relaxed) {
                crate::log_important!(target: "apps";
                    "shell3/show: copy-retired backend=virgl command=resource-copy-region colors=metafmt bold=off cpu-rgba-paint=0\n"
                );
            }
        } else {
            crate::log_once!(target: "apps";
                "shell3/show: bcs0-retired backend=legacy command=xy-src-copy-blt rop=cc colors=metafmt bold=off cpu-rgba-paint=0 staging-frame=0\n"
            );
        }
    }
    if crate::allcaps::text_blit::DIAGNOSTICS {
        let ns = crate::ui4::text_blit::nanos(crate::ui4::text_blit::stamp().wrapping_sub(present_started));
        crate::log_info!(target:"apps";"shell3/present: ap={} glyphs={} cpu_glyphs={} using_pan={} wall_ns={}\n",
            crate::percpu::current_slot(),glyph_count,if cpu_paint {glyph_count} else {0},using_pan,ns);
    }
    let segments = if updates.is_empty() {
        fallback_segments
    } else {
        &updates
    };
    Ok(super::damage_for_segments(segments, surface.width, surface.height, surface.scale)
        .or_else(|| previous.is_none().then_some(crate::ui4::DamageRect::FULL))
        .map(|damage| {
            if clearing {
                crate::ui4::DamageRect::FULL
            } else {
                damage
            }
        }))
}

fn glyphs_for_update(
    view: FrameRgbaView,
    update: &super::super::SegmentUpdate,
    scale: u32,
    output: &mut Vec<crate::intel::GucBcs0RgbaCopy>,
    reveals: &[super::super::update::StripReveal],
    cached: &[bool; 2],
) -> Result<(), &'static str> {
    let (cell_width, cell_height) = crate::r::services::shell3_font_service::cell_extent(scale)
        .ok_or("shell3-font-physical-size-unavailable")?;
    let row = match update.row {
        super::super::SpecialRows::TitleRow => 0,
        super::super::SpecialRows::StatusRow => 1,
        super::super::SpecialRows::PromtRow => 2,
        super::super::SpecialRows::MatrixRow(index) => index as u32 + 3,
    };
    let y = row * cell_height;
    if y >= view.height {
        return Ok(());
    }
    let mut characters = update.text.chars();
    let count = update.remove.max(update.text.chars().count());
    for column in 0..count {
        let Some(x) = update
            .offset
            .checked_add(column)
            .and_then(|c| c.checked_mul(cell_width as usize))
            .and_then(|x| u32::try_from(x).ok())
        else {
            break;
        };
        if x >= view.width {
            break;
        }
        let character = characters.next().unwrap_or(' ');
        let cell_column = update.offset + column;
        if reveals.iter().any(|input| input.row == row as usize && cached[input.row]
            && cell_column >= input.start && input.spans.iter().any(|span| span.contains(&(cell_column - input.start)))) {
            continue;
        }
        let width = (cell_width).min(view.width - x);
        let height = (cell_height).min(view.height - y);
        output.push(glyph_for_cell(
            x,
            y,
            width,
            height,
            (character, update.colors.get(column).copied().flatten()),
            scale,
        )?);
    }
    Ok(())
}

pub(super) fn glyph_for_cell(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    cell: (char, Option<super::super::RgbaColor>),
    scale: u32,
) -> Result<crate::intel::GucBcs0RgbaCopy, &'static str> {
    let (cell_width, cell_height) = crate::r::services::shell3_font_service::cell_extent(scale)
        .ok_or("shell3-font-physical-size-unavailable")?;
    let mut copy = crate::r::services::shell3_font_service::glyph(
        cell.0,
        cell_width,
        cell_height,
        cell.1.unwrap_or(super::FOREGROUND).rgba(),
        cell.1.and_then(super::super::RgbaColor::background).unwrap_or(super::BACKGROUND),
        cell.1.is_some_and(super::super::RgbaColor::underline),
    )?;
    copy.destination_x = x;
    copy.destination_y = y;
    copy.width = copy.width.min(width);
    copy.height = copy.height.min(height);
    Ok(copy)
}

pub(super) fn bcs_surface(view: FrameRgbaView) -> crate::intel::GucBcs0RgbaSurface {
    crate::intel::GucBcs0RgbaSurface {
        phys: view.phys,
        gpu: view.gpu,
        bytes: view.byte_len,
        width: view.width,
        height: view.height,
        pitch_bytes: view.pitch,
    }
}
