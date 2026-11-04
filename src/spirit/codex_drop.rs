//! Lilly introduces dropped attachments through her existing paired keyboard.
use crate::r::services::keyboard_control_service::{self as keyboard, KeyboardControlPrincipal};
use crate::ui4::{WindowId, WindowOwner};
use spin::Mutex;

struct Introduction {
    window: WindowId,
    device: keyboard::KeyboardControlDevice,
    lease: Option<super::ui4_interaction::NativeIoGuard>,
    cancelled: bool,
}
static INTRODUCTION: Mutex<Option<Introduction>> = Mutex::new(None);

pub(crate) fn introduce_attachments(window: WindowId, text: &str) -> bool {
    let Ok(guard) = super::ui4_interaction::try_acquire_native_io() else {
        return false;
    };
    let Some(device) = guard.keyboard() else {
        return false;
    };
    let Ok(source) = super::lilly_cursor::selection_source() else {
        return false;
    };
    if crate::ui4::select_window_for_cursor(source, WindowOwner::CODEX_PROMPT_SERVICE, window)
        .is_err()
    {
        return false;
    }
    if keyboard::submit_text(
        KeyboardControlPrincipal::Kernel,
        device.handle,
        text,
        48,
        true,
    )
    .is_err()
    {
        return false;
    }
    *INTRODUCTION.lock() = Some(Introduction {
        window,
        device,
        lease: Some(guard),
        cancelled: false,
    });
    true
}

fn stop(intro: &mut Introduction) {
    if intro.lease.is_some() {
        let _ = keyboard::submit_command(
            KeyboardControlPrincipal::Kernel,
            intro.device.handle,
            keyboard::KeyboardControlCommand {
                opcode: keyboard::KEYBOARD_CONTROL_OPCODE_WAIT,
                flags: keyboard::KEYBOARD_CONTROL_FLAG_CLEAR_QUEUE,
                duration_ms: 1,
                ..Default::default()
            },
        );
        // The serializer keeps this lease draining through the final release.
        intro.lease.take();
    }
    intro.cancelled = true;
}

/// Physical typing takes over immediately; ignore already-buffered Lilly
/// strokes after that takeover. Never clear another automation's program.
pub(crate) fn attachment_intro_input(
    window: WindowId,
    event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> bool {
    let mut state = INTRODUCTION.lock();
    let Some(intro) = state.as_mut().filter(|intro| intro.window == window) else {
        return true;
    };
    if event.slot_id == intro.device.slot_id {
        return !intro.cancelled;
    }
    stop(intro);
    true
}

pub(crate) fn poll_attachment_intro(window: WindowId) {
    let mut state = INTRODUCTION.lock();
    if let Some(intro) = state.as_mut().filter(|intro| intro.window == window) {
        if keyboard::keyboard_is_idle(KeyboardControlPrincipal::Kernel, intro.device.handle)
            .unwrap_or(false)
        {
            intro.lease.take();
        }
    }
}

pub(crate) fn cancel_attachment_intro(window: WindowId) {
    let mut state = INTRODUCTION.lock();
    if state.as_ref().is_some_and(|intro| intro.window == window) {
        if let Some(mut intro) = state.take() {
            stop(&mut intro);
        }
    }
}
