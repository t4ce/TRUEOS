//! Kernel mouse-motion service: mediated "mouse fly" programs for vCursors.
//!
//! Callers receive an opaque capability. They may queue bounded teleport,
//! button, wheel, line, quadratic or cubic commands; this station applies
//! policy, clocks animation and is the only control-plane component which
//! emits those commands into the existing virtual-cursor HID ring.

use alloc::{sync::Arc, vec::Vec as AllocVec};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use heapless::{Deque, String, Vec};
use serde_json::Value;
use spin::Mutex;
use trueos_time::{Duration, Timer};

pub const MOUSE_CONTROL_OPCODE_TELEPORT: u8 = 1;
pub const MOUSE_CONTROL_OPCODE_STROKE: u8 = 2;
pub const MOUSE_CONTROL_OPCODE_BUTTONS: u8 = 3;
pub const MOUSE_CONTROL_OPCODE_WHEEL: u8 = 4;

pub const MOUSE_CONTROL_PATH_LINE: u8 = 0;
pub const MOUSE_CONTROL_PATH_QUADRATIC: u8 = 1;
pub const MOUSE_CONTROL_PATH_CUBIC: u8 = 2;

pub const MOUSE_CONTROL_EASING_LINEAR: u8 = 0;
pub const MOUSE_CONTROL_EASING_FAST_LINEAR: u8 = 1;
pub const MOUSE_CONTROL_EASING_NATURAL: u8 = 2;

pub const MOUSE_CONTROL_FLAG_CLEAR_QUEUE: u8 = 1 << 0;
const MOUSE_CONTROL_EVENT_FLAG_STATION: u32 = 1 << 16;
const MOUSE_CONTROL_EVENT_FLAG_TELEPORT: u32 = 1 << 17;
const MOUSE_CONTROL_EVENT_FLAG_ANIMATED: u32 = 1 << 18;
/// A mediated cursor which contributes presentation, never application input.
/// Keep this on every queued HID event so retirement cannot make stale motion
/// interactive after its station record has gone away.
pub(crate) const MOUSE_CONTROL_EVENT_FLAG_VISUAL_ONLY: u32 =
    crate::usb2::hid::HID_CURSOR_EVENT_FLAG_VISUAL_ONLY;

// Preserve the existing 32-cursor allowance alongside 32 visual markers owned
// by the trusted Spirit-thoughts service. Other principals retain their quota.
const MAX_CURSORS: usize = 64;
const MAX_CURSORS_PER_PRINCIPAL: usize = 8;
const MAX_THOUGHT_CURSORS: usize = 32;
const THOUGHT_CURSOR_PRINCIPAL: MouseControlPrincipal = MouseControlPrincipal::KernelApp(0x53);
const MAX_COMMANDS_PER_CURSOR: usize = 32;
const MAX_LABEL_BYTES: usize = 32;
const MAX_JSON_BYTES: usize = 16 * 1024;
const MIN_STROKE_MS: u32 = 8;
const MAX_STROKE_MS: u32 = 30_000;
// Sampling the resident font does not resize its atlas. Permit physical text
// sizes beyond the atlas's native em while keeping derived cursor work bounded.
const MAX_GLYPH_HEIGHT: u16 = 4096;
const MAX_CURSOR_TEXT_SCALARS: usize = 128;
const MAX_CURSOR_TEXT_PIXELS: u32 = 16 * 1024 * 1024;
const TICK_MS: u64 = 8;
const VCURSOR_SLOT_BASE: u32 = 0x5600_0000;

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
static NEXT_SLOT: AtomicU32 = AtomicU32::new(1);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MouseControlPrincipal {
    Kernel,
    KernelApp(u8),
    Vm(u8),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MouseControlCursor {
    pub(crate) handle: u64,
    pub(crate) slot_id: u32,
}

/// Color-free text runs prepared once from the shared microfont service. The
/// software-cursor plane only translates, clips and recolors these on motion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MouseControlGlyph {
    pub(crate) text: Arc<str>,
    /// The leading span uses the cursor's mutable tint; the rest retains the
    /// metadata's text tint. Runs never cross a character-cell boundary.
    pub(crate) prefix_width: u16,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) runs: Arc<[MouseControlGlyphRun]>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MouseControlGlyphRun {
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) width: u16,
    pub(crate) coverage: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MouseControlVisualMetadata {
    pub(crate) color: Option<crate::graphics::primitives::Rgba8>,
    pub(crate) text_color: Option<crate::graphics::primitives::Rgba8>,
    pub(crate) glyph: Option<Arc<MouseControlGlyph>>,
    pub(crate) visible: bool,
    pub(crate) visual_only: bool,
}

