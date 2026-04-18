use super::*;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use trueos_gfx_core::Rgba8;

static UI2_CURSOR_CAP_DROP_COUNT: AtomicU32 = AtomicU32::new(0);
pub(crate) const UI2_FUN_CURSOR_ICONS_ENABLED: bool = true;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui2CursorColor {
    Blue,
    Red,
    Green,
    Amber,
    Violet,
    Cyan,
}

impl Ui2CursorColor {
    #[inline]
    pub(crate) const fn from_slot_id(slot_id: u32) -> Self {
        match slot_id.saturating_sub(1) % 6 {
            0 => Self::Blue,
            1 => Self::Red,
            2 => Self::Green,
            3 => Self::Amber,
            4 => Self::Violet,
            _ => Self::Cyan,
        }
    }

    #[inline]
    pub(crate) const fn rgba(self) -> (u8, u8, u8, u8) {
        match self {
            Self::Blue => (0x3B, 0x82, 0xF6, 0xFF),
            Self::Red => (0xEF, 0x44, 0x44, 0xFF),
            Self::Green => (0x10, 0xB9, 0x81, 0xFF),
            Self::Amber => (0xF5, 0x9E, 0x0B, 0xFF),
            Self::Violet => (0x8B, 0x5C, 0xF6, 0xFF),
            Self::Cyan => (0x06, 0xB6, 0xD4, 0xFF),
        }
    }

    #[inline]
    pub(crate) const fn rgba8(self) -> Rgba8 {
        let (r, g, b, a) = self.rgba();
        Rgba8::new(r, g, b, a)
    }

    #[inline]
    pub(crate) const fn spirit_glyph(self) -> char {
        match self {
            Self::Blue => '🦋',
            Self::Red => '🦊',
            Self::Green => '🦎',
            Self::Amber => '🦁',
            Self::Violet => '🦄',
            Self::Cyan => '🐕',
        }
    }
}

#[inline]
pub(crate) fn cursor_color(slot_id: u32) -> (u8, u8, u8, u8) {
    Ui2CursorColor::from_slot_id(slot_id).rgba()
}

#[inline]
pub(crate) fn cursor_color_rgba8(slot_id: u32) -> Rgba8 {
    Ui2CursorColor::from_slot_id(slot_id).rgba8()
}

#[inline]
pub(crate) fn cursor_spirit_glyph(slot_id: u32) -> Option<char> {
    UI2_FUN_CURSOR_ICONS_ENABLED.then_some(Ui2CursorColor::from_slot_id(slot_id).spirit_glyph())
}

#[inline]
fn should_log_input_diag(count: u32) -> bool {
    count <= UI2_INPUT_DIAG_LOG_FIRST
        || (count > UI2_INPUT_DIAG_LOG_FIRST && count.is_multiple_of(UI2_INPUT_DIAG_LOG_EVERY))
}

#[inline]
fn cursor_event_is_physical(event: &crate::usb2::hid::TrueosHidCursorEvent) -> bool {
    matches!(event.hid_kind, 2 | 3)
}

fn note_cursor_event_source(state: &mut Ui2State, event: &crate::usb2::hid::TrueosHidCursorEvent) {
    if cursor_event_is_physical(event) {
        return;
    }
    state.non_physical_cursor_event_count = state.non_physical_cursor_event_count.wrapping_add(1);
    let count = state.non_physical_cursor_event_count;
    if should_log_input_diag(count) {
        crate::log!(
            "ui2: cursor-input-source non-physical count={} seq={} kind={} ctrl={} slot={} ep={} buttons=0x{:X} wheel={} flags=0x{:X}\n",
            count,
            event.seq,
            event.hid_kind,
            event.controller_id,
            event.slot_id,
            event.ep_target,
            event.buttons_down,
            event.wheel,
            event.flags
        );
    }
}

fn note_keyboard_event_source(
    state: &mut Ui2State,
    event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
) {
    if (event.flags & crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_SYNTHETIC) == 0 {
        return;
    }
    state.synthetic_keyboard_event_count = state.synthetic_keyboard_event_count.wrapping_add(1);
    let count = state.synthetic_keyboard_event_count;
    if should_log_input_diag(count) {
        crate::log!(
            "ui2: keyboard-input-source synthetic count={} seq={} dev_seq={} ctrl={} slot={} ep={} kind={} key_code={} codepoint={} flags=0x{:X}\n",
            count,
            event.seq,
            event.device_seq,
            event.controller_id,
            event.slot_id,
            event.ep_target,
            event.kind,
            event.key_code,
            event.codepoint,
            event.flags
        );
    }
}

fn cursor_index(state: &Ui2State, slot_id: u32) -> Option<usize> {
    state
        .cursors
        .iter()
        .position(|cursor| cursor.slot_id == slot_id)
}

fn ensure_cursor_index(state: &mut Ui2State, slot_id: u32) -> Option<usize> {
    if let Some(idx) = cursor_index(state, slot_id) {
        return Some(idx);
    }
    if state.cursors.len() >= UI2_CURSOR_CAP {
        let drop_count = UI2_CURSOR_CAP_DROP_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if drop_count <= 8 || drop_count.is_multiple_of(32) {
            crate::log!(
                "ui2: cursor-cap drop slot={} cap={} tracked={} count={}\n",
                slot_id,
                UI2_CURSOR_CAP,
                state.cursors.len(),
                drop_count
            );
        }
        return None;
    }
    state.cursors.push(Ui2CursorState {
        slot_id,
        ..Ui2CursorState::default()
    });
    Some(state.cursors.len() - 1)
}

