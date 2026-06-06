use embassy_time::{Duration as EmbassyDuration, Timer};

const TASK_NAME: &str = "ui2-intel-canvas3d-plane-patch-demo";
const UI2_INTEL_CANVAS3D_PLANE_PATCH_TEX_ID: u32 =
    crate::tst::ui2::ids::Ui2DemoTexId::IntelCanvas3dPlanePatch.get();
const UI2_INTEL_CANVAS3D_PLANE_PATCH_WINDOW_W: u32 = 540;
const UI2_INTEL_CANVAS3D_PLANE_PATCH_WINDOW_H: u32 = 540;
const UI2_INTEL_CANVAS3D_PLANE_PATCH_WINDOW_Z: i16 = 38;
const UI2_INTEL_CANVAS3D_PLANE_PATCH_FRAME_MS: u64 = 500;
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
    let mut frame = 0u32;
    let mut logged_start = false;
    let mut texture_seed_queued = false;
    let mut failed_frames = 0u32;

    loop {
        if crate::r::spawn_service::task_stop_requested(TASK_NAME) {
            break;
        }

        let texture_surface = crate::r::io::cabi::texture_gpgpu_rgba8_surface(surface.tex_id());
        let Some(texture_surface) = texture_surface else {
            if !texture_seed_queued {
                texture_seed_queued = true;
                let bytes = (UI2_INTEL_CANVAS3D_PLANE_PATCH_CONTENT_PX as usize)
                    .saturating_mul(UI2_INTEL_CANVAS3D_PLANE_PATCH_CONTENT_PX as usize)
                    .saturating_mul(4);
                let mut seed_rgba = alloc::vec![0x10; bytes];
                for pixel in seed_rgba.chunks_exact_mut(4) {
                    pixel[3] = 0xFF;
                }
                if !crate::r::io::cabi::queue_texture_rgba_image_upload_owned(
                    surface.tex_id(),
                    UI2_INTEL_CANVAS3D_PLANE_PATCH_CONTENT_PX,
                    UI2_INTEL_CANVAS3D_PLANE_PATCH_CONTENT_PX,
                    seed_rgba,
                    window_id,
                    "ui2-intel-canvas3d-plane-patch-demo-seed",
                ) {
                    crate::log!("ui2-intel-canvas3d-plane-patch-demo: seed upload failed\n");
                    break;
                }
            }
            Timer::after(EmbassyDuration::from_millis(50)).await;
            continue;
        };

        match crate::intel::gpgpu::ui2_canvas3d_plane_patch_render_surface_frame(
            frame,
            texture_surface,
        ) {
            Some(result) if result.ok => {
                failed_frames = 0;
                if !logged_start {
                    logged_start = true;
                    crate::log!(
                        "ui2-intel-canvas3d-plane-patch-demo: mode=plane-patch-cube6-worklist-ui2-direct-texture ok={} texture={}x{} submits={} submit_ms={} cadence_us={}\n",
                        result.ok as u8,
                        texture_surface.width,
                        texture_surface.height,
                        result.submitted,
                        result.total_submit_ms,
                        result.cadence_us
                    );
                }
                let present_queued = crate::r::ui2::request_window_content_present(
                    window_id,
                    "ui2-intel-canvas3d-plane-patch-demo-present",
                );
                if !present_queued && (frame <= 4 || frame.is_multiple_of(120)) {
                    crate::log!(
                        "ui2-intel-canvas3d-plane-patch-demo: present request skipped frame={}\n",
                        frame
                    );
                }
                frame = frame.wrapping_add(1);
            }
            _ => {
                failed_frames = failed_frames.saturating_add(1);
                if failed_frames <= 4 || failed_frames.is_multiple_of(120) {
                    crate::log!(
                        "ui2-intel-canvas3d-plane-patch-demo: frame failed count={} frame={}\n",
                        failed_frames,
                        frame
                    );
                }
            }
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms(
            TASK_NAME,
            UI2_INTEL_CANVAS3D_PLANE_PATCH_FRAME_MS,
        )
        .await
        {
            break;
        }
    }
}
