//! Spirit-owned software cursor for Lilly.
//!
//! Registration and motion still pass through the one mediated vCursor
//! station. This module only retains Lilly's single capability and authors her
//! first bounded choreography after Spirit's hardware CUR_POS move is proven.

use spin::Mutex;

use crate::graphics::primitives::Rgba8;
use crate::r::mouse_motion_service::{
    MOUSE_CONTROL_EASING_LINEAR, MOUSE_CONTROL_EASING_NATURAL, MOUSE_CONTROL_FLAG_CLEAR_QUEUE,
    MOUSE_CONTROL_OPCODE_BUTTONS, MOUSE_CONTROL_OPCODE_STROKE, MOUSE_CONTROL_PATH_LINE,
    MOUSE_CONTROL_PATH_QUADRATIC, MouseControlCommand, MouseControlCursor, MouseControlError,
    MouseControlPrincipal,
};

const LILLY_CURSOR_LABEL: &str = "Spirit/Lilly";
const LILLY_HUT_COMBO_ID: u32 = 0x4C49_4C59;
const LILLY_CURSOR_COLOR: Rgba8 = Rgba8::new(255, 55, 255, 255);
const PRIMARY_BUTTON_MASK: u32 = 1 << 0;
const LILLY_INITIAL_APPROACH_MS: u32 = 420;
const LILLY_OUTLINE_EDGE_MS: u32 = 240;
const LILLY_WINDOW_APPROACH_MIN_MS: u32 = 260;
const LILLY_WINDOW_APPROACH_MAX_MS: u32 = 900;
const LILLY_DOBBY_MOVE_MIN_MS: u32 = 80;
const LILLY_DOBBY_MOVE_MAX_MS: u32 = 640;

struct LillyCursorState {
    cursor: Option<MouseControlCursor>,
    initial_outline_queued: bool,
}

impl LillyCursorState {
    const fn new() -> Self {
        Self {
            cursor: None,
            initial_outline_queued: false,
        }
    }

    fn ensure_cursor(&mut self) -> Result<MouseControlCursor, MouseControlError> {
        if let Some(cursor) = self.cursor {
            return Ok(cursor);
        }
        let cursor = crate::r::mouse_motion_service::request_cursor(
            MouseControlPrincipal::Kernel,
            LILLY_CURSOR_LABEL,
            Some(LILLY_CURSOR_COLOR),
        )?;
        self.cursor = Some(cursor);
        crate::log_info!(
            target: "gfx";
            "trueos-spirit: Lilly vcursor registered tag={} handle={} slot={} color={},{},{} owner=Spirit cardinality=one presentation=ui4-slot4\n",
            LILLY_CURSOR_LABEL,
            cursor.handle,
            cursor.slot_id,
            LILLY_CURSOR_COLOR.r,
            LILLY_CURSOR_COLOR.g,
            LILLY_CURSOR_COLOR.b,
        );
        Ok(cursor)
    }
}

static LILLY_CURSOR: Mutex<LillyCursorState> = Mutex::new(LillyCursorState::new());

pub(super) fn register_once() -> Result<MouseControlCursor, MouseControlError> {
    LILLY_CURSOR.lock().ensure_cursor()
}

pub(super) fn selection_source() -> Result<crate::ui4::Ui4CursorSource, MouseControlError> {
    let cursor = register_once()?;
    Ok(crate::ui4::Ui4CursorSource {
        controller_id: 0,
        slot_id: cursor.slot_id,
        ep_target: 0,
        hid_kind: crate::r::cursor::HID_KIND_VIRTUAL_CURSOR,
    })
}

