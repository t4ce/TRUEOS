use embassy_time::{Duration as EmbassyDuration, Timer};

const TASK_NAME: &str = "ui2-intel-canvas3d-demo";
const UI2_INTEL_CANVAS3D_TEX_ID: u32 = crate::tst::ui2::ids::Ui2DemoTexId::IntelCanvas3d.get();
const UI2_INTEL_CANVAS3D_WINDOW_W: u32 = 360;
const UI2_INTEL_CANVAS3D_WINDOW_H: u32 = 120;
const UI2_INTEL_CANVAS3D_WINDOW_Z: i16 = 37;
const UI2_INTEL_CANVAS3D_FRAME_MS: u64 = 33;

#[embassy_executor::task]
pub async fn ui2_intel_canvas3d_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(TASK_NAME);
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "Intel Canvas3D",
        crate::r::ui2::Ui2Rect {
            x: 18.0,
            y: 620.0,
            w: UI2_INTEL_CANVAS3D_WINDOW_W as f32,
            h: UI2_INTEL_CANVAS3D_WINDOW_H as f32,
        },
        UI2_INTEL_CANVAS3D_WINDOW_Z,
        220,
        UI2_INTEL_CANVAS3D_TEX_ID,
        false,
        [0x08, 0x08, 0x10, 0xFF],
    ) else {
        return;
    };
    let _ = surface.bind_spawn_task(TASK_NAME);
    let _ = crate::r::ui2::set_window_title(surface.window_id(), "Intel Canvas3D");

    Timer::after(EmbassyDuration::from_millis(1)).await;
    let mut frame = 0u32;
    let mut logged_start = false;

    loop {
        if crate::r::spawn_service::task_stop_requested(TASK_NAME) {
            break;
        }

        match crate::intel::gpgpu::ui2_canvas3d_archaeology_project_frame(frame) {
            Some(result) => {
                if !logged_start {
                    logged_start = true;
                    crate::log!(
                        "ui2-intel-canvas3d-demo: mode=ico90-transform-project ok={} primary={}x{} vertices={} cadence_us={}\n",
                        result.ok as u8,
                        result.primary_width,
                        result.primary_height,
                        result.vertex_count,
                        result.cadence_us
                    );
                }
                frame = frame.wrapping_add(1);
            }
            None => {
                let _ = crate::r::ui2::set_window_title(
                    surface.window_id(),
                    "Intel Canvas3D unavailable",
                );
                crate::log!("ui2-intel-canvas3d-demo: frame failed\n");
                break;
            }
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms(TASK_NAME, UI2_INTEL_CANVAS3D_FRAME_MS)
            .await
        {
            break;
        }
    }
}
