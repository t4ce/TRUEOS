use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Timer};
use trueos_gfx_core::{Rgba8, TEX_VERTEX_SIZE, ViewTransform, push_tex_quad_px};

const LOADSCREEN_BG_RGB: u32 = 0xF2EEE8;
const LOADSCREEN_TITLE_RGB: (u8, u8, u8) = (0x10, 0x10, 0x12);
const LOADSCREEN_MSG: &[u8] = b"TRUE OS";
const LOADSCREEN_TILE_H_FACTOR: f32 = 0.085;
const LOADSCREEN_TILE_H_MIN: f32 = 44.0;
const LOADSCREEN_TILE_H_MAX: f32 = 124.0;
const LOADSCREEN_WAIT_POLL_MS: u64 = 100;
const LOADSCREEN_MIN_LIFETIME_MS: u64 = 4_000;
const LOADSCREEN_ANIM_FRAME_MS: u64 = 33;
const LOADSCREEN_TITLE_ALPHA: u8 = 242;

fn draw_texture_rect_uv_no_present(
    tex_id: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    view_w: u32,
    view_h: u32,
) -> bool {
    if tex_id == 0 || !(width > 0.0 && height > 0.0) {
        return false;
    }

    let transform = ViewTransform::from_extent(view_w, view_h);
    let mut verts = Vec::with_capacity(6 * TEX_VERTEX_SIZE);
    push_tex_quad_px(
        &mut verts,
        transform,
        x,
        y,
        x + width,
        y + height,
        [u0, v0, u1, v1],
        Rgba8::new(255, 255, 255, 255),
    );

    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0)
    };
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_tex_triangles_no_present(
            tex_id,
            verts.as_ptr(),
            verts.len(),
        )
    };
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    rc == 0
}

fn draw_athlasfont_bucket_preview(fb_w: u32, fb_h: u32) {
    if !crate::r::readiness::is_set(crate::r::readiness::GFX_ATHLASFONT_READY) {
        return;
    }

    let sample_h = 24.0f32;
    let pad = 8.0f32;
    let gap = 4.0f32;
    let mut samples = [[None; 8]; 3];
    let mut col_w = [0.0f32; 3];
    let total_h = sample_h * 8.0 + gap * 7.0;
    let section_lookup = crate::gfx::athlasfont::imba_athlas_lookup_char('§');

    for size_case in 0..3usize {
        for bucket in 0..8usize {
            let Some(tex_id) = crate::gfx::athlasfont::imba_athlas_bucket_tex_id(size_case, bucket)
            else {
                continue;
            };
            let Some(codepoints) =
                crate::gfx::athlasfont::athlasmetrics::athlas_bucket_codepoints(bucket as u8)
            else {
                continue;
            };
            if codepoints.is_empty() {
                continue;
            }
            let mut tex_w = 0u32;
            let mut tex_h = 0u32;
            let rc = unsafe {
                crate::r::io::cabi::trueos_cabi_gfx_texture_dimensions(
                    tex_id,
                    &mut tex_w,
                    &mut tex_h,
                )
            };
            if rc != 0 || tex_w == 0 || tex_h == 0 {
                continue;
            }
            let slot = match section_lookup {
                Some(lookup) if lookup.bucket as usize == bucket => lookup.slot as usize,
                _ => 0usize,
            }
            .min(codepoints.len().saturating_sub(1));
            let grid_w = 16usize;
            let grid_h = codepoints.len().div_ceil(grid_w).max(1);
            let cell_w_px = tex_w as f32 / grid_w as f32;
            let cell_h_px = tex_h as f32 / grid_h as f32;
            let draw_w = if cell_h_px > 0.0 {
                sample_h * (cell_w_px / cell_h_px)
            } else {
                sample_h
            };
            let col = slot % grid_w;
            let row = slot / grid_w;
            let u0 = col as f32 / grid_w as f32;
            let v0 = row as f32 / grid_h as f32;
            let u1 = (col + 1) as f32 / grid_w as f32;
            let v1 = (row + 1) as f32 / grid_h as f32;
            samples[size_case][bucket] = Some((tex_id, draw_w, u0, v0, u1, v1));
            col_w[size_case] = col_w[size_case].max(draw_w);
        }
    }

    let total_w = col_w[0] + col_w[1] + col_w[2] + gap * 2.0;
    if total_w <= 0.0 || total_h <= 0.0 {
        return;
    }

    let panel_x = (fb_w as f32 - pad - total_w - 6.0).max(0.0);
    let panel_y = (fb_h as f32 - pad - total_h - 6.0).max(0.0);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        panel_x,
        panel_y,
        total_w + 6.0,
        total_h + 6.0,
        (0xFF, 0xFF, 0xFF, 208),
        fb_w,
        fb_h,
    );

    let mut x = panel_x + 3.0;
    for size_case in 0..3usize {
        let mut y = panel_y + 3.0;
        for bucket in 0..8usize {
            if let Some((tex_id, draw_w, u0, v0, u1, v1)) = samples[size_case][bucket] {
                let _ = draw_texture_rect_uv_no_present(
                    tex_id, x, y, draw_w, sample_h, u0, v0, u1, v1, fb_w, fb_h,
                );
            }
            y += sample_h + gap;
        }
        x += col_w[size_case] + gap;
    }
}