pub(super) fn note_selection_change(window: &mut Ui2Window) {
    window.dirty = true;
    window.last_reason = "cursor-select";
}

fn set_cursor_selected_window(state: &mut Ui2State, slot_id: u32, next_window_id: u32) -> bool {
    let Some(cursor_idx) = ensure_cursor_index(state, slot_id) else {
        return false;
    };
    if state.cursors[cursor_idx].selected_window_id == next_window_id {
        return false;
    }

    let mut changed = false;
    let top_z = state
        .windows
        .iter()
        .map(|window| window.z)
        .max()
        .unwrap_or(0);
    let mut raised = false;
    for window in &mut state.windows {
        if let Some(pos) = window
            .selected_cursor_slots
            .iter()
            .position(|selected_slot_id| *selected_slot_id == slot_id)
        {
            window.selected_cursor_slots.remove(pos);
            note_selection_change(window);
            changed = true;
        }
    }

    if next_window_id != 0 {
        if let Some(window) = window_mut(state, next_window_id) {
            if !window
                .selected_cursor_slots
                .iter()
                .any(|selected_slot_id| *selected_slot_id == slot_id)
            {
                window.selected_cursor_slots.push(slot_id);
                note_selection_change(window);
                changed = true;
            }
            let next_z = top_z.saturating_add(1);
            if window.z != next_z {
                window.z = next_z;
                window.dirty = true;
                window.last_reason = "cursor-select";
                raised = true;
            }
        }
    }

    state.cursors[cursor_idx].selected_window_id = next_window_id;
    if changed || raised {
        state.compose_reason = "cursor-select";
        if raised && next_window_id != 0 {
            refresh_window_hit_entries(state, next_window_id);
        }
        UI2_DIRTY.store(true, Ordering::Release);
        crate::log!("ui2: cursor-select slot={} window={}\n", slot_id, next_window_id);
    }
    changed
}

