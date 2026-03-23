use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Once;

use crate::gfx::imbafont::{ImbaFontFace, ImbaFontRunLayout};

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

#[derive(Clone, Copy)]
struct LoadscreenStripSpec {
    face: ImbaFontFace,
    rgb: (u8, u8, u8),
    scale_start: f32,
    scale_end: f32,
}

#[derive(Clone, Copy)]
struct LoadscreenStripLayout {
    spec: LoadscreenStripSpec,
    layout: ImbaFontRunLayout,
}

const LOADSCREEN_BG_RGB: u32 = 0xF4F4F4;
const LOADSCREEN_LOGO_TEX_ID: u32 = 1002;
const LOGO_PAD_X: f32 = 24.0;
const LOGO_PAD_Y: f32 = 24.0;
const TEXT_PAD_X: f32 = 12.0;
const TEXT_PAD_Y: f32 = 10.0;
const STRIP_GAP_Y: f32 = 8.0;
const STRIP_ROW_GAP_Y: f32 = 10.0;
const STRIP_TILE_SCALE: f32 = 0.72;
const STRIP_SCALE_START: f32 = 0.70;
const STRIP_SCALE_END: f32 = 1.30;

static LOADSCREEN_LOGO: Once<LogoTexture> = Once::new();
static LOADSCREEN_LOGO_UPLOADED: AtomicBool = AtomicBool::new(false);

const STRIP_SPECS: [LoadscreenStripSpec; 4] = [
    LoadscreenStripSpec {
        face: ImbaFontFace::Regular,
        rgb: (0, 0, 0),
        scale_start: STRIP_SCALE_START,
        scale_end: STRIP_SCALE_END,
    },
    LoadscreenStripSpec {
        face: ImbaFontFace::Block,
        rgb: (0, 0, 0),
        scale_start: 0.58,
        scale_end: 1.52,
    },
    LoadscreenStripSpec {
        face: ImbaFontFace::Grow,
        rgb: (0, 0, 0),
        scale_start: 0.48,
        scale_end: 1.74,
    },
    LoadscreenStripSpec {
        face: ImbaFontFace::Impact,
        rgb: (0, 0, 0),
        scale_start: 0.38,
        scale_end: 1.98,
    },
];

#[inline]
fn boot_probe_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

fn draw_de_flag_bottom_left_in_frame(view_w: u32, view_h: u32) -> bool {
    let vw = view_w.max(1) as f32;
    let vh = view_h.max(1) as f32;

    let flag_w = 128.0f32.min(vw);
    let flag_h = 128.0f32.min(vh);
    let x = 0.0f32;
    let y = (vh - flag_h).max(0.0);
    let stripe_h = flag_h / 3.0;

    let top_ok = crate::gfx::lyon::draw_solid_rect_no_present(
        x,
        y,
        flag_w,
        stripe_h,
        (0x00, 0x00, 0x00, 0xFF),
        view_w,
        view_h,
    );
    let mid_ok = crate::gfx::lyon::draw_solid_rect_no_present(
        x,
        y + stripe_h,
        flag_w,
        stripe_h,
        (0xDD, 0x00, 0x00, 0xFF),
        view_w,
        view_h,
    );
    let bot_ok = crate::gfx::lyon::draw_solid_rect_no_present(
        x,
        y + (2.0 * stripe_h),
        flag_w,
        flag_h - (2.0 * stripe_h),
        (0xFF, 0xCE, 0x00, 0xFF),
        view_w,
        view_h,
    );

    top_ok && mid_ok && bot_ok
}

fn centered_text_origin(msg: &[u8], fb_w: f32, fb_h: f32) -> (f32, f32, f32, f32) {
    let atlas = crate::gfx::text::font_atlas_large_view();
    let fallback = atlas.index.get(b'?' as usize).copied().unwrap_or(0);
    let space_adv_px = atlas.cell_w as f32 * 0.60;

    let glyph_slot = |ch: u8| {
        let mut slot = atlas.index.get(ch as usize).copied().unwrap_or(fallback);
        if slot == u16::MAX {
            slot = fallback;
        }
        slot
    };

    let mut width_px = 0.0f32;
    for &ch in msg {
        if ch == b' ' {
            width_px += space_adv_px;
            continue;
        }

        let slot = glyph_slot(ch);
        let glyph_w_px = atlas
            .widths
            .get(slot as usize)
            .copied()
            .unwrap_or(atlas.cell_w as u8) as f32;
        width_px += glyph_w_px;
    }

    let x = ((fb_w - width_px) * 0.5).max(0.0);
    let y = ((fb_h - atlas.cell_h as f32) * 0.5).max(0.0);
    (x, y, width_px, atlas.cell_h as f32)
}

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

