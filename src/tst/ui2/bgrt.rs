use alloc::vec::Vec;

const UI2_BGRT_TEX_ID: u32 = crate::tst::ui2::ids::Ui2DemoTexId::Bgrt.get();
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

fn upload_bgrt_texture(
    tex_id: u32,
    window_id: u32,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
) -> bool {
    crate::r::io::cabi::queue_texture_rgba_image_upload_owned(
        tex_id,
        width,
        height,
        rgba,
        window_id,
        "ui2-bgrt-demo-upload",
    )
}

async fn wait_bgrt_texture_ready(tex_id: u32, attempts: usize) -> bool {
    const ASYNC_TEX_STATUS_READY: i32 = 2;
    for _ in 0..attempts {
        if crate::r::io::cabi::trueos_cabi_gfx_texture_status(tex_id) == ASYNC_TEX_STATUS_READY {
            return true;
        }
        embassy_time::Timer::after(embassy_time::Duration::from_millis(1)).await;
    }
    false
}

#[embassy_executor::task]
pub async fn ui2_bgrt_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-bgrt-demo");
    let Some((width, height, pixels)) = crate::efi::acpi::bgrt::decoded_logo_rgba() else {
        crate::log!("ui2-bgrt-demo: no BGRT logo available\n");
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
        crate::log!("ui2-bgrt-demo: window creation failed tex={}\n", UI2_BGRT_TEX_ID);
        return;
    };
    let _ = surface.bind_spawn_task("ui2-bgrt-demo");

    let window_id = surface.window_id();
    let _ = crate::r::ui2::set_window_left_scrollbar_visible(window_id, false);
    let _ = crate::r::ui2::set_window_bottom_scrollbar_visible(window_id, false);
    let _ = crate::r::ui2::set_window_content_fit_scale(window_id, true);

    let upload_ok = upload_bgrt_texture(UI2_BGRT_TEX_ID, window_id, rgba, width_u32, height_u32);

    if !upload_ok {
        crate::log!(
            "ui2-bgrt-demo: upload failed window={} tex={} size={}x{}\n",
            surface.window_id(),
            surface.tex_id(),
            width,
            height
        );
        return;
    }
    let ready = wait_bgrt_texture_ready(UI2_BGRT_TEX_ID, 120).await;
    let present = if ready {
        crate::r::ui2::request_window_content_present(window_id, "ui2-bgrt-demo-ready")
    } else {
        false
    };

    crate::log!(
        "ui2-bgrt-demo: window={} tex={} size={}x{} mode={} ready={} present={}\n",
        surface.window_id(),
        surface.tex_id(),
        width,
        height,
        if intel_direct { "direct" } else { "queued" },
        ready as u8,
        present as u8
    );

    loop {
        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-bgrt-demo", 33).await {
            break;
        }
    }
}
