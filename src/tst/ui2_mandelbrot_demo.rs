use embassy_time::{Duration as EmbassyDuration, Timer};

const UI2_MANDELBROT_TEX_ID: u32 = 4_702;
const UI2_MANDELBROT_RT_W: u32 = 768;
const UI2_MANDELBROT_RT_H: u32 = 512;
const UI2_MANDELBROT_WINDOW_Z: i16 = 31;

fn mandelbrot_window_rect() -> crate::r::ui2::Ui2Rect {
    let (fb_w, fb_h) = crate::vga::framebuffer_dimensions().unwrap_or((1600, 900));
    let margin_x = 40.0f32;
    let margin_y = 92.0f32;
    let x = ((fb_w as f32) - UI2_MANDELBROT_RT_W as f32 - margin_x).max(24.0);
    let y = ((fb_h as f32) - UI2_MANDELBROT_RT_H as f32 - margin_y).max(96.0);
    crate::r::ui2::Ui2Rect {
        x,
        y,
        w: UI2_MANDELBROT_RT_W as f32,
        h: UI2_MANDELBROT_RT_H as f32,
    }
}

#[embassy_executor::task]
pub async fn ui2_mandelbrot_demo_task() {
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "Demo Mandelbrot",
        mandelbrot_window_rect(),
        UI2_MANDELBROT_WINDOW_Z,
        128,
        UI2_MANDELBROT_TEX_ID,
        false,
        [0x08, 0x0B, 0x10, 0xFF],
    ) else {
        return;
    };

    let window_id = surface.window_id();
    let (surface_w, surface_h) = surface.size();
    crate::log!(
        "ui2-mandelbrot-demo: window={} tex={} size={}x{} start\n",
        window_id,
        surface.tex_id(),
        surface_w,
        surface_h
    );

    let _ = crate::r::ui2::set_window_title(window_id, "Seahorse Valley");

    // Let the blank window present first so the window frame lands before shader work is queued.
    Timer::after(EmbassyDuration::from_millis(1)).await;
    let start_ticks = embassy_time_driver::now();
    let tick_hz = embassy_time_driver::TICK_HZ;

    loop {
        let elapsed_ticks = embassy_time_driver::now().saturating_sub(start_ticks);
        if !surface.render_mandelbrot(elapsed_ticks, tick_hz, "ui2-mandelbrot-demo") {
            let _ = crate::r::ui2::set_window_title(window_id, "Seahorse Valley (unavailable)");
            crate::log!("ui2-mandelbrot-demo: window={} queue failed\n", window_id);
            break;
        }

        Timer::after(EmbassyDuration::from_millis(33)).await;
    }
}
