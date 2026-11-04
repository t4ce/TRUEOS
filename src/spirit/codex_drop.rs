//! Lilly introduces dropped attachments through her existing paired keyboard.
use crate::r::services::keyboard_control_service::{self as keyboard, KeyboardControlPrincipal};
use crate::ui4::{WindowId, WindowOwner};

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
    // Dropping the native lease marks it draining until the cursor/keyboard
    // program completes. Blueprint automation cannot interleave its own input.
    keyboard::submit_text(
        KeyboardControlPrincipal::Kernel,
        device.handle,
        text,
        48,
        true,
    )
    .is_ok()
}
