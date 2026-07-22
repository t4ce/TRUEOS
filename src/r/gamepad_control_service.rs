//! Capability-backed virtual-gamepad programs for automation tools.
//!
//! TRUEOS does not route gamepads into UI4 yet. This station deliberately
//! establishes the high-level ownership, queueing, interpolation and snapshot
//! contract now, so later IT/automation consumers do not need a raw HID
//! injection surface.

use alloc::vec::Vec as AllocVec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use embassy_time::{Duration, Timer};
use heapless::{Deque, String, Vec};
use serde_json::Value;
use spin::Mutex;

pub const GAMEPAD_CONTROL_OPCODE_SET: u8 = 1;
pub const GAMEPAD_CONTROL_OPCODE_TWEEN: u8 = 2;
pub const GAMEPAD_CONTROL_OPCODE_WAIT: u8 = 3;
pub const GAMEPAD_CONTROL_EASING_LINEAR: u8 = 0;
pub const GAMEPAD_CONTROL_EASING_NATURAL: u8 = 1;
pub const GAMEPAD_CONTROL_FLAG_CLEAR_QUEUE: u8 = 1 << 0;

const MAX_GAMEPADS: usize = 16;
const MAX_GAMEPADS_PER_PRINCIPAL: usize = 4;
const MAX_COMMANDS_PER_GAMEPAD: usize = 64;
const MAX_LABEL_BYTES: usize = 32;
const MAX_JSON_BYTES: usize = 16 * 1024;
const MAX_COMMAND_MS: u32 = 30_000;
const TICK_MS: u64 = 8;
const VGAMEPAD_SLOT_BASE: u32 = 0x5800_0000;

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
static NEXT_SLOT: AtomicU32 = AtomicU32::new(1);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GamepadControlPrincipal {
    Kernel,
    Vm(u8),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GamepadControlDevice {
    pub(crate) handle: u64,
    pub(crate) slot_id: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct GamepadControlCommand {
    pub opcode: u8,
    pub easing: u8,
    pub flags: u8,
    pub reserved0: u8,
    pub duration_ms: u32,
    pub buttons_set: u32,
    pub buttons_clear: u32,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
    pub left_trigger: u16,
    pub right_trigger: u16,
}

impl From<v::vinput::GamepadControlCommand> for GamepadControlCommand {
    fn from(command: v::vinput::GamepadControlCommand) -> Self {
        Self {
            opcode: command.opcode,
            easing: command.easing,
            flags: command.flags,
            reserved0: command.reserved0,
            duration_ms: command.duration_ms,
            buttons_set: command.buttons_set,
            buttons_clear: command.buttons_clear,
            left_x: command.left_x,
            left_y: command.left_y,
            right_x: command.right_x,
            right_y: command.right_y,
            left_trigger: command.left_trigger,
            right_trigger: command.right_trigger,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct GamepadControlSnapshot {
    pub slot_id: u32,
    pub sequence: u32,
    pub buttons_down: u32,
    pub reserved0: u32,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
    pub left_trigger: u16,
    pub right_trigger: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GamepadControlError {
    Invalid,
    Denied,
    NotFound,
    Capacity,
    QueueFull,
    BadJson,
}

impl GamepadControlError {
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
struct ActiveGamepadCommand {
    command: GamepadControlCommand,
    from_left_x: i16,
    from_left_y: i16,
    from_right_x: i16,
    from_right_y: i16,
    from_left_trigger: u16,
    from_right_trigger: u16,
    start_ms: u64,
}

struct GamepadRecord {
    capability: GamepadControlDevice,
    principal: GamepadControlPrincipal,
    label: String<MAX_LABEL_BYTES>,
    snapshot: GamepadControlSnapshot,
    active: Option<ActiveGamepadCommand>,
    commands: Deque<GamepadControlCommand, MAX_COMMANDS_PER_GAMEPAD>,
}

struct GamepadControlStation {
    gamepads: Vec<GamepadRecord, MAX_GAMEPADS>,
}

impl GamepadControlStation {
    const fn new() -> Self {
        Self {
            gamepads: Vec::new(),
        }
    }

    fn request(
        &mut self,
        principal: GamepadControlPrincipal,
        label: &str,
    ) -> Result<GamepadControlDevice, GamepadControlError> {
        if self
            .gamepads
            .iter()
            .filter(|gamepad| gamepad.principal == principal)
            .count()
            >= MAX_GAMEPADS_PER_PRINCIPAL
            || self.gamepads.len() >= MAX_GAMEPADS
        {
            return Err(GamepadControlError::Capacity);
        }
        let handle = NEXT_HANDLE.fetch_add(1, Ordering::AcqRel).max(1);
        let slot_id = VGAMEPAD_SLOT_BASE | NEXT_SLOT.fetch_add(1, Ordering::AcqRel).max(1);
        let mut stored_label = String::new();
        for ch in label.chars() {
            if stored_label.push(ch).is_err() {
                break;
            }
        }
        let capability = GamepadControlDevice { handle, slot_id };
        self.gamepads
            .push(GamepadRecord {
                capability,
                principal,
                label: stored_label,
                snapshot: GamepadControlSnapshot {
                    slot_id,
                    ..GamepadControlSnapshot::default()
                },
                active: None,
                commands: Deque::new(),
            })
            .map_err(|_| GamepadControlError::Capacity)?;
        crate::log_info!(target: "input";
            "gamepad-control: gamepad allocated handle={} slot={} principal={:?} label={} policy=mediated\n",
            handle,
            slot_id,
            principal,
            label,
        );
        Ok(capability)
    }

    fn index(
        &self,
        principal: GamepadControlPrincipal,
        handle: u64,
    ) -> Result<usize, GamepadControlError> {
        let index = self
            .gamepads
            .iter()
            .position(|gamepad| gamepad.capability.handle == handle)
            .ok_or(GamepadControlError::NotFound)?;
        if self.gamepads[index].principal != principal {
            return Err(GamepadControlError::Denied);
        }
        Ok(index)
    }

    fn release(
        &mut self,
        principal: GamepadControlPrincipal,
        handle: u64,
    ) -> Result<(), GamepadControlError> {
        let index = self.index(principal, handle)?;
        let gamepad = self.gamepads.remove(index);
        crate::log_info!(target: "input";
            "gamepad-control: gamepad released handle={} slot={} principal={:?} label={}\n",
            gamepad.capability.handle,
            gamepad.capability.slot_id,
            gamepad.principal,
            gamepad.label.as_str(),
        );
        Ok(())
    }

    fn submit(
        &mut self,
        principal: GamepadControlPrincipal,
        handle: u64,
        command: GamepadControlCommand,
    ) -> Result<(), GamepadControlError> {
        validate_command(command)?;
        let index = self.index(principal, handle)?;
        let gamepad = &mut self.gamepads[index];
        if command.flags & GAMEPAD_CONTROL_FLAG_CLEAR_QUEUE != 0 {
            gamepad.commands.clear();
            gamepad.active = None;
        }
        gamepad
            .commands
            .push_back(command)
            .map_err(|_| GamepadControlError::QueueFull)
    }

    fn submit_program(
        &mut self,
        principal: GamepadControlPrincipal,
        handle: u64,
        commands: &[GamepadControlCommand],
    ) -> Result<(), GamepadControlError> {
        if commands.is_empty() || commands.len() > MAX_COMMANDS_PER_GAMEPAD {
            return Err(GamepadControlError::Invalid);
        }
        for command in commands {
            validate_command(*command)?;
        }
        let index = self.index(principal, handle)?;
        let clear = commands
            .first()
            .is_some_and(|command| command.flags & GAMEPAD_CONTROL_FLAG_CLEAR_QUEUE != 0);
        let gamepad = &mut self.gamepads[index];
        let available = if clear {
            MAX_COMMANDS_PER_GAMEPAD
        } else {
            MAX_COMMANDS_PER_GAMEPAD.saturating_sub(gamepad.commands.len())
        };
        if commands.len() > available {
            return Err(GamepadControlError::QueueFull);
        }
        if clear {
            gamepad.commands.clear();
            gamepad.active = None;
        }
        for command in commands.iter().copied() {
            gamepad
                .commands
                .push_back(command)
                .map_err(|_| GamepadControlError::QueueFull)?;
        }
        Ok(())
    }

    fn tick(&mut self, now_ms: u64) {
        for gamepad in &mut self.gamepads {
            if gamepad.active.is_none() {
                while let Some(command) = gamepad.commands.pop_front() {
                    apply_buttons(&mut gamepad.snapshot, command.buttons_set, command.buttons_clear);
                    match command.opcode {
                        GAMEPAD_CONTROL_OPCODE_SET => {
                            apply_targets(&mut gamepad.snapshot, command);
                            bump_sequence(&mut gamepad.snapshot);
                        }
                        GAMEPAD_CONTROL_OPCODE_TWEEN | GAMEPAD_CONTROL_OPCODE_WAIT => {
                            gamepad.active = Some(ActiveGamepadCommand {
                                command,
                                from_left_x: gamepad.snapshot.left_x,
                                from_left_y: gamepad.snapshot.left_y,
                                from_right_x: gamepad.snapshot.right_x,
                                from_right_y: gamepad.snapshot.right_y,
                                from_left_trigger: gamepad.snapshot.left_trigger,
                                from_right_trigger: gamepad.snapshot.right_trigger,
                                start_ms: now_ms,
                            });
                            if command.buttons_set != 0 || command.buttons_clear != 0 {
                                bump_sequence(&mut gamepad.snapshot);
                            }
                            break;
                        }
                        _ => {}
                    }
                }
            }

            let Some(active) = gamepad.active else {
                continue;
            };
            let duration = active.command.duration_ms.max(1);
            let elapsed = now_ms.saturating_sub(active.start_ms);
            let done = elapsed >= u64::from(duration);
            if active.command.opcode == GAMEPAD_CONTROL_OPCODE_TWEEN {
                let t = if done {
                    1.0
                } else {
                    elapsed as f64 / f64::from(duration)
                };
                let t = if active.command.easing == GAMEPAD_CONTROL_EASING_NATURAL {
                    0.5 - 0.5 * libm::cos(core::f64::consts::PI * t)
                } else {
                    t
                };
                gamepad.snapshot.left_x = lerp_i16(active.from_left_x, active.command.left_x, t);
                gamepad.snapshot.left_y = lerp_i16(active.from_left_y, active.command.left_y, t);
                gamepad.snapshot.right_x = lerp_i16(active.from_right_x, active.command.right_x, t);
                gamepad.snapshot.right_y = lerp_i16(active.from_right_y, active.command.right_y, t);
                gamepad.snapshot.left_trigger =
                    lerp_u16(active.from_left_trigger, active.command.left_trigger, t);
                gamepad.snapshot.right_trigger =
                    lerp_u16(active.from_right_trigger, active.command.right_trigger, t);
                bump_sequence(&mut gamepad.snapshot);
            }
            if done {
                gamepad.active = None;
            }
        }
    }
}

static STATION: Mutex<GamepadControlStation> = Mutex::new(GamepadControlStation::new());

pub(crate) fn request_gamepad(
    principal: GamepadControlPrincipal,
    label: &str,
) -> Result<GamepadControlDevice, GamepadControlError> {
    STATION.lock().request(principal, label)
}

pub(crate) fn release_gamepad(
    principal: GamepadControlPrincipal,
    handle: u64,
) -> Result<(), GamepadControlError> {
    STATION.lock().release(principal, handle)
}

pub(crate) fn submit_command(
    principal: GamepadControlPrincipal,
    handle: u64,
    command: GamepadControlCommand,
) -> Result<(), GamepadControlError> {
    STATION.lock().submit(principal, handle, command)
}

pub(crate) fn submit_json(
    principal: GamepadControlPrincipal,
    handle: u64,
    bytes: &[u8],
) -> Result<usize, GamepadControlError> {
    if bytes.is_empty() || bytes.len() > MAX_JSON_BYTES {
        return Err(GamepadControlError::BadJson);
    }
    let value = serde_json::from_slice::<Value>(bytes).map_err(|_| GamepadControlError::BadJson)?;
    let values: AllocVec<Value> = match value.get("commands").and_then(Value::as_array) {
        Some(commands) => commands.clone(),
        None => alloc::vec![value],
    };
    if values.is_empty() || values.len() > MAX_COMMANDS_PER_GAMEPAD {
        return Err(GamepadControlError::Capacity);
    }
    let mut commands = AllocVec::with_capacity(values.len());
    for value in &values {
        commands.push(command_from_json(value)?);
    }
    let count = commands.len();
    STATION
        .lock()
        .submit_program(principal, handle, commands.as_slice())?;
    Ok(count)
}

pub(crate) fn gamepad_is_idle(
    principal: GamepadControlPrincipal,
    handle: u64,
) -> Result<bool, GamepadControlError> {
    let station = STATION.lock();
    let index = station.index(principal, handle)?;
    let gamepad = &station.gamepads[index];
    Ok(gamepad.active.is_none() && gamepad.commands.is_empty())
}

pub(crate) fn gamepad_snapshot(
    principal: GamepadControlPrincipal,
    handle: u64,
) -> Result<GamepadControlSnapshot, GamepadControlError> {
    let station = STATION.lock();
    let index = station.index(principal, handle)?;
    Ok(station.gamepads[index].snapshot)
}

#[embassy_executor::task]
pub(crate) async fn gamepad_control_service_task() {
    crate::log_info!(target: "input";
        "gamepad-control: station online protocol=binary+json ops=set,tween,wait easing=linear,natural sink=capability-snapshot\n"
    );
    loop {
        STATION.lock().tick(uptime_ms());
        Timer::after(Duration::from_millis(TICK_MS)).await;
    }
}

fn validate_command(command: GamepadControlCommand) -> Result<(), GamepadControlError> {
    if command.buttons_set & command.buttons_clear != 0 || command.duration_ms > MAX_COMMAND_MS {
        return Err(GamepadControlError::Invalid);
    }
    match command.opcode {
        GAMEPAD_CONTROL_OPCODE_SET => Ok(()),
        GAMEPAD_CONTROL_OPCODE_TWEEN
            if command.duration_ms != 0
                && matches!(
                    command.easing,
                    GAMEPAD_CONTROL_EASING_LINEAR | GAMEPAD_CONTROL_EASING_NATURAL
                ) =>
        {
            Ok(())
        }
        GAMEPAD_CONTROL_OPCODE_WAIT if command.duration_ms != 0 => Ok(()),
        _ => Err(GamepadControlError::Invalid),
    }
}

fn apply_buttons(snapshot: &mut GamepadControlSnapshot, set: u32, clear: u32) {
    snapshot.buttons_down = (snapshot.buttons_down | set) & !clear;
}

fn apply_targets(snapshot: &mut GamepadControlSnapshot, command: GamepadControlCommand) {
    snapshot.left_x = command.left_x;
    snapshot.left_y = command.left_y;
    snapshot.right_x = command.right_x;
    snapshot.right_y = command.right_y;
    snapshot.left_trigger = command.left_trigger;
    snapshot.right_trigger = command.right_trigger;
}

fn bump_sequence(snapshot: &mut GamepadControlSnapshot) {
    snapshot.sequence = snapshot.sequence.wrapping_add(1).max(1);
}

fn lerp_i16(from: i16, to: i16, t: f64) -> i16 {
    libm::round(f64::from(from) + f64::from(to - from) * t)
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

fn lerp_u16(from: u16, to: u16, t: f64) -> u16 {
    libm::round(f64::from(from) + (f64::from(to) - f64::from(from)) * t)
        .clamp(0.0, f64::from(u16::MAX)) as u16
}

fn command_from_json(value: &Value) -> Result<GamepadControlCommand, GamepadControlError> {
    let object = value.as_object().ok_or(GamepadControlError::BadJson)?;
    let opcode = match object
        .get("op")
        .and_then(Value::as_str)
        .ok_or(GamepadControlError::BadJson)?
    {
        "set" => GAMEPAD_CONTROL_OPCODE_SET,
        "tween" => GAMEPAD_CONTROL_OPCODE_TWEEN,
        "wait" => GAMEPAD_CONTROL_OPCODE_WAIT,
        _ => return Err(GamepadControlError::BadJson),
    };
    let command = GamepadControlCommand {
        opcode,
        easing: match object
            .get("easing")
            .and_then(Value::as_str)
            .unwrap_or("linear")
        {
            "linear" => GAMEPAD_CONTROL_EASING_LINEAR,
            "natural" => GAMEPAD_CONTROL_EASING_NATURAL,
            _ => return Err(GamepadControlError::BadJson),
        },
        flags: if object
            .get("clear_queue")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            GAMEPAD_CONTROL_FLAG_CLEAR_QUEUE
        } else {
            0
        },
        duration_ms: json_u32(object.get("duration_ms"))?,
        buttons_set: json_u32(object.get("buttons_down"))?,
        buttons_clear: json_u32(object.get("buttons_up"))?,
        left_x: json_i16(object.get("left_x"))?,
        left_y: json_i16(object.get("left_y"))?,
        right_x: json_i16(object.get("right_x"))?,
        right_y: json_i16(object.get("right_y"))?,
        left_trigger: json_u16(object.get("left_trigger"))?,
        right_trigger: json_u16(object.get("right_trigger"))?,
        ..GamepadControlCommand::default()
    };
    validate_command(command)?;
    Ok(command)
}

fn json_u32(value: Option<&Value>) -> Result<u32, GamepadControlError> {
    match value {
        Some(value) => u32::try_from(value.as_u64().ok_or(GamepadControlError::BadJson)?)
            .map_err(|_| GamepadControlError::BadJson),
        None => Ok(0),
    }
}

fn json_i16(value: Option<&Value>) -> Result<i16, GamepadControlError> {
    match value {
        Some(value) => i16::try_from(value.as_i64().ok_or(GamepadControlError::BadJson)?)
            .map_err(|_| GamepadControlError::BadJson),
        None => Ok(0),
    }
}

fn json_u16(value: Option<&Value>) -> Result<u16, GamepadControlError> {
    match value {
        Some(value) => u16::try_from(value.as_u64().ok_or(GamepadControlError::BadJson)?)
            .map_err(|_| GamepadControlError::BadJson),
        None => Ok(0),
    }
}

fn uptime_ms() -> u64 {
    let ticks = embassy_time_driver::now() as u128;
    let hz = embassy_time_driver::TICK_HZ as u128;
    if hz == 0 { 0 } else { ((ticks * 1000) / hz) as u64 }
}