/// Replace Lilly's pending software-cursor choreography with one natural
/// quadratic flight to the top-left of a UI4 frame. This never touches the
/// Intel hardware cursor position owned by Spirit.
pub(super) fn queue_window_approach(
    target_x: i32,
    target_y: i32,
    screen_width: u32,
    screen_height: u32,
) -> Result<u32, MouseControlError> {
    if screen_width == 0 || screen_height == 0 {
        return Err(MouseControlError::Invalid);
    }
    let cursor = {
        let mut state = LILLY_CURSOR.lock();
        let cursor = state.ensure_cursor()?;
        // A real UI4 target supersedes the one-time boot outline. Prevent a
        // delayed first hardware Spirit move from queuing that outline after
        // Lilly has already selected an application frame.
        state.initial_outline_queued = true;
        cursor
    };
    let (from_x, from_y) = crate::r::mouse_motion_service::cursor_position(
        MouseControlPrincipal::Kernel,
        cursor.handle,
    )?;
    let screen_last_x = i64::from(screen_width.saturating_sub(1)).min(i64::from(i32::MAX));
    let screen_last_y = i64::from(screen_height.saturating_sub(1)).min(i64::from(i32::MAX));
    let target_x = i64::from(target_x).clamp(0, screen_last_x);
    let target_y = i64::from(target_y).clamp(0, screen_last_y);
    let from_x64 = i64::from(from_x);
    let from_y64 = i64::from(from_y);
    let dx = target_x.saturating_sub(from_x64);
    let dy = target_y.saturating_sub(from_y64);
    let distance = dx.unsigned_abs().max(dy.unsigned_abs());
    let duration_ms = u32::try_from(distance / 2)
        .unwrap_or(LILLY_WINDOW_APPROACH_MAX_MS)
        .clamp(LILLY_WINDOW_APPROACH_MIN_MS, LILLY_WINDOW_APPROACH_MAX_MS);

    // A small perpendicular offset produces an unmistakable curve without
    // requiring any additional path segments or state.
    let denominator = dx.unsigned_abs().saturating_add(dy.unsigned_abs()).max(1);
    let bend = (distance / 5).clamp(24, 120);
    let midpoint_x = from_x64.saturating_add(dx / 2);
    let midpoint_y = from_y64.saturating_add(dy / 2);
    let control_x = midpoint_x
        .saturating_sub(
            dy.saturating_mul(i64::try_from(bend).unwrap_or(120))
                / i64::try_from(denominator).unwrap_or(i64::MAX),
        )
        .clamp(0, screen_last_x) as i32;
    let control_y = midpoint_y
        .saturating_add(
            dx.saturating_mul(i64::try_from(bend).unwrap_or(120))
                / i64::try_from(denominator).unwrap_or(i64::MAX),
        )
        .clamp(0, screen_last_y) as i32;
    let command = MouseControlCommand {
        opcode: MOUSE_CONTROL_OPCODE_STROKE,
        path: MOUSE_CONTROL_PATH_QUADRATIC,
        easing: MOUSE_CONTROL_EASING_NATURAL,
        flags: MOUSE_CONTROL_FLAG_CLEAR_QUEUE,
        duration_ms,
        x: target_x as i32,
        y: target_y as i32,
        control1_x: control_x,
        control1_y: control_y,
        ..MouseControlCommand::default()
    };
    crate::r::mouse_motion_service::submit_command(
        MouseControlPrincipal::Kernel,
        cursor.handle,
        command,
    )?;
    crate::log_info!(
        target: "gfx";
        "trueos-spirit: Lilly window approach queued tag={} handle={} slot={} from={}x{} control={}x{} target={}x{} duration_ms={} path=quadratic-natural plane=ui4-slot4 hardware_cur_pos=unchanged\n",
        LILLY_CURSOR_LABEL,
        cursor.handle,
        cursor.slot_id,
        from_x,
        from_y,
        control_x,
        control_y,
        command.x,
        command.y,
        duration_ms,
    );
    Ok(duration_ms)
}

pub(super) fn window_approach_complete() -> Result<bool, MouseControlError> {
    let cursor = register_once()?;
    crate::r::mouse_motion_service::cursor_is_idle(MouseControlPrincipal::Kernel, cursor.handle)
}

