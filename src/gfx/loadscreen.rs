use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Once;

#[repr(C)]
#[derive(Clone, Copy)]
struct TexVertex {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

struct LogoTexture {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

const LOADSCREEN_LOGO_TEX_ID: u32 = 1002;
const LOGO_PAD_X: f32 = 24.0;
const LOGO_PAD_Y: f32 = 24.0;
const LOADSCREEN_ICON_GAP_Y: f32 = 8.0;
const LOADSCREEN_ICON_TILE_SCALE: f32 = 0.72;
const LOADSCREEN_ICON_SCALE_START: f32 = 0.70;
const LOADSCREEN_ICON_SCALE_END: f32 = 1.30;

static LOADSCREEN_LOGO: Once<LogoTexture> = Once::new();
static LOADSCREEN_LOGO_UPLOADED: AtomicBool = AtomicBool::new(false);

#[inline]
fn boot_probe_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

fn loadscreen_logo() -> &'static LogoTexture {
    LOADSCREEN_LOGO.call_once(|| {
        let (pixels, width, height) = crate::vga::get_logo_buffer();
        let mut rgba = Vec::with_capacity(pixels.len().saturating_mul(4));
        for pixel in pixels {
            rgba.extend_from_slice(&pixel.to_le_bytes());
        }

        LogoTexture {
            rgba,
            width: width as u32,
            height: height as u32,
        }
    })
}

#[inline]
fn loadscreen_icon_scale(index: usize, total: usize) -> f32 {
    if total <= 1 {
        return 1.0;
    }

    let t = index as f32 / (total.saturating_sub(1)) as f32;
    let eased = t * t * (3.0 - 2.0 * t);
    LOADSCREEN_ICON_SCALE_START + (LOADSCREEN_ICON_SCALE_END - LOADSCREEN_ICON_SCALE_START) * eased
}

fn draw_textured_quad_in_frame(
    tex_id: u32,
    x0: f32,
    y0: f32,
    width: f32,
    height: f32,
    view_w: u32,
    view_h: u32,
    alpha: u8,
) -> bool {
    if width <= 0.0 || height <= 0.0 {
        return false;
    }

    let x1 = (x0 + width).min(view_w as f32);
    let y1 = (y0 + height).min(view_h as f32);

    let nx0 = (2.0 * (x0.max(0.0) / view_w.max(1) as f32)) - 1.0;
    let ny0 = 1.0 - (2.0 * (y0.max(0.0) / view_h.max(1) as f32));
    let nx1 = (2.0 * (x1 / view_w.max(1) as f32)) - 1.0;
    let ny1 = 1.0 - (2.0 * (y1 / view_h.max(1) as f32));

    let verts = [
        TexVertex {
            x: nx0,
            y: ny1,
            u: 0.0,
            v: 1.0,
            r: 255,
            g: 255,
            b: 255,
            a: alpha,
        },
        TexVertex {
            x: nx1,
            y: ny1,
            u: 1.0,
            v: 1.0,
            r: 255,
            g: 255,
            b: 255,
            a: alpha,
        },
        TexVertex {
            x: nx1,
            y: ny0,
            u: 1.0,
            v: 0.0,
            r: 255,
            g: 255,
            b: 255,
            a: alpha,
        },
        TexVertex {
            x: nx0,
            y: ny1,
            u: 0.0,
            v: 1.0,
            r: 255,
            g: 255,
            b: 255,
            a: alpha,
        },
        TexVertex {
            x: nx1,
            y: ny0,
            u: 1.0,
            v: 0.0,
            r: 255,
            g: 255,
            b: 255,
            a: alpha,
        },
        TexVertex {
            x: nx0,
            y: ny0,
            u: 0.0,
            v: 0.0,
            r: 255,
            g: 255,
            b: 255,
            a: alpha,
        },
    ];

    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0)
    };

    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_tex_triangles_no_present(
            tex_id,
            verts.as_ptr() as *const u8,
            core::mem::size_of_val(&verts),
        )
    };
    rc == 0
}