fn build_strip_layouts(view_w: f32, text_y: f32, text_h: f32) -> Vec<LoadscreenStripLayout> {
    let tile_h = (text_h * STRIP_TILE_SCALE).max(12.0);
    let mut top_y = text_y + text_h + STRIP_GAP_Y;
    let mut layouts = Vec::with_capacity(STRIP_SPECS.len());

    for spec in STRIP_SPECS {
        let Some(layout) = crate::gfx::imbafont::layout_run_centered(
            spec.face,
            view_w,
            top_y,
            tile_h,
            spec.scale_start,
            spec.scale_end,
        ) else {
            continue;
        };

        top_y = layout.baseline_y + layout.vis_bottom + STRIP_ROW_GAP_Y;
        layouts.push(LoadscreenStripLayout { spec, layout });
    }

    layouts
}

fn draw_strip_layouts_in_frame(
    strips: &[LoadscreenStripLayout],
    view_w: u32,
    view_h: u32,
    alpha: u8,
) {
    for strip in strips {
        let _ = crate::gfx::imbafont::draw_run_in_frame(
            strip.spec.face,
            &strip.layout,
            view_w,
            view_h,
            strip.spec.rgb,
            alpha,
            strip.spec.scale_start,
            strip.spec.scale_end,
        );
    }
}

#[embassy_executor::task]
pub async fn gfx_loadscreen_task() {
    const MSG: &[u8] = b"TRUE OS \xA7";

    let (fb_w, fb_h) = crate::limine::framebuffer_response()
        .and_then(|resp| resp.framebuffers().next())
        .map(|fb| (fb.width() as f32, fb.height() as f32))
        .unwrap_or((1024.0, 768.0));
    let (text_x, text_y, text_w, text_h) = centered_text_origin(MSG, fb_w, fb_h);
    let text_clear_x = (text_x - TEXT_PAD_X).max(0.0);
    let text_clear_y = (text_y - TEXT_PAD_Y).max(0.0);
    let text_clear_w = (text_w + (TEXT_PAD_X * 2.0)).min(fb_w - text_clear_x);
    let text_clear_h = (text_h + (TEXT_PAD_Y * 2.0)).min(fb_h - text_clear_y);
    let strip_layouts = build_strip_layouts(fb_w, text_y, text_h);

    let (clear_x, clear_y, clear_w, clear_h) = if strip_layouts.is_empty() {
        (text_clear_x, text_clear_y, text_clear_w, text_clear_h)
    } else {
        let mut min_x = text_clear_x;
        let mut min_y = text_clear_y;
        let mut max_x = text_clear_x + text_clear_w;
        let mut max_y = text_clear_y + text_clear_h;

        for strip in &strip_layouts {
            min_x = min_x.min((strip.layout.origin_x + strip.layout.vis_left).max(0.0));
            min_y = min_y.min((strip.layout.baseline_y + strip.layout.vis_top).max(0.0));
            max_x = max_x.max((strip.layout.origin_x + strip.layout.vis_right).min(fb_w));
            max_y = max_y.max((strip.layout.baseline_y + strip.layout.vis_bottom).min(fb_h));
        }

        (
            min_x,
            min_y,
            (max_x - min_x).max(0.0),
            (max_y - min_y).max(0.0),
        )
    };

    let start_ms = boot_probe_ms();
    crate::log!("GFX Loadscreen\n");
    crate::log!("boot-probe: loadscreen start ms={}\n", start_ms);

    crate::gfx::with_cabi_frame_lock(|| {
        let begin_rc =
            unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame(LOADSCREEN_BG_RGB) };
        if begin_rc == 0 {
            let _ = crate::gfx::lyon::lyon_geom_api_demo_no_present(fb_w as u32, fb_h as u32);
            let _ = draw_de_flag_bottom_left_in_frame(fb_w as u32, fb_h as u32);
            let _ = draw_logo_in_frame(fb_w as u32, fb_h as u32);
            let _ = crate::gfx::text::draw_atlas_text_in_frame_alpha(
                MSG,
                text_x,
                text_y,
                fb_w as u32,
                fb_h as u32,
                255,
            );
            draw_strip_layouts_in_frame(&strip_layouts, fb_w as u32, fb_h as u32, 255);
            unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
        } else {
            crate::log!("gfx-loadscreen: begin_frame failed rc={}\n", begin_rc);
        }
    });

    let mut frame: u32 = 0;
    while !crate::r::readiness::is_set(crate::r::readiness::LOADSCREEN_END) {
        Timer::after(EmbassyDuration::from_millis(16)).await;

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
                let _ = draw_de_flag_bottom_left_in_frame(fb_w as u32, fb_h as u32);
                let _ = draw_logo_in_frame(fb_w as u32, fb_h as u32);
                let _ = crate::gfx::text::draw_atlas_text_in_frame_alpha(
                    MSG,
                    text_x,
                    text_y,
                    fb_w as u32,
                    fb_h as u32,
                    alpha,
                );
                draw_strip_layouts_in_frame(&strip_layouts, fb_w as u32, fb_h as u32, alpha);
                unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
            }
        });
        frame = frame.wrapping_add(1);
    }

    crate::log!(
        "boot-probe: loadscreen end ms={} frames={} lived_ms={}\n",
        boot_probe_ms(),
        frame,
        boot_probe_ms().saturating_sub(start_ms)
    );
}