fn process_cursor_event(state: &mut Ui2State, event: crate::usb2::hid::TrueosHidCursorEvent) {
    let slot_id = event.slot_id;
    if slot_id == 0 {
        return;
    }

    let Some((px, py)) = ui2_cursor_px_for_source(
        state.view_w,
        state.view_h,
        event.controller_id,
        slot_id,
        event.ep_target,
        event.hid_kind,
    ) else {
        return;
    };
    let press_hit = ui2_hit_for_cursor_source(
        state.view_w,
        state.view_h,
        event.controller_id,
        slot_id,
        event.ep_target,
        event.hid_kind,
    )
    .map(|(_, _, hit)| hit);
    let release_hit = ui2_hit_for_cursor_source(
        state.view_w,
        state.view_h,
        event.controller_id,
        slot_id,
        event.ep_target,
        event.hid_kind,
    )
    .map(|(_, _, hit)| hit);
    let press_system_button_action = press_hit.and_then(|target| {
        if target.kind == Ui2HitKind::WindowDecoration {
            system_button_action_at(state, target.owner_window_id, px, py)
        } else {
            None
        }
    });
    let press_window_id = if (event.buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0 {
        press_hit.map(|target| target.owner_window_id).unwrap_or(0)
    } else {
        0
    };
    let release_window_id = release_hit
        .map(|target| target.owner_window_id)
        .unwrap_or(0);
    let Some(cursor_idx) = ensure_cursor_index(state, slot_id) else {
        return;
    };

    let mut select_window_id: Option<u32> = None;
    let mut window_button_action: Option<(u32, Ui2SystemButtonAction)> = None;
    let mut begin_move_drag = false;
    let mut begin_resize_drag: Option<(u32, u32)> = None;
    let mut begin_scroll_drag = false;
    let mut begin_scroll_pan_window_id = 0u32;
    let mut click_candidate_window_id = 0u32;
    let mut click_candidate_item_id = 0u32;
    let mut click_press_x = 0.0f32;
    let mut click_press_y = 0.0f32;
    let mut try_offline_dock = false;
    {
        let cursor = &mut state.cursors[cursor_idx];
        let prev_buttons_down = cursor.buttons_down;
        let primary_was_down = (prev_buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0;
        let primary_is_down = (event.buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0;
        let middle_was_down = (prev_buttons_down & UI2_MIDDLE_BUTTON_MASK) != 0;
        let middle_is_down = (event.buttons_down & UI2_MIDDLE_BUTTON_MASK) != 0;

        cursor.x = px;
        cursor.y = py;
        cursor.buttons_down = event.buttons_down;

        if !primary_was_down && primary_is_down {
            cursor.press_x = px;
            cursor.press_y = py;
            cursor.press_window_id = press_window_id;
            cursor.press_item_id = press_hit
                .filter(|target| target.kind == Ui2HitKind::BrowserInteractive)
                .map(|target| target.item_id)
                .unwrap_or(0);
            cursor.press_armed = press_window_id != 0;
            begin_move_drag = true;
        } else if primary_was_down && !primary_is_down {
            if cursor.press_armed
                && cursor.press_window_id != 0
                && cursor.press_window_id == release_window_id
                && is_simple_click(cursor.press_x, cursor.press_y, px, py)
            {
                click_candidate_window_id = release_window_id;
                click_candidate_item_id = release_hit
                    .filter(|target| {
                        target.kind == Ui2HitKind::BrowserInteractive
                            && target.item_id != 0
                            && target.item_id == cursor.press_item_id
                    })
                    .map(|target| target.item_id)
                    .unwrap_or(0);
                click_press_x = cursor.press_x;
                click_press_y = cursor.press_y;
            } else if cursor.press_window_id == 0
                && release_window_id == 0
                && is_simple_click(cursor.press_x, cursor.press_y, px, py)
            {
                try_offline_dock = true;
            }
            cursor.press_armed = false;
            cursor.press_window_id = 0;
            cursor.press_item_id = 0;
        }

        if !middle_was_down
            && middle_is_down
            && let Some(target) = press_hit
            && matches!(target.kind, Ui2HitKind::WindowBody | Ui2HitKind::BrowserInteractive)
        {
            begin_scroll_pan_window_id = target.owner_window_id;
        }
    }

    // Deferred offline dock click (after cursor borrow is released).
    if try_offline_dock {
        if !ui2_win_register::handle_offline_dock_click(state, px, py) {
            select_window_id = Some(0);
        } else {
            UI2_DIRTY.store(true, Ordering::Release);
        }
    }

    if begin_move_drag && let Some(target) = press_hit {
        match target.kind {
            Ui2HitKind::WindowResizeButton => {
                begin_resize_drag = Some((
                    target.owner_window_id,
                    UI2_WINDOW_RESIZE_RIGHT | UI2_WINDOW_RESIZE_BOTTOM,
                ));
            }
            Ui2HitKind::WindowVerticalScrollbar => {
                begin_scroll_drag = true;
            }
            Ui2HitKind::WindowHorizontalScrollbar => {}
            Ui2HitKind::WindowDecoration => {
                if press_system_button_action.is_none() {
                    let _ =
                        begin_move_drag_for_cursor(state, slot_id, target.owner_window_id, px, py);
                }
            }
            Ui2HitKind::WindowBody | Ui2HitKind::BrowserInteractive => {}
        }
    }
    if let Some((window_id, edge_mask)) = begin_resize_drag {
        let _ = begin_window_resize_for_cursor(state, slot_id, window_id, edge_mask);
    }
    if begin_scroll_drag {
        let _ = begin_vertical_scroll_drag_for_cursor(state, slot_id, px, py);
    }
    if begin_scroll_pan_window_id != 0 {
        let _ =
            begin_window_scroll_pan_for_cursor(state, slot_id, begin_scroll_pan_window_id, px, py);
        let _ = set_cursor_selected_window(state, slot_id, begin_scroll_pan_window_id);
    }

    update_move_drag_for_cursor(state, slot_id, px, py, event.buttons_down);
    update_resize_drag_for_cursor(state, slot_id, px, py, event.buttons_down);
    let _ = update_scroll_drag_for_cursor(state, slot_id, py, event.buttons_down);
    let _ = update_scroll_pan_for_cursor(state, slot_id, px, py, event.buttons_down);

    if click_candidate_window_id != 0 {
        let press_action =
            system_button_action_at(state, click_candidate_window_id, click_press_x, click_press_y);
        let release_action = system_button_action_at(state, click_candidate_window_id, px, py);
        if let (Some(press_action), Some(release_action)) = (press_action, release_action) {
            if press_action == release_action {
                window_button_action = Some((click_candidate_window_id, release_action));
            } else {
                select_window_id = Some(click_candidate_window_id);
            }
        } else {
            select_window_id = Some(click_candidate_window_id);
        }
        if click_candidate_item_id != 0 {
            let _ =
                note_window_item_click(state, click_candidate_window_id, click_candidate_item_id);
        }
    }

    if let Some(select_window_id) = select_window_id {
        let _ = set_cursor_selected_window(state, slot_id, select_window_id);
    }
    if let Some((window_id, action)) = window_button_action {
        let _ = handle_system_button_action(state, window_id, action);
    }

    if event.wheel != 0 {
        let selected_window_id = state.cursors[cursor_idx].selected_window_id;
        state.compose_reason = "wheel-input";
        UI2_DIRTY.store(true, Ordering::Release);
        let _ = forward_cursor_wheel_to_selected_window(state, selected_window_id, event.wheel);
    }
}

fn forward_cursor_wheel_to_selected_window(
    state: &mut Ui2State,
    window_id: u32,
    wheel: i16,
) -> bool {
    if window_id == 0 || wheel == 0 {
        return false;
    }
    let Some(window) = state.windows.iter().find(|window| window.id == window_id) else {
        return false;
    };
    if !window_content_participates_in_composition(window) {
        return false;
    }
    match window.kind {
        Ui2WindowKind::HostedBrowser => {
            let content_id = window_browser_instance_id(window);
            let snapshot = browser_surface_state_for_window(window);
            let scroll_delta = -(wheel as i32) * UI2_WHEEL_SCROLL_STEP_PX;
            let next_scroll = clamp_hosted_browser_scroll(
                &snapshot,
                i64::from(normalized_hosted_browser_scroll(&snapshot))
                    .saturating_add(i64::from(scroll_delta)),
            );
            if hosted_set_scroll_y(content_id, next_scroll) {
                state.compose_reason = "wheel-scroll";
                true
            } else {
                false
            }
        }
        Ui2WindowKind::HostedSurface => {
            let content_id = window_hosted_content_id(window);
            if content_id == 0 {
                return false;
            }
            let snapshot = hosted_surface_state_for_window(window);
            let scroll_delta = -(wheel as i32) * UI2_WHEEL_SCROLL_STEP_PX;
            let next_scroll = clamp_hosted_browser_scroll(
                &snapshot,
                i64::from(normalized_hosted_browser_scroll(&snapshot))
                    .saturating_add(i64::from(scroll_delta)),
            );
            if hosted_set_scroll_y(content_id, next_scroll) {
                state.compose_reason = "wheel-scroll";
                true
            } else {
                false
            }
        }
        Ui2WindowKind::Hosted3d => false,
    }
}

pub(super) fn pump_cursor_selection(state: &mut Ui2State) {
    let mut events = [crate::usb2::hid::TrueosHidCursorEvent::default(); UI2_CURSOR_EVENT_BATCH];
    loop {
        let (next_seq, dropped, wrote) =
            crate::usb2::hid::read_cursor_events_since(state.cursor_read_seq, &mut events);
        if dropped != 0 {
            crate::log!(
                "ui2: cursor-event-drop read_seq={} dropped={}\n",
                state.cursor_read_seq,
                dropped
            );
        }
        if wrote == 0 {
            break;
        }
        state.cursor_read_seq = next_seq;
        for event in events.iter().take(wrote) {
            note_cursor_event_source(state, event);
            process_cursor_event(state, *event);
        }
        if wrote < events.len() {
            break;
        }
    }
}

fn selected_window_id_for_keyboard(state: &Ui2State) -> Option<u32> {
    for idx in sorted_window_indices(state).into_iter().rev() {
        let window = &state.windows[idx];
        if !window.visible
            || window.composition_locked
            || window.state == Ui2WindowStateKind::Minimized
            || window.selected_cursor_slots.is_empty()
        {
            continue;
        }
        return Some(window.id);
    }
    None
}

fn selected_hosted_surface_window_ids_for_keyboard(state: &Ui2State) -> Vec<u32> {
    let mut out = Vec::new();
    for idx in sorted_window_indices(state).into_iter().rev() {
        let window = &state.windows[idx];
        if !window.visible
            || window.composition_locked
            || window.state == Ui2WindowStateKind::Minimized
            || window.selected_cursor_slots.is_empty()
            || window.kind != Ui2WindowKind::HostedSurface
        {
            continue;
        }
        out.push(window.id);
    }
    out
}

fn keyboard_output_modifiers_to_browser_mask(modifiers: u8) -> u8 {
    let mut out = 0u8;
    if (modifiers & ((1 << 1) | (1 << 5))) != 0 {
        out |= HOSTED_KEYBOARD_MOD_SHIFT;
    }
    if (modifiers & ((1 << 0) | (1 << 4))) != 0 {
        out |= HOSTED_KEYBOARD_MOD_CTRL;
    }
    if (modifiers & ((1 << 2) | (1 << 6))) != 0 {
        out |= HOSTED_KEYBOARD_MOD_ALT;
    }
    if (modifiers & ((1 << 3) | (1 << 7))) != 0 {
        out |= HOSTED_KEYBOARD_MOD_META;
    }
    out
}

fn keyboard_output_key_name(
    event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> Option<String> {
    let named = match event.key_code {
        crate::r::keyboard::KEYBOARD_KEY_BACKSPACE => Some("Backspace"),
        crate::r::keyboard::KEYBOARD_KEY_TAB => Some("Tab"),
        crate::r::keyboard::KEYBOARD_KEY_ENTER => Some("Enter"),
        crate::r::keyboard::KEYBOARD_KEY_ESCAPE => Some("Escape"),
        crate::r::keyboard::KEYBOARD_KEY_SPACE => Some("Space"),
        crate::r::keyboard::KEYBOARD_KEY_DELETE => Some("Delete"),
        crate::r::keyboard::KEYBOARD_KEY_INSERT => Some("Insert"),
        crate::r::keyboard::KEYBOARD_KEY_HOME => Some("Home"),
        crate::r::keyboard::KEYBOARD_KEY_END => Some("End"),
        crate::r::keyboard::KEYBOARD_KEY_PAGE_UP => Some("PageUp"),
        crate::r::keyboard::KEYBOARD_KEY_PAGE_DOWN => Some("PageDown"),
        crate::r::keyboard::KEYBOARD_KEY_ARROW_UP => Some("ArrowUp"),
        crate::r::keyboard::KEYBOARD_KEY_ARROW_DOWN => Some("ArrowDown"),
        crate::r::keyboard::KEYBOARD_KEY_ARROW_LEFT => Some("ArrowLeft"),
        crate::r::keyboard::KEYBOARD_KEY_ARROW_RIGHT => Some("ArrowRight"),
        crate::r::keyboard::KEYBOARD_KEY_F1 => Some("F1"),
        crate::r::keyboard::KEYBOARD_KEY_F2 => Some("F2"),
        crate::r::keyboard::KEYBOARD_KEY_F3 => Some("F3"),
        crate::r::keyboard::KEYBOARD_KEY_F4 => Some("F4"),
        crate::r::keyboard::KEYBOARD_KEY_F5 => Some("F5"),
        crate::r::keyboard::KEYBOARD_KEY_F6 => Some("F6"),
        crate::r::keyboard::KEYBOARD_KEY_F7 => Some("F7"),
        crate::r::keyboard::KEYBOARD_KEY_F8 => Some("F8"),
        crate::r::keyboard::KEYBOARD_KEY_F9 => Some("F9"),
        crate::r::keyboard::KEYBOARD_KEY_F10 => Some("F10"),
        crate::r::keyboard::KEYBOARD_KEY_F11 => Some("F11"),
        crate::r::keyboard::KEYBOARD_KEY_F12 => Some("F12"),
        _ => None,
    };
    if let Some(name) = named {
        return Some(String::from(name));
    }
    char::from_u32(event.codepoint).map(|ch| {
        let mut value = String::new();
        value.push(ch);
        value
    })
}

fn browser_keyboard_event_from_output(
    event: crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> Option<UiHostedKeyboardEvent> {
    match event.kind {
        crate::r::keyboard::KEYBOARD_OUTPUT_KIND_TEXT => {
            let utf8_len = (event.utf8_len as usize).min(event.utf8.len());
            if utf8_len == 0 {
                return char::from_u32(event.codepoint).map(|ch| {
                    let mut text = String::new();
                    text.push(ch);
                    UiHostedKeyboardEvent::Text { text }
                });
            }
            let text = core::str::from_utf8(&event.utf8[..utf8_len]).ok()?;
            if text.is_empty() {
                return None;
            }
            Some(UiHostedKeyboardEvent::Text {
                text: String::from(text),
            })
        }
        crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY => {
            let key = keyboard_output_key_name(&event)?;
            Some(UiHostedKeyboardEvent::Key {
                key,
                modifiers: keyboard_output_modifiers_to_browser_mask(event.modifiers),
            })
        }
        _ => None,
    }
}

pub(super) fn pump_keyboard_input(state: &mut Ui2State) {
    let selected_window_id = selected_window_id_for_keyboard(state);
    let mut raw_events =
        [crate::r::keyboard::TrueosKeyboardOutputEvent::default(); UI2_KEYBOARD_EVENT_BATCH];
    loop {
        let (next_seq, dropped, wrote) =
            crate::r::keyboard::read_output_events_since(state.keyboard_read_seq, &mut raw_events);
        if dropped != 0 {
            crate::log!(
                "ui2: keyboard-event-drop read_seq={} dropped={}\n",
                state.keyboard_read_seq,
                dropped
            );
        }
        if wrote == 0 {
            break;
        }
        state.keyboard_read_seq = next_seq;

        if let Some(window_id) = selected_window_id {
            let window_kind = state
                .windows
                .iter()
                .find(|window| window.id == window_id)
                .map(|window| window.kind);
            match window_kind {
                Some(Ui2WindowKind::HostedBrowser) => {
                    let mut events = Vec::new();
                    for event in raw_events.iter().take(wrote).copied() {
                        note_keyboard_event_source(state, &event);
                        if let Some(next) = browser_keyboard_event_from_output(event) {
                            events.push(next);
                        }
                    }
                    let content_id = state
                        .windows
                        .iter()
                        .find(|window| window.id == window_id)
                        .map(window_browser_instance_id)
                        .unwrap_or(0);
                    if !events.is_empty()
                        && !hosted_queue_keyboard_events(content_id, events.as_slice())
                    {
                        crate::log!(
                            "ui2: keyboard-forward-drop window={} count={}\n",
                            window_id,
                            events.len()
                        );
                    }
                }
                Some(Ui2WindowKind::HostedSurface) => {
                    let selected_surface_window_ids =
                        selected_hosted_surface_window_ids_for_keyboard(state);
                    for event in raw_events.iter().take(wrote).copied() {
                        note_keyboard_event_source(state, &event);
                        for target_window_id in selected_surface_window_ids.iter().copied() {
                            if crate::shell2::queue_ui2_shell_keyboard_event(
                                target_window_id,
                                event,
                            ) {
                                continue;
                            }
                            let _ = crate::tst_gfx_tetris::queue_ui2_keyboard_event(
                                target_window_id,
                                event,
                            );
                        }
                    }
                }
                Some(Ui2WindowKind::Hosted3d) => {
                    for event in raw_events.iter().take(wrote) {
                        note_keyboard_event_source(state, event);
                    }
                }
                None => {}
            }
        } else {
            for event in raw_events.iter().take(wrote) {
                note_keyboard_event_source(state, event);
            }
        }

        if wrote < raw_events.len() {
            break;
        }
    }
}

#[inline]
fn window_uses_live_resize(kind: Ui2WindowKind) -> bool {
    !matches!(kind, Ui2WindowKind::HostedBrowser)
}

pub(super) fn pick_drag_cursor_slot(state: &Ui2State, window: &Ui2Window) -> Option<u32> {
    for slot_id in &window.selected_cursor_slots {
        if let Some(cursor) = state
            .cursors
            .iter()
            .find(|cursor| cursor.slot_id == *slot_id)
            && (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0
        {
            return Some(*slot_id);
        }
    }
    for cursor in &state.cursors {
        if cursor.selected_window_id == window.id
            && (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0
        {
            return Some(cursor.slot_id);
        }
    }
    for cursor in &state.cursors {
        if (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
            continue;
        }
        if rect_contains_point(window.rect, cursor.x, cursor.y) {
            return Some(cursor.slot_id);
        }
    }
    None
}

fn begin_move_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    window_id: u32,
    cursor_x: f32,
    cursor_y: f32,
) -> bool {
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == window_id && window_is_renderable(window))
        .cloned()
    else {
        return false;
    };
    let top_z = state
        .windows
        .iter()
        .map(|window| window.z)
        .max()
        .unwrap_or(0);
    let mut next_rect = window.rect;
    if window.state == Ui2WindowStateKind::Maximized {
        let view_w = (state.view_w as f32).max(1.0);
        let view_h = (state.view_h as f32).max(1.0);
        let restored = if window.restore_rect.w > 0.0 && window.restore_rect.h > 0.0 {
            normalized_window_rect_for_window_for_view(
                state.view_w,
                state.view_h,
                &window,
                window.restore_rect,
            )
        } else {
            Ui2Rect::new(0.0, 0.0, (view_w * 0.75).max(1.0), (view_h * 0.75).max(1.0))
        };
        let cursor_ratio_x = ((cursor_x - window.rect.x) / window.rect.w.max(1.0)).clamp(0.0, 1.0);
        let grab_dy =
            (cursor_y - window.rect.y).clamp(0.0, UI2_TITLE_H.min(restored.h).max(1.0) - 1.0);
        next_rect = restored;
        next_rect.x =
            (cursor_x - (next_rect.w * cursor_ratio_x)).clamp(0.0, (view_w - next_rect.w).max(0.0));
        next_rect.y = (cursor_y - grab_dy).clamp(0.0, (view_h - next_rect.h).max(0.0));
    }
    if let Some(window_mut) = window_mut(state, window_id) {
        if window.state == Ui2WindowStateKind::Maximized {
            window_mut.rect = next_rect;
            window_mut.restore_rect = next_rect;
            window_mut.state = Ui2WindowStateKind::Normal;
        }
        window_mut.z = top_z.saturating_add(1);
    }
    let _ = set_cursor_selected_window(state, slot_id, window_id);
    let _ = note_window_dirty(state, window_id, "begin-window-move");
    if window.state == Ui2WindowStateKind::Maximized {
        let _ = note_window_viewport_sync_needed(state, window_id);
    }
    clear_window_drag_claims(state, window_id);
    clear_other_drag_modes_for_slot(state, slot_id);
    upsert_move_drag(
        state,
        Ui2WindowMoveDrag {
            active: true,
            window_id,
            cursor_slot_id: slot_id,
            grab_dx: cursor_x - next_rect.x,
            grab_dy: cursor_y - next_rect.y,
            edge_actions_armed: window_edge_drop_action(state, cursor_x, cursor_y).is_none(),
        },
    );
    state.compose_reason = "begin-window-move";
    refresh_window_hit_entries(state, window_id);
    true
}

pub(super) fn begin_window_resize_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    window_id: u32,
    edge_mask: u32,
) -> bool {
    let edge_mask = edge_mask
        & (UI2_WINDOW_RESIZE_LEFT
            | UI2_WINDOW_RESIZE_TOP
            | UI2_WINDOW_RESIZE_RIGHT
            | UI2_WINDOW_RESIZE_BOTTOM);
    if !is_valid_resize_edge_mask(edge_mask) {
        return false;
    }
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == window_id && window_is_renderable(window))
        .cloned()
    else {
        return false;
    };
    if window.state != Ui2WindowStateKind::Normal {
        return false;
    }
    let Some(cursor) = state
        .cursors
        .iter()
        .find(|cursor| cursor.slot_id == slot_id)
        .copied()
    else {
        return false;
    };
    if (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        return false;
    }

    clear_window_drag_claims(state, window_id);
    clear_other_drag_modes_for_slot(state, slot_id);
    upsert_resize_drag(
        state,
        Ui2WindowResizeDrag {
            active: true,
            window_id,
            cursor_slot_id: slot_id,
            live_apply: window_uses_live_resize(window.kind),
            edge_mask,
            start_cursor_x: cursor.x,
            start_cursor_y: cursor.y,
            start_rect: window.rect,
            preview_rect: window.rect,
        },
    );
    state.compose_reason = "begin-window-resize";
    let top_z = state
        .windows
        .iter()
        .map(|window| window.z)
        .max()
        .unwrap_or(0);
    if let Some(window_mut) = window_mut(state, window_id) {
        window_mut.z = top_z.saturating_add(1);
    }
    let noted = note_window_dirty(state, window_id, "begin-window-resize");
    if noted {
        refresh_window_hit_entries(state, window_id);
    }
    noted
}

fn browser_vertical_scrollbar_metrics(
    state: &Ui2State,
    window: &Ui2Window,
) -> Option<(Ui2Rect, f32, f32, u32)> {
    let Some(snapshot) = window_scroll_snapshot(window) else {
        return None;
    };
    let track = window_vertical_scrollbar_rect(state, window)?;
    let viewport_h = snapshot.viewport_height.max(1);
    let content_h = snapshot.content_height.max(viewport_h);
    let scroll_range = hosted_browser_scroll_max(&snapshot);
    let thumb_h =
        libm::fmaxf(10.0, (track.h * (viewport_h as f32 / content_h as f32)).min(track.h));
    let thumb_y = if scroll_range > 0 {
        let avail = (track.h - thumb_h).max(0.0);
        track.y
            + (avail * (normalized_hosted_browser_scroll(&snapshot) as f32 / scroll_range as f32))
    } else {
        track.y
    };
    Some((track, thumb_h, thumb_y, scroll_range))
}

fn begin_vertical_scroll_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_x: f32,
    cursor_y: f32,
) -> bool {
    let Some(window_id) = state
        .cursors
        .iter()
        .find(|cursor| cursor.slot_id == slot_id)
        .map(|cursor| cursor.press_window_id)
        .filter(|id| *id != 0)
    else {
        return false;
    };
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == window_id && window_is_renderable(window))
    else {
        return false;
    };
    let Some((track, thumb_h, thumb_y, _)) = browser_vertical_scrollbar_metrics(state, window)
    else {
        return false;
    };
    let thumb_rect = Ui2Rect::new(track.x, thumb_y, track.w, thumb_h);
    let grab_offset = if rect_contains_point(thumb_rect, cursor_x, cursor_y) {
        (cursor_y - thumb_y).clamp(0.0, thumb_h)
    } else {
        thumb_h * 0.5
    };
    clear_window_drag_claims(state, window_id);
    clear_other_drag_modes_for_slot(state, slot_id);
    upsert_scroll_drag(
        state,
        Ui2WindowScrollDrag {
            active: true,
            window_id,
            cursor_slot_id: slot_id,
            track_rect: track,
            thumb_extent: thumb_h,
            grab_offset,
        },
    );
    update_scroll_drag_for_cursor(state, slot_id, cursor_y, UI2_PRIMARY_BUTTON_MASK)
}

fn begin_window_scroll_pan_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    window_id: u32,
    cursor_x: f32,
    cursor_y: f32,
) -> bool {
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == window_id && window_is_renderable(window))
    else {
        return false;
    };
    if window_scroll_snapshot(window).is_none() {
        return false;
    }
    clear_window_drag_claims(state, window_id);
    clear_other_drag_modes_for_slot(state, slot_id);
    upsert_scroll_pan_drag(
        state,
        Ui2WindowScrollPanDrag {
            active: true,
            window_id,
            cursor_slot_id: slot_id,
            last_cursor_x: cursor_x,
            last_cursor_y: cursor_y,
        },
    );
    state.compose_reason = "begin-scroll-pan";
    true
}

