use embassy_time::{Duration as EmbassyDuration, Timer};

// Manual tuning levers for the animated loadscreen burst.
const LOADSCREEN_BG_RGB: u32 = 0xFFFFFF;
const LOADSCREEN_TITLE_TEX_ID: u32 = 4_703;
const LOADSCREEN_ANIM_FRAME_MS: u64 = 33;
const LOADSCREEN_MIN_LIFETIME_MS: u64 = 4_000;
const LOADSCREEN_WAIT_POLL_MS: u64 = 100;
const LOADSCREEN_MSG: &[u8] = b"TRUE OS";

const LOADSCREEN_TILE_H_FACTOR: f32 = 0.16;
const LOADSCREEN_TILE_H_MIN: f32 = 56.0;
const LOADSCREEN_TILE_H_MAX: f32 = 140.0;
const LOADSCREEN_MASK_PADDING_TILES: f32 = 0.16;

const LOADSCREEN_TITLE_RGB: (u8, u8, u8) = (8, 10, 16);
const LOADSCREEN_TITLE_ALPHA_BASE: f32 = 244.0;
const LOADSCREEN_TITLE_ALPHA_SWING: f32 = 8.0;
const LOADSCREEN_TITLE_ALPHA_FREQ: f32 = 1.6;
const LOADSCREEN_TITLE_ALPHA_PHASE: f32 = 0.4;

const LOADSCREEN_DRIFT_A_FREQ: f32 = 2.2;
const LOADSCREEN_DRIFT_A_PHASE: f32 = 0.0;
const LOADSCREEN_DRIFT_B_FREQ: f32 = 3.6;
const LOADSCREEN_DRIFT_B_PHASE: f32 = 0.9;
const LOADSCREEN_DRIFT_C_FREQ: f32 = 1.4;
const LOADSCREEN_DRIFT_C_PHASE: f32 = 0.5;
const LOADSCREEN_DRIFT_D_FREQ: f32 = 5.1;
const LOADSCREEN_DRIFT_D_PHASE: f32 = 1.7;

const LOADSCREEN_FX_GLOBAL_X_BIAS_TILES: f32 = 0.0;
const LOADSCREEN_FX_GLOBAL_UV_BIAS: f32 = 0.0;
const LOADSCREEN_FX_GLOBAL_SCALE_BIAS: f32 = 1.0;
const LOADSCREEN_FX_GLOBAL_ALPHA_BIAS: f32 = 0.0;

const LOADSCREEN_BAND_X_PAD_TILES: f32 = 0.22;
const LOADSCREEN_BAND_Y_OFFSET: f32 = 0.34;
const LOADSCREEN_BAND_W_PAD_TILES: f32 = 0.44;
const LOADSCREEN_BAND_H_TILES: f32 = 0.32;
const LOADSCREEN_BAND_MIN_H: f32 = 18.0;
const LOADSCREEN_BAND_MID_BASE: f32 = 0.47;
const LOADSCREEN_BAND_MID_SWING: f32 = 0.035;
const LOADSCREEN_BAND_X_DRIFT_TILES: f32 = 0.012;
const LOADSCREEN_BAND_Y_DRIFT_TILES: f32 = 0.020;
const LOADSCREEN_BAND_W_DRIFT_TILES: f32 = 0.025;
const LOADSCREEN_BAND_LEFT_RGBA: (u8, u8, u8, u8) = (0x2E, 0xD8, 0xFF, 20);
const LOADSCREEN_BAND_MID_RGB: (u8, u8, u8) = (0xFF, 0xD6, 0x54);
const LOADSCREEN_BAND_MID_ALPHA_BASE: f32 = 46.0;
const LOADSCREEN_BAND_MID_ALPHA_SWING: f32 = 10.0;
const LOADSCREEN_BAND_MID_ALPHA_MIN: f32 = 28.0;
const LOADSCREEN_BAND_MID_ALPHA_MAX: f32 = 64.0;
const LOADSCREEN_BAND_RIGHT_RGBA: (u8, u8, u8, u8) = (0xFF, 0x72, 0x8D, 20);

