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
    let rotate =
        |x: f32, y: f32| -> (f32, f32) { ((x * cos_p) - (y * sin_p), (x * sin_p) + (y * cos_p)) };
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

fn render_triangle_frame(surface: &crate::v::ui2::Ui2SurfaceWindow, phase: f32) -> bool {
    let verts = triangle_vertices(phase);
    let bytes = unsafe {
        core::slice::from_raw_parts(verts.as_ptr() as *const u8, core::mem::size_of_val(&verts))
    };
    surface.render_rgb_triangles(0x10141A, bytes, "triangle-demo")
}

#[embassy_executor::task]
pub async fn ui2_triangle_demo_task() {
    let Some(surface) = crate::v::ui2::Ui2SurfaceWindow::new(
        "Demo Triangle",
        crate::v::ui2::Ui2Rect {
            x: UI2_TRIANGLE_WINDOW_X,
            y: UI2_TRIANGLE_WINDOW_Y,
            w: UI2_TRIANGLE_RT_W as f32,
            h: UI2_TRIANGLE_RT_H as f32,
        },
        UI2_TRIANGLE_WINDOW_Z,
        128,
        UI2_TRIANGLE_TEX_ID,
        false,
        [0x10, 0x14, 0x1A, 0xFF],
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
        phase += 0.04;
        if phase > core::f32::consts::TAU {
            phase -= core::f32::consts::TAU;
        }
        Timer::after(EmbassyDuration::from_millis(66)).await;
    }
}