fn update_scroll_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_y: f32,
    buttons_down: u32,
) -> bool {
    let Some(drag_idx) = state
        .scroll_drags
        .iter()
        .position(|drag| drag.active && drag.cursor_slot_id == slot_id)
    else {
        return false;
    };
    if (buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        state.scroll_drags.remove(drag_idx);
        return false;
    }
    let drag = state.scroll_drags[drag_idx];
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == drag.window_id && window_is_renderable(window))
    else {
        state.scroll_drags.remove(drag_idx);
        return false;
    };
    let Some((track, _thumb_h, _thumb_y, scroll_range)) =
        browser_vertical_scrollbar_metrics(state, window)
    else {
        state.scroll_drags.remove(drag_idx);
        return false;
    };
    state.scroll_drags[drag_idx].track_rect = track;
    if scroll_range == 0 {
        return false;
    }
    let avail = (track.h - state.scroll_drags[drag_idx].thumb_extent).max(0.0);
    if avail <= 0.0 {
        return false;
    }
    let thumb_y =
        (cursor_y - state.scroll_drags[drag_idx].grab_offset).clamp(track.y, track.y + avail);
    let ratio = ((thumb_y - track.y) / avail).clamp(0.0, 1.0);
    let snapshot = hosted_surface_state_for_window(window);
    let next_scroll =
        clamp_hosted_browser_scroll(&snapshot, libm::roundf(ratio * scroll_range as f32) as i64);
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == drag.window_id)
    else {
        return false;
    };
    let content_id = window_hosted_content_id(window);
    if content_id == 0 {
        return false;
    }
    if hosted_set_scroll_y(content_id, next_scroll) {
        state.compose_reason = "scrollbar-drag";
        true
    } else {
        false
    }
}

