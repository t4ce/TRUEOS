use embassy_time::{Duration as EmbassyDuration, Timer};

const UI2_MANDELBROT_TEX_ID: u32 = crate::tst::ui2::ids::Ui2DemoTexId::Mandelbrot.get();
const UI2_MANDELBROT_RT_W: u32 = 768;
const UI2_MANDELBROT_RT_H: u32 = 512;
const UI2_MANDELBROT_WINDOW_Z: i16 = 31;

#[embassy_executor::task]
pub async fn ui2_mandelbrot_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-mandelbrot-demo");
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "Demo Mandelbrot",
        crate::r::ui2::Ui2Rect {
            x: 10.0,
            y: 10.0,
            w: UI2_MANDELBROT_RT_W as f32,
            h: UI2_MANDELBROT_RT_H as f32,
        },
        UI2_MANDELBROT_WINDOW_Z,
        128,
        UI2_MANDELBROT_TEX_ID,
        false,
        [0x08, 0x0B, 0x10, 0xFF],
    ) else {
        return;
    };
    let _ = surface.bind_spawn_task("ui2-mandelbrot-demo");

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
        if crate::r::spawn_service::task_stop_requested("ui2-mandelbrot-demo") {
            break;
        }
        let elapsed_ticks = embassy_time_driver::now().saturating_sub(start_ticks);
        if !surface.render_mandelbrot(elapsed_ticks, tick_hz, "ui2-mandelbrot-demo") {
            let _ = crate::r::ui2::set_window_title(window_id, "Seahorse Valley (unavailable)");
            crate::log!("ui2-mandelbrot-demo: window={} queue failed\n", window_id);
            break;
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-mandelbrot-demo", 33).await {
            break;
        }
    }
}
