use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::r::ui2::Ui2HostedInteractiveRect;

const UI2_MANDELBROT_TEX_ID: u32 = crate::tst::ui2::ids::Ui2DemoTexId::Mandelbrot.get();
const UI2_MANDELBROT_RT_W: u32 = 768;
const UI2_MANDELBROT_RT_H: u32 = 512;
const UI2_MANDELBROT_WINDOW_Z: i16 = 31;
const UI2_MANDELBROT_ITEM_SURFACE: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DemoFractal {
    Mandelbrot,
    Julia,
    BurningShip,
}

impl DemoFractal {
    const fn next(self) -> Self {
        match self {
            Self::Mandelbrot => Self::Julia,
            Self::Julia => Self::BurningShip,
            Self::BurningShip => Self::Mandelbrot,
        }
    }

    const fn title(self) -> &'static str {
        match self {
            Self::Mandelbrot => "Seahorse Valley",
            Self::Julia => "Julia Set",
            Self::BurningShip => "Burning Ship",
        }
    }
}

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
    let _ = surface.set_interactives(&[Ui2HostedInteractiveRect {
        item_id: UI2_MANDELBROT_ITEM_SURFACE,
        x: 0,
        y: 0,
        width: UI2_MANDELBROT_RT_W,
        height: UI2_MANDELBROT_RT_H,
    }]);

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
    let mut fractal = DemoFractal::Mandelbrot;
    let mut last_click_seq = 0u32;

    loop {
        if crate::r::spawn_service::task_stop_requested("ui2-mandelbrot-demo") {
            break;
        }
        if let Some((seq, _item_id, _cursor_slot)) =
            crate::r::ui2::take_window_last_clicked_item_with_cursor(window_id)
            && seq != last_click_seq
        {
            last_click_seq = seq;
            fractal = fractal.next();
            let _ = crate::r::ui2::set_window_title(window_id, fractal.title());
        }
        let elapsed_ticks = embassy_time_driver::now().saturating_sub(start_ticks);
        let rendered = match fractal {
            DemoFractal::Mandelbrot => {
                surface.render_mandelbrot(elapsed_ticks, tick_hz, "ui2-mandelbrot-demo")
            }
            DemoFractal::Julia => {
                surface.render_julia(elapsed_ticks, tick_hz, "ui2-mandelbrot-demo")
            }
            DemoFractal::BurningShip => {
                surface.render_burning_ship(elapsed_ticks, tick_hz, "ui2-mandelbrot-demo")
            }
        };
        if !rendered {
            let _ = crate::r::ui2::set_window_title(window_id, "Fractal shader unavailable");
            crate::log!("ui2-mandelbrot-demo: window={} queue failed\n", window_id);
            break;
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-mandelbrot-demo", 33).await {
            break;
        }
    }
}