/// Bind Spirit's cursor and keyboard into one AI HUT combo. This keeps Lilly's
/// keystrokes routed to Lilly's own selected frame even if a physical mouse
/// selects a different user window while she is typing.
pub(super) fn bind_keyboard(keyboard_slot: u32) -> Result<(), &'static str> {
    let cursor = register_once().map_err(|_| "lilly-cursor-unavailable")?;
    if !crate::usb2::hid::hut::upsert_combo(
        LILLY_HUT_COMBO_ID,
        crate::usb2::hid::hut::HidSourceKind::Ai,
        LILLY_CURSOR_LABEL,
    ) || !crate::usb2::hid::hut::bind_combo_mouse(LILLY_HUT_COMBO_ID, 0, cursor.slot_id, 0)
        || !crate::usb2::hid::hut::bind_combo_keyboard(LILLY_HUT_COMBO_ID, 0, keyboard_slot, 0)
    {
        return Err("lilly-hut-combo-capacity");
    }
    Ok(())
}

/// Enqueue a real primary-button down/up pair at Lilly's current software
/// cursor position. The response path calls this only after direct UI4 focus,
/// so the click reaches Gridpaper cell zero rather than being absorbed.
pub(super) fn queue_primary_click() -> Result<(), MouseControlError> {
    let cursor = register_once()?;
    let program = [
        MouseControlCommand {
            opcode: MOUSE_CONTROL_OPCODE_BUTTONS,
            flags: MOUSE_CONTROL_FLAG_CLEAR_QUEUE,
            buttons_set: PRIMARY_BUTTON_MASK,
            ..MouseControlCommand::default()
        },
        MouseControlCommand {
            opcode: MOUSE_CONTROL_OPCODE_BUTTONS,
            buttons_clear: PRIMARY_BUTTON_MASK,
            ..MouseControlCommand::default()
        },
    ];
    crate::r::mouse_motion_service::submit_program(
        MouseControlPrincipal::Kernel,
        cursor.handle,
        &program,
    )
}

/// Replace Lilly's pending choreography with one bounded pointer action.
///
/// The motion and optional primary click burst are submitted as one station program,
/// so Dobby cannot expose a button-down half gesture or interleave another
/// producer between the approach and any click transitions.
pub(super) fn queue_pointer_action(
    target_x: i32,
    target_y: i32,
    pointer_buttons: u32,
    primary_click: bool,
    click_count: u32,
    click_delay_ms: u32,
) -> Result<u32, MouseControlError> {
    if click_count == 0 || click_count > 10 {
        return Err(MouseControlError::Invalid);
    }
    if !(100..=1000).contains(&click_delay_ms) {
        return Err(MouseControlError::Invalid);
    }
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().ok_or(MouseControlError::Invalid)?;
    if screen_width == 0 || screen_height == 0 {
        return Err(MouseControlError::Invalid);
    }
    let cursor = register_once()?;
    let (from_x, from_y) = crate::r::mouse_motion_service::cursor_position(
        MouseControlPrincipal::Kernel,
        cursor.handle,
    )?;
    let last_x = i64::from(screen_width.saturating_sub(1)).min(i64::from(i32::MAX));
    let last_y = i64::from(screen_height.saturating_sub(1)).min(i64::from(i32::MAX));
    let target_x = i64::from(target_x).clamp(0, last_x) as i32;
    let target_y = i64::from(target_y).clamp(0, last_y) as i32;
    let distance = i64::from(target_x)
        .saturating_sub(i64::from(from_x))
        .unsigned_abs()
        .max(
            i64::from(target_y)
                .saturating_sub(i64::from(from_y))
                .unsigned_abs(),
        );
    let duration_ms = u32::try_from(distance / 3)
        .unwrap_or(LILLY_DOBBY_MOVE_MAX_MS)
        .clamp(LILLY_DOBBY_MOVE_MIN_MS, LILLY_DOBBY_MOVE_MAX_MS);
    let click_buttons = if primary_click {
        if pointer_buttons == 0 {
            PRIMARY_BUTTON_MASK
        } else {
            pointer_buttons
        }
    } else {
        pointer_buttons
    };
    let click_count = click_count.max(1);
    let move_command = MouseControlCommand {
        opcode: MOUSE_CONTROL_OPCODE_STROKE,
        path: MOUSE_CONTROL_PATH_LINE,
        easing: MOUSE_CONTROL_EASING_NATURAL,
        flags: MOUSE_CONTROL_FLAG_CLEAR_QUEUE,
        duration_ms,
        x: target_x,
        y: target_y,
        ..MouseControlCommand::default()
    };
    if primary_click {
        let mut click_program = [MouseControlCommand::default(); 31];
        let mut command_count = 0usize;
        click_program[command_count] = move_command;
        command_count += 1;
        let mut click_index = 0u32;
        while click_index < click_count {
            click_program[command_count] = MouseControlCommand {
                opcode: MOUSE_CONTROL_OPCODE_BUTTONS,
                buttons_set: click_buttons,
                ..MouseControlCommand::default()
            };
            command_count += 1;
            click_program[command_count] = MouseControlCommand {
                opcode: MOUSE_CONTROL_OPCODE_BUTTONS,
                buttons_clear: click_buttons,
                ..MouseControlCommand::default()
            };
            command_count += 1;
            if click_index + 1 < click_count {
                click_program[command_count] = MouseControlCommand {
                    opcode: MOUSE_CONTROL_OPCODE_STROKE,
                    path: MOUSE_CONTROL_PATH_LINE,
                    easing: MOUSE_CONTROL_EASING_LINEAR,
                    x: target_x,
                    y: target_y,
                    duration_ms: click_delay_ms,
                    ..MouseControlCommand::default()
                };
                command_count += 1;
            }
            click_index += 1;
        }
        crate::r::mouse_motion_service::submit_program(
            MouseControlPrincipal::Kernel,
            cursor.handle,
            &click_program[..command_count],
        )?;
        return Ok(duration_ms);
    }
    let buttons_down = crate::r::mouse_motion_service::cursor_buttons(
        MouseControlPrincipal::Kernel,
        cursor.handle,
    )?;
    let button_command = MouseControlCommand {
        opcode: MOUSE_CONTROL_OPCODE_BUTTONS,
        buttons_set: click_buttons & !buttons_down,
        buttons_clear: buttons_down & !click_buttons,
        ..MouseControlCommand::default()
    };
    if button_command.buttons_set == 0 && button_command.buttons_clear == 0 {
        crate::r::mouse_motion_service::submit_program(
            MouseControlPrincipal::Kernel,
            cursor.handle,
            core::slice::from_ref(&move_command),
        )?;
        return Ok(duration_ms);
    }
    let move_and_button_program = [button_command, move_command];
    crate::r::mouse_motion_service::submit_program(
        MouseControlPrincipal::Kernel,
        cursor.handle,
        &move_and_button_program,
    )?;
    Ok(duration_ms)
}

