//! Classical BCS0 monochrome expansion of Shell3's colored MicroFont cells.

use alloc::vec::Vec;
use trueos_time::{Duration, Timer};
use crate::ui4::{FrameRgbaView, acquire_frame_buffer, cancel_frame_buffer,
    publish_frame_buffer, writable_rgba_view};
use super::{Ui4Surface, rendered_lines};

pub(super) async fn present(
    surface: &mut Ui4Surface,
    lines: [&[(char, Option<super::super::RgbaColor>)]; 3],
    fallback_segments: &[super::super::SegmentUpdate],
    poisoned: &mut bool,
) -> Result<Option<crate::ui4::DamageRect>, &'static str> {
    let lease = acquire_frame_buffer(surface.frame).map_err(|_| "shell3-show-mono-destination-busy")?;
    let index = lease.buffer_index as usize;
    let previous = surface.frame_contents[index].clone();
    let current = rendered_lines(lines);
    let updates = super::super::update::diff_rendered_lines(previous.as_ref(), &current);
    let view = match writable_rgba_view(lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            return Err("shell3-show-mono-destination-view");
        }
    };
    let mut glyphs = Vec::new();
    for update in &updates {
        glyphs_for_update(view, update, surface.scale, &mut glyphs);
    }
    // A later chunk can fail admission after earlier chunks changed pixels.
    // Any retry must then repaint this entire buffer, including erased cells.
    surface.frame_contents[index] = None;
    let clearing = surface.clear_buffers[index];
    if clearing || !glyphs.is_empty() {
        crate::intel::dma_cache_flush_range(view.virt, view.byte_len);
    }
    if clearing {
        let submission = match crate::intel::queue_guc_bcs0_rgba_fill(
            bcs_surface(view), u32::from_le_bytes(super::BACKGROUND.rgba())) {
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
                crate::intel::GucBcs0CopyCompletion::Pending => Timer::after(Duration::from_millis(1)).await,
                crate::intel::GucBcs0CopyCompletion::Complete => break,
                _ => return Err("shell3-show-scale-clear-retirement-uncertain"),
            }
        }
        *poisoned = false;
    }
    for chunk in glyphs.chunks(crate::intel::GUC_BCS0_MONO_MAX_GLYPHS) {
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
                crate::intel::GucBcs0CopyCompletion::Pending => Timer::after(Duration::from_millis(1)).await,
                crate::intel::GucBcs0CopyCompletion::Complete => break,
                crate::intel::GucBcs0CopyCompletion::Failed
                | crate::intel::GucBcs0CopyCompletion::InvalidSubmission => {
                    return Err("shell3-show-mono-retirement-uncertain");
                }
            }
        }
        *poisoned = false;
    }
    crate::intel::dma_cache_flush_range(view.virt, view.byte_len);
    if publish_frame_buffer(lease).is_err() {
        let _ = cancel_frame_buffer(lease);
        return Err("shell3-show-frame-publish");
    }
    surface.frame_contents[index] = Some(current);
    surface.clear_buffers[index] = false;
    if !glyphs.is_empty() {
        crate::log_once!(target: "apps";
            "shell3/show: bcs0-retired backend=legacy command=xy-mono-src-copy-blt rop=cc colors=metafmt bold=off cpu-rgba-paint=0 staging-frame=0\n"
        );
    }
    let segments = if updates.is_empty() { fallback_segments } else { &updates };
    Ok(super::damage_for_segments(segments, surface.width, surface.height, surface.scale)
        .or_else(|| previous.is_none().then_some(crate::ui4::DamageRect::FULL))
        .map(|damage| if clearing { crate::ui4::DamageRect::FULL } else { damage }))
}

fn glyphs_for_update(
    view: FrameRgbaView,
    update: &super::super::SegmentUpdate,
    scale: u32,
    output: &mut Vec<crate::intel::GucBcs0MonoGlyph>,
) {
    let row = match update.row {
        super::super::SpecialRows::TitleRow => 0,
        super::super::SpecialRows::StatusRow => 1,
        super::super::SpecialRows::PromtRow => 2,
    };
    let y = row * microfont::FHEIGHT as u32 * scale;
    if y >= view.height { return; }
    let mut characters = update.text.chars();
    let count = update.remove.max(update.text.chars().count());
    for column in 0..count {
        let Some(x) = update.offset.checked_add(column)
            .and_then(|c| c.checked_mul(microfont::FWIDTH * scale as usize))
            .and_then(|x| u32::try_from(x).ok()) else { break; };
        if x >= view.width { break; }
        let character = characters.next().unwrap_or(' ');
        let atlas = microfont::glyph_byte(character);
        let bits = microfont::font_pixels(atlas);
        let mut mask = [0u8; 64];
        let width = (microfont::FWIDTH as u32 * scale).min(view.width - x);
        let height = (microfont::FHEIGHT as u32 * scale).min(view.height - y);
        // Match MicroFont's existing q placement. Each row is a 16-bit word,
        // with its leftmost pixel in the MSB of the first byte.
        let bias = usize::from(atlas == b'q');
        for py in 0..height as usize {
            for px in 0..width as usize {
                let sx = px / scale as usize;
                let sy = py / scale as usize;
                if sx < bias { continue; }
                let bit = sy * microfont::FWIDTH + sx - bias;
                if bit < 64 && bits & (1 << (63 - bit)) != 0 {
                    mask[py * 2 + px / 8] |= 0x80 >> (px % 8);
                }
            }
        }
        output.push(crate::intel::GucBcs0MonoGlyph {
            x, y, width, height, mask,
            foreground: u32::from_le_bytes(update.colors.get(column).copied().flatten()
                .unwrap_or(super::FOREGROUND).rgba()),
            background: u32::from_le_bytes(super::BACKGROUND.rgba()),
        });
    }
}

fn bcs_surface(view: FrameRgbaView) -> crate::intel::GucBcs0RgbaSurface {
    crate::intel::GucBcs0RgbaSurface {
        phys: view.phys, gpu: view.gpu, bytes: view.byte_len,
        width: view.width, height: view.height, pitch_bytes: view.pitch,
    }
}