impl MouseControlVisualMetadata {
    fn interactive(color: Option<crate::graphics::primitives::Rgba8>) -> Self {
        Self {
            color,
            text_color: None,
            glyph: None,
            visible: true,
            visual_only: false,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct MouseControlCommand {
    pub opcode: u8,
    pub path: u8,
    pub easing: u8,
    pub flags: u8,
    pub duration_ms: u32,
    pub x: i32,
    pub y: i32,
    pub control1_x: i32,
    pub control1_y: i32,
    pub control2_x: i32,
    pub control2_y: i32,
    pub buttons_set: u32,
    pub buttons_clear: u32,
    pub wheel: i16,
    pub reserved: u16,
}

impl From<v::vinput::MouseMotionCommand> for MouseControlCommand {
    fn from(command: v::vinput::MouseMotionCommand) -> Self {
        Self {
            opcode: command.opcode,
            path: command.path,
            easing: command.easing,
            flags: command.flags,
            duration_ms: command.duration_ms,
            x: command.x,
            y: command.y,
            control1_x: command.control1_x,
            control1_y: command.control1_y,
            control2_x: command.control2_x,
            control2_y: command.control2_y,
            buttons_set: command.buttons_set,
            buttons_clear: command.buttons_clear,
            wheel: command.wheel,
            reserved: command.reserved,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MouseControlError {
    Invalid,
    Denied,
    NotFound,
    Capacity,
    QueueFull,
    BadJson,
    Unavailable,
}

impl MouseControlError {
    pub(crate) const fn code(self) -> i32 {
        match self {
            Self::Invalid => -1,
            Self::Denied => -2,
            Self::NotFound => -3,
            Self::Capacity => -4,
            Self::QueueFull => -5,
            Self::BadJson => -6,
            Self::Unavailable => -7,
        }
    }
}

#[derive(Copy, Clone)]
struct ActiveStroke {
    command: MouseControlCommand,
    from_x: i32,
    from_y: i32,
    start_ms: u64,
    duration_ms: u32,
}

struct CursorRecord {
    capability: MouseControlCursor,
    principal: MouseControlPrincipal,
    label: String<MAX_LABEL_BYTES>,
    visual: MouseControlVisualMetadata,
    x: i32,
    y: i32,
    buttons_down: u32,
    active: Option<ActiveStroke>,
    commands: Deque<MouseControlCommand, MAX_COMMANDS_PER_CURSOR>,
}

struct MouseControlStation {
    cursors: Vec<CursorRecord, MAX_CURSORS>,
}

impl MouseControlStation {
    const fn new() -> Self {
        Self {
            cursors: Vec::new(),
        }
    }

    fn request(
        &mut self,
        principal: MouseControlPrincipal,
        label: &str,
        visual: MouseControlVisualMetadata,
    ) -> Result<MouseControlCursor, MouseControlError> {
        let trusted_visual = principal == THOUGHT_CURSOR_PRINCIPAL && visual.visual_only;
        let baseline_cursors = self
            .cursors
            .iter()
            .filter(|cursor| {
                !(cursor.principal == THOUGHT_CURSOR_PRINCIPAL && cursor.visual.visual_only)
            })
            .count();
        if (!trusted_visual && baseline_cursors >= MAX_CURSORS - MAX_THOUGHT_CURSORS)
            || self
                .cursors
                .iter()
                .filter(|cursor| cursor.principal == principal)
                .count()
                >= if trusted_visual {
                    MAX_THOUGHT_CURSORS
                } else {
                    MAX_CURSORS_PER_PRINCIPAL
                }
            || self.cursors.len() >= MAX_CURSORS
        {
            return Err(MouseControlError::Capacity);
        }
        let handle = NEXT_HANDLE.fetch_add(1, Ordering::AcqRel).max(1);
        let mut slot_id = VCURSOR_SLOT_BASE | NEXT_SLOT.fetch_add(1, Ordering::AcqRel).max(1);
        while crate::r::cursor::slot_id_in_use(slot_id)
            || self
                .cursors
                .iter()
                .any(|cursor| cursor.capability.slot_id == slot_id)
        {
            slot_id = VCURSOR_SLOT_BASE | NEXT_SLOT.fetch_add(1, Ordering::AcqRel).max(1);
        }
        let mut stored_label = String::new();
        for ch in label.chars() {
            if stored_label.push(ch).is_err() {
                break;
            }
        }
        let (width, height) = viewport_dimensions();
        let capability = MouseControlCursor { handle, slot_id };
        self.cursors
            .push(CursorRecord {
                capability,
                principal,
                label: stored_label,
                visual: visual.clone(),
                x: (width / 2) as i32,
                y: (height / 2) as i32,
                buttons_down: 0,
                active: None,
                commands: Deque::new(),
            })
            .map_err(|_| MouseControlError::Capacity)?;
        crate::log_info!(target: "input";
            "mouse-control: cursor allocated handle={} slot={} principal={:?} label={} visual_color={:?} policy=mediated\n",
            handle,
            slot_id,
            principal,
            label,
            visual.color,
        );
        Ok(capability)
    }

    fn cursor_index(
        &self,
        principal: MouseControlPrincipal,
        handle: u64,
    ) -> Result<usize, MouseControlError> {
        let index = self
            .cursors
            .iter()
            .position(|cursor| cursor.capability.handle == handle)
            .ok_or(MouseControlError::NotFound)?;
        if self.cursors[index].principal != principal {
            return Err(MouseControlError::Denied);
        }
        Ok(index)
    }

    fn release(
        &mut self,
        principal: MouseControlPrincipal,
        handle: u64,
    ) -> Result<(), MouseControlError> {
        let index = self.cursor_index(principal, handle)?;
        let cursor = self.cursors.remove(index);
        if cursor.buttons_down != 0 {
            emit_cursor(cursor.capability.slot_id, cursor.x, cursor.y, 0, 0, 0);
        }
        crate::usb2::hid::retire_virtual_cursor(cursor.capability.slot_id);
        crate::log_info!(target: "input";
            "mouse-control: cursor released handle={} slot={} principal={:?} label={}\n",
            cursor.capability.handle,
            cursor.capability.slot_id,
            cursor.principal,
            cursor.label.as_str()
        );
        Ok(())
    }

    fn release_principal(&mut self, principal: MouseControlPrincipal) -> usize {
        let mut released = 0;
        while let Some(handle) = self
            .cursors
            .iter()
            .find(|cursor| cursor.principal == principal)
            .map(|cursor| cursor.capability.handle)
        {
            if self.release(principal, handle).is_err() {
                break;
            }
            released += 1;
        }
        released
    }

    fn set_visual_color(
        &mut self,
        principal: MouseControlPrincipal,
        handle: u64,
        color: crate::graphics::primitives::Rgba8,
    ) -> Result<bool, MouseControlError> {
        let index = self.cursor_index(principal, handle)?;
        if self.cursors[index].visual.color == Some(color) { return Ok(false); }
        self.cursors[index].visual.color = Some(color);
        Ok(true)
    }

    fn set_visible(
        &mut self,
        principal: MouseControlPrincipal,
        handle: u64,
        visible: bool,
    ) -> Result<bool, MouseControlError> {
        let index = self.cursor_index(principal, handle)?;
        if self.cursors[index].visual.visible == visible { return Ok(false); }
        self.cursors[index].visual.visible = visible;
        Ok(true)
    }

    fn submit(
        &mut self,
        principal: MouseControlPrincipal,
        handle: u64,
        command: MouseControlCommand,
    ) -> Result<(), MouseControlError> {
        validate_command(&command)?;
        let index = self.cursor_index(principal, handle)?;
        let cursor = &mut self.cursors[index];
        validate_cursor_command(cursor, &command)?;
        if command.flags & MOUSE_CONTROL_FLAG_CLEAR_QUEUE != 0 {
            cursor.commands.clear();
            cursor.active = None;
        }
        cursor
            .commands
            .push_back(command)
            .map_err(|_| MouseControlError::QueueFull)
    }

    fn submit_program(
        &mut self,
        principal: MouseControlPrincipal,
        handle: u64,
        commands: &[MouseControlCommand],
    ) -> Result<(), MouseControlError> {
        if commands.is_empty() || commands.len() > MAX_COMMANDS_PER_CURSOR {
            return Err(MouseControlError::Invalid);
        }
        for command in commands {
            validate_command(command)?;
        }
        let index = self.cursor_index(principal, handle)?;
        let clear = commands
            .first()
            .is_some_and(|command| command.flags & MOUSE_CONTROL_FLAG_CLEAR_QUEUE != 0);
        let cursor = &mut self.cursors[index];
        for command in commands {
            validate_cursor_command(cursor, command)?;
        }
        let available = if clear {
            MAX_COMMANDS_PER_CURSOR
        } else {
            MAX_COMMANDS_PER_CURSOR.saturating_sub(cursor.commands.len())
        };
        if commands.len() > available {
            return Err(MouseControlError::QueueFull);
        }
        if clear {
            cursor.commands.clear();
            cursor.active = None;
        }
        for command in commands.iter().copied() {
            cursor
                .commands
                .push_back(command)
                .map_err(|_| MouseControlError::QueueFull)?;
        }
        Ok(())
    }

    fn legacy_write(
        &mut self,
        principal: MouseControlPrincipal,
        slot_id: u32,
        x: i32,
        y: i32,
        buttons_down: u32,
        wheel: i32,
        flags: u32,
    ) -> Result<(), MouseControlError> {
        let cursor = self
            .cursors
            .iter()
            .find(|cursor| cursor.capability.slot_id == slot_id)
            .ok_or(MouseControlError::NotFound)?;
        if cursor.principal != principal {
            return Err(MouseControlError::Denied);
        }
        let command = MouseControlCommand {
            opcode: MOUSE_CONTROL_OPCODE_TELEPORT,
            flags: MOUSE_CONTROL_FLAG_CLEAR_QUEUE,
            x,
            y,
            buttons_set: buttons_down & !cursor.buttons_down,
            buttons_clear: cursor.buttons_down & !buttons_down,
            wheel: wheel.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            reserved: (flags & u32::from(u16::MAX)) as u16,
            ..MouseControlCommand::default()
        };
        let handle = cursor.capability.handle;
        self.submit(principal, handle, command)
    }

    fn tick(&mut self, now_ms: u64) {
        for cursor in &mut self.cursors {
            if cursor.active.is_none() {
                while let Some(command) = cursor.commands.pop_front() {
                    let previous_buttons = cursor.buttons_down;
                    apply_button_sets(cursor, command.buttons_set, command.buttons_clear);
                    let buttons_changed = previous_buttons != cursor.buttons_down;
                    match command.opcode {
                        MOUSE_CONTROL_OPCODE_STROKE => {
                            cursor.active = Some(ActiveStroke {
                                command,
                                from_x: cursor.x,
                                from_y: cursor.y,
                                start_ms: now_ms,
                                duration_ms: effective_duration(cursor, command),
                            });
                            if buttons_changed || command.wheel != 0 {
                                emit_cursor(
                                    cursor.capability.slot_id,
                                    cursor.x,
                                    cursor.y,
                                    cursor.buttons_down,
                                    command.wheel,
                                    cursor_event_flags(cursor, MOUSE_CONTROL_EVENT_FLAG_STATION),
                                );
                            }
                            break;
                        }
                        MOUSE_CONTROL_OPCODE_TELEPORT => {
                            let (x, y) = clamp_position(command.x, command.y);
                            cursor.x = x;
                            cursor.y = y;
                            emit_cursor(
                                cursor.capability.slot_id,
                                x,
                                y,
                                cursor.buttons_down,
                                command.wheel,
                                cursor_event_flags(
                                    cursor,
                                    MOUSE_CONTROL_EVENT_FLAG_STATION
                                        | MOUSE_CONTROL_EVENT_FLAG_TELEPORT,
                                ),
                            );
                        }
                        MOUSE_CONTROL_OPCODE_BUTTONS | MOUSE_CONTROL_OPCODE_WHEEL => {
                            emit_cursor(
                                cursor.capability.slot_id,
                                cursor.x,
                                cursor.y,
                                cursor.buttons_down,
                                command.wheel,
                                cursor_event_flags(cursor, MOUSE_CONTROL_EVENT_FLAG_STATION),
                            );
                        }
                        _ => {}
                    }
                }
            }

            let Some(active) = cursor.active else {
                continue;
            };
            let elapsed = now_ms.saturating_sub(active.start_ms);
            let done = elapsed >= u64::from(active.duration_ms);
            let t = if done {
                1.0
            } else {
                elapsed as f64 / f64::from(active.duration_ms.max(1))
            };
            let t = apply_easing(t, active.command.easing);
            let (x, y) = evaluate_path(active, t);
            let (x, y) = clamp_position(x, y);
            if x != cursor.x || y != cursor.y || done {
                cursor.x = x;
                cursor.y = y;
                emit_cursor(
                    cursor.capability.slot_id,
                    x,
                    y,
                    cursor.buttons_down,
                    0,
                    cursor_event_flags(
                        cursor,
                        MOUSE_CONTROL_EVENT_FLAG_STATION | MOUSE_CONTROL_EVENT_FLAG_ANIMATED,
                    ),
                );
            }
            if done {
                cursor.active = None;
            }
        }
    }
}

static STATION: Mutex<MouseControlStation> = Mutex::new(MouseControlStation::new());

pub(crate) fn request_cursor(
    principal: MouseControlPrincipal,
    label: &str,
    visual_color: Option<crate::graphics::primitives::Rgba8>,
) -> Result<MouseControlCursor, MouseControlError> {
    STATION.lock().request(
        principal,
        label,
        MouseControlVisualMetadata::interactive(visual_color),
    )
}

/// Allocate a non-interactive glyph in the existing virtual-cursor station.
/// It accepts ordinary motion programs but cannot inject buttons or wheel,
/// select frames, acquire keyboard focus, or participate in desktop hover.
pub(crate) fn request_glyph_cursor(
    principal: MouseControlPrincipal,
    label: &str,
    character: char,
    height_px: u16,
    color: crate::graphics::primitives::Rgba8,
) -> Result<MouseControlCursor, MouseControlError> {
    if character.is_control() || character.is_whitespace() {
        return Err(MouseControlError::Invalid);
    }
    let mut buffer = [0; 4];
    let text = character.encode_utf8(&mut buffer);
    cursor_text_dimensions(text, height_px)?;
    // The single-glyph API keeps its original strict missing-glyph policy.
    let atlas = super::microfont_mask_service::ready_atlas()
        .ok_or(MouseControlError::Unavailable)?;
    atlas.glyph(character).ok_or(MouseControlError::Invalid)?;
    request_text_cursor(principal, label, text, height_px, 1, color, color)
}

/// Allocate an immutable text shape on the same non-interactive cursor plane.
/// Only the leading span follows `set_cursor_visual_color`; the remaining text
/// keeps its initial tint. Text pixels always come from Paper's resident atlas.
pub(crate) fn request_text_cursor(
    principal: MouseControlPrincipal,
    label: &str,
    text: &str,
    height_px: u16,
    tinted_prefix_chars: usize,
    prefix_color: crate::graphics::primitives::Rgba8,
    text_color: crate::graphics::primitives::Rgba8,
) -> Result<MouseControlCursor, MouseControlError> {
    let (width, _) = cursor_text_dimensions(text, height_px)?;
    let count = text.chars().count();
    if tinted_prefix_chars > count { return Err(MouseControlError::Invalid); }
    let prefix_width = (usize::from(width) / count * tinted_prefix_chars) as u16;
    let shared = STATION
        .lock()
        .cursors
        .iter()
        .filter_map(|cursor| cursor.visual.glyph.as_ref())
        .find(|glyph| glyph.text.as_ref() == text && glyph.height == height_px
            && glyph.prefix_width == prefix_width)
        .cloned();
    let glyph = match shared {
        Some(glyph) => glyph,
        None => Arc::new(prepare_cursor_text(text, height_px, prefix_width)?),
    };
    STATION.lock().request(
        principal,
        label,
        MouseControlVisualMetadata {
            color: Some(prefix_color),
            text_color: Some(text_color),
            glyph: Some(glyph),
            visible: true,
            visual_only: true,
        },
    )
}

/// Exact fixed-grid run geometry, including spaces. Geometry is available while
/// the shared boot atlas is still warming, so callers can arrange labels first.
pub(crate) fn cursor_text_dimensions(
    text: &str,
    height_px: u16,
) -> Result<(u16, u16), MouseControlError> {
    if !(1..=MAX_GLYPH_HEIGHT).contains(&height_px)
        || text.is_empty() || text.chars().any(char::is_control) {
        return Err(MouseControlError::Invalid);
    }
    let count = text.chars().count();
    if count > MAX_CURSOR_TEXT_SCALARS { return Err(MouseControlError::Capacity); }
    let cell_width = (u32::from(height_px) * 6 + 5) / 11;
    let width = cell_width.checked_mul(count as u32).ok_or(MouseControlError::Capacity)?;
    if width > u32::from(u16::MAX)
        || width * u32::from(height_px) > MAX_CURSOR_TEXT_PIXELS {
        return Err(MouseControlError::Capacity);
    }
    Ok((width as u16, height_px))
}

/// Change a mediated cursor's leading tint without rerasterizing its shape or changing
/// its identity, position, input behavior, or queued motion.
pub(crate) fn set_cursor_visual_color(
    principal: MouseControlPrincipal,
    handle: u64,
    color: crate::graphics::primitives::Rgba8,
) -> Result<(), MouseControlError> {
    let changed = STATION.lock().set_visual_color(principal, handle, color)?;
    if changed { crate::ui4::notify_cursor_visual_change(); }
    Ok(())
}

/// Hide presentation while preserving the mediated cursor and its choreography.
pub(crate) fn set_cursor_visible(
    principal: MouseControlPrincipal,
    handle: u64,
    visible: bool,
) -> Result<(), MouseControlError> {
    let changed = STATION.lock().set_visible(principal, handle, visible)?;
    if changed { crate::ui4::notify_cursor_visual_change(); }
    Ok(())
}

pub(crate) fn cursor_visual_metadata(slot_id: u32) -> Option<MouseControlVisualMetadata> {
    STATION
        .lock()
        .cursors
        .iter()
        .find(|cursor| cursor.capability.slot_id == slot_id)
        .map(|cursor| cursor.visual.clone())
}

/// Resolve presentation metadata for one mediated vCursor without exposing
/// its capability or command queue to UI4.
pub(crate) fn cursor_visual_color(slot_id: u32) -> Option<crate::graphics::primitives::Rgba8> {
    STATION
        .lock()
        .cursors
        .iter()
        .find(|cursor| cursor.capability.slot_id == slot_id)
        .and_then(|cursor| cursor.visual.color)
}

pub(crate) fn release_cursor(
    principal: MouseControlPrincipal,
    handle: u64,
) -> Result<(), MouseControlError> {
    STATION.lock().release(principal, handle)
}

pub(crate) fn release_principal(principal: MouseControlPrincipal) -> usize {
    STATION.lock().release_principal(principal)
}

pub(crate) fn submit_command(
    principal: MouseControlPrincipal,
    handle: u64,
    command: MouseControlCommand,
) -> Result<(), MouseControlError> {
    STATION.lock().submit(principal, handle, command)
}

pub(crate) fn submit_program(
    principal: MouseControlPrincipal,
    handle: u64,
    commands: &[MouseControlCommand],
) -> Result<(), MouseControlError> {
    STATION.lock().submit_program(principal, handle, commands)
}

pub(crate) fn submit_json(
    principal: MouseControlPrincipal,
    handle: u64,
    bytes: &[u8],
) -> Result<usize, MouseControlError> {
    if bytes.is_empty() || bytes.len() > MAX_JSON_BYTES {
        return Err(MouseControlError::BadJson);
    }
    let value = serde_json::from_slice::<Value>(bytes).map_err(|_| MouseControlError::BadJson)?;
    let values: AllocVec<Value> = match value.get("commands").and_then(Value::as_array) {
        Some(commands) => commands.clone(),
        None => alloc::vec![value],
    };
    if values.is_empty() || values.len() > MAX_COMMANDS_PER_CURSOR {
        return Err(MouseControlError::Capacity);
    }
    let mut commands = AllocVec::with_capacity(values.len());
    for value in &values {
        commands.push(command_from_json(value)?);
    }
    STATION
        .lock()
        .submit_program(principal, handle, &commands)?;
    Ok(commands.len())
}

pub(crate) fn legacy_write_cursor(
    principal: MouseControlPrincipal,
    slot_id: u32,
    x: i32,
    y: i32,
    buttons_down: u32,
    wheel: i32,
    flags: u32,
) -> Result<(), MouseControlError> {
    STATION
        .lock()
        .legacy_write(principal, slot_id, x, y, buttons_down, wheel, flags)
}

pub(crate) fn cursor_is_idle(
    principal: MouseControlPrincipal,
    handle: u64,
) -> Result<bool, MouseControlError> {
    let station = STATION.lock();
    let index = station.cursor_index(principal, handle)?;
    let cursor = &station.cursors[index];
    Ok(cursor.active.is_none() && cursor.commands.is_empty())
}

pub(crate) fn cursor_position(
    principal: MouseControlPrincipal,
    handle: u64,
) -> Result<(i32, i32), MouseControlError> {
    let station = STATION.lock();
    let index = station.cursor_index(principal, handle)?;
    let cursor = &station.cursors[index];
    Ok((cursor.x, cursor.y))
}

pub(crate) fn cursor_buttons(
    principal: MouseControlPrincipal,
    handle: u64,
) -> Result<u32, MouseControlError> {
    let station = STATION.lock();
    let index = station.cursor_index(principal, handle)?;
    let cursor = &station.cursors[index];
    Ok(cursor.buttons_down)
}

#[trueos_executor::task]
pub(crate) async fn mouse_motion_service_task() {
    crate::log_info!(target: "input";
        "mouse-control: station online protocol=binary+json paths=teleport,line,quadratic,cubic easing=linear,fastlinear,natural sink=vcursor-ring\n"
    );
    loop {
        STATION.lock().tick(uptime_ms());
        Timer::after(Duration::from_millis(TICK_MS)).await;
    }
}

fn validate_command(command: &MouseControlCommand) -> Result<(), MouseControlError> {
    if command.buttons_set & command.buttons_clear != 0 {
        return Err(MouseControlError::Invalid);
    }
    match command.opcode {
        MOUSE_CONTROL_OPCODE_TELEPORT
        | MOUSE_CONTROL_OPCODE_BUTTONS
        | MOUSE_CONTROL_OPCODE_WHEEL => Ok(()),
        MOUSE_CONTROL_OPCODE_STROKE => {
            if !matches!(
                command.path,
                MOUSE_CONTROL_PATH_LINE | MOUSE_CONTROL_PATH_QUADRATIC | MOUSE_CONTROL_PATH_CUBIC
            ) || !matches!(
                command.easing,
                MOUSE_CONTROL_EASING_LINEAR
                    | MOUSE_CONTROL_EASING_FAST_LINEAR
                    | MOUSE_CONTROL_EASING_NATURAL
            ) || command.duration_ms > MAX_STROKE_MS
            {
                return Err(MouseControlError::Invalid);
            }
            Ok(())
        }
        _ => Err(MouseControlError::Invalid),
    }
}

fn validate_cursor_command(
    cursor: &CursorRecord,
    command: &MouseControlCommand,
) -> Result<(), MouseControlError> {
    if cursor.visual.visual_only
        && (command.buttons_set != 0
            || command.buttons_clear != 0
            || command.wheel != 0
            || matches!(
                command.opcode,
                MOUSE_CONTROL_OPCODE_BUTTONS | MOUSE_CONTROL_OPCODE_WHEEL
            ))
    {
        return Err(MouseControlError::Denied);
    }
    Ok(())
}

fn cursor_event_flags(cursor: &CursorRecord, flags: u32) -> u32 {
    flags
        | if cursor.visual.visual_only {
            MOUSE_CONTROL_EVENT_FLAG_VISUAL_ONLY
        } else {
            0
        }
}

fn prepare_cursor_text(
    text: &str,
    height: u16,
    prefix_width: u16,
) -> Result<MouseControlGlyph, MouseControlError> {
    let (width, _) = cursor_text_dimensions(text, height)?;
    let cell_width = (u32::from(height) * 6 + 5) / 11;
    // Paper and Shell3 already warm this immutable R8 alphabet once at boot.
    // Derive only color-free row runs; never rasterize another glyph or atlas.
    let atlas = super::microfont_mask_service::ready_atlas()
        .ok_or(MouseControlError::Unavailable)?;
    let mut runs = AllocVec::new();
    for (column, character) in text.chars().enumerate() {
        let mask = atlas.glyph(character).or_else(|| atlas.glyph('?'))
            .ok_or(MouseControlError::Invalid)?;
        for y in 0..u32::from(height) {
            let mut from = 0;
            let mut coverage = atlas.sample(mask, 0, y, cell_width, u32::from(height));
            for x in 1..=cell_width {
                let next = if x < cell_width {
                    atlas.sample(mask, x, y, cell_width, u32::from(height))
                } else {
                    0
                };
                if next != coverage || x == cell_width {
                    if coverage != 0 {
                        runs.push(MouseControlGlyphRun {
                            x: (column as u32 * cell_width + from) as u16,
                            y: y as u16,
                            width: (x - from) as u16,
                            coverage,
                        });
                    }
                    from = x;
                    coverage = next;
                }
            }
        }
    }
    if runs.is_empty() {
        return Err(MouseControlError::Invalid);
    }
    Ok(MouseControlGlyph {
        text: Arc::from(text),
        prefix_width,
        width,
        height,
        runs: Arc::from(runs.into_boxed_slice()),
    })
}

fn apply_button_sets(cursor: &mut CursorRecord, set: u32, clear: u32) {
    cursor.buttons_down = (cursor.buttons_down | set) & !clear;
}

fn effective_duration(cursor: &CursorRecord, command: MouseControlCommand) -> u32 {
    if command.duration_ms != 0 {
        return command.duration_ms.clamp(MIN_STROKE_MS, MAX_STROKE_MS);
    }
    let dx = f64::from(command.x.saturating_sub(cursor.x));
    let dy = f64::from(command.y.saturating_sub(cursor.y));
    let distance = libm::sqrt(dx * dx + dy * dy);
    let px_per_ms = if command.easing == MOUSE_CONTROL_EASING_FAST_LINEAR {
        2.4
    } else {
        1.1
    };
    ((distance / px_per_ms) as u32).clamp(MIN_STROKE_MS, MAX_STROKE_MS)
}

fn apply_easing(t: f64, easing: u8) -> f64 {
    let t = t.clamp(0.0, 1.0);
    match easing {
        MOUSE_CONTROL_EASING_FAST_LINEAR => 1.0 - libm::pow(1.0 - t, 3.0),
        MOUSE_CONTROL_EASING_NATURAL => 0.5 - 0.5 * libm::cos(core::f64::consts::PI * t),
        _ => t,
    }
}

fn evaluate_path(active: ActiveStroke, t: f64) -> (i32, i32) {
    let command = active.command;
    let p0x = f64::from(active.from_x);
    let p0y = f64::from(active.from_y);
    let p1x = f64::from(command.x);
    let p1y = f64::from(command.y);
    let (x, y) = match command.path {
        MOUSE_CONTROL_PATH_QUADRATIC => {
            let u = 1.0 - t;
            let c1x = f64::from(command.control1_x);
            let c1y = f64::from(command.control1_y);
            (
                u * u * p0x + 2.0 * u * t * c1x + t * t * p1x,
                u * u * p0y + 2.0 * u * t * c1y + t * t * p1y,
            )
        }
        MOUSE_CONTROL_PATH_CUBIC => {
            let u = 1.0 - t;
            let c1x = f64::from(command.control1_x);
            let c1y = f64::from(command.control1_y);
            let c2x = f64::from(command.control2_x);
            let c2y = f64::from(command.control2_y);
            (
                u * u * u * p0x + 3.0 * u * u * t * c1x + 3.0 * u * t * t * c2x + t * t * t * p1x,
                u * u * u * p0y + 3.0 * u * u * t * c1y + 3.0 * u * t * t * c2y + t * t * t * p1y,
            )
        }
        _ => (p0x + (p1x - p0x) * t, p0y + (p1y - p0y) * t),
    };
    (libm::round(x) as i32, libm::round(y) as i32)
}

fn clamp_position(x: i32, y: i32) -> (i32, i32) {
    let (width, height) = viewport_dimensions();
    (
        x.clamp(0, width.saturating_sub(1) as i32),
        y.clamp(0, height.saturating_sub(1) as i32),
    )
}

fn emit_cursor(slot_id: u32, x: i32, y: i32, buttons: u32, wheel: i16, flags: u32) {
    let (width, height) = viewport_dimensions();
    let nx = f64::from(x) / f64::from(width.saturating_sub(1).max(1));
    let ny = f64::from(y) / f64::from(height.saturating_sub(1).max(1));
    crate::usb2::hid::inject_virtual_cursor_event(slot_id, nx, ny, buttons, wheel, flags);
}

fn viewport_dimensions() -> (u32, u32) {
    crate::ui4::output_dimensions()
        .or_else(|| {
            crate::limine::framebuffer_response()
                .and_then(|response| response.framebuffers().first().copied())
                .map(|framebuffer| (framebuffer.width as u32, framebuffer.height as u32))
        })
        .unwrap_or((320, 200))
}

fn uptime_ms() -> u64 {
    let ticks = embassy_time_driver::now() as u128;
    let hz = embassy_time_driver::TICK_HZ as u128;
    if hz == 0 {
        0
    } else {
        ((ticks * 1000) / hz) as u64
    }
}

fn command_from_json(value: &Value) -> Result<MouseControlCommand, MouseControlError> {
    let object = value.as_object().ok_or(MouseControlError::BadJson)?;
    let op = object
        .get("op")
        .and_then(Value::as_str)
        .ok_or(MouseControlError::BadJson)?;
    let mut command = MouseControlCommand::default();
    command.opcode = match op {
        "teleport" => MOUSE_CONTROL_OPCODE_TELEPORT,
        "stroke" => MOUSE_CONTROL_OPCODE_STROKE,
        "buttons" => MOUSE_CONTROL_OPCODE_BUTTONS,
        "wheel" => MOUSE_CONTROL_OPCODE_WHEEL,
        _ => return Err(MouseControlError::BadJson),
    };
    if matches!(
        command.opcode,
        MOUSE_CONTROL_OPCODE_TELEPORT | MOUSE_CONTROL_OPCODE_STROKE
    ) {
        command.x = json_i32(object.get("x"))?;
        command.y = json_i32(object.get("y"))?;
    }
    command.control1_x = json_i32_optional(object.get("control1_x"))?;
    command.control1_y = json_i32_optional(object.get("control1_y"))?;
    command.control2_x = json_i32_optional(object.get("control2_x"))?;
    command.control2_y = json_i32_optional(object.get("control2_y"))?;
    command.duration_ms = json_u32_optional(object.get("duration_ms"))?;
    command.buttons_set = json_u32_optional(object.get("buttons_down"))?;
    command.buttons_clear = json_u32_optional(object.get("buttons_up"))?;
    command.wheel =
        json_i32_optional(object.get("wheel"))?.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    command.flags = if object
        .get("clear_queue")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        MOUSE_CONTROL_FLAG_CLEAR_QUEUE
    } else {
        0
    };
    command.path = match object.get("path").and_then(Value::as_str).unwrap_or("line") {
        "line" => MOUSE_CONTROL_PATH_LINE,
        "quadratic" => MOUSE_CONTROL_PATH_QUADRATIC,
        "cubic" => MOUSE_CONTROL_PATH_CUBIC,
        _ => return Err(MouseControlError::BadJson),
    };
    command.easing = match object
        .get("speed")
        .or_else(|| object.get("easing"))
        .and_then(Value::as_str)
        .unwrap_or("linear")
    {
        "linear" => MOUSE_CONTROL_EASING_LINEAR,
        "fastlinear" => MOUSE_CONTROL_EASING_FAST_LINEAR,
        "natural" => MOUSE_CONTROL_EASING_NATURAL,
        _ => return Err(MouseControlError::BadJson),
    };
    validate_command(&command)?;
    Ok(command)
}

fn json_i32(value: Option<&Value>) -> Result<i32, MouseControlError> {
    let value = value
        .and_then(Value::as_i64)
        .ok_or(MouseControlError::BadJson)?;
    i32::try_from(value).map_err(|_| MouseControlError::BadJson)
}

fn json_i32_optional(value: Option<&Value>) -> Result<i32, MouseControlError> {
    match value {
        Some(value) => i32::try_from(value.as_i64().ok_or(MouseControlError::BadJson)?)
            .map_err(|_| MouseControlError::BadJson),
        None => Ok(0),
    }
}

fn json_u32_optional(value: Option<&Value>) -> Result<u32, MouseControlError> {
    match value {
        Some(value) => u32::try_from(value.as_u64().ok_or(MouseControlError::BadJson)?)
            .map_err(|_| MouseControlError::BadJson),
        None => Ok(0),
    }
}

#[cfg(test)]
mod visual_cursor_tests {
    use super::*;
    use crate::graphics::primitives::Rgba8;

    fn visual() -> MouseControlVisualMetadata {
        MouseControlVisualMetadata {
            color: Some(Rgba8::new(0, 0, 0, 255)),
            text_color: None,
            glyph: None,
            visible: true,
            visual_only: true,
        }
    }

    #[test]
    fn trusted_visual_budget_preserves_all_32_existing_cursors() {
        let mut station = MouseControlStation::new();
        for principal in 0..4 {
            for _ in 0..MAX_CURSORS_PER_PRINCIPAL {
                station.request(MouseControlPrincipal::Vm(principal), "input", MouseControlVisualMetadata::interactive(None)).unwrap();
            }
        }
        assert_eq!(station.request(MouseControlPrincipal::Vm(4), "overflow", visual()), Err(MouseControlError::Capacity));
        for _ in 0..MAX_THOUGHT_CURSORS {
            station.request(THOUGHT_CURSOR_PRINCIPAL, "marker", visual()).unwrap();
        }
        assert_eq!(station.cursors.len(), MAX_CURSORS);
        assert_eq!(station.request(THOUGHT_CURSOR_PRINCIPAL, "overflow", visual()), Err(MouseControlError::Capacity));
    }

    #[test]
    fn ordinary_glyph_principals_retain_eight_cursor_limit() {
        let mut station = MouseControlStation::new();
        for _ in 0..MAX_CURSORS_PER_PRINCIPAL {
            station.request(MouseControlPrincipal::Vm(7), "marker", visual()).unwrap();
        }
        assert_eq!(station.request(MouseControlPrincipal::Vm(7), "overflow", visual()), Err(MouseControlError::Capacity));
        assert!(station.request(MouseControlPrincipal::Vm(8), "independent", visual()).is_ok());
    }

    #[test]
    fn visual_program_rejects_input_atomically_before_queue_clear() {
        let mut station = MouseControlStation::new();
        let cursor = station.request(THOUGHT_CURSOR_PRINCIPAL, "marker", visual()).unwrap();
        let motion = MouseControlCommand { opcode: MOUSE_CONTROL_OPCODE_TELEPORT, x: 10, y: 20, ..Default::default() };
        station.submit(THOUGHT_CURSOR_PRINCIPAL, cursor.handle, motion).unwrap();
        for input in [
            MouseControlCommand { opcode: MOUSE_CONTROL_OPCODE_BUTTONS, ..Default::default() },
            MouseControlCommand { opcode: MOUSE_CONTROL_OPCODE_WHEEL, ..Default::default() },
            MouseControlCommand { buttons_set: 1, ..motion },
            MouseControlCommand { wheel: 1, ..motion },
        ] {
            assert_eq!(station.submit_program(THOUGHT_CURSOR_PRINCIPAL, cursor.handle, &[
                MouseControlCommand { flags: MOUSE_CONTROL_FLAG_CLEAR_QUEUE, ..motion }, input,
            ]), Err(MouseControlError::Denied));
            assert_eq!(station.cursors[0].commands.len(), 1);
        }
    }

    #[test]
    fn visual_metadata_updates_preserve_motion_and_authority() {
        let mut station = MouseControlStation::new();
        let cursor = station.request(THOUGHT_CURSOR_PRINCIPAL, "marker", visual()).unwrap();
        station.submit(THOUGHT_CURSOR_PRINCIPAL, cursor.handle, MouseControlCommand {
            opcode: MOUSE_CONTROL_OPCODE_STROKE, duration_ms: 1_000, x: 20, y: 30, ..Default::default()
        }).unwrap();
        let color = Rgba8::new(0, 160, 255, 255);
        assert_eq!(station.set_visual_color(MouseControlPrincipal::Vm(7), cursor.handle, color), Err(MouseControlError::Denied));
        assert_eq!(station.set_visible(MouseControlPrincipal::Vm(7), cursor.handle, false), Err(MouseControlError::Denied));
        station.set_visual_color(THOUGHT_CURSOR_PRINCIPAL, cursor.handle, color).unwrap();
        station.set_visible(THOUGHT_CURSOR_PRINCIPAL, cursor.handle, false).unwrap();
        assert_eq!(station.cursors[0].visual.color, Some(color));
        assert!(!station.cursors[0].visual.visible);
        assert!(station.cursors[0].visual.visual_only);
        assert_eq!(station.cursors[0].commands.len(), 1);
        station.set_visible(THOUGHT_CURSOR_PRINCIPAL, cursor.handle, true).unwrap();
        assert_eq!(station.cursors[0].commands.len(), 1);
    }

    #[test]
    fn visual_motion_uses_existing_stroke_clock_and_tags_every_event() {
        let mut station = MouseControlStation::new();
        let cursor = station.request(THOUGHT_CURSOR_PRINCIPAL, "marker", visual()).unwrap();
        station.submit_program(THOUGHT_CURSOR_PRINCIPAL, cursor.handle, &[
            MouseControlCommand { opcode: MOUSE_CONTROL_OPCODE_TELEPORT, x: 0, y: 0, ..Default::default() },
            MouseControlCommand { opcode: MOUSE_CONTROL_OPCODE_STROKE, duration_ms: 100, x: 100, y: 60, ..Default::default() },
        ]).unwrap();
        station.tick(0);
        station.tick(50);
        assert_eq!((station.cursors[0].x, station.cursors[0].y), (50, 30));
        station.tick(100);
        assert_eq!((station.cursors[0].x, station.cursors[0].y), (100, 60));
        for flags in [MOUSE_CONTROL_EVENT_FLAG_STATION, MOUSE_CONTROL_EVENT_FLAG_TELEPORT, MOUSE_CONTROL_EVENT_FLAG_ANIMATED] {
            assert_ne!(cursor_event_flags(&station.cursors[0], flags) & MOUSE_CONTROL_EVENT_FLAG_VISUAL_ONLY, 0);
        }
        assert_eq!(station.cursors[0].buttons_down, 0);
        assert!(station.cursors[0].active.is_none());
    }

    #[test]
    fn glyph_registration_requires_the_shared_font_and_rejects_invalid_geometry() {
        let request = |character, height| request_glyph_cursor(
            THOUGHT_CURSOR_PRINCIPAL, "glyph-test", character, height,
            Rgba8::new(0, 0, 0, 255),
        );
        if super::super::microfont_mask_service::ready_atlas().is_none() {
            assert_eq!(
                request('§', 48),
                Err(MouseControlError::Unavailable)
            );
        }
        for (character, height) in [
            (' ', 16),
            ('\n', 16),
            ('§', 0),
            ('§', MAX_GLYPH_HEIGHT + 1),
        ] {
            assert_eq!(request(character, height), Err(MouseControlError::Invalid));
        }
    }

    #[test]
    fn cursor_release_drops_registration_and_rejects_foreign_owner() {
        let mut station = MouseControlStation::new();
        let cursor = station.request(THOUGHT_CURSOR_PRINCIPAL, "marker", visual()).unwrap();
        assert_eq!(station.release(MouseControlPrincipal::Vm(2), cursor.handle), Err(MouseControlError::Denied));
        station.release(THOUGHT_CURSOR_PRINCIPAL, cursor.handle).unwrap();
        assert!(station.cursors.is_empty());
        assert_eq!(station.release(THOUGHT_CURSOR_PRINCIPAL, cursor.handle), Err(MouseControlError::NotFound));
    }
}
