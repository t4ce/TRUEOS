//! Spirit-owned software cursor for Lilly.
//!
//! Registration and motion still pass through the one mediated vCursor
//! station. This module only retains Lilly's single capability and authors her
//! first bounded choreography after Spirit's hardware CUR_POS move is proven.

use spin::Mutex;

use crate::graphics::primitives::Rgba8;
use crate::r::mouse_motion_service::{
    MOUSE_CONTROL_EASING_LINEAR, MOUSE_CONTROL_EASING_NATURAL, MOUSE_CONTROL_FLAG_CLEAR_QUEUE,
    MOUSE_CONTROL_OPCODE_STROKE, MOUSE_CONTROL_PATH_LINE, MouseControlCommand, MouseControlCursor,
    MouseControlError, MouseControlPrincipal,
};

const LILLY_CURSOR_LABEL: &str = "Spirit/Lilly";
const LILLY_CURSOR_COLOR: Rgba8 = Rgba8::new(255, 55, 255, 255);
const LILLY_INITIAL_APPROACH_MS: u32 = 420;
const LILLY_OUTLINE_EDGE_MS: u32 = 240;

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

/// Queue one atomic approach-plus-outline program after Spirit's first real
/// hardware move. `(spirit_left, spirit_top)` are the exact CUR_POS screen
/// coordinates, including possible negative cursor-plane coordinates.
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
