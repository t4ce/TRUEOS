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
        255,
        UI2_INTEL_CANVAS3D_TEX_ID,
        false,
        [0x08, 0x08, 0x10, 0xFF],
    ) else {
        return;
    };
    let _ = surface.bind_spawn_task(TASK_NAME);
    let window_id = surface.window_id();
    let _ = crate::r::ui2::set_window_title(window_id, "Intel Canvas3D");
    let _ = crate::r::ui2::set_window_left_scrollbar_visible(window_id, false);
    let _ = crate::r::ui2::set_window_bottom_scrollbar_visible(window_id, false);

    Timer::after(EmbassyDuration::from_millis(1)).await;
    let mut frame = 0u32;
    let mut logged_start = false;

    loop {
        if crate::r::spawn_service::task_stop_requested(TASK_NAME) {
            break;
        }

        let Some(content_rect) = crate::r::ui2::window_content_rect_by_id(window_id) else {
            Timer::after(EmbassyDuration::from_millis(1)).await;
            continue;
        };
        let rect_w = content_rect.w.max(1.0) as u32;
        let rect_h = content_rect.h.max(1.0) as u32;

        match crate::intel::gpgpu::ui2_canvas3d_archaeology_project_texture_frame(
            frame, rect_w, rect_h,
        ) {
            Some(texture_frame) => {
                let result = texture_frame.result;
                if !logged_start {
                    logged_start = true;
                    crate::log!(
                        "ui2-intel-canvas3d-demo: mode=ico90-transform-project-ui2-texture ok={} texture={}x{} vertices={} cadence_us={}\n",
                        result.ok as u8,
                        texture_frame.width,
                        texture_frame.height,
                        result.vertex_count,
                        result.cadence_us
                    );
                }
                if !crate::r::io::cabi::queue_texture_rgba_image_upload_owned(
                    surface.tex_id(),
                    texture_frame.width,
                    texture_frame.height,
                    texture_frame.rgba,
                    window_id,
                    "ui2-intel-canvas3d-demo-present",
                ) {
                    crate::log!("ui2-intel-canvas3d-demo: texture upload failed\n");
                    break;
                }
                frame = frame.wrapping_add(1);
            }
            None => {
                let _ = crate::r::ui2::set_window_title(window_id, "Intel Canvas3D unavailable");
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
