//! Classical BCS0 XY_SRC_COPY_BLT adapter for Shell3's MicroFont patches.

use alloc::vec::Vec;
use trueos_time::{Duration, Timer};

use crate::ui4::{
    FrameRgbaView, acquire_frame_buffer, acquire_published_frame,
    cancel_frame_buffer, publish_frame_buffer, published_rgba_view, release_published_frame,
    writable_rgba_view,
};

use super::{Ui4Surface, rendered_lines};

pub(super) async fn present(
    surface: &mut Ui4Surface,
    lines: [&str; 3],
    fallback_segments: &[super::super::SegmentUpdate],
    poisoned: &mut bool,
) -> Result<Option<crate::ui4::DamageRect>, &'static str> {
    let source_frame = surface.source.ok_or("shell3-show-copy-source-missing")?;
    let source_lease = acquire_frame_buffer(source_frame)
        .map_err(|_| "shell3-show-copy-source-busy")?;
    let source_index = source_lease.buffer_index as usize;
    let source_previous = surface.source_contents[source_index].clone();
    let current = rendered_lines(lines);
    let source_updates = super::super::update::diff_rendered_lines(source_previous.as_ref(), &current);
    let source_view = match writable_rgba_view(source_lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = cancel_frame_buffer(source_lease);
            return Err("shell3-show-copy-source-view");
        }
    };
    if super::cpu::paint_segments(source_view, &source_updates).is_err() {
        let _ = cancel_frame_buffer(source_lease);
        return Err("shell3-show-copy-glyph-paint");
    }
    if publish_frame_buffer(source_lease).is_err() {
        let _ = cancel_frame_buffer(source_lease);
        return Err("shell3-show-copy-source-publish");
    }
    surface.source_contents[source_index] = Some(current.clone());

    let source_read = match acquire_published_frame(source_frame) {
        Ok(lease) => lease,
        Err(_) => return Err("shell3-show-copy-source-read-lease"),
    };
    let source_read_view = match published_rgba_view(source_read) {
        Ok(view) => view,
        Err(_) => {
            let _ = release_published_frame(source_read);
            return Err("shell3-show-copy-source-read-view");
        }
    };

    let destination_lease = match acquire_frame_buffer(surface.frame) {
        Ok(lease) => lease,
        Err(_) => {
            let _ = release_published_frame(source_read);
            return Err("shell3-show-copy-destination-busy");
        }
    };
    let destination_index = destination_lease.buffer_index as usize;
    let destination_previous = surface.frame_contents[destination_index].clone();
    let destination_updates =
        super::super::update::diff_rendered_lines(destination_previous.as_ref(), &current);
    let destination_view = match writable_rgba_view(destination_lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = release_published_frame(source_read);
            let _ = cancel_frame_buffer(destination_lease);
            return Err("shell3-show-copy-destination-view");
        }
    };

    let mut copies = Vec::new();
    for update in &destination_updates {
        if let Some(copy) = copy_for_update(source_read_view, destination_view, update) {
            copies.push(copy);
        }
    }
    if !copies.is_empty() {
        let submission = match crate::intel::queue_guc_bcs0_legacy_rgba_copies(bcs_surface(destination_view), &copies) {
            Ok(submission) => submission,
            Err(crate::intel::GucBcs0CopySubmitError::SubmitFailed) => {
                *poisoned = true;
                return Err("shell3-show-bcs0-submit-uncertain");
            }
            Err(_) => {
                let _ = release_published_frame(source_read);
                let _ = cancel_frame_buffer(destination_lease);
                return Err("shell3-show-bcs0-unavailable");
            }
        };
        // Keep both the source reader and destination writer pinned until BCS0 retires.
        *poisoned = true;
        loop {
            match crate::intel::poll_guc_bcs0_rgba_copies(submission) {
                crate::intel::GucBcs0CopyCompletion::Pending => {
                    Timer::after(Duration::from_millis(1)).await;
                }
                crate::intel::GucBcs0CopyCompletion::Complete => {
                    crate::log_once!(target: "apps";
                        "shell3/show: bcs0-retired backend=legacy command=xy-src-copy-blt rop=cc rgba=all glyphs=microfont\n"
                    );
                    break;
                }
                crate::intel::GucBcs0CopyCompletion::Failed
                | crate::intel::GucBcs0CopyCompletion::InvalidSubmission => {
                    return Err("shell3-show-bcs0-retirement-uncertain");
                }
            }
        }
    }
    let _ = release_published_frame(source_read);
    *poisoned = false;
    crate::intel::dma_cache_flush_range(destination_view.virt, destination_view.byte_len);
    if publish_frame_buffer(destination_lease).is_err() {
        let _ = cancel_frame_buffer(destination_lease);
        return Err("shell3-show-frame-publish");
    }
    *poisoned = false;
    surface.frame_contents[destination_index] = Some(current);

    let segments = if destination_updates.is_empty() {
        fallback_segments
    } else {
        &destination_updates
    };
    let damage = super::damage_for_segments(segments, surface.width, surface.height)
        .or_else(|| destination_previous.is_none().then_some(crate::ui4::DamageRect::FULL));
    Ok(damage)
}

fn copy_for_update(
    source: FrameRgbaView,
    destination: FrameRgbaView,
    update: &super::super::SegmentUpdate,
) -> Option<crate::intel::GucBcs0RgbaCopy> {
    let row = match update.row {
        super::super::SpecialRows::TitleRow => 0u32,
        super::super::SpecialRows::StatusRow => 1,
        super::super::SpecialRows::PromtRow => 2,
    };
    let x = u32::try_from(update.offset).ok()?.checked_mul(microfont::FWIDTH as u32)?;
    let y = row.checked_mul(microfont::FHEIGHT as u32)?;
    let columns = update.remove.max(update.text.chars().count());
    let width = u32::try_from(columns).ok()?.checked_mul(microfont::FWIDTH as u32)?;
    if width == 0 || x >= source.width || x >= destination.width || y >= source.height || y >= destination.height {
        return None;
    }
    Some(crate::intel::GucBcs0RgbaCopy {
        source: bcs_surface(source),
        source_x: x,
        source_y: y,
        destination_x: x,
        destination_y: y,
        width: width.min(source.width - x).min(destination.width - x),
        height: (microfont::FHEIGHT as u32).min(source.height - y).min(destination.height - y),
    })
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
