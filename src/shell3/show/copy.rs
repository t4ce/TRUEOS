//! BCS0 copy adapter for Shell3's UI4 image frames.

use trueos_time::{Duration, Timer};

use crate::ui4::{
    acquire_frame_buffer, cancel_frame_buffer, publish_frame_buffer, writable_rgba_view,
};

use super::Ui4Surface;

pub(super) async fn present(
    surface: &Ui4Surface,
    lines: [&str; 3],
    poisoned: &mut bool,
) -> Result<(), &'static str> {
    let source_frame = surface.source.ok_or("shell3-show-copy-source-missing")?;
    let source_lease = acquire_frame_buffer(source_frame)
        .map_err(|_| "shell3-show-copy-source-busy")?;
    let source_view = match writable_rgba_view(source_lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = cancel_frame_buffer(source_lease);
            return Err("shell3-show-copy-source-view");
        }
    };
    if super::cpu::paint_text(source_view, lines).is_err() {
        let _ = cancel_frame_buffer(source_lease);
        return Err("shell3-show-copy-glyph-paint");
    }
    crate::intel::dma_cache_flush_range(source_view.virt, source_view.byte_len);

    let destination_lease = match acquire_frame_buffer(surface.frame) {
        Ok(lease) => lease,
        Err(_) => {
            let _ = cancel_frame_buffer(source_lease);
            return Err("shell3-show-copy-destination-busy");
        }
    };
    let destination_view = match writable_rgba_view(destination_lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = cancel_frame_buffer(source_lease);
            let _ = cancel_frame_buffer(destination_lease);
            return Err("shell3-show-copy-destination-view");
        }
    };
    let source = bcs_surface(source_view);
    let destination = bcs_surface(destination_view);
    let copy = crate::intel::GucBcs0RgbaCopy {
        source,
        source_x: 0,
        source_y: 0,
        destination_x: 0,
        destination_y: 0,
        width: surface.width,
        height: surface.height,
    };
    let submission = match crate::intel::queue_guc_bcs0_rgba_copies(destination, &[copy]) {
        Ok(submission) => submission,
        Err(crate::intel::GucBcs0CopySubmitError::SubmitFailed) => {
            *poisoned = true;
            return Err("shell3-show-bcs0-submit-uncertain");
        }
        Err(_) => {
            let _ = cancel_frame_buffer(source_lease);
            let _ = cancel_frame_buffer(destination_lease);
            return Err("shell3-show-bcs0-unavailable");
        }
    };
    // Both leases remain pinned if this future is cancelled before BCS0 retires.
    *poisoned = true;

    loop {
        match crate::intel::poll_guc_bcs0_rgba_copies(submission) {
            crate::intel::GucBcs0CopyCompletion::Pending => {
                Timer::after(Duration::from_millis(1)).await;
            }
            crate::intel::GucBcs0CopyCompletion::Complete => break,
            crate::intel::GucBcs0CopyCompletion::Failed
            | crate::intel::GucBcs0CopyCompletion::InvalidSubmission => {
                return Err("shell3-show-bcs0-retirement-uncertain");
            }
        }
    }

    let _ = cancel_frame_buffer(source_lease);
    crate::intel::dma_cache_flush_range(destination_view.virt, destination_view.byte_len);
    if publish_frame_buffer(destination_lease).is_err() {
        let _ = cancel_frame_buffer(destination_lease);
        return Err("shell3-show-frame-publish");
    }
    *poisoned = false;
    Ok(())
}

fn bcs_surface(view: crate::ui4::FrameRgbaView) -> crate::intel::GucBcs0RgbaSurface {
    crate::intel::GucBcs0RgbaSurface {
        phys: view.phys,
        gpu: view.gpu,
        bytes: view.byte_len,
        width: view.width,
        height: view.height,
        pitch_bytes: view.pitch,
    }
}