const LOADSCREEN_CORE_X_BASE_TILES: f32 = 0.025;
const LOADSCREEN_CORE_X_SWING_TILES: f32 = 0.010;
const LOADSCREEN_CORE_Y_BASE_TILES: f32 = 0.055;
const LOADSCREEN_CORE_Y_SWING_TILES: f32 = 0.012;
const LOADSCREEN_CORE_W_SCALE_BASE: f32 = 1.015;
const LOADSCREEN_CORE_W_SCALE_SWING: f32 = 0.010;
const LOADSCREEN_CORE_H_SCALE_BASE: f32 = 1.02;
const LOADSCREEN_CORE_H_SCALE_SWING: f32 = 0.008;
const LOADSCREEN_CORE_RGBA_RGB: (u8, u8, u8) = (0x08, 0x10, 0x1A);
const LOADSCREEN_CORE_ALPHA_BASE: f32 = 44.0;
const LOADSCREEN_CORE_ALPHA_SWING: f32 = 8.0;
const LOADSCREEN_CORE_ALPHA_MIN: f32 = 30.0;
const LOADSCREEN_CORE_ALPHA_MAX: f32 = 54.0;

const LOADSCREEN_SLICE_0_Y0: f32 = 0.00;
const LOADSCREEN_SLICE_0_Y1: f32 = 0.24;
const LOADSCREEN_SLICE_0_DX_BASE_TILES: f32 = -0.10;
const LOADSCREEN_SLICE_0_DX_SWING_TILES: f32 = 0.030;
const LOADSCREEN_SLICE_0_UV_BASE: f32 = 0.020;
const LOADSCREEN_SLICE_0_UV_SWING: f32 = 0.010;
const LOADSCREEN_SLICE_0_RGB: (u8, u8, u8) = (0x00, 0xC8, 0xFF);
const LOADSCREEN_SLICE_0_ALPHA_BASE: f32 = 104.0;
const LOADSCREEN_SLICE_0_ALPHA_SWING: f32 = 18.0;
const LOADSCREEN_SLICE_0_ALPHA_MIN: f32 = 78.0;
const LOADSCREEN_SLICE_0_ALPHA_MAX: f32 = 126.0;

const LOADSCREEN_SLICE_1_Y0: f32 = 0.24;
const LOADSCREEN_SLICE_1_Y1: f32 = 0.48;
const LOADSCREEN_SLICE_1_DX_BASE_TILES: f32 = 0.07;
const LOADSCREEN_SLICE_1_DX_SWING_TILES: f32 = 0.026;
const LOADSCREEN_SLICE_1_UV_BASE: f32 = -0.016;
const LOADSCREEN_SLICE_1_UV_SWING: f32 = 0.010;
const LOADSCREEN_SLICE_1_RGB: (u8, u8, u8) = (0xFF, 0x72, 0x54);
const LOADSCREEN_SLICE_1_ALPHA_BASE: f32 = 92.0;
const LOADSCREEN_SLICE_1_ALPHA_SWING: f32 = 16.0;
const LOADSCREEN_SLICE_1_ALPHA_MIN: f32 = 72.0;
const LOADSCREEN_SLICE_1_ALPHA_MAX: f32 = 112.0;

const LOADSCREEN_SLICE_2_Y0: f32 = 0.48;
const LOADSCREEN_SLICE_2_Y1: f32 = 0.70;
const LOADSCREEN_SLICE_2_DX_BASE_TILES: f32 = -0.04;
const LOADSCREEN_SLICE_2_DX_SWING_TILES: f32 = 0.020;
const LOADSCREEN_SLICE_2_UV_BASE: f32 = 0.030;
const LOADSCREEN_SLICE_2_UV_SWING: f32 = 0.012;
const LOADSCREEN_SLICE_2_RGB: (u8, u8, u8) = (0xC7, 0xFF, 0x52);
const LOADSCREEN_SLICE_2_ALPHA_BASE: f32 = 72.0;
const LOADSCREEN_SLICE_2_ALPHA_SWING: f32 = 14.0;
const LOADSCREEN_SLICE_2_ALPHA_MIN: f32 = 56.0;
const LOADSCREEN_SLICE_2_ALPHA_MAX: f32 = 92.0;

