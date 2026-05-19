extern crate alloc;

use alloc::vec::Vec;

use crate::r::ui2::{self, Ui2Rect};

const UI2_GBOI_TEX_ID: u32 = crate::tst::ui2::ids::Ui2DemoTexId::Gboi.get();
const UI2_GBOI_CONTENT_ID: u32 = crate::tst::ui2::ids::Ui2DemoContentId::Gboi.get();
const UI2_GBOI_TASK_NAME: &str = "ui2-gboi-demo";
const UI2_GBOI_WINDOW_TITLE: &str = "GBOI";
const UI2_GBOI_VIEW_W: u32 = 256;
const UI2_GBOI_VIEW_H: u32 = 240;
const UI2_GBOI_WINDOW_X: f32 = 760.0;
const UI2_GBOI_WINDOW_Y: f32 = 140.0;
const UI2_GBOI_WINDOW_Z: i16 = 41;
const UI2_GBOI_WINDOW_ALPHA: u8 = 0xFF;

fn argb_to_rgba_owned(argb: &[u32]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(argb.len().saturating_mul(4));
    for px in argb {
        rgba.push((px >> 16) as u8);
        rgba.push((px >> 8) as u8);
        rgba.push(*px as u8);
        rgba.push((px >> 24) as u8);
    }
    rgba
}

fn render_no_rom_frame(emulator: &crate::gboi::nes::NesEmulator) -> Vec<u8> {
    let mut argb = alloc::vec![0u32; (UI2_GBOI_VIEW_W * UI2_GBOI_VIEW_H) as usize];
    emulator.render(argb.as_mut_slice(), UI2_GBOI_VIEW_W as usize, UI2_GBOI_VIEW_H as usize);
    argb_to_rgba_owned(argb.as_slice())
}

#[embassy_executor::task]
pub async fn ui2_gboi_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(UI2_GBOI_TASK_NAME);
    let emulator = crate::gboi::nes::NesEmulator::new();

    let Some(surface) = ui2::Ui2SurfaceWindow::get_or_create_for_hosted_content_with_size(
        UI2_GBOI_WINDOW_TITLE,
        Ui2Rect {
            x: UI2_GBOI_WINDOW_X,
            y: UI2_GBOI_WINDOW_Y,
            w: UI2_GBOI_VIEW_W as f32,
            h: UI2_GBOI_VIEW_H as f32,
        },
        UI2_GBOI_WINDOW_Z,
        UI2_GBOI_WINDOW_ALPHA,
        UI2_GBOI_CONTENT_ID,
        UI2_GBOI_TEX_ID,
        true,
        UI2_GBOI_VIEW_W,
        UI2_GBOI_VIEW_H,
    ) else {
        crate::log!("ui2-gboi-demo: window creation failed tex={}\n", UI2_GBOI_TEX_ID);
        return;
    };

    let _ = surface.bind_spawn_task(UI2_GBOI_TASK_NAME);
    let _ = ui2::set_window_title(surface.window_id(), UI2_GBOI_WINDOW_TITLE);
    let _ = ui2::set_window_decorations(surface.window_id(), ui2::Ui2WindowDecorationMode::System);
    let _ = ui2::set_window_bottom_bar_visible(surface.window_id(), true);
    let _ = ui2::set_window_left_scrollbar_visible(surface.window_id(), false);
    let _ = ui2::set_window_bottom_scrollbar_visible(surface.window_id(), false);
    let _ = ui2::set_window_resize_maintain_aspect(surface.window_id(), true);
    let _ = ui2::set_window_content_preserve_scale(surface.window_id(), true);

    let pixels = render_no_rom_frame(&emulator);
    if !surface.upload_rgba_owned(pixels, "ui2-gboi-demo-present") {
        crate::log!("ui2-gboi-demo: upload failed tex={}\n", surface.tex_id());
        return;
    }

    loop {
        if crate::r::spawn_service::wait_task_or_timeout_ms(UI2_GBOI_TASK_NAME, 3_600_000).await {
            break;
        }
    }
}