fn update_scroll_pan_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_x: f32,
    cursor_y: f32,
    buttons_down: u32,
) -> bool {
    let Some(drag_idx) = state
        .scroll_pan_drags
        .iter()
        .position(|drag| drag.active && drag.cursor_slot_id == slot_id)
    else {
        return false;
    };
    if (buttons_down & UI2_MIDDLE_BUTTON_MASK) == 0 {
        state.scroll_pan_drags.remove(drag_idx);
        return false;
    }
    let drag = state.scroll_pan_drags[drag_idx];
    let Some(window) = state.windows.iter().find(|window| {
        window.id == drag.window_id && window_content_participates_in_composition(window)
    }) else {
        state.scroll_pan_drags.remove(drag_idx);
        return false;
    };
    if window_scroll_snapshot(window).is_none() {
        state.scroll_pan_drags.remove(drag_idx);
        return false;
    }

    let dx = cursor_x - drag.last_cursor_x;
    let dy = cursor_y - drag.last_cursor_y;
    state.scroll_pan_drags[drag_idx].last_cursor_x = cursor_x;
    state.scroll_pan_drags[drag_idx].last_cursor_y = cursor_y;

    let dx_px = libm::roundf(dx) as i32;
    let dy_px = libm::roundf(dy) as i32;
    if dx_px == 0 && dy_px == 0 {
        return false;
    }

    let snapshot = hosted_surface_state_for_window(window);
    let next_scroll_x = clamp_hosted_browser_scroll_x(
        &snapshot,
        i64::from(normalized_hosted_browser_scroll_x(&snapshot)).saturating_sub(i64::from(dx_px)),
    );
    let next_scroll_y = clamp_hosted_browser_scroll(
        &snapshot,
        i64::from(normalized_hosted_browser_scroll(&snapshot)).saturating_sub(i64::from(dy_px)),
    );
    let content_id = window_hosted_content_id(window);
    if content_id == 0 {
        return false;
    }
    if hosted_set_scroll(content_id, next_scroll_x, next_scroll_y) {
        state.compose_reason = "scroll-pan";
        true
    } else {
        false
    }
}

