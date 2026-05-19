extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::r::ui2::{self, Ui2Rect};
use spin::Mutex;

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
const UI2_GBOI_FRAME_MS: u64 = 16;
const UI2_GBOI_KEYBOARD_BATCH: usize = 16;
const UI2_GBOI_INPUT_QUEUE_CAP: usize = 64;

static UI2_GBOI_INPUT: Mutex<Ui2GboiInputRuntime> = Mutex::new(Ui2GboiInputRuntime {
    window_id: 0,
    events: VecDeque::new(),
});

struct Ui2GboiInputRuntime {
    window_id: u32,
    events: VecDeque<crate::r::keyboard::TrueosKeyboardOutputEvent>,
}

pub(crate) fn queue_keyboard_event(
    window_id: u32,
    event: crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> bool {
    let mut runtime = UI2_GBOI_INPUT.lock();
    if window_id == 0 || runtime.window_id != window_id {
        return false;
    }
    if runtime.events.len() >= UI2_GBOI_INPUT_QUEUE_CAP {
        let _ = runtime.events.pop_front();
    }
    runtime.events.push_back(event);
    true
}

fn attach_keyboard_window(window_id: u32) {
    let mut runtime = UI2_GBOI_INPUT.lock();
    runtime.window_id = window_id;
    runtime.events.clear();
}

fn drain_keyboard_events(
    out: &mut [crate::r::keyboard::TrueosKeyboardOutputEvent; UI2_GBOI_KEYBOARD_BATCH],
) -> usize {
    let mut runtime = UI2_GBOI_INPUT.lock();
    let mut count = 0usize;
    while count < out.len() {
        let Some(event) = runtime.events.pop_front() else {
            break;
        };
        out[count] = event;
        count += 1;
    }
    count
}

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

fn render_frame(emulator: &crate::gboi::nes::NesEmulator) -> Vec<u8> {
    let mut argb = alloc::vec![0u32; (UI2_GBOI_VIEW_W * UI2_GBOI_VIEW_H) as usize];
    emulator.render(argb.as_mut_slice(), UI2_GBOI_VIEW_W as usize, UI2_GBOI_VIEW_H as usize);
    argb_to_rgba_owned(argb.as_slice())
}

fn push_pressed_button(
    pressed_buttons: &mut [Option<crate::gboi::nes::NesControllerButton>; UI2_GBOI_KEYBOARD_BATCH],
    pressed_button_count: &mut usize,
    button: crate::gboi::nes::NesControllerButton,
) {
    if *pressed_button_count < pressed_buttons.len() {
        pressed_buttons[*pressed_button_count] = Some(button);
        *pressed_button_count += 1;
    }
}

#[embassy_executor::task]
pub async fn ui2_gboi_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(UI2_GBOI_TASK_NAME);
    let mut emulator = crate::gboi::nes::NesEmulator::new();

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

    let _ = surface.bind_hosted_scroll_state(UI2_GBOI_CONTENT_ID, UI2_GBOI_VIEW_W, UI2_GBOI_VIEW_H);
    attach_keyboard_window(surface.window_id());
    let mut raw_events =
        [crate::r::keyboard::TrueosKeyboardOutputEvent::default(); UI2_GBOI_KEYBOARD_BATCH];
    let mut pressed_buttons: [Option<crate::gboi::nes::NesControllerButton>;
        UI2_GBOI_KEYBOARD_BATCH] = [None; UI2_GBOI_KEYBOARD_BATCH];
    let mut pressed_button_count = 0usize;

    loop {
        if crate::r::spawn_service::task_stop_requested(UI2_GBOI_TASK_NAME) {
            break;
        }

        loop {
            let wrote = drain_keyboard_events(&mut raw_events);
            if wrote == 0 {
                break;
            }
            for event in raw_events.iter().take(wrote).copied() {
                let Some(control) = crate::gboi::HostControl::from_keyboard_event(event) else {
                    continue;
                };
                if let Some(button) = control.nes_button() {
                    emulator.set_button(button, true);
                    push_pressed_button(&mut pressed_buttons, &mut pressed_button_count, button);
                }
            }
        }

        emulator.tick();

        for button in pressed_buttons.iter_mut().take(pressed_button_count) {
            if let Some(button) = button.take() {
                emulator.set_button(button, false);
            }
        }
        pressed_button_count = 0;

        let pixels = render_frame(&emulator);
        if !surface.upload_rgba_owned(pixels, "ui2-gboi-demo-present") {
            crate::log!("ui2-gboi-demo: upload failed tex={}\n", surface.tex_id());
            return;
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms(UI2_GBOI_TASK_NAME, UI2_GBOI_FRAME_MS)
            .await
        {
            break;
        }
    }
}
