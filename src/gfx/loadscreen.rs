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

static LOADSCREEN_LOGO: Once<LogoTexture> = Once::new();
static LOADSCREEN_LOGO_UPLOADED: AtomicBool = AtomicBool::new(false);

fn loadscreen_logo() -> &'static LogoTexture {
    LOADSCREEN_LOGO.call_once(|| {
        let (pixels, width, height) = crate::vga::get_logo_buffer();
        let mut rgba = Vec::with_capacity(pixels.len().saturating_mul(4));
        for pixel in pixels {
            rgba.push(((pixel >> 16) & 0xFF) as u8);
            rgba.push(((pixel >> 8) & 0xFF) as u8);
            rgba.push((pixel & 0xFF) as u8);
            rgba.push((pixel >> 24) as u8);
        }
        LogoTexture {
            rgba,
            width: width as u32,
            height: height as u32,
        }
    })
}

fn ensure_logo_uploaded() -> bool {
    if LOADSCREEN_LOGO_UPLOADED.load(Ordering::Acquire) {
        return true;
    }

    let logo = loadscreen_logo();
    let rc = unsafe {
        crate::surface::io::cabi::trueos_cabi_gfx_upload_texture_rgba_image(
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
    let x1 = (x0 + logo.width as f32).min(view_w as f32);
    let y1 = (y0 + logo.height as f32).min(view_h as f32);

    let nx0 = (2.0 * (x0 / view_w.max(1) as f32)) - 1.0;
    let ny0 = 1.0 - (2.0 * (y0 / view_h.max(1) as f32));
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
            a: 255,
        },
        TexVertex {
            x: nx1,
            y: ny1,
            u: 1.0,
            v: 1.0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        TexVertex {
            x: nx1,
            y: ny0,
            u: 1.0,
            v: 0.0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        TexVertex {
            x: nx0,
            y: ny1,
            u: 0.0,
            v: 1.0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        TexVertex {
            x: nx1,
            y: ny0,
            u: 1.0,
            v: 0.0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        TexVertex {
            x: nx0,
            y: ny0,
            u: 0.0,
            v: 0.0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
    ];

    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe {
        crate::surface::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0)
    };

    let rc = unsafe {
        crate::surface::io::cabi::trueos_cabi_gfx_draw_tex_triangles_no_present(
            LOADSCREEN_LOGO_TEX_ID,
            verts.as_ptr() as *const u8,
            core::mem::size_of_val(&verts),
        )
    };
    rc == 0
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
    let clear_x = (text_x - TEXT_PAD_X).max(0.0);
    let clear_y = (text_y - TEXT_PAD_Y).max(0.0);
    let clear_w = (text_w + (TEXT_PAD_X * 2.0)).min(fb_w - clear_x);
    let clear_h = (atlas.cell_h as f32 + (TEXT_PAD_Y * 2.0)).min(fb_h - clear_y);

    crate::gfx::with_cabi_frame_lock(|| {
        let begin_rc =
            unsafe { crate::surface::io::cabi::trueos_cabi_gfx_begin_frame(LOADSCREEN_BG_RGB) };
        if begin_rc == 0 {
            let _ = crate::gfx::lyon::lyon_geom_api_demo_no_present(fb_w as u32, fb_h as u32);
            let _ = draw_logo_in_frame(fb_w as u32, fb_h as u32);
            crate::gfx::text::draw_atlas_text_in_frame_alpha(
                MSG,
                text_x,
                text_y,
                fb_w as u32,
                fb_h as u32,
                255,
            );
            unsafe { crate::surface::io::cabi::trueos_cabi_gfx_end_frame() };
        }
    });

    let mut frame: u32 = 0;
    while !crate::v::readiness::is_set(crate::v::readiness::LOADSCREEN_END) {
        let phase = (frame as f32) * (core::f32::consts::TAU / 120.0);
        let alpha = ((libm::sinf(phase) * 0.5 + 0.5) * 255.0) as u8;
        crate::gfx::with_cabi_frame_lock(|| {
            let begin_rc = unsafe {
                crate::surface::io::cabi::trueos_cabi_gfx_begin_frame_preserve(LOADSCREEN_BG_RGB)
            };
            if begin_rc == 0 {
                let _ = unsafe {
                    crate::surface::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0)
                };
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
                unsafe { crate::surface::io::cabi::trueos_cabi_gfx_end_frame() };
            }
        });
        frame = frame.wrapping_add(1);
        Timer::after(EmbassyDuration::from_millis(16)).await;
    }
}
