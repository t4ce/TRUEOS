use embassy_time::{Duration as EmbassyDuration, Timer};

const LOADSCREEN_TITLE_TEX_ID: u32 = 4_703;
const LOADSCREEN_MIN_LIFETIME_MS: u64 = 4_000;
const LOADSCREEN_WAIT_POLL_MS: u64 = 100;

#[repr(C)]
#[derive(Clone, Copy)]
struct LoadscreenTexVertex {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

fn draw_mask_quad_uv_no_present(
    tex_id: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    rgba: (u8, u8, u8, u8),
    view_w: u32,
    view_h: u32,
    linear: bool,
) -> bool {
    if tex_id == 0 || !(width > 0.0 && height > 0.0) {
        return false;
    }

    let vw = view_w.max(1) as f32;
    let vh = view_h.max(1) as f32;
    let left = (2.0 * (x / vw)) - 1.0;
    let right = (2.0 * ((x + width) / vw)) - 1.0;
    let top = 1.0 - (2.0 * (y / vh));
    let bottom = 1.0 - (2.0 * ((y + height) / vh));
    let verts = [
        LoadscreenTexVertex {
            x: left,
            y: bottom,
            u: u0,
            v: v1,
            r: rgba.0,
            g: rgba.1,
            b: rgba.2,
            a: rgba.3,
        },
        LoadscreenTexVertex {
            x: right,
            y: bottom,
            u: u1,
            v: v1,
            r: rgba.0,
            g: rgba.1,
            b: rgba.2,
            a: rgba.3,
        },
        LoadscreenTexVertex {
            x: right,
            y: top,
            u: u1,
            v: v0,
            r: rgba.0,
            g: rgba.1,
            b: rgba.2,
            a: rgba.3,
        },
        LoadscreenTexVertex {
            x: left,
            y: bottom,
            u: u0,
            v: v1,
            r: rgba.0,
            g: rgba.1,
            b: rgba.2,
            a: rgba.3,
        },
        LoadscreenTexVertex {
            x: right,
            y: top,
            u: u1,
            v: v0,
            r: rgba.0,
            g: rgba.1,
            b: rgba.2,
            a: rgba.3,
        },
        LoadscreenTexVertex {
            x: left,
            y: top,
            u: u0,
            v: v0,
            r: rgba.0,
            g: rgba.1,
            b: rgba.2,
            a: rgba.3,
        },
    ];

    let filter = if linear { 1 } else { 0 };
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, filter, filter) };
    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0)
    };
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_tex_triangles_no_present(
            tex_id,
            verts.as_ptr() as *const u8,
            verts.len() * core::mem::size_of::<LoadscreenTexVertex>(),
        )
    };
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    rc == 0
}

fn draw_mask_slice_no_present(
    tex_id: u32,
    title: &crate::gfx::imbafont::ImbaFontMaskTexture,
    y0: f32,
    y1: f32,
    dx: f32,
    uv_dx: f32,
    rgba: (u8, u8, u8, u8),
    view_w: u32,
    view_h: u32,
) -> bool {
    let slice_top = y0.clamp(0.0, 1.0);
    let slice_bottom = y1.clamp(slice_top, 1.0);
    let slice_h = (slice_bottom - slice_top) * title.height as f32;
    if slice_h <= 0.0 {
        return true;
    }

    draw_mask_quad_uv_no_present(
        tex_id,
        title.draw_x + dx,
        title.draw_y + slice_top * title.height as f32,
        title.width as f32,
        slice_h,
        uv_dx,
        slice_top,
        1.0 + uv_dx,
        slice_bottom,
        rgba,
        view_w,
        view_h,
        true,
    )
}

fn draw_loadscreen_title_fx(
    tex_id: u32,
    title: &crate::gfx::imbafont::ImbaFontMaskTexture,
    tile_h: f32,
    view_w: u32,
    view_h: u32,
) {
    let band_x = title.draw_x - tile_h * 0.22;
    let band_y = title.draw_y + title.height as f32 * 0.34;
    let band_w = title.width as f32 + tile_h * 0.44;
    let band_h = (tile_h * 0.32).max(18.0);
    let _ = crate::gfx::lyon::draw_horizontal_three_stop_rect_no_present(
        band_x,
        band_y,
        band_w,
        band_h,
        (0x2E, 0xD8, 0xFF, 20),
        (0xFF, 0xD6, 0x54, 46),
        (0xFF, 0x72, 0x8D, 20),
        0.47,
        view_w,
        view_h,
    );

    let _ = draw_mask_quad_uv_no_present(
        tex_id,
        title.draw_x + tile_h * 0.025,
        title.draw_y + tile_h * 0.055,
        title.width as f32 * 1.015,
        title.height as f32 * 1.02,
        0.0,
        0.0,
        1.0,
        1.0,
        (0x08, 0x10, 0x1A, 44),
        view_w,
        view_h,
        true,
    );

    let _ = draw_mask_slice_no_present(
        tex_id,
        title,
        0.00,
        0.24,
        -tile_h * 0.10,
        0.020,
        (0x00, 0xC8, 0xFF, 104),
        view_w,
        view_h,
    );
    let _ = draw_mask_slice_no_present(
        tex_id,
        title,
        0.24,
        0.48,
        tile_h * 0.07,
        -0.016,
        (0xFF, 0x72, 0x54, 92),
        view_w,
        view_h,
    );
    let _ = draw_mask_slice_no_present(
        tex_id,
        title,
        0.48,
        0.70,
        -tile_h * 0.04,
        0.030,
        (0xC7, 0xFF, 0x52, 72),
        view_w,
        view_h,
    );
    let _ = draw_mask_slice_no_present(
        tex_id,
        title,
        0.70,
        1.00,
        tile_h * 0.09,
        -0.024,
        (0x36, 0x8E, 0xFF, 88),
        view_w,
        view_h,
    );
    let _ = draw_mask_slice_no_present(
        tex_id,
        title,
        0.40,
        0.58,
        tile_h * 0.15,
        -0.040,
        (0xFF, 0x3D, 0xB8, 82),
        view_w,
        view_h,
    );
}