/// Enqueue one complete clockwise trace of a brokered UI4 frame. The first
/// stroke naturally approaches its clipped top-left corner; the four linear
/// strokes then follow the exact outer pixel boundary and return there. This
/// is the software-cursor counterpart of Lilly's one-shot boot outline and
/// never moves Spirit's Intel hardware cursor.
pub(super) fn queue_window_outline(
    frame_x: i32,
    frame_y: i32,
    frame_width: u32,
    frame_height: u32,
    screen_width: u32,
    screen_height: u32,
) -> Result<(), MouseControlError> {
    if frame_width == 0 || frame_height == 0 || screen_width == 0 || screen_height == 0 {
        return Err(MouseControlError::Invalid);
    }
    let cursor = register_once()?;
    let (left, right) = clipped_axis(frame_x, frame_width, screen_width);
    let (top, bottom) = clipped_axis(frame_y, frame_height, screen_height);
    let program = [
        stroke(left, top, LILLY_INITIAL_APPROACH_MS, MOUSE_CONTROL_EASING_NATURAL, true),
        stroke(right, top, LILLY_OUTLINE_EDGE_MS, MOUSE_CONTROL_EASING_LINEAR, false),
        stroke(right, bottom, LILLY_OUTLINE_EDGE_MS, MOUSE_CONTROL_EASING_LINEAR, false),
        stroke(left, bottom, LILLY_OUTLINE_EDGE_MS, MOUSE_CONTROL_EASING_LINEAR, false),
        stroke(left, top, LILLY_OUTLINE_EDGE_MS, MOUSE_CONTROL_EASING_LINEAR, false),
    ];
    crate::r::mouse_motion_service::submit_program(
        MouseControlPrincipal::Kernel,
        cursor.handle,
        &program,
    )?;
    crate::log_info!(
        target: "gfx";
        "trueos-spirit: Lilly selected-frame outline queued tag={} handle={} slot={} commands={} outline={}x{}..{}x{} broker_rect={}x{}+{},{} clipped_to_screen={} path=natural-approach+clockwise-linear-once plane=ui4-slot4 hardware_cur_pos=unchanged\n",
        LILLY_CURSOR_LABEL,
        cursor.handle,
        cursor.slot_id,
        program.len(),
        left,
        top,
        right,
        bottom,
        frame_width,
        frame_height,
        frame_x,
        frame_y,
        (left != frame_x || top != frame_y) as u8,
    );
    Ok(())
}