fn update_move_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_x: f32,
    cursor_y: f32,
    buttons_down: u32,
) {
    let Some(drag_idx) = state
        .move_drags
        .iter()
        .position(|drag| drag.active && drag.cursor_slot_id == slot_id)
    else {
        return;
    };
    if (buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        state.move_drags.remove(drag_idx);
        return;
    }
    let edge_action = window_edge_drop_action(state, cursor_x, cursor_y);
    if !state.move_drags[drag_idx].edge_actions_armed {
        if edge_action.is_none() {
            state.move_drags[drag_idx].edge_actions_armed = true;
        }
    } else if let Some(action) = edge_action {
        let window_id = state.move_drags[drag_idx].window_id;
        state.move_drags.remove(drag_idx);
        let _ = apply_window_edge_drop_action(state, window_id, action);
        return;
    }
    let next_x = cursor_x - state.move_drags[drag_idx].grab_dx;
    let next_y = cursor_y - state.move_drags[drag_idx].grab_dy;
    let window_id = state.move_drags[drag_idx].window_id;
    let Some(window) = window_mut(state, window_id) else {
        state.move_drags.remove(drag_idx);
        return;
    };
    let mut moved = false;
    if window.rect.x != next_x || window.rect.y != next_y {
        window.rect.x = next_x;
        window.rect.y = next_y;
        window.restore_rect = window.rect;
        moved = true;
    }
    if moved {
        state.compose_reason = "window-drag";
        let _ = note_window_dirty(state, window_id, "window-drag");
        let _ = note_window_viewport_sync_needed(state, window_id);
        refresh_window_hit_entries(state, window_id);
    }
}

