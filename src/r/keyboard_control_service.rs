//! Capability-backed, clocked virtual-keyboard programs for automation tools.
//!
//! This is the keyboard peer of `mouse_motion_service`: callers own opaque
//! capabilities and submit bounded binary or JSON programs. Only this station
//! updates the virtual keyboard's HUT state and output-event stream.

use alloc::vec::Vec as AllocVec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use embassy_time::{Duration, Timer};
use heapless::{Deque, String, Vec};
use serde_json::Value;
use spin::Mutex;

pub const KEYBOARD_CONTROL_OPCODE_STROKE: u8 = 1;
pub const KEYBOARD_CONTROL_OPCODE_DOWN: u8 = 2;
pub const KEYBOARD_CONTROL_OPCODE_UP: u8 = 3;
pub const KEYBOARD_CONTROL_OPCODE_WAIT: u8 = 4;
pub const KEYBOARD_CONTROL_FLAG_CLEAR_QUEUE: u8 = 1 << 0;

const MAX_KEYBOARDS: usize = 16;
const MAX_KEYBOARDS_PER_PRINCIPAL: usize = 4;
const MAX_COMMANDS_PER_KEYBOARD: usize = 64;
const MAX_LABEL_BYTES: usize = 32;
const MAX_JSON_BYTES: usize = 16 * 1024;
const DEFAULT_STROKE_MS: u32 = 48;
const MAX_COMMAND_MS: u32 = 30_000;
const TICK_MS: u64 = 8;
const VKEYBOARD_SLOT_BASE: u32 = 0x5700_0000;

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
static NEXT_SLOT: AtomicU32 = AtomicU32::new(1);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum KeyboardControlPrincipal {
    Kernel,
    Vm(u8),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct KeyboardControlDevice {
    pub(crate) handle: u64,
    pub(crate) slot_id: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct KeyboardControlCommand {
    pub opcode: u8,
    pub flags: u8,
    pub modifiers: u8,
    pub reserved0: u8,
    pub duration_ms: u32,
    pub codepoint: u32,
    pub key_code: u16,
    pub reserved1: u16,
}

impl From<v::vinput::KeyboardControlCommand> for KeyboardControlCommand {
    fn from(command: v::vinput::KeyboardControlCommand) -> Self {
        Self {
            opcode: command.opcode,
            flags: command.flags,
            modifiers: command.modifiers,
            reserved0: command.reserved0,
            duration_ms: command.duration_ms,
            codepoint: command.codepoint,
            key_code: command.key_code,
            reserved1: command.reserved1,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum KeyboardControlError {
    Invalid,
    Denied,
    NotFound,
    Capacity,
    QueueFull,
    BadJson,
}

impl KeyboardControlError {
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
struct ActiveKey {
    key_code: u8,
    modifiers: u8,
    finish_ms: u64,
}

struct KeyboardRecord {
    capability: KeyboardControlDevice,
    principal: KeyboardControlPrincipal,
    label: String<MAX_LABEL_BYTES>,
    modifiers: u8,
    keys: [u8; 6],
    device_seq: u32,
    active: Option<ActiveKey>,
    commands: Deque<KeyboardControlCommand, MAX_COMMANDS_PER_KEYBOARD>,
}

struct KeyboardControlStation {
    keyboards: Vec<KeyboardRecord, MAX_KEYBOARDS>,
}

impl KeyboardControlStation {
    const fn new() -> Self {
        Self {
            keyboards: Vec::new(),
        }
    }

    fn request(
        &mut self,
        principal: KeyboardControlPrincipal,
        label: &str,
    ) -> Result<KeyboardControlDevice, KeyboardControlError> {
        if self
            .keyboards
            .iter()
            .filter(|keyboard| keyboard.principal == principal)
            .count()
            >= MAX_KEYBOARDS_PER_PRINCIPAL
            || self.keyboards.len() >= MAX_KEYBOARDS
        {
            return Err(KeyboardControlError::Capacity);
        }
        let handle = NEXT_HANDLE.fetch_add(1, Ordering::AcqRel).max(1);
        let slot_id = VKEYBOARD_SLOT_BASE | NEXT_SLOT.fetch_add(1, Ordering::AcqRel).max(1);
        let mut stored_label = String::new();
        for ch in label.chars() {
            if stored_label.push(ch).is_err() {
                break;
            }
        }
        let capability = KeyboardControlDevice { handle, slot_id };
        let record = KeyboardRecord {
            capability,
            principal,
            label: stored_label,
            modifiers: 0,
            keys: [0; 6],
            device_seq: 0,
            active: None,
            commands: Deque::new(),
        };
        emit_report(&record);
        self.keyboards
            .push(record)
            .map_err(|_| KeyboardControlError::Capacity)?;
        crate::log_info!(target: "input";
            "keyboard-control: keyboard allocated handle={} slot={} principal={:?} label={} policy=mediated\n",
            handle,
            slot_id,
            principal,
            label,
        );
        Ok(capability)
    }

    fn index(
        &self,
        principal: KeyboardControlPrincipal,
        handle: u64,
    ) -> Result<usize, KeyboardControlError> {
        let index = self
            .keyboards
            .iter()
            .position(|keyboard| keyboard.capability.handle == handle)
            .ok_or(KeyboardControlError::NotFound)?;
        if self.keyboards[index].principal != principal {
            return Err(KeyboardControlError::Denied);
        }
        Ok(index)
    }

    fn release(
        &mut self,
        principal: KeyboardControlPrincipal,
        handle: u64,
    ) -> Result<(), KeyboardControlError> {
        let index = self.index(principal, handle)?;
        let mut keyboard = self.keyboards.remove(index);
        keyboard.modifiers = 0;
        keyboard.keys = [0; 6];
        emit_report(&keyboard);
        crate::usb2::hid::remove_hid_slot(0, keyboard.capability.slot_id);
        crate::log_info!(target: "input";
            "keyboard-control: keyboard released handle={} slot={} principal={:?} label={}\n",
            keyboard.capability.handle,
            keyboard.capability.slot_id,
            keyboard.principal,
            keyboard.label.as_str(),
        );
        Ok(())
    }

    fn submit(
        &mut self,
        principal: KeyboardControlPrincipal,
        handle: u64,
        command: KeyboardControlCommand,
    ) -> Result<(), KeyboardControlError> {
        validate_command(command)?;
        let index = self.index(principal, handle)?;
        let keyboard = &mut self.keyboards[index];
        if command.flags & KEYBOARD_CONTROL_FLAG_CLEAR_QUEUE != 0 {
            keyboard.commands.clear();
            keyboard.active = None;
            keyboard.modifiers = 0;
            keyboard.keys = [0; 6];
            emit_report(keyboard);
        }
        keyboard
            .commands
            .push_back(command)
            .map_err(|_| KeyboardControlError::QueueFull)
    }

    fn submit_program(
        &mut self,
        principal: KeyboardControlPrincipal,
        handle: u64,
        commands: &[KeyboardControlCommand],
    ) -> Result<(), KeyboardControlError> {
        if commands.is_empty() || commands.len() > MAX_COMMANDS_PER_KEYBOARD {
            return Err(KeyboardControlError::Invalid);
        }
        for command in commands {
            validate_command(*command)?;
        }
        let index = self.index(principal, handle)?;
        let clear = commands
            .first()
            .is_some_and(|command| command.flags & KEYBOARD_CONTROL_FLAG_CLEAR_QUEUE != 0);
        let keyboard = &mut self.keyboards[index];
        let available = if clear {
            MAX_COMMANDS_PER_KEYBOARD
        } else {
            MAX_COMMANDS_PER_KEYBOARD.saturating_sub(keyboard.commands.len())
        };
        if commands.len() > available {
            return Err(KeyboardControlError::QueueFull);
        }
        if clear {
            keyboard.commands.clear();
            keyboard.active = None;
            keyboard.modifiers = 0;
            keyboard.keys = [0; 6];
            emit_report(keyboard);
        }
        for command in commands.iter().copied() {
            keyboard
                .commands
                .push_back(command)
                .map_err(|_| KeyboardControlError::QueueFull)?;
        }
        Ok(())
    }

    fn tick(&mut self, now_ms: u64) {
        for keyboard in &mut self.keyboards {
            if let Some(active) = keyboard.active {
                if now_ms < active.finish_ms {
                    continue;
                }
                if active.key_code != 0 {
                    remove_key(&mut keyboard.keys, active.key_code);
                    keyboard.modifiers &= !active.modifiers;
                    emit_report(keyboard);
                }
                keyboard.active = None;
            }

            while keyboard.active.is_none() {
                let Some(command) = keyboard.commands.pop_front() else {
                    break;
                };
                match command.opcode {
                    KEYBOARD_CONTROL_OPCODE_STROKE => {
                        let duration = command
                            .duration_ms
                            .max(DEFAULT_STROKE_MS)
                            .min(MAX_COMMAND_MS);
                        if let Some((key_code, inferred_modifiers)) = command_key(command) {
                            let modifiers = command.modifiers | inferred_modifiers;
                            keyboard.modifiers |= modifiers;
                            insert_key(&mut keyboard.keys, key_code);
                            emit_report(keyboard);
                            keyboard.active = Some(ActiveKey {
                                key_code,
                                modifiers,
                                finish_ms: now_ms.saturating_add(u64::from(duration)),
                            });
                        } else if let Some(ch) = char::from_u32(command.codepoint) {
                            crate::r::keyboard::push_output_char(
                                0,
                                keyboard.capability.slot_id,
                                0,
                                uptime_ms_u32(),
                                keyboard.device_seq,
                                command.modifiers,
                                ch,
                                crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_SYNTHETIC,
                            );
                            keyboard.active = Some(ActiveKey {
                                key_code: 0,
                                modifiers: 0,
                                finish_ms: now_ms.saturating_add(u64::from(duration)),
                            });
                        }
                    }
                    KEYBOARD_CONTROL_OPCODE_DOWN => {
                        if let Some((key_code, inferred_modifiers)) = command_key(command) {
                            keyboard.modifiers |= command.modifiers | inferred_modifiers;
                            insert_key(&mut keyboard.keys, key_code);
                            emit_report(keyboard);
                        }
                    }
                    KEYBOARD_CONTROL_OPCODE_UP => {
                        if let Some((key_code, inferred_modifiers)) = command_key(command) {
                            remove_key(&mut keyboard.keys, key_code);
                            keyboard.modifiers &= !(command.modifiers | inferred_modifiers);
                            emit_report(keyboard);
                        }
                    }
                    KEYBOARD_CONTROL_OPCODE_WAIT => {
                        keyboard.active = Some(ActiveKey {
                            key_code: 0,
                            modifiers: 0,
                            finish_ms: now_ms.saturating_add(u64::from(command.duration_ms)),
                        });
                    }
                    _ => {}
                }
            }
        }
    }
}

static STATION: Mutex<KeyboardControlStation> = Mutex::new(KeyboardControlStation::new());

pub(crate) fn request_keyboard(
    principal: KeyboardControlPrincipal,
    label: &str,
) -> Result<KeyboardControlDevice, KeyboardControlError> {
    STATION.lock().request(principal, label)
}

pub(crate) fn release_keyboard(
    principal: KeyboardControlPrincipal,
    handle: u64,
) -> Result<(), KeyboardControlError> {
    STATION.lock().release(principal, handle)
}

pub(crate) fn submit_command(
    principal: KeyboardControlPrincipal,
    handle: u64,
    command: KeyboardControlCommand,
) -> Result<(), KeyboardControlError> {
    STATION.lock().submit(principal, handle, command)
}

pub(crate) fn submit_text(
    principal: KeyboardControlPrincipal,
    handle: u64,
    text: &str,
    interval_ms: u32,
    clear_queue: bool,
) -> Result<usize, KeyboardControlError> {
    let mut commands = AllocVec::new();
    for (index, ch) in text.chars().enumerate() {
        if commands.len() >= MAX_COMMANDS_PER_KEYBOARD {
            return Err(KeyboardControlError::Capacity);
        }
        commands.push(KeyboardControlCommand {
            opcode: KEYBOARD_CONTROL_OPCODE_STROKE,
            flags: if clear_queue && index == 0 {
                KEYBOARD_CONTROL_FLAG_CLEAR_QUEUE
            } else {
                0
            },
            duration_ms: interval_ms.max(DEFAULT_STROKE_MS),
            codepoint: ch as u32,
            ..KeyboardControlCommand::default()
        });
    }
    let count = commands.len();
    STATION
        .lock()
        .submit_program(principal, handle, commands.as_slice())?;
    Ok(count)
}

pub(crate) fn submit_json(
    principal: KeyboardControlPrincipal,
    handle: u64,
    bytes: &[u8],
) -> Result<usize, KeyboardControlError> {
    if bytes.is_empty() || bytes.len() > MAX_JSON_BYTES {
        return Err(KeyboardControlError::BadJson);
    }
    let value =
        serde_json::from_slice::<Value>(bytes).map_err(|_| KeyboardControlError::BadJson)?;
    let values: AllocVec<Value> = match value.get("commands").and_then(Value::as_array) {
        Some(commands) => commands.clone(),
        None => alloc::vec![value],
    };
    let mut commands = AllocVec::new();
    for value in &values {
        let object = value.as_object().ok_or(KeyboardControlError::BadJson)?;
        let op = object
            .get("op")
            .and_then(Value::as_str)
            .ok_or(KeyboardControlError::BadJson)?;
        if op == "text" {
            let text = object
                .get("text")
                .and_then(Value::as_str)
                .ok_or(KeyboardControlError::BadJson)?;
            let interval = json_u32(object.get("interval_ms"))?.max(DEFAULT_STROKE_MS);
            let clear_queue = object
                .get("clear_queue")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            for (index, ch) in text.chars().enumerate() {
                if commands.len() >= MAX_COMMANDS_PER_KEYBOARD {
                    return Err(KeyboardControlError::Capacity);
                }
                commands.push(KeyboardControlCommand {
                    opcode: KEYBOARD_CONTROL_OPCODE_STROKE,
                    flags: if clear_queue && index == 0 {
                        KEYBOARD_CONTROL_FLAG_CLEAR_QUEUE
                    } else {
                        0
                    },
                    duration_ms: interval,
                    codepoint: ch as u32,
                    ..KeyboardControlCommand::default()
                });
            }
            continue;
        }
        let opcode = match op {
            "stroke" => KEYBOARD_CONTROL_OPCODE_STROKE,
            "down" => KEYBOARD_CONTROL_OPCODE_DOWN,
            "up" => KEYBOARD_CONTROL_OPCODE_UP,
            "wait" => KEYBOARD_CONTROL_OPCODE_WAIT,
            _ => return Err(KeyboardControlError::BadJson),
        };
        if commands.len() >= MAX_COMMANDS_PER_KEYBOARD {
            return Err(KeyboardControlError::Capacity);
        }
        commands.push(KeyboardControlCommand {
            opcode,
            flags: if object
                .get("clear_queue")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                KEYBOARD_CONTROL_FLAG_CLEAR_QUEUE
            } else {
                0
            },
            modifiers: json_u32(object.get("modifiers"))?.min(u8::MAX as u32) as u8,
            duration_ms: json_u32(object.get("duration_ms"))?,
            codepoint: json_u32(object.get("codepoint"))?,
            key_code: json_u32(object.get("key_code"))?.min(u16::MAX as u32) as u16,
            ..KeyboardControlCommand::default()
        });
    }
    if commands.is_empty() || commands.len() > MAX_COMMANDS_PER_KEYBOARD {
        return Err(KeyboardControlError::Capacity);
    }
    let count = commands.len();
    STATION
        .lock()
        .submit_program(principal, handle, commands.as_slice())?;
    Ok(count)
}

pub(crate) fn keyboard_is_idle(
    principal: KeyboardControlPrincipal,
    handle: u64,
) -> Result<bool, KeyboardControlError> {
    let station = STATION.lock();
    let index = station.index(principal, handle)?;
    let keyboard = &station.keyboards[index];
    Ok(keyboard.active.is_none() && keyboard.commands.is_empty())
}

#[embassy_executor::task]
pub(crate) async fn keyboard_control_service_task() {
    crate::log_info!(target: "input";
        "keyboard-control: station online protocol=binary+json ops=stroke,down,up,wait,text sink=keyboard-hut+output-ring\n"
    );
    loop {
        STATION.lock().tick(uptime_ms());
        Timer::after(Duration::from_millis(TICK_MS)).await;
    }
}

fn validate_command(command: KeyboardControlCommand) -> Result<(), KeyboardControlError> {
    if command.duration_ms > MAX_COMMAND_MS {
        return Err(KeyboardControlError::Invalid);
    }
    match command.opcode {
        KEYBOARD_CONTROL_OPCODE_STROKE
        | KEYBOARD_CONTROL_OPCODE_DOWN
        | KEYBOARD_CONTROL_OPCODE_UP => {
            if command.key_code == 0 && char::from_u32(command.codepoint).is_none() {
                Err(KeyboardControlError::Invalid)
            } else {
                Ok(())
            }
        }
        KEYBOARD_CONTROL_OPCODE_WAIT if command.duration_ms != 0 => Ok(()),
        _ => Err(KeyboardControlError::Invalid),
    }
}

fn command_key(command: KeyboardControlCommand) -> Option<(u8, u8)> {
    if command.key_code != 0 {
        return u8::try_from(command.key_code).ok().map(|key| (key, 0));
    }
    char::from_u32(command.codepoint).and_then(hid_key_for_char)
}

fn hid_key_for_char(ch: char) -> Option<(u8, u8)> {
    const SHIFT: u8 = 1 << 1;
    match ch {
        'a'..='z' => Some((0x04 + (ch as u8 - b'a'), 0)),
        'A'..='Z' => Some((0x04 + (ch as u8 - b'A'), SHIFT)),
        '1'..='9' => Some((0x1E + (ch as u8 - b'1'), 0)),
        '0' => Some((0x27, 0)),
        '\n' | '\r' => Some((0x28, 0)),
        '\u{001b}' => Some((0x29, 0)),
        '\u{0008}' => Some((0x2A, 0)),
        '\t' => Some((0x2B, 0)),
        ' ' => Some((0x2C, 0)),
        '-' => Some((0x2D, 0)),
        '_' => Some((0x2D, SHIFT)),
        '=' => Some((0x2E, 0)),
        '+' => Some((0x2E, SHIFT)),
        '[' => Some((0x2F, 0)),
        '{' => Some((0x2F, SHIFT)),
        ']' => Some((0x30, 0)),
        '}' => Some((0x30, SHIFT)),
        '\\' => Some((0x31, 0)),
        ';' => Some((0x33, 0)),
        ':' => Some((0x33, SHIFT)),
        '\'' => Some((0x34, 0)),
        '"' => Some((0x34, SHIFT)),
        '`' => Some((0x35, 0)),
        '~' => Some((0x35, SHIFT)),
        ',' => Some((0x36, 0)),
        '<' => Some((0x36, SHIFT)),
        '.' => Some((0x37, 0)),
        '>' => Some((0x37, SHIFT)),
        '/' => Some((0x38, 0)),
        '?' => Some((0x38, SHIFT)),
        _ => None,
    }
}

fn insert_key(keys: &mut [u8; 6], key_code: u8) {
    if keys.contains(&key_code) {
        return;
    }
    if let Some(slot) = keys.iter_mut().find(|key| **key == 0) {
        *slot = key_code;
    }
}

fn remove_key(keys: &mut [u8; 6], key_code: u8) {
    for key in keys.iter_mut() {
        if *key == key_code {
            *key = 0;
        }
    }
}

fn emit_report(keyboard: &KeyboardRecord) {
    let ascii = crate::usb2::hid::keyboard::boot_ascii_for_keys(keyboard.modifiers, keyboard.keys);
    crate::r::keyboard::apply_report(
        0,
        keyboard.capability.slot_id,
        0,
        uptime_ms_u32(),
        keyboard.device_seq,
        keyboard.modifiers,
        keyboard.keys,
        ascii,
    );
    crate::usb2::hid::hut::upsert_keyboard_state(
        0,
        keyboard.capability.slot_id,
        0,
        keyboard.modifiers,
        keyboard.keys,
        ascii,
        crate::usb2::hid::hut::HidSourceKind::Ai,
        keyboard.label.as_str(),
        true,
    );
}

fn json_u32(value: Option<&Value>) -> Result<u32, KeyboardControlError> {
    match value {
        Some(value) => u32::try_from(value.as_u64().ok_or(KeyboardControlError::BadJson)?)
            .map_err(|_| KeyboardControlError::BadJson),
        None => Ok(0),
    }
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

fn uptime_ms_u32() -> u32 {
    uptime_ms().min(u64::from(u32::MAX)) as u32
}