#[inline]
fn boot_probe_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

#[embassy_executor::task]
pub async fn gfx_loadscreen_task() {
    const LOADSCREEN_BG_RGB: u32 = 0xFFFFFF;
    const MSG: &[u8] = b"TRUE OS";

    let (fb_w, fb_h) = crate::limine::framebuffer_response()
        .and_then(|resp| resp.framebuffers().next())
        .map(|fb| (fb.width() as f32, fb.height() as f32))
        .unwrap_or((1024.0, 768.0));
    let tile_h = (fb_h * 0.16).clamp(56.0, 140.0);
    let text_layout = crate::gfx::imbafont::layout_text_centered(
        crate::gfx::imbafont::ImbaFontFace::Grow,
        MSG,
        fb_w,
        fb_h,
        tile_h,
    );
    let title_mask = text_layout.and_then(|layout| {
        crate::gfx::imbafont::rasterize_text_mask_texture(
            crate::gfx::imbafont::ImbaFontFace::Grow,
            MSG,
            &layout,
            tile_h * 0.16,
        )
    });
    let title_mask_uploaded = if let Some(mask) = title_mask.as_ref() {
        let rc = unsafe {
            crate::r::io::cabi::trueos_cabi_gfx_upload_texture_rgba(
                LOADSCREEN_TITLE_TEX_ID,
                mask.width,
                mask.height,
                mask.rgba.as_ptr(),
                mask.rgba.len(),
            )
        };
        if rc != 0 {
            crate::log!("gfx-loadscreen: title mask upload failed rc={}\n", rc);
        }
        rc == 0
    } else {
        false
    };

    let mut first_frame_ms = 0u64;

    loop {
        crate::gfx::with_cabi_frame_lock(|| {
            let begin_rc =
                unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame(LOADSCREEN_BG_RGB) };
            if begin_rc != 0 {
                crate::log!("gfx-loadscreen: begin_frame failed rc={}\n", begin_rc);
                return;
            }

            if title_mask_uploaded {
                if let Some(mask) = title_mask.as_ref() {
                    draw_loadscreen_title_fx(
                        LOADSCREEN_TITLE_TEX_ID,
                        mask,
                        tile_h,
                        fb_w as u32,
                        fb_h as u32,
                    );
                }
            }

            if let Some(layout) = text_layout {
                let _ = crate::gfx::imbafont::draw_text_in_frame(
                    crate::gfx::imbafont::ImbaFontFace::Grow,
                    MSG,
                    &layout,
                    fb_w as u32,
                    fb_h as u32,
                    (8, 10, 16),
                    244,
                );
            }
            unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
            if first_frame_ms == 0 {
                first_frame_ms = boot_probe_ms();
                crate::r::readiness::set(crate::r::readiness::LOADSCREEN_FRAME_READY);
            }
        });

        if first_frame_ms != 0 {
            break;
        }

        Timer::after(EmbassyDuration::from_millis(LOADSCREEN_WAIT_POLL_MS)).await;
    }

    crate::log!("boot-probe: loadscreen start ms={}\n", first_frame_ms);
    let min_end_ms = first_frame_ms.saturating_add(LOADSCREEN_MIN_LIFETIME_MS);

    loop {
        let now_ms = boot_probe_ms();
        if now_ms >= min_end_ms {
            crate::r::readiness::set(crate::r::readiness::LOADSCREEN_END);
            break;
        }

        Timer::after(EmbassyDuration::from_millis(LOADSCREEN_WAIT_POLL_MS)).await;
    }

    crate::log!(
        "boot-probe: loadscreen end ms={} lived_ms={}\n",
        boot_probe_ms(),
        boot_probe_ms().saturating_sub(first_frame_ms)
    );
}