fn draw_icon_strip_in_frame(
    layout: &crate::gfx::imbafont::ImbaFontRunLayout,
    view_w: u32,
    view_h: u32,
    alpha: u8,
) -> bool {
    crate::gfx::imbafont::draw_run_in_frame(
        crate::gfx::imbafont::ImbaFontFace::Regular,
        layout,
        view_w,
        view_h,
        (0, 0, 0),
        alpha,
        LOADSCREEN_ICON_SCALE_START,
        LOADSCREEN_ICON_SCALE_END,
    )
}

fn loadscreen_icon_layout(
    view_w: f32,
    text_y: f32,
    text_h: f32,
) -> Option<crate::gfx::imbafont::ImbaFontRunLayout> {
    crate::gfx::imbafont::layout_run_centered(
        crate::gfx::imbafont::ImbaFontFace::Regular,
        view_w,
        text_y + text_h + LOADSCREEN_ICON_GAP_Y,
        (text_h * LOADSCREEN_ICON_TILE_SCALE).max(12.0),
        LOADSCREEN_ICON_SCALE_START,
        LOADSCREEN_ICON_SCALE_END,
    )
}

fn draw_icon_strip_with_text_metrics(
    view_w: u32,
    view_h: u32,
    text_y: f32,
    text_h: f32,
    alpha: u8,
) -> bool {
    let Some(layout) = loadscreen_icon_layout(view_w as f32, text_y, text_h) else {
        return false;
    };
    draw_icon_strip_in_frame(&layout, view_w, view_h, alpha)
}

fn ensure_logo_uploaded() -> bool {
    if LOADSCREEN_LOGO_UPLOADED.load(Ordering::Acquire) {
        return true;
    }

    let logo = loadscreen_logo();
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_upload_texture_rgba_image(
            LOADSCREEN_LOGO_TEX_ID,
            logo.width,
            logo.height,
            logo.rgba.as_ptr(),
            logo.rgba.len(),
        )
    };
    if rc != 0 {
        return false;
    }
    LOADSCREEN_LOGO_UPLOADED.store(true, Ordering::Release);
    true
}

fn draw_logo_in_frame(view_w: u32, view_h: u32) -> bool {
    if !ensure_logo_uploaded() {
        return false;
    }

    let logo = loadscreen_logo();
    if logo.width == 0 || logo.height == 0 {
        return false;
    }

    let x0 = (view_w as f32 - logo.width as f32 - LOGO_PAD_X).max(0.0);
    let y0 = (view_h as f32 - logo.height as f32 - LOGO_PAD_Y).max(0.0);
    draw_textured_quad_in_frame(
        LOADSCREEN_LOGO_TEX_ID,
        x0,
        y0,
        logo.width as f32,
        logo.height as f32,
        view_w,
        view_h,
        255,
    )
}

