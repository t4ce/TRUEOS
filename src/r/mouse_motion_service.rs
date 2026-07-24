//! Kernel mouse-motion service: mediated "mouse fly" programs for vCursors.
//!
//! Callers receive an opaque capability. They may queue bounded teleport,
//! button, wheel, line, quadratic or cubic commands; this station applies
//! policy, clocks animation and is the only control-plane component which
//! emits those commands into the existing virtual-cursor HID ring.

use alloc::vec::Vec as AllocVec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use embassy_time::{Duration, Timer};
use heapless::{Deque, String, Vec};
use serde_json::Value;
use spin::Mutex;

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

const MAX_CURSORS: usize = 32;
const MAX_CURSORS_PER_PRINCIPAL: usize = 8;
const MAX_COMMANDS_PER_CURSOR: usize = 32;
const MAX_LABEL_BYTES: usize = 32;
const MAX_JSON_BYTES: usize = 16 * 1024;
const MIN_STROKE_MS: u32 = 8;
const MAX_STROKE_MS: u32 = 30_000;
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
    visual_color: Option<crate::graphics::primitives::Rgba8>,
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
        visual_color: Option<crate::graphics::primitives::Rgba8>,
    ) -> Result<MouseControlCursor, MouseControlError> {
        if self
            .cursors
            .iter()
            .filter(|cursor| cursor.principal == principal)
            .count()
            >= MAX_CURSORS_PER_PRINCIPAL
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
                visual_color,
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
            visual_color,
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
        let _ = crate::r::cursor::remove_snapshots(0, cursor.capability.slot_id);
        let _ = crate::usb2::hid::hut::remove_slot(0, cursor.capability.slot_id);
        crate::log_info!(target: "input";
            "mouse-control: cursor released handle={} slot={} principal={:?} label={}\n",
            cursor.capability.handle,
            cursor.capability.slot_id,
            cursor.principal,
            cursor.label.as_str()
        );
        Ok(())
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
                                    MOUSE_CONTROL_EVENT_FLAG_STATION,
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
                                MOUSE_CONTROL_EVENT_FLAG_STATION
                                    | MOUSE_CONTROL_EVENT_FLAG_TELEPORT,
                            );
                        }
                        MOUSE_CONTROL_OPCODE_BUTTONS | MOUSE_CONTROL_OPCODE_WHEEL => {
                            emit_cursor(
                                cursor.capability.slot_id,
                                cursor.x,
                                cursor.y,
                                cursor.buttons_down,
                                command.wheel,
                                MOUSE_CONTROL_EVENT_FLAG_STATION,
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
                    MOUSE_CONTROL_EVENT_FLAG_STATION | MOUSE_CONTROL_EVENT_FLAG_ANIMATED,
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
    STATION.lock().request(principal, label, visual_color)
}

/// Resolve presentation metadata for one mediated vCursor without exposing
/// its capability or command queue to UI4.
pub(crate) fn cursor_visual_color(slot_id: u32) -> Option<crate::graphics::primitives::Rgba8> {
    STATION
        .lock()
        .cursors
        .iter()
        .find(|cursor| cursor.capability.slot_id == slot_id)
        .and_then(|cursor| cursor.visual_color)
}

pub(crate) fn release_cursor(
    principal: MouseControlPrincipal,
    handle: u64,
) -> Result<(), MouseControlError> {
    STATION.lock().release(principal, handle)
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

#[embassy_executor::task]
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
    (x.clamp(0, width.saturating_sub(1) as i32), y.clamp(0, height.saturating_sub(1) as i32))
}

fn emit_cursor(slot_id: u32, x: i32, y: i32, buttons: u32, wheel: i16, flags: u32) {
    let (width, height) = viewport_dimensions();
    let nx = f64::from(x) / f64::from(width.saturating_sub(1).max(1));
    let ny = f64::from(y) / f64::from(height.saturating_sub(1).max(1));
    crate::usb2::hid::inject_virtual_cursor_event(slot_id, nx, ny, buttons, wheel, flags);
}

fn viewport_dimensions() -> (u32, u32) {
    crate::intel::active_scanout_dimensions()
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
    if matches!(command.opcode, MOUSE_CONTROL_OPCODE_TELEPORT | MOUSE_CONTROL_OPCODE_STROKE) {
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