/// Queue one atomic approach-plus-outline program after Spirit's first real
/// hardware move. `(spirit_left, spirit_top)` are the exact CUR_POS screen
/// coordinates produced by Spirit's bounded cursor-plane mapping.
pub(super) fn queue_initial_outline_once(
    spirit_left: i32,
    spirit_top: i32,
    spirit_extent: u32,
    screen_width: u32,
    screen_height: u32,
) -> Result<bool, MouseControlError> {
    if spirit_extent == 0 || screen_width == 0 || screen_height == 0 {
        return Err(MouseControlError::Invalid);
    }

    let mut state = LILLY_CURSOR.lock();
    if state.initial_outline_queued {
        return Ok(false);
    }
    let cursor = state.ensure_cursor()?;
    let (left, right) = clipped_axis(spirit_left, spirit_extent, screen_width);
    let (top, bottom) = clipped_axis(spirit_top, spirit_extent, screen_height);
    let program = [
        stroke(left, top, LILLY_INITIAL_APPROACH_MS, MOUSE_CONTROL_EASING_NATURAL, true),
        stroke(right, top, LILLY_OUTLINE_EDGE_MS, MOUSE_CONTROL_EASING_LINEAR, false),
        stroke(right, bottom, LILLY_OUTLINE_EDGE_MS, MOUSE_CONTROL_EASING_LINEAR, false),
        stroke(left, bottom, LILLY_OUTLINE_EDGE_MS, MOUSE_CONTROL_EASING_LINEAR, false),
        stroke(left, top, LILLY_OUTLINE_EDGE_MS, MOUSE_CONTROL_EASING_LINEAR, false),
    ];
    crate::r::mouse_motion_service::submit_program(
        MouseControlPrincipal::Kernel,
        cursor.handle,
        &program,
    )?;
    state.initial_outline_queued = true;
    crate::log_info!(
        target: "gfx";
        "trueos-spirit: Lilly vcursor initial program queued tag={} handle={} slot={} commands={} approach={}x{} outline={}x{}..{}x{} spirit_rect={}x{}+{},{} clipped_to_screen={} color={},{},{}\n",
        LILLY_CURSOR_LABEL,
        cursor.handle,
        cursor.slot_id,
        program.len(),
        left,
        top,
        left,
        top,
        right,
        bottom,
        spirit_extent,
        spirit_extent,
        spirit_left,
        spirit_top,
        (left != spirit_left || top != spirit_top) as u8,
        LILLY_CURSOR_COLOR.r,
        LILLY_CURSOR_COLOR.g,
        LILLY_CURSOR_COLOR.b,
    );
    Ok(true)
}

fn stroke(x: i32, y: i32, duration_ms: u32, easing: u8, clear_queue: bool) -> MouseControlCommand {
    MouseControlCommand {
        opcode: MOUSE_CONTROL_OPCODE_STROKE,
        path: MOUSE_CONTROL_PATH_LINE,
        easing,
        flags: if clear_queue {
            MOUSE_CONTROL_FLAG_CLEAR_QUEUE
        } else {
            0
        },
        duration_ms,
        x,
        y,
        ..MouseControlCommand::default()
    }
}

fn clipped_axis(origin: i32, extent: u32, screen_extent: u32) -> (i32, i32) {
    let screen_last = i64::from(screen_extent.saturating_sub(1));
    let start = i64::from(origin).clamp(0, screen_last);
    let end = i64::from(origin)
        .saturating_add(i64::from(extent.saturating_sub(1)))
        .clamp(0, screen_last);
    (start as i32, end as i32)
}
