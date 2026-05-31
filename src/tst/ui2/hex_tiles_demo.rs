extern crate alloc;

use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::r::ui2::{self, Ui2HostedInteractiveRect};

const UI2_BEE_TILES_TEX_ID: u32 = crate::tst::ui2::ids::Ui2DemoTexId::BeeTiles.get();
const UI2_BEE_TILES_VIEW_W: u32 = 768;
const UI2_BEE_TILES_VIEW_H: u32 = 512;
const UI2_BEE_TILES_RT_SCALE: u32 = 2;
const UI2_BEE_TILES_RT_W: u32 = UI2_BEE_TILES_VIEW_W * UI2_BEE_TILES_RT_SCALE;
const UI2_BEE_TILES_RT_H: u32 = UI2_BEE_TILES_VIEW_H * UI2_BEE_TILES_RT_SCALE;
const UI2_BEE_TILES_WINDOW_Z: i16 = 32;
const UI2_BEE_TILES_FRAME_MS: u64 = 33;
const UI2_BEE_TILES_ITEM_SURFACE: u32 = 1;

#[embassy_executor::task]
pub async fn ui2_hex_tiles_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-bee-tiles-demo");
    let Some(pixel_count) = (UI2_BEE_TILES_RT_W as usize).checked_mul(UI2_BEE_TILES_RT_H as usize)
    else {
        return;
    };
    let Some(byte_len) = pixel_count.checked_mul(4) else {
        return;
    };
    let mut clear_pixels = Vec::with_capacity(byte_len);
    for _ in 0..pixel_count {
        clear_pixels.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    }
    if !crate::r::io::cabi::queue_texture_rgba_image_upload_owned(
        UI2_BEE_TILES_TEX_ID,
        UI2_BEE_TILES_RT_W,
        UI2_BEE_TILES_RT_H,
        clear_pixels,
        0,
        "ui2-bee-tiles-init",
    ) {
        return;
    }

    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::create_from_existing_texture_with_size(
        "Bee Tiles",
        crate::r::ui2::Ui2Rect {
            x: 96.0,
            y: 72.0,
            w: UI2_BEE_TILES_VIEW_W as f32,
            h: UI2_BEE_TILES_VIEW_H as f32,
        },
        UI2_BEE_TILES_WINDOW_Z,
        224,
        UI2_BEE_TILES_TEX_ID,
        true,
        UI2_BEE_TILES_RT_W,
        UI2_BEE_TILES_RT_H,
    ) else {
        return;
    };
    let _ = surface.bind_spawn_task("ui2-bee-tiles-demo");
    let _ = ui2::set_window_resize_maintain_aspect(surface.window_id(), true);
    let _ = surface.set_interactives(&[Ui2HostedInteractiveRect {
        item_id: UI2_BEE_TILES_ITEM_SURFACE,
        x: 0,
        y: 0,
        width: UI2_BEE_TILES_VIEW_W,
        height: UI2_BEE_TILES_VIEW_H,
    }]);

    let window_id = surface.window_id();
    let (surface_w, surface_h) = surface.size();
    crate::log!(
        "ui2-bee-tiles-demo: window={} tex={} size={}x{} start\n",
        window_id,
        surface.tex_id(),
        surface_w,
        surface_h
    );

    Timer::after(EmbassyDuration::from_millis(1)).await;
    let start_ticks = embassy_time_driver::now();
    let tick_hz = embassy_time_driver::TICK_HZ;

    loop {
        if crate::r::spawn_service::task_stop_requested("ui2-bee-tiles-demo") {
            break;
        }

        let elapsed_ticks = embassy_time_driver::now().saturating_sub(start_ticks);
        if !surface.render_bee_tiles(elapsed_ticks, tick_hz, "ui2-bee-tiles-demo") {
            let _ = crate::r::ui2::set_window_title(window_id, "Bee tiles shader unavailable");
            crate::log!("ui2-bee-tiles-demo: window={} queue failed\n", window_id);
            break;
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms(
            "ui2-bee-tiles-demo",
            UI2_BEE_TILES_FRAME_MS,
        )
        .await
        {
            break;
        }
    }
}