fn render_loadscreen_frame(
    bg_rgb: u32,
    msg: &[u8],
    text_layout: Option<crate::gfx::imbafont::ImbaFontRunLayout>,
    fb_w: u32,
    fb_h: u32,
) -> bool {
    let begin_rc = unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame(bg_rgb) };
    if begin_rc != 0 {
        crate::log!("gfx-loadscreen: begin_frame failed rc={}\n", begin_rc);
        return false;
    }

    if let Some(layout) = text_layout {
        let _ = crate::gfx::imbafont::draw_text_in_frame(
            crate::gfx::imbafont::ImbaFontFace::Font,
            msg,
            &layout,
            fb_w,
            fb_h,
            LOADSCREEN_TITLE_RGB,
            LOADSCREEN_TITLE_ALPHA,
        );
    }

    draw_athlasfont_bucket_preview(fb_w, fb_h);

    unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
    true
}

#[inline]
fn boot_probe_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

#[embassy_executor::task]
pub async fn gfx_loadscreen_task() {
    crate::r::readiness::set_loadscreen_expire_requested(false);

    let (fb_w, fb_h) = crate::limine::framebuffer_response()
        .and_then(|resp| resp.framebuffers().next())
        .map(|fb| (fb.width() as f32, fb.height() as f32))
        .unwrap_or((1024.0, 768.0));
    let tile_h =
        (fb_h * LOADSCREEN_TILE_H_FACTOR).clamp(LOADSCREEN_TILE_H_MIN, LOADSCREEN_TILE_H_MAX);
    let text_layout = crate::gfx::imbafont::layout_text_centered(
        crate::gfx::imbafont::ImbaFontFace::Font,
        LOADSCREEN_MSG,
        fb_w,
        fb_h,
        tile_h,
    );

    let mut first_frame_ms = 0u64;

    loop {
        crate::gfx::with_cabi_frame_lock(|| {
            if render_loadscreen_frame(
                LOADSCREEN_BG_RGB,
                LOADSCREEN_MSG,
                text_layout,
                fb_w as u32,
                fb_h as u32,
            ) && first_frame_ms == 0
            {
                first_frame_ms = boot_probe_ms();
                crate::r::readiness::set(
                    crate::r::readiness::LOADSCREEN_FRAME_READY
                        | crate::r::readiness::LOADSCREEN_COVER_READY,
                );
            }
        });

        if first_frame_ms != 0 {
            break;
        }

        Timer::after(EmbassyDuration::from_millis(LOADSCREEN_WAIT_POLL_MS)).await;
    }

    let min_end_ms = first_frame_ms.saturating_add(LOADSCREEN_MIN_LIFETIME_MS);

    loop {
        let now_ms = boot_probe_ms();
        let min_lifetime_reached = now_ms >= min_end_ms;
        let expire_requested = crate::r::readiness::loadscreen_expire_requested();
        let athlasfont_ready =
            crate::r::readiness::is_set(crate::r::readiness::GFX_ATHLASFONT_READY);
        if min_lifetime_reached && expire_requested && athlasfont_ready {
            crate::r::readiness::set(crate::r::readiness::LOADSCREEN_END);
            break;
        }

        crate::gfx::with_cabi_frame_lock(|| {
            let _ = render_loadscreen_frame(
                LOADSCREEN_BG_RGB,
                LOADSCREEN_MSG,
                text_layout,
                fb_w as u32,
                fb_h as u32,
            );
        });

        Timer::after(EmbassyDuration::from_millis(LOADSCREEN_ANIM_FRAME_MS)).await;
    }

    crate::log!(
        "boot-probe: loadscreen end ms={} lived_ms={}\n",
        boot_probe_ms(),
        boot_probe_ms().saturating_sub(first_frame_ms)
    );
}