const LOADSCREEN_SLICE_3_Y0: f32 = 0.70;
const LOADSCREEN_SLICE_3_Y1: f32 = 1.00;
const LOADSCREEN_SLICE_3_DX_BASE_TILES: f32 = 0.09;
const LOADSCREEN_SLICE_3_DX_SWING_TILES: f32 = 0.025;
const LOADSCREEN_SLICE_3_UV_BASE: f32 = -0.024;
const LOADSCREEN_SLICE_3_UV_SWING: f32 = 0.010;
const LOADSCREEN_SLICE_3_RGB: (u8, u8, u8) = (0x36, 0x8E, 0xFF);
const LOADSCREEN_SLICE_3_ALPHA_BASE: f32 = 88.0;
const LOADSCREEN_SLICE_3_ALPHA_SWING: f32 = 12.0;
const LOADSCREEN_SLICE_3_ALPHA_MIN: f32 = 70.0;
const LOADSCREEN_SLICE_3_ALPHA_MAX: f32 = 102.0;

const LOADSCREEN_SLICE_4_Y0: f32 = 0.40;
const LOADSCREEN_SLICE_4_Y1: f32 = 0.58;
const LOADSCREEN_SLICE_4_DX_BASE_TILES: f32 = 0.15;
const LOADSCREEN_SLICE_4_DX_SWING_TILES: f32 = 0.032;
const LOADSCREEN_SLICE_4_UV_BASE: f32 = -0.040;
const LOADSCREEN_SLICE_4_UV_SWING: f32 = 0.015;
const LOADSCREEN_SLICE_4_RGB: (u8, u8, u8) = (0xFF, 0x3D, 0xB8);
const LOADSCREEN_SLICE_4_ALPHA_BASE: f32 = 82.0;
const LOADSCREEN_SLICE_4_ALPHA_SWING: f32 = 16.0;
const LOADSCREEN_SLICE_4_ALPHA_MIN: f32 = 64.0;
const LOADSCREEN_SLICE_4_ALPHA_MAX: f32 = 102.0;

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
    anim_ms: u64,
) {
    let anim_t = anim_ms as f32 * 0.001;
    let drift_a = libm::sinf(anim_t * LOADSCREEN_DRIFT_A_FREQ + LOADSCREEN_DRIFT_A_PHASE);
    let drift_b = libm::sinf(anim_t * LOADSCREEN_DRIFT_B_FREQ + LOADSCREEN_DRIFT_B_PHASE);
    let drift_c = libm::cosf(anim_t * LOADSCREEN_DRIFT_C_FREQ + LOADSCREEN_DRIFT_C_PHASE);
    let drift_d = libm::sinf(anim_t * LOADSCREEN_DRIFT_D_FREQ + LOADSCREEN_DRIFT_D_PHASE);

    let global_x_bias = tile_h * LOADSCREEN_FX_GLOBAL_X_BIAS_TILES;
    let global_uv_bias = LOADSCREEN_FX_GLOBAL_UV_BIAS;
    let global_scale = LOADSCREEN_FX_GLOBAL_SCALE_BIAS;
    let alpha_bias = LOADSCREEN_FX_GLOBAL_ALPHA_BIAS;

    let band_x = title.draw_x - tile_h * LOADSCREEN_BAND_X_PAD_TILES
        + tile_h * LOADSCREEN_BAND_X_DRIFT_TILES * drift_c
        + global_x_bias;
    let band_y = title.draw_y
        + title.height as f32 * LOADSCREEN_BAND_Y_OFFSET
        + tile_h * LOADSCREEN_BAND_Y_DRIFT_TILES * drift_a;
    let band_w = (title.width as f32
        + tile_h * (LOADSCREEN_BAND_W_PAD_TILES + LOADSCREEN_BAND_W_DRIFT_TILES * drift_b))
        * global_scale;
    let band_h = (tile_h * LOADSCREEN_BAND_H_TILES).max(LOADSCREEN_BAND_MIN_H);
    let band_mid =
        (LOADSCREEN_BAND_MID_BASE + LOADSCREEN_BAND_MID_SWING * drift_d).clamp(0.15, 0.85);
    let band_mid_alpha =
        (LOADSCREEN_BAND_MID_ALPHA_BASE + LOADSCREEN_BAND_MID_ALPHA_SWING * drift_b + alpha_bias)
            .clamp(LOADSCREEN_BAND_MID_ALPHA_MIN, LOADSCREEN_BAND_MID_ALPHA_MAX) as u8;
    let _ = crate::gfx::lyon::draw_horizontal_three_stop_rect_no_present(
        band_x,
        band_y,
        band_w,
        band_h,
        LOADSCREEN_BAND_LEFT_RGBA,
        (
            LOADSCREEN_BAND_MID_RGB.0,
            LOADSCREEN_BAND_MID_RGB.1,
            LOADSCREEN_BAND_MID_RGB.2,
            band_mid_alpha,
        ),
        LOADSCREEN_BAND_RIGHT_RGBA,
        band_mid,
        view_w,
        view_h,
    );

    let core_alpha =
        (LOADSCREEN_CORE_ALPHA_BASE + LOADSCREEN_CORE_ALPHA_SWING * drift_c + alpha_bias)
            .clamp(LOADSCREEN_CORE_ALPHA_MIN, LOADSCREEN_CORE_ALPHA_MAX) as u8;
    let _ = draw_mask_quad_uv_no_present(
        tex_id,
        title.draw_x
            + tile_h * (LOADSCREEN_CORE_X_BASE_TILES + LOADSCREEN_CORE_X_SWING_TILES * drift_b)
            + global_x_bias,
        title.draw_y
            + tile_h * (LOADSCREEN_CORE_Y_BASE_TILES + LOADSCREEN_CORE_Y_SWING_TILES * drift_a),
        title.width as f32
            * (LOADSCREEN_CORE_W_SCALE_BASE + LOADSCREEN_CORE_W_SCALE_SWING * drift_d.abs())
            * global_scale,
        title.height as f32
            * (LOADSCREEN_CORE_H_SCALE_BASE + LOADSCREEN_CORE_H_SCALE_SWING * drift_c.abs())
            * global_scale,
        0.0,
        0.0,
        1.0,
        1.0,
        (
            LOADSCREEN_CORE_RGBA_RGB.0,
            LOADSCREEN_CORE_RGBA_RGB.1,
            LOADSCREEN_CORE_RGBA_RGB.2,
            core_alpha,
        ),
        view_w,
        view_h,
        true,
    );

    let _ = draw_mask_slice_no_present(
        tex_id,
        title,
        LOADSCREEN_SLICE_0_Y0,
        LOADSCREEN_SLICE_0_Y1,
        tile_h * (LOADSCREEN_SLICE_0_DX_BASE_TILES + LOADSCREEN_SLICE_0_DX_SWING_TILES * drift_a)
            + global_x_bias,
        LOADSCREEN_SLICE_0_UV_BASE + LOADSCREEN_SLICE_0_UV_SWING * drift_b + global_uv_bias,
        (
            LOADSCREEN_SLICE_0_RGB.0,
            LOADSCREEN_SLICE_0_RGB.1,
            LOADSCREEN_SLICE_0_RGB.2,
            (LOADSCREEN_SLICE_0_ALPHA_BASE + LOADSCREEN_SLICE_0_ALPHA_SWING * drift_c + alpha_bias)
                .clamp(LOADSCREEN_SLICE_0_ALPHA_MIN, LOADSCREEN_SLICE_0_ALPHA_MAX)
                as u8,
        ),
        view_w,
        view_h,
    );
    let _ = draw_mask_slice_no_present(
        tex_id,
        title,
        LOADSCREEN_SLICE_1_Y0,
        LOADSCREEN_SLICE_1_Y1,
        tile_h * (LOADSCREEN_SLICE_1_DX_BASE_TILES + LOADSCREEN_SLICE_1_DX_SWING_TILES * drift_b)
            + global_x_bias,
        LOADSCREEN_SLICE_1_UV_BASE + LOADSCREEN_SLICE_1_UV_SWING * drift_d + global_uv_bias,
        (
            LOADSCREEN_SLICE_1_RGB.0,
            LOADSCREEN_SLICE_1_RGB.1,
            LOADSCREEN_SLICE_1_RGB.2,
            (LOADSCREEN_SLICE_1_ALPHA_BASE + LOADSCREEN_SLICE_1_ALPHA_SWING * drift_a + alpha_bias)
                .clamp(LOADSCREEN_SLICE_1_ALPHA_MIN, LOADSCREEN_SLICE_1_ALPHA_MAX)
                as u8,
        ),
        view_w,
        view_h,
    );
    let _ = draw_mask_slice_no_present(
        tex_id,
        title,
        LOADSCREEN_SLICE_2_Y0,
        LOADSCREEN_SLICE_2_Y1,
        tile_h * (LOADSCREEN_SLICE_2_DX_BASE_TILES + LOADSCREEN_SLICE_2_DX_SWING_TILES * drift_c)
            + global_x_bias,
        LOADSCREEN_SLICE_2_UV_BASE + LOADSCREEN_SLICE_2_UV_SWING * drift_a + global_uv_bias,
        (
            LOADSCREEN_SLICE_2_RGB.0,
            LOADSCREEN_SLICE_2_RGB.1,
            LOADSCREEN_SLICE_2_RGB.2,
            (LOADSCREEN_SLICE_2_ALPHA_BASE + LOADSCREEN_SLICE_2_ALPHA_SWING * drift_d + alpha_bias)
                .clamp(LOADSCREEN_SLICE_2_ALPHA_MIN, LOADSCREEN_SLICE_2_ALPHA_MAX)
                as u8,
        ),
        view_w,
        view_h,
    );
    let _ = draw_mask_slice_no_present(
        tex_id,
        title,
        LOADSCREEN_SLICE_3_Y0,
        LOADSCREEN_SLICE_3_Y1,
        tile_h * (LOADSCREEN_SLICE_3_DX_BASE_TILES + LOADSCREEN_SLICE_3_DX_SWING_TILES * drift_d)
            + global_x_bias,
        LOADSCREEN_SLICE_3_UV_BASE + LOADSCREEN_SLICE_3_UV_SWING * drift_c + global_uv_bias,
        (
            LOADSCREEN_SLICE_3_RGB.0,
            LOADSCREEN_SLICE_3_RGB.1,
            LOADSCREEN_SLICE_3_RGB.2,
            (LOADSCREEN_SLICE_3_ALPHA_BASE + LOADSCREEN_SLICE_3_ALPHA_SWING * drift_b + alpha_bias)
                .clamp(LOADSCREEN_SLICE_3_ALPHA_MIN, LOADSCREEN_SLICE_3_ALPHA_MAX)
                as u8,
        ),
        view_w,
        view_h,
    );
    let _ = draw_mask_slice_no_present(
        tex_id,
        title,
        LOADSCREEN_SLICE_4_Y0,
        LOADSCREEN_SLICE_4_Y1,
        tile_h * (LOADSCREEN_SLICE_4_DX_BASE_TILES + LOADSCREEN_SLICE_4_DX_SWING_TILES * drift_c)
            + global_x_bias,
        LOADSCREEN_SLICE_4_UV_BASE + LOADSCREEN_SLICE_4_UV_SWING * drift_b + global_uv_bias,
        (
            LOADSCREEN_SLICE_4_RGB.0,
            LOADSCREEN_SLICE_4_RGB.1,
            LOADSCREEN_SLICE_4_RGB.2,
            (LOADSCREEN_SLICE_4_ALPHA_BASE + LOADSCREEN_SLICE_4_ALPHA_SWING * drift_d + alpha_bias)
                .clamp(LOADSCREEN_SLICE_4_ALPHA_MIN, LOADSCREEN_SLICE_4_ALPHA_MAX)
                as u8,
        ),
        view_w,
        view_h,
    );
}

