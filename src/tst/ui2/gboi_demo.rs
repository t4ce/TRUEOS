extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::r::ui2::{self, Ui2Rect};
use spin::Mutex;

const UI2_GBOI_TEX_ID: u32 = crate::tst::ui2::ids::Ui2DemoTexId::Gboi.get();
const UI2_GBOI_CONTENT_ID: u32 = crate::tst::ui2::ids::Ui2DemoContentId::Gboi.get();
const UI2_GBOI_TASK_NAME: &str = "ui2-gboi-demo";
const UI2_GBOI_WINDOW_TITLE: &str = "GBOI";
const UI2_GBOI_VIEW_W: u32 = 160;
const UI2_GBOI_VIEW_H: u32 = 144;
const UI2_GBOI_DIRECT_SCALE: u32 = 4;
const UI2_GBOI_WINDOW_X: f32 = 760.0;
const UI2_GBOI_WINDOW_Y: f32 = 140.0;
const UI2_GBOI_WINDOW_Z: i16 = 41;
const UI2_GBOI_WINDOW_ALPHA: u8 = 0xFF;
const UI2_GBOI_FRAME_MS: u64 = 16;
const UI2_GBOI_SPEED_MULTIPLIER: usize = 2;
const UI2_GBOI_AUDIO_ENABLED: bool = true;
const UI2_GBOI_KEYBOARD_BATCH: usize = 16;
const UI2_GBOI_INPUT_QUEUE_CAP: usize = 64;
const UI2_GBOI_BOOT_ROM_7Z: &[u8] =
    include_bytes!("../../../crates/trueos-gboi/SuperMarioBros.Deluxe.7z"); // 

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum GboiPresentMode {
    Ui2Window,
    IntelPrimaryTopRight,
}

fn choose_present_mode() -> GboiPresentMode {
    if crate::intel::has_claimed_device() {
        GboiPresentMode::IntelPrimaryTopRight
    } else {
        GboiPresentMode::Ui2Window
    }
}

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

fn render_frame(emulator: &crate::gboi::gb::GameBoyEmulator) -> Vec<u8> {
    let mut argb = alloc::vec![0u32; (UI2_GBOI_VIEW_W * UI2_GBOI_VIEW_H) as usize];
    emulator.render(argb.as_mut_slice(), UI2_GBOI_VIEW_W as usize, UI2_GBOI_VIEW_H as usize);
    argb_to_rgba_owned(argb.as_slice())
}

fn render_direct_frame(emulator: &crate::gboi::gb::GameBoyEmulator) -> (Vec<u8>, u32, u32) {
    let out_w = UI2_GBOI_VIEW_W.saturating_mul(UI2_GBOI_DIRECT_SCALE.max(1));
    let out_h = UI2_GBOI_VIEW_H.saturating_mul(UI2_GBOI_DIRECT_SCALE.max(1));
    let mut argb = alloc::vec![0u32; (out_w * out_h) as usize];
    emulator.render(argb.as_mut_slice(), out_w as usize, out_h as usize);
    (argb_to_rgba_owned(argb.as_slice()), out_w, out_h)
}

fn pump_audio(
    emulator: &mut crate::gboi::gb::GameBoyEmulator,
    stream: &mut crate::aud::dmg::DmgAudioStream,
    samples: &mut Vec<i16>,
) {
    if !UI2_GBOI_AUDIO_ENABLED {
        return;
    }

    samples.clear();
    emulator.drain_audio_samples_into(samples);
    if samples.is_empty() {
        return;
    }

    if let Err(err) = stream.push_samples(samples.as_slice()) {
        crate::log!("ui2-gboi-demo: hda dmg stream skipped err={}\n", err);
    }
}

fn push_pressed_button(
    pressed_buttons: &mut [Option<crate::gboi::gb::GameBoyButton>; UI2_GBOI_KEYBOARD_BATCH],
    pressed_button_count: &mut usize,
    button: crate::gboi::gb::GameBoyButton,
) {
    if *pressed_button_count < pressed_buttons.len() {
        pressed_buttons[*pressed_button_count] = Some(button);
        *pressed_button_count += 1;
    }
}