#[embassy_executor::task]
pub async fn gfx_loadscreen_task() {
    const LOADSCREEN_BG_RGB: u32 = 0xF4F4F4;
    const MSG: &[u8] = b"TRUE OS \xA7";
    const TEXT_PAD_X: f32 = 12.0;
    const TEXT_PAD_Y: f32 = 10.0;

    let (fb_w, fb_h) = crate::limine::framebuffer_response()
        .and_then(|resp| resp.framebuffers().next())
        .map(|fb| (fb.width() as f32, fb.height() as f32))
        .unwrap_or((1024.0, 768.0));
    let start_ms = boot_probe_ms();
    crate::log!("boot-probe: loadscreen start ms={}\n", start_ms);

    let atlas = crate::gfx::text::font_atlas_large_view();
    let fallback = atlas.index.get(b'?' as usize).copied().unwrap_or(0);
    let mut text_w = 0.0f32;
    for &ch in MSG {
        let mut slot = atlas.index.get(ch as usize).copied().unwrap_or(fallback);
        if slot == u16::MAX {
            slot = fallback;
        }
        text_w += atlas
            .widths
            .get(slot as usize)
            .copied()
            .unwrap_or(atlas.cell_w as u8) as f32;
    }
    let text_x = ((fb_w - text_w) * 0.5).max(0.0);
    let text_y = ((fb_h - atlas.cell_h as f32) * 0.5).max(0.0);
    let text_clear_x = (text_x - TEXT_PAD_X).max(0.0);
    let text_clear_y = (text_y - TEXT_PAD_Y).max(0.0);
    let text_clear_w = (text_w + (TEXT_PAD_X * 2.0)).min(fb_w - text_clear_x);
    let text_clear_h = (atlas.cell_h as f32 + (TEXT_PAD_Y * 2.0)).min(fb_h - text_clear_y);
    let icon_layout = loadscreen_icon_layout(fb_w, text_y, atlas.cell_h as f32);
    let (clear_x, clear_y, clear_w, clear_h) = if let Some(layout) = icon_layout {
        let icon_x0 = (layout.origin_x + layout.vis_left).max(0.0);
        let icon_y0 = (layout.baseline_y + layout.vis_top).max(0.0);
        let icon_x1 = (layout.origin_x + layout.vis_right).min(fb_w);
        let icon_y1 = (layout.baseline_y + layout.vis_bottom).min(fb_h);
        let clear_x = text_clear_x.min(icon_x0);
        let clear_y = text_clear_y.min(icon_y0);
        let clear_x1 = (text_clear_x + text_clear_w).max(icon_x1);
        let clear_y1 = (text_clear_y + text_clear_h).max(icon_y1);
        (
            clear_x,
            clear_y,
            (clear_x1 - clear_x).max(0.0),
            (clear_y1 - clear_y).max(0.0),
        )
    } else {
        (text_clear_x, text_clear_y, text_clear_w, text_clear_h)
    };

    crate::gfx::with_cabi_frame_lock(|| {
        let begin_rc =
            unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame(LOADSCREEN_BG_RGB) };
        if begin_rc == 0 {
            let _ = draw_logo_in_frame(fb_w as u32, fb_h as u32);
            crate::gfx::text::draw_atlas_text_in_frame_alpha(
                MSG,
                text_x,
                text_y,
                fb_w as u32,
                fb_h as u32,
                255,
            );
            let _ = draw_icon_strip_with_text_metrics(
                fb_w as u32,
                fb_h as u32,
                text_y,
                atlas.cell_h as f32,
                255,
            );
            unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
            crate::r::readiness::set(crate::r::readiness::LOADSCREEN_FRAME_READY);
        }
    });

    let mut frame: u32 = 1;
    let deadline = embassy_time::Instant::now() + EmbassyDuration::from_secs(5);
    while embassy_time::Instant::now() < deadline {
        let phase = (frame as f32) * (core::f32::consts::TAU / 120.0);
        let alpha = ((libm::sinf(phase) * 0.5 + 0.5) * 255.0) as u8;
        crate::gfx::with_cabi_frame_lock(|| {
            let begin_rc = unsafe {
                crate::r::io::cabi::trueos_cabi_gfx_begin_frame_preserve(LOADSCREEN_BG_RGB)
            };
            if begin_rc == 0 {
                let _ =
                    unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
                let _ = crate::gfx::lyon::draw_solid_rect_no_present(
                    clear_x,
                    clear_y,
                    clear_w,
                    clear_h,
                    (0xF4, 0xF4, 0xF4, 0xFF),
                    fb_w as u32,
                    fb_h as u32,
                );
                let _ = draw_logo_in_frame(fb_w as u32, fb_h as u32);
                crate::gfx::text::draw_atlas_text_in_frame_alpha(
                    MSG,
                    text_x,
                    text_y,
                    fb_w as u32,
                    fb_h as u32,
                    alpha,
                );
                let _ = draw_icon_strip_with_text_metrics(
                    fb_w as u32,
                    fb_h as u32,
                    text_y,
                    atlas.cell_h as f32,
                    alpha,
                );
                unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
            }
        });
        frame = frame.wrapping_add(1);
        Timer::after(EmbassyDuration::from_millis(16)).await;
    }
    crate::r::readiness::set(crate::r::readiness::LOADSCREEN_END);
    crate::log!(
        "boot-probe: loadscreen end ms={} frames={} lived_ms={}\n",
        boot_probe_ms(),
        frame,
        boot_probe_ms().saturating_sub(start_ms)
    );
}
