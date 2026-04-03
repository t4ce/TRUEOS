extern crate alloc;

use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Timer};
use trueos_gfx_core::{RgbVertex, Rgba8, push_rgb_quad_ndc, push_rgb_vertices_bytes};

const UI2_TRIANGLE_TEX_ID: u32 = 4_700;
const UI2_TRIANGLE_RT_W: u32 = 384;
const UI2_TRIANGLE_RT_H: u32 = 240;
const UI2_TRIANGLE_WINDOW_X: f32 = 220.0;
const UI2_TRIANGLE_WINDOW_Y: f32 = 160.0;
const UI2_TRIANGLE_WINDOW_Z: i16 = 30;
const UI2_TRIANGLE_BG_RGBA: [u8; 4] = [0x10, 0x14, 0x1A, 0xFF];
const UI2_TRIANGLE_PHASE_SPEED_RAD_PER_SEC: f32 = 1.2;
const UI2_TRIANGLE_DIRECT_FRAME_MS: u64 = 16;
const UI2_TRIANGLE_WINDOW_FRAME_MS: u64 = 66;

#[inline]
fn phase_step_for_frame_ms(frame_ms: u64) -> f32 {
    UI2_TRIANGLE_PHASE_SPEED_RAD_PER_SEC * (frame_ms as f32 / 1000.0)
}

#[inline]
fn screen_extent() -> Option<(u32, u32)> {
    crate::limine::framebuffer_response()
        .and_then(|resp| resp.framebuffers().next())
        .map(|fb| (fb.width() as u32, fb.height() as u32))
}

#[inline]
fn px_to_screen_ndc(x: f32, y: f32, screen_w: u32, screen_h: u32) -> (f32, f32) {
    let w = screen_w.max(1) as f32;
    let h = screen_h.max(1) as f32;
    (((2.0 * x) / w) - 1.0, 1.0 - ((2.0 * y) / h))
}

#[inline]
fn map_window_vertex_to_screen(vertex: RgbVertex, screen_w: u32, screen_h: u32) -> RgbVertex {
    let local_x = ((vertex.x + 1.0) * 0.5) * UI2_TRIANGLE_RT_W as f32;
    let local_y = ((1.0 - vertex.y) * 0.5) * UI2_TRIANGLE_RT_H as f32;
    let px = UI2_TRIANGLE_WINDOW_X + local_x;
    let py = UI2_TRIANGLE_WINDOW_Y + local_y;
    let (x, y) = px_to_screen_ndc(px, py, screen_w, screen_h);
    RgbVertex {
        x,
        y,
        color: vertex.color,
    }
}

fn build_direct_screen_frame(phase: f32, screen_w: u32, screen_h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let bg = Rgba8::new(
        UI2_TRIANGLE_BG_RGBA[0],
        UI2_TRIANGLE_BG_RGBA[1],
        UI2_TRIANGLE_BG_RGBA[2],
        UI2_TRIANGLE_BG_RGBA[3],
    );

    let (left, top) =
        px_to_screen_ndc(UI2_TRIANGLE_WINDOW_X, UI2_TRIANGLE_WINDOW_Y, screen_w, screen_h);
    let (right, bottom) = px_to_screen_ndc(
        UI2_TRIANGLE_WINDOW_X + UI2_TRIANGLE_RT_W as f32,
        UI2_TRIANGLE_WINDOW_Y + UI2_TRIANGLE_RT_H as f32,
        screen_w,
        screen_h,
    );
    push_rgb_quad_ndc(&mut out, left, top, right, bottom, bg);

    let verts = triangle_vertices(phase);
    let mapped = [
        map_window_vertex_to_screen(verts[0], screen_w, screen_h),
        map_window_vertex_to_screen(verts[1], screen_w, screen_h),
        map_window_vertex_to_screen(verts[2], screen_w, screen_h),
    ];
    push_rgb_vertices_bytes(&mut out, &mapped);
    out
}