fn load_boot_rom(emulator: &mut crate::gboi::gb::GameBoyEmulator) {
    match crate::z7::extract_single_file_to_vec(UI2_GBOI_BOOT_ROM_7Z) {
        Ok(rom) => {
            if !emulator.load_rom(rom.as_slice()) {
                crate::log!("ui2-gboi-demo: boot rom load failed bytes={}\n", rom.len());
            }
        }
        Err(err) => {
            crate::log!(
                "ui2-gboi-demo: boot rom 7z decode failed archive_bytes={} err={:?}\n",
                UI2_GBOI_BOOT_ROM_7Z.len(),
                err
            );
        }
    }
}

fn step_emulator(emulator: &mut crate::gboi::gb::GameBoyEmulator) {
    for _ in 0..UI2_GBOI_SPEED_MULTIPLIER {
        emulator.tick();
    }
}

async fn run_intel_primary_mode(mut emulator: crate::gboi::gb::GameBoyEmulator) {
    let mut audio_stream = crate::aud::dmg::DmgAudioStream::new();
    let mut audio_samples = Vec::new();

    crate::log!(
        "ui2-gboi-demo: present mode=intel-primary-top-right size={}x{} scale={} output={}x{}\n",
        UI2_GBOI_VIEW_W,
        UI2_GBOI_VIEW_H,
        UI2_GBOI_DIRECT_SCALE,
        UI2_GBOI_VIEW_W.saturating_mul(UI2_GBOI_DIRECT_SCALE.max(1)),
        UI2_GBOI_VIEW_H.saturating_mul(UI2_GBOI_DIRECT_SCALE.max(1))
    );

    loop {
        if crate::r::spawn_service::task_stop_requested(UI2_GBOI_TASK_NAME) {
            break;
        }

        step_emulator(&mut emulator);
        pump_audio(&mut emulator, &mut audio_stream, &mut audio_samples);

        let (pixels, out_w, out_h) = render_direct_frame(&emulator);
        if !crate::intel::present_rgba_primary_top_right(
            pixels.as_slice(),
            out_w,
            out_h,
            out_w as usize * 4,
        ) {
            crate::log!("ui2-gboi-demo: intel primary present failed\n");
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms(UI2_GBOI_TASK_NAME, UI2_GBOI_FRAME_MS)
            .await
        {
            break;
        }
    }
}

async fn run_ui2_window_mode(mut emulator: crate::gboi::gb::GameBoyEmulator) {
    crate::r::readiness::wait_for(
        crate::r::readiness::UI2_READY | crate::r::readiness::GFX_TEXTURE_UPLOAD_SERVICE_READY,
    )
    .await;

    crate::log!(
        "ui2-gboi-demo: present mode=ui2-window size={}x{}\n",
        UI2_GBOI_VIEW_W,
        UI2_GBOI_VIEW_H
    );

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
        false,
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
    let _ = ui2::set_window_content_preserve_scale(surface.window_id(), false);

    let _ = surface.bind_hosted_scroll_state(UI2_GBOI_CONTENT_ID, UI2_GBOI_VIEW_W, UI2_GBOI_VIEW_H);
    attach_keyboard_window(surface.window_id());
    let mut raw_events =
        [crate::r::keyboard::TrueosKeyboardOutputEvent::default(); UI2_GBOI_KEYBOARD_BATCH];
    let mut pressed_buttons: [Option<crate::gboi::gb::GameBoyButton>;
        UI2_GBOI_KEYBOARD_BATCH] = [None; UI2_GBOI_KEYBOARD_BATCH];
    let mut pressed_button_count = 0usize;
    let mut audio_stream = crate::aud::dmg::DmgAudioStream::new();
    let mut audio_samples = Vec::new();

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
                if let Some(button) = control.gb_button() {
                    emulator.set_button(button, true);
                    push_pressed_button(&mut pressed_buttons, &mut pressed_button_count, button);
                }
            }
        }

        step_emulator(&mut emulator);
        pump_audio(&mut emulator, &mut audio_stream, &mut audio_samples);

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

#[embassy_executor::task]
pub async fn ui2_gboi_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(UI2_GBOI_TASK_NAME);
    let mut emulator = crate::gboi::gb::GameBoyEmulator::new();
    load_boot_rom(&mut emulator);

    match choose_present_mode() {
        GboiPresentMode::Ui2Window => run_ui2_window_mode(emulator).await,
        GboiPresentMode::IntelPrimaryTopRight => run_intel_primary_mode(emulator).await,
    }
}
