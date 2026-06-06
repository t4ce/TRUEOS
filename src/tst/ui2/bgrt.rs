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

fn scale_rgba_nearest(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Option<Vec<u8>> {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return None;
    }
    let src_pixels = (src_w as usize).checked_mul(src_h as usize)?;
    if src.len() != src_pixels.checked_mul(4)? {
        return None;
    }
    let dst_pixels = (dst_w as usize).checked_mul(dst_h as usize)?;
    let mut dst = vec![0u8; dst_pixels.checked_mul(4)?];
    for y in 0..dst_h {
        let sy = (y as u64 * src_h as u64 / dst_h as u64) as usize;
        for x in 0..dst_w {
            let sx = (x as u64 * src_w as u64 / dst_w as u64) as usize;
            let src_off = (sy * src_w as usize + sx) * 4;
            let dst_off = (y as usize * dst_w as usize + x as usize) * 4;
            dst[dst_off..dst_off + 4].copy_from_slice(&src[src_off..src_off + 4]);
        }
    }
    Some(dst)
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
    let _ = crate::r::ui2::set_window_resize_maintain_aspect(window_id, true);

    let upload_ok =
        upload_bgrt_texture(UI2_BGRT_TEX_ID, window_id, rgba.clone(), width_u32, height_u32);

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

    let mut uploaded_w = width_u32;
    let mut uploaded_h = height_u32;
    loop {
        if let Some(content) = crate::r::ui2::window_content_rect_by_id(window_id) {
            let next_w = (content.w.max(1.0) + 0.5) as u32;
            let next_h = (content.h.max(1.0) + 0.5) as u32;
            if next_w != uploaded_w || next_h != uploaded_h {
                let Some(resized) =
                    scale_rgba_nearest(&rgba, width_u32, height_u32, next_w, next_h)
                else {
                    crate::log!(
                        "ui2-bgrt-demo: resize scale failed tex={} size={}x{}\n",
                        UI2_BGRT_TEX_ID,
                        next_w,
                        next_h
                    );
                    break;
                };
                if !upload_bgrt_texture(UI2_BGRT_TEX_ID, window_id, resized, next_w, next_h) {
                    crate::log!(
                        "ui2-bgrt-demo: resize upload failed tex={} size={}x{}\n",
                        UI2_BGRT_TEX_ID,
                        next_w,
                        next_h
                    );
                    break;
                }
                uploaded_w = next_w;
                uploaded_h = next_h;
                crate::log!(
                    "ui2-bgrt-demo: resized tex={} size={}x{}\n",
                    UI2_BGRT_TEX_ID,
                    uploaded_w,
                    uploaded_h
                );
            }
        }
        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-bgrt-demo", 33).await {
            break;
        }
    }
}
