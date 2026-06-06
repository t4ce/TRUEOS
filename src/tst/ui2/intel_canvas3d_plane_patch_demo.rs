use embassy_time::{Duration as EmbassyDuration, Timer};

const TASK_NAME: &str = "ui2-intel-canvas3d-plane-patch-demo";
const UI2_INTEL_CANVAS3D_PLANE_PATCH_TEX_ID: u32 =
    crate::tst::ui2::ids::Ui2DemoTexId::IntelCanvas3dPlanePatch.get();
const UI2_INTEL_CANVAS3D_PLANE_PATCH_WINDOW_W: u32 = 540;
const UI2_INTEL_CANVAS3D_PLANE_PATCH_WINDOW_H: u32 = 540;
const UI2_INTEL_CANVAS3D_PLANE_PATCH_WINDOW_Z: i16 = 38;
const UI2_INTEL_CANVAS3D_PLANE_PATCH_IDLE_MS: u64 = 1_000;
const UI2_INTEL_CANVAS3D_PLANE_PATCH_CONTENT_PX: u32 = 512;

#[embassy_executor::task]
pub async fn ui2_intel_canvas3d_plane_patch_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(TASK_NAME);
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "Intel Plane Patch",
        crate::r::ui2::Ui2Rect {
            x: 392.0,
            y: 116.0,
            w: UI2_INTEL_CANVAS3D_PLANE_PATCH_WINDOW_W as f32,
            h: UI2_INTEL_CANVAS3D_PLANE_PATCH_WINDOW_H as f32,
        },
        UI2_INTEL_CANVAS3D_PLANE_PATCH_WINDOW_Z,
        255,
        UI2_INTEL_CANVAS3D_PLANE_PATCH_TEX_ID,
        false,
        [0x06, 0x08, 0x0C, 0xFF],
    ) else {
        return;
    };
    let _ = surface.bind_spawn_task(TASK_NAME);
    let window_id = surface.window_id();
    let _ = crate::r::ui2::set_window_title(window_id, "Intel Plane Patch");
    let _ = crate::r::ui2::set_window_left_scrollbar_visible(window_id, false);
    let _ = crate::r::ui2::set_window_bottom_scrollbar_visible(window_id, false);

    Timer::after(EmbassyDuration::from_millis(1)).await;
    match crate::intel::gpgpu::ui2_canvas3d_plane_patch_texture_frame(
        0,
        UI2_INTEL_CANVAS3D_PLANE_PATCH_CONTENT_PX,
        UI2_INTEL_CANVAS3D_PLANE_PATCH_CONTENT_PX,
    ) {
        Some(texture_frame) => {
            let result = texture_frame.result;
            crate::log!(
                "ui2-intel-canvas3d-plane-patch-demo: mode=plane-patch-cube6-worklist-ui2-texture-once ok={} texture={}x{} colored={} submits={}\n",
                result.ok as u8,
                texture_frame.width,
                texture_frame.height,
                result.stamped_pixels,
                result.submitted
            );
            if !crate::r::io::cabi::queue_texture_rgba_image_upload_owned(
                surface.tex_id(),
                texture_frame.width,
                texture_frame.height,
                texture_frame.rgba,
                window_id,
                "ui2-intel-canvas3d-plane-patch-demo-present",
            ) {
                crate::log!("ui2-intel-canvas3d-plane-patch-demo: texture upload failed\n");
                return;
            }
        }
        None => {
            let _ = crate::r::ui2::set_window_title(window_id, "Intel Plane Patch unavailable");
            crate::log!("ui2-intel-canvas3d-plane-patch-demo: frame failed\n");
            return;
        }
    }

    loop {
        if crate::r::spawn_service::task_stop_requested(TASK_NAME)
            || crate::r::spawn_service::wait_task_or_timeout_ms(
                TASK_NAME,
                UI2_INTEL_CANVAS3D_PLANE_PATCH_IDLE_MS,
            )
            .await
        {
            break;
        }
    }
}
