use alloc::vec::Vec;

const UI2_BGRT_TEX_ID: u32 = crate::tst_ui2_ids::Ui2DemoTexId::Bgrt.get();
const UI2_BGRT_WINDOW_X: f32 = 240.0;
const UI2_BGRT_WINDOW_Y: f32 = 120.0;
const UI2_BGRT_WINDOW_Z: i16 = 31;

fn bgrt_pixels_to_rgba(pixels: &[u32]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(pixels.len().saturating_mul(4));
    for &pixel in pixels {
        let r = ((pixel >> 16) & 0xFF) as u8;
        let g = ((pixel >> 8) & 0xFF) as u8;
        let b = (pixel & 0xFF) as u8;
        rgba.extend_from_slice(&[r, g, b, 0xFF]);
    }
    rgba
}

#[embassy_executor::task]
pub async fn ui2_bgrt_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-bgrt-demo");
    let Some((width, height, pixels)) = crate::efi::acpi::bgrt::decoded_logo_rgba() else {
        crate::log_trace!("ui2-bgrt-demo: no BGRT logo available\n");
        return;
    };
    let width_u32: u32 = width.try_into().expect("BGRT width exceeds u32");
    let height_u32: u32 = height.try_into().expect("BGRT height exceeds u32");

    let intel_direct = crate::gfx::is_intel_active();
    let rgba = bgrt_pixels_to_rgba(pixels);
    let content_rect = crate::r::ui2::Ui2Rect {
        x: UI2_BGRT_WINDOW_X,
        y: UI2_BGRT_WINDOW_Y,
        w: width as f32,
        h: height as f32,
    };

    if intel_direct {
        let rc = unsafe {
            crate::r::io::cabi::trueos_cabi_gfx_upload_texture_rgba_image(
                UI2_BGRT_TEX_ID,
                width_u32,
                height_u32,
                rgba.as_ptr(),
                rgba.len(),
            )
        };
        if rc != 0 {
            crate::log_trace!(
                "ui2-bgrt-demo: direct upload failed tex={} size={}x{} rc={}\n",
                UI2_BGRT_TEX_ID,
                width,
                height,
                rc
            );
            return;
        }
    }

    let surface = if intel_direct {
        crate::r::ui2::Ui2SurfaceWindow::create_from_existing_texture_with_size(
            "Demo BGRT",
            content_rect,
            UI2_BGRT_WINDOW_Z,
            128,
            UI2_BGRT_TEX_ID,
            true,
            width_u32,
            height_u32,
        )
    } else {
        crate::r::ui2::Ui2SurfaceWindow::new(
            "Demo BGRT",
            content_rect,
            UI2_BGRT_WINDOW_Z,
            128,
            UI2_BGRT_TEX_ID,
            true,
            [0x00, 0x00, 0x00, 0x00],
        )
    };

    let Some(surface) = surface else {
        crate::log_trace!("ui2-bgrt-demo: window creation failed tex={}\n", UI2_BGRT_TEX_ID);
        return;
    };
    let _ = surface.bind_spawn_task("ui2-bgrt-demo");

    let window_id = surface.window_id();
    let _ = crate::r::ui2::set_window_left_scrollbar_visible(window_id, false);
    let _ = crate::r::ui2::set_window_bottom_scrollbar_visible(window_id, false);
    let _ = crate::r::ui2::set_window_resize_maintain_aspect(window_id, true);

    let upload_ok = if intel_direct {
        true
    } else {
        surface.upload_rgba_owned(rgba, "ui2-bgrt-demo-upload")
    };

    if !upload_ok {
        crate::log_trace!(
            "ui2-bgrt-demo: upload failed window={} tex={} size={}x{}\n",
            surface.window_id(),
            surface.tex_id(),
            width,
            height
        );
        return;
    }

    crate::log_trace!(
        "ui2-bgrt-demo: window={} tex={} size={}x{} mode={}\n",
        surface.window_id(),
        surface.tex_id(),
        width,
        height,
        if intel_direct { "direct" } else { "queued" }
    );

    loop {
        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-bgrt-demo", 3_600_000).await {
            break;
        }
    }
}
