//! Classical BCS0 monochrome expansion of Shell3's colored MicroFont cells.

use super::{Ui4Surface, rendered_lines};
use crate::ui4::{
    FrameRgbaView, acquire_frame_buffer, cancel_frame_buffer, publish_frame_buffer,
    writable_rgba_view,
};
use alloc::vec::Vec;
use trueos_time::{Duration, Timer};

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
        acquire_frame_buffer(surface.frame).map_err(|_| "shell3-show-mono-destination-busy")?;
    let index = lease.buffer_index as usize;
    let previous = surface.frame_contents[index].clone();
    let current = rendered_lines(lines);
    let updates = super::super::update::diff_rendered_lines(previous.as_deref(), &current);
    let view = match writable_rgba_view(lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            return Err("shell3-show-mono-destination-view");
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
        glyphs_for_update(view, update, surface.scale, &mut glyphs, reveals, &cached);
    }
    // A later chunk can fail admission after earlier chunks changed pixels.
    // Any retry must then repaint this entire buffer, including erased cells.
    let clearing = surface.clear_buffers[index];
    surface.frame_contents[index] = None;
    // Sparse fresh-frame painting requires a known background. If a later
    // chunk fails, the next attempt must clear any partially painted pixels.
    surface.clear_buffers[index] = true;
    if clearing {
        crate::intel::dma_cache_flush_range(view.virt, view.byte_len);
    }
    if clearing {
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
    let cpu_paint = !*poisoned && unsafe {
        crate::ui4::text_blit::try_mono(bcs_surface(view), &glyphs)
    };
    if !cpu_paint && !glyphs.is_empty() {
        crate::intel::dma_cache_flush_range(view.virt, view.byte_len);
    }
    for chunk in glyphs.chunks(crate::intel::GUC_BCS0_MONO_MAX_GLYPHS).filter(|_| !cpu_paint) {
        let submission = match crate::intel::queue_guc_bcs0_mono_glyphs(bcs_surface(view), chunk) {
            Ok(submission) => submission,
            Err(crate::intel::GucBcs0CopySubmitError::SubmitFailed) => {
                *poisoned = true;
                return Err("shell3-show-mono-submit-uncertain");
            }
            Err(_) => {
                let _ = cancel_frame_buffer(lease);
                return Err("shell3-show-mono-unavailable");
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
                    return Err("shell3-show-mono-retirement-uncertain");
                }
            }
        }
        *poisoned = false;
    }
    crate::ui4::text_blit::report("frame-mono", crate::ui4::text_blit::mono_pixels(&glyphs),
        cpu_paint, if cpu_paint {0} else {glyphs.len().div_ceil(crate::intel::GUC_BCS0_MONO_MAX_GLYPHS)}, paint_started);
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
    if !cpu_paint {
        crate::intel::dma_cache_flush_range(view.virt, view.byte_len);
    }
    if publish_frame_buffer(lease).is_err() {
        let _ = cancel_frame_buffer(lease);
        return Err("shell3-show-frame-publish");
    }
    surface.frame_contents[index] = Some(current);
    surface.clear_buffers[index] = false;
    if !glyphs.is_empty() && !cpu_paint {
        crate::log_once!(target: "apps";
            "shell3/show: bcs0-retired backend=legacy command=xy-mono-src-copy-blt rop=cc colors=metafmt bold=off cpu-rgba-paint=0 staging-frame=0\n"
        );
    }
    if crate::allcaps::text_blit::DIAGNOSTICS {
        let ns = crate::ui4::text_blit::nanos(crate::ui4::text_blit::stamp().wrapping_sub(present_started));
        crate::log_info!(target:"apps";"shell3/present: ap={} glyphs={} cpu_glyphs={} using_pan={} wall_ns={}\n",
            crate::percpu::current_slot(),glyphs.len(),if cpu_paint {glyphs.len()} else {0},using_pan,ns);
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
    output: &mut Vec<crate::intel::GucBcs0MonoGlyph>,
    reveals: &[super::super::update::StripReveal],
    cached: &[bool; 2],
) {
    let row = match update.row {
        super::super::SpecialRows::TitleRow => 0,
        super::super::SpecialRows::StatusRow => 1,
        super::super::SpecialRows::PromtRow => 2,
        super::super::SpecialRows::MatrixRow(index) => index as u32 + 3,
    };
    let y = row * microfont::FHEIGHT as u32 * scale;
    if y >= view.height {
        return;
    }
    let mut characters = update.text.chars();
    let count = update.remove.max(update.text.chars().count());
    for column in 0..count {
        let Some(x) = update
            .offset
            .checked_add(column)
            .and_then(|c| c.checked_mul(microfont::FWIDTH * scale as usize))
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
        let width = (microfont::FWIDTH as u32 * scale).min(view.width - x);
        let height = (microfont::FHEIGHT as u32 * scale).min(view.height - y);
        output.push(glyph_for_cell(
            x,
            y,
            width,
            height,
            (character, update.colors.get(column).copied().flatten()),
            scale,
        ));
    }
}

pub(super) fn glyph_for_cell(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    cell: (char, Option<super::super::RgbaColor>),
    scale: u32,
) -> crate::intel::GucBcs0MonoGlyph {
    let bits = microfont::glyph_cell_pixels(cell.0);
    let bias = usize::from(microfont::glyph_byte(cell.0) == b'q');
    let mut mask = [0u8; 64];
    for py in 0..height as usize {
        for px in 0..width as usize {
            let sx = px / scale as usize;
            let sy = py / scale as usize;
            let bit = sy * microfont::FWIDTH + sx.saturating_sub(bias);
            let count = microfont::FWIDTH * microfont::FHEIGHT;
            let ink = sx >= bias && bit < count && bits & (1 << (count - 1 - bit)) != 0;
            let underline = cell.1.is_some_and(super::super::RgbaColor::underline)
                && sy == microfont::FHEIGHT - 1;
            if ink || underline {
                mask[py * 2 + px / 8] |= 0x80 >> (px % 8);
            }
        }
    }
    crate::intel::GucBcs0MonoGlyph {
        x,
        y,
        width,
        height,
        mask,
        foreground: u32::from_le_bytes(cell.1.unwrap_or(super::FOREGROUND).rgba()),
        background: u32::from_le_bytes(
            cell.1
                .and_then(super::super::RgbaColor::background)
                .unwrap_or(super::BACKGROUND),
        ),
    }
}

fn bcs_surface(view: FrameRgbaView) -> crate::intel::GucBcs0RgbaSurface {
    crate::intel::GucBcs0RgbaSurface {
        phys: view.phys,
        gpu: view.gpu,
        bytes: view.byte_len,
        width: view.width,
        height: view.height,
        pitch_bytes: view.pitch,
    }
}