fn update_resize_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_x: f32,
    cursor_y: f32,
    buttons_down: u32,
) {
    let Some(drag_idx) = state
        .resize_drags
        .iter()
        .position(|drag| drag.active && drag.cursor_slot_id == slot_id)
    else {
        return;
    };
    if (buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        let drag = state.resize_drags[drag_idx];
        if drag.active && !drag.live_apply && drag.preview_rect != drag.start_rect {
            if let Some(window) = window_mut(state, drag.window_id) {
                window.rect = drag.preview_rect;
                window.restore_rect = drag.preview_rect;
            }
            state.compose_reason = "window-resize-commit";
            let _ = commit_window_geometry_change(state, drag.window_id, "window-resize-commit");
        } else if drag.active && !drag.live_apply {
            state.compose_reason = "window-resize-cancel";
            let _ = note_window_dirty(state, drag.window_id, "window-resize-cancel");
        }
        state.resize_drags.remove(drag_idx);
        return;
    }

    let drag = state.resize_drags[drag_idx];
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == drag.window_id)
    else {
        state.resize_drags.remove(drag_idx);
        return;
    };
    if window.state == Ui2WindowStateKind::Maximized {
        state.resize_drags.remove(drag_idx);
        return;
    }

    let mut next = drag.start_rect;
    let (min_w, min_h) = ui2_window_min_size(window);
    let dx = cursor_x - drag.start_cursor_x;
    let dy = cursor_y - drag.start_cursor_y;
    let right = drag.start_rect.x + drag.start_rect.w;
    let bottom = drag.start_rect.y + drag.start_rect.h;

    if (drag.edge_mask & UI2_WINDOW_RESIZE_LEFT) != 0 {
        let max_x = right - min_w;
        next.x = libm::fminf(drag.start_rect.x + dx, max_x);
        next.w = (right - next.x).max(min_w);
    } else if (drag.edge_mask & UI2_WINDOW_RESIZE_RIGHT) != 0 {
        next.w = (drag.start_rect.w + dx).max(min_w);
    }

    if (drag.edge_mask & UI2_WINDOW_RESIZE_TOP) != 0 {
        let max_y = bottom - min_h;
        next.y = libm::fminf(drag.start_rect.y + dy, max_y);
        next.h = (bottom - next.y).max(min_h);
    } else if (drag.edge_mask & UI2_WINDOW_RESIZE_BOTTOM) != 0 {
        next.h = (drag.start_rect.h + dy).max(min_h);
    }

    if drag.live_apply {
        if window.rect != next {
            if let Some(window) = window_mut(state, drag.window_id) {
                window.rect = next;
                window.restore_rect = next;
            }
            state.compose_reason = "window-resize-drag";
            let _ = note_window_dirty(state, drag.window_id, "window-resize-drag");
            let _ = note_window_viewport_sync_needed(state, drag.window_id);
            refresh_window_hit_entries(state, drag.window_id);
        }
    } else if drag.preview_rect != next {
        state.resize_drags[drag_idx].preview_rect = next;
        state.compose_reason = "window-resize-preview";
        let _ = note_window_dirty(state, drag.window_id, "window-resize-preview");
    }
}