fn render_loadscreen_frame(
    bg_rgb: u32,
    msg: &[u8],
    text_layout: Option<crate::gfx::imbafont::ImbaFontRunLayout>,
    title_mask: Option<&crate::gfx::imbafont::ImbaFontMaskTexture>,
    title_mask_uploaded: bool,
    tile_h: f32,
    fb_w: u32,
    fb_h: u32,
    anim_ms: u64,
) -> bool {
    let begin_rc = unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame(bg_rgb) };
    if begin_rc != 0 {
        crate::log!("gfx-loadscreen: begin_frame failed rc={}\n", begin_rc);
        return false;
    }

    if title_mask_uploaded && let Some(mask) = title_mask {
        draw_loadscreen_title_fx(LOADSCREEN_TITLE_TEX_ID, mask, tile_h, fb_w, fb_h, anim_ms);
    }

    if let Some(layout) = text_layout {
        let title_alpha = (LOADSCREEN_TITLE_ALPHA_BASE
            + LOADSCREEN_TITLE_ALPHA_SWING
                * libm::sinf(
                    anim_ms as f32 * 0.001 * LOADSCREEN_TITLE_ALPHA_FREQ
                        + LOADSCREEN_TITLE_ALPHA_PHASE,
                )
            + LOADSCREEN_FX_GLOBAL_ALPHA_BIAS)
            .clamp(236.0, 250.0) as u8;
        let _ = crate::gfx::imbafont::draw_text_in_frame(
            crate::gfx::imbafont::ImbaFontFace::Grow,
            msg,
            &layout,
            fb_w,
            fb_h,
            LOADSCREEN_TITLE_RGB,
            title_alpha,
        );
    }

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
    let (fb_w, fb_h) = crate::limine::framebuffer_response()
        .and_then(|resp| resp.framebuffers().next())
        .map(|fb| (fb.width() as f32, fb.height() as f32))
        .unwrap_or((1024.0, 768.0));
    let tile_h =
        (fb_h * LOADSCREEN_TILE_H_FACTOR).clamp(LOADSCREEN_TILE_H_MIN, LOADSCREEN_TILE_H_MAX);
    let text_layout = crate::gfx::imbafont::layout_text_centered(
        crate::gfx::imbafont::ImbaFontFace::Grow,
        LOADSCREEN_MSG,
        fb_w,
        fb_h,
        tile_h,
    );
    let title_mask = text_layout.and_then(|layout| {
        crate::gfx::imbafont::rasterize_text_mask_texture(
            crate::gfx::imbafont::ImbaFontFace::Grow,
            LOADSCREEN_MSG,
            &layout,
            tile_h * LOADSCREEN_MASK_PADDING_TILES,
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
            if render_loadscreen_frame(
                LOADSCREEN_BG_RGB,
                LOADSCREEN_MSG,
                text_layout,
                title_mask.as_ref(),
                title_mask_uploaded,
                tile_h,
                fb_w as u32,
                fb_h as u32,
                0,
            ) && first_frame_ms == 0
            {
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

        let anim_ms = now_ms.saturating_sub(first_frame_ms);
        crate::gfx::with_cabi_frame_lock(|| {
            let _ = render_loadscreen_frame(
                LOADSCREEN_BG_RGB,
                LOADSCREEN_MSG,
                text_layout,
                title_mask.as_ref(),
                title_mask_uploaded,
                tile_h,
                fb_w as u32,
                fb_h as u32,
                anim_ms,
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
