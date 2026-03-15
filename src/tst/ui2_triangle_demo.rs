use embassy_time::{Duration as EmbassyDuration, Timer};

const UI2_TRIANGLE_TEX_ID: u32 = 4_700;
const UI2_TRIANGLE_RT_W: u32 = 384;
const UI2_TRIANGLE_RT_H: u32 = 240;
const UI2_TRIANGLE_WINDOW_X: f32 = 220.0;
const UI2_TRIANGLE_WINDOW_Y: f32 = 160.0;
const UI2_TRIANGLE_WINDOW_Z: i16 = 30;

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct RgbVertex {
    x: f32,
    y: f32,
    r: u8,
    g: u8,
    b: u8,
    pad: u8,
}

fn triangle_vertices(phase: f32) -> [RgbVertex; 3] {
    let cos_p = libm::cosf(phase);
    let sin_p = libm::sinf(phase);
    let rotate = |x: f32, y: f32| -> (f32, f32) { ((x * cos_p) - (y * sin_p), (x * sin_p) + (y * cos_p)) };
    let (x0, y0) = rotate(0.0, -0.65);
    let (x1, y1) = rotate(-0.7, 0.55);
    let (x2, y2) = rotate(0.7, 0.55);
    [
        RgbVertex {
            x: x0,
            y: y0,
            r: 0xFF,
            g: 0x52,
            b: 0x52,
            pad: 0xFF,
        },
        RgbVertex {
            x: x1,
            y: y1,
            r: 0x40,
            g: 0xE3,
            b: 0x92,
            pad: 0xFF,
        },
        RgbVertex {
            x: x2,
            y: y2,
            r: 0x5A,
            g: 0x9C,
            b: 0xFF,
            pad: 0xFF,
        },
    ]
}

fn upload_triangle_render_target() -> bool {
    let clear = [0x10u8, 0x14u8, 0x1Au8, 0xFFu8];
    let pixels = alloc::vec![clear; (UI2_TRIANGLE_RT_W as usize) * (UI2_TRIANGLE_RT_H as usize)]
        .into_iter()
        .flatten()
        .collect::<alloc::vec::Vec<u8>>();
    let rc = unsafe {
        crate::surface::io::cabi::trueos_cabi_gfx_upload_texture_rgba_image(
            UI2_TRIANGLE_TEX_ID,
            UI2_TRIANGLE_RT_W,
            UI2_TRIANGLE_RT_H,
            pixels.as_ptr(),
            pixels.len(),
        )
    };
    if rc != 0 {
        crate::log!(
            "ui2-triangle-demo: upload render target failed tex={} rc={}\n",
            UI2_TRIANGLE_TEX_ID,
            rc
        );
        return false;
    }
    true
}

fn render_triangle_frame(phase: f32) -> bool {
    let verts = triangle_vertices(phase);
    let bytes = unsafe {
        core::slice::from_raw_parts(
            verts.as_ptr() as *const u8,
            core::mem::size_of_val(&verts),
        )
    };
    crate::surface::io::cabi::render_rgb_triangles_to_texture(UI2_TRIANGLE_TEX_ID, 0x10141A, bytes)
        == 0
}

#[embassy_executor::task]
pub async fn ui2_triangle_demo_task() {
    if !upload_triangle_render_target() {
        return;
    }

    let window_id = crate::v::ui2::create_texture_content_window(
        "Demo Triangle",
        crate::v::ui2::Ui2Rect {
            x: UI2_TRIANGLE_WINDOW_X,
            y: UI2_TRIANGLE_WINDOW_Y,
            w: UI2_TRIANGLE_RT_W as f32,
            h: UI2_TRIANGLE_RT_H as f32,
        },
        UI2_TRIANGLE_WINDOW_Z,
        255,
        UI2_TRIANGLE_TEX_ID,
        false,
    );
    crate::log!(
        "ui2-triangle-demo: window={} tex={} size={}x{}\n",
        window_id,
        UI2_TRIANGLE_TEX_ID,
        UI2_TRIANGLE_RT_W,
        UI2_TRIANGLE_RT_H
    );

    let mut phase = 0.0f32;
    loop {
        if render_triangle_frame(phase) {
            let _ = crate::v::ui2::request_window_repaint(window_id, "triangle-demo");
        }
        phase += 0.04;
        if phase > core::f32::consts::TAU {
            phase -= core::f32::consts::TAU;
        }
        Timer::after(EmbassyDuration::from_millis(66)).await;
    }
}