fn render_triangle_frame_direct_screen(phase: f32) -> bool {
    let Some((screen_w, screen_h)) = screen_extent() else {
        return false;
    };

    let blob = build_direct_screen_frame(phase, screen_w, screen_h);
    if blob.is_empty() {
        return false;
    }

    let rc_begin = unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame_preserve(0) };
    if rc_begin != 0 {
        return false;
    }

    let rc_scissor = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_scissor(
            UI2_TRIANGLE_WINDOW_X.max(0.0) as u32,
            UI2_TRIANGLE_WINDOW_Y.max(0.0) as u32,
            UI2_TRIANGLE_RT_W,
            UI2_TRIANGLE_RT_H,
        )
    };
    if rc_scissor != 0 {
        let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
        return false;
    }

    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };

    let rc_draw = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_rgb_triangles_no_present(blob.as_ptr(), blob.len())
    };
    if rc_draw != 0 {
        let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
        return false;
    }

    unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() == 0 }
}

fn triangle_vertices(phase: f32) -> [RgbVertex; 3] {
    let cos_p = libm::cosf(phase);
    let sin_p = libm::sinf(phase);
    let rotate =
        |x: f32, y: f32| -> (f32, f32) { ((x * cos_p) - (y * sin_p), (x * sin_p) + (y * cos_p)) };
    let (x0, y0) = rotate(0.0, -0.65);
    let (x1, y1) = rotate(-0.7, 0.55);
    let (x2, y2) = rotate(0.7, 0.55);
    [
        RgbVertex {
            x: x0,
            y: y0,
            color: Rgba8::new(0xFF, 0x52, 0x52, 0xFF),
        },
        RgbVertex {
            x: x1,
            y: y1,
            color: Rgba8::new(0x40, 0xE3, 0x92, 0xFF),
        },
        RgbVertex {
            x: x2,
            y: y2,
            color: Rgba8::new(0x5A, 0x9C, 0xFF, 0xFF),
        },
    ]
}

fn render_triangle_frame(surface: &crate::r::ui2::Ui2SurfaceWindow, phase: f32) -> bool {
    let verts = triangle_vertices(phase);
    let bytes = unsafe {
        core::slice::from_raw_parts(verts.as_ptr() as *const u8, core::mem::size_of_val(&verts))
    };
    surface.render_rgb_triangles(0x10141A, bytes, "triangle-demo")
}

#[embassy_executor::task]
pub async fn ui2_triangle_demo_task() {
    if crate::gfx::is_intel_active() {
        crate::log!(
            "ui2-triangle-demo: mode=direct-screen rect={}x{}+{},{}\n",
            UI2_TRIANGLE_RT_W,
            UI2_TRIANGLE_RT_H,
            UI2_TRIANGLE_WINDOW_X as i32,
            UI2_TRIANGLE_WINDOW_Y as i32
        );

        let mut phase = 0.0f32;
        loop {
            let _ = render_triangle_frame_direct_screen(phase);
            phase += phase_step_for_frame_ms(UI2_TRIANGLE_DIRECT_FRAME_MS);
            if phase > core::f32::consts::TAU {
                phase -= core::f32::consts::TAU;
            }
            Timer::after(EmbassyDuration::from_millis(UI2_TRIANGLE_DIRECT_FRAME_MS)).await;
        }
    }

    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "Demo Triangle",
        crate::r::ui2::Ui2Rect {
            x: UI2_TRIANGLE_WINDOW_X,
            y: UI2_TRIANGLE_WINDOW_Y,
            w: UI2_TRIANGLE_RT_W as f32,
            h: UI2_TRIANGLE_RT_H as f32,
        },
        UI2_TRIANGLE_WINDOW_Z,
        128,
        UI2_TRIANGLE_TEX_ID,
        false,
        UI2_TRIANGLE_BG_RGBA,
    ) else {
        return;
    };
    let window_id = surface.window_id();
    let (surface_w, surface_h) = surface.size();
    crate::log!(
        "ui2-triangle-demo: window={} tex={} size={}x{}\n",
        window_id,
        surface.tex_id(),
        surface_w,
        surface_h
    );

    let mut phase = 0.0f32;
    loop {
        let _ = render_triangle_frame(&surface, phase);
        phase += phase_step_for_frame_ms(UI2_TRIANGLE_WINDOW_FRAME_MS);
        if phase > core::f32::consts::TAU {
            phase -= core::f32::consts::TAU;
        }
        Timer::after(EmbassyDuration::from_millis(UI2_TRIANGLE_WINDOW_FRAME_MS)).await;
    }
}
