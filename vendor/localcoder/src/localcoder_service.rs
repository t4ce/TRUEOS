use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::sync::{Mutex, OnceLock};

const MAX_MOVE_DURATION_MS: u32 = 120_000;
const DEFAULT_MOVE_DURATION_MS: u32 = 180;
const MAX_ORBIT_RADIUS_PX: u32 = 4_096;
const MAX_ORBIT_LOOP_DURATION_MS: u32 = 120_000;
const DEFAULT_ORBIT_LOOP_DURATION_MS: u32 = 1_400;
const MAX_ORBIT_LOOPS: u32 = 64;
const DEFAULT_ORBIT_LOOPS: u32 = 1;
const MAX_CLICK_REPEAT: u32 = 10;
const DEFAULT_CLICK_REPEAT: u32 = 1;
const MAX_CLICK_DELAY_MS: u32 = 3_000;
const DEFAULT_CLICK_DELAY_MS: u32 = 120;
const MAX_BUTTON_MASK: u32 = 255;
const DEFAULT_BUTTON_MASK: u32 = 1;

type LocalcoderServiceHandler = fn(LocalcoderServiceCommand) -> Result<String>;
type LocalcoderServiceContextProvider = fn() -> String;

pub type LocalcoderServiceResult = Result<String>;

static LOCALCODER_SERVICE_HANDLER: OnceLock<Mutex<Option<LocalcoderServiceHandler>>> =
    OnceLock::new();
static LOCALCODER_SERVICE_CONTEXT_PROVIDER: OnceLock<Mutex<Option<LocalcoderServiceContextProvider>>> =
    OnceLock::new();

#[derive(Debug, Clone)]
pub enum LocalcoderServiceCommand {
    SpawnCursor,
    MoveAbs {
        cursor_slot_id: Option<u32>,
        x_px: Option<i32>,
        y_px: Option<i32>,
        x_norm: Option<f64>,
        y_norm: Option<f64>,
        duration_ms: u32,
    },
    Orbit {
        cursor_slot_id: Option<u32>,
        center_x_px: Option<i32>,
        center_y_px: Option<i32>,
        center_x_norm: Option<f64>,
        center_y_norm: Option<f64>,
        radius_px: Option<u32>,
        radius_norm: Option<f64>,
        loop_duration_ms: u32,
        loops: u32,
    },
    Click {
        cursor_slot_id: Option<u32>,
        buttons_down: u32,
        repeat: u32,
        delay_ms: u32,
    },
    ButtonDown {
        cursor_slot_id: Option<u32>,
        buttons_down: u32,
    },
    ButtonUp {
        cursor_slot_id: Option<u32>,
        buttons_up: u32,
    },
    SetButtons {
        cursor_slot_id: Option<u32>,
        buttons_down: u32,
    },
}

fn handler_cell() -> &'static Mutex<Option<LocalcoderServiceHandler>> {
    LOCALCODER_SERVICE_HANDLER.get_or_init(|| Mutex::new(None))
}

fn context_provider_cell() -> &'static Mutex<Option<LocalcoderServiceContextProvider>> {
    LOCALCODER_SERVICE_CONTEXT_PROVIDER.get_or_init(|| Mutex::new(None))
}

pub fn register_handler(handler: LocalcoderServiceHandler) {
    let mut guard = handler_cell().lock().expect("localcoder service handler lock poisoned");
    *guard = Some(handler);
}

pub fn service_error(message: impl Into<String>) -> anyhow::Error {
    anyhow!(message.into())
}

pub fn register_context_provider(provider: LocalcoderServiceContextProvider) {
    let mut guard = context_provider_cell()
        .lock()
        .expect("localcoder service context provider lock poisoned");
    *guard = Some(provider);
}

pub fn is_registered() -> bool {
    handler_cell()
        .lock()
        .expect("localcoder service handler lock poisoned")
        .is_some()
}

pub fn tool_name() -> &'static str {
    "localcoder_service"
}

pub fn tool_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool_name(),
            "description": "Drive the TRUEOS AI cursor with smooth absolute motion, orbit motion, clicks, and explicit button state changes.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["spawn_cursor", "move_abs", "orbit", "click", "button_down", "button_up", "set_buttons"],
                        "description": "Cursor action to queue."
                    },
                    "cursor_slot_id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Optional AI cursor slot to target. Omit to use the default AI cursor. Use spawn_cursor first to create additional cursors."
                    },
                    "x_px": {
                        "type": "integer",
                        "description": "Absolute X coordinate in pixels for move_abs. Use x_px/y_px together."
                    },
                    "y_px": {
                        "type": "integer",
                        "description": "Absolute Y coordinate in pixels for move_abs. Use x_px/y_px together."
                    },
                    "x_norm": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "description": "Normalized X coordinate for move_abs where 0 is left and 1 is right. Use x_norm/y_norm together."
                    },
                    "y_norm": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "description": "Normalized Y coordinate for move_abs where 0 is top and 1 is bottom. Use x_norm/y_norm together."
                    },
                    "duration_ms": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": MAX_MOVE_DURATION_MS,
                        "description": "Total movement time in milliseconds for move_abs."
                    },
                    "center_x_px": {
                        "type": "integer",
                        "description": "Absolute X center in pixels for orbit. Use center_x_px/center_y_px together."
                    },
                    "center_y_px": {
                        "type": "integer",
                        "description": "Absolute Y center in pixels for orbit. Use center_x_px/center_y_px together."
                    },
                    "center_x_norm": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "description": "Normalized X center for orbit. Use center_x_norm/center_y_norm together."
                    },
                    "center_y_norm": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "description": "Normalized Y center for orbit. Use center_x_norm/center_y_norm together."
                    },
                    "radius_px": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_ORBIT_RADIUS_PX,
                        "description": "Orbit radius in pixels."
                    },
                    "radius_norm": {
                        "type": "number",
                        "minimum": 0.001,
                        "maximum": 1,
                        "description": "Orbit radius as a normalized fraction of the smaller viewport dimension."
                    },
                    "loop_duration_ms": {
                        "type": "integer",
                        "minimum": 16,
                        "maximum": MAX_ORBIT_LOOP_DURATION_MS,
                        "description": "Milliseconds for one completed orbit loop."
                    },
                    "loops": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_ORBIT_LOOPS,
                        "description": "Number of orbit loops to run."
                    },
                    "buttons_down": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_BUTTON_MASK,
                        "description": "Mouse button bitmask. left=1, right=2, middle=4, back=8, forward=16. Prefer button or buttons when possible."
                    },
                    "buttons_up": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_BUTTON_MASK,
                        "description": "Mouse button bitmask to release for button_up. Prefer button or buttons when possible."
                    },
                    "button": {
                        "type": "string",
                        "enum": ["left", "right", "middle", "back", "forward"],
                        "description": "Preferred single-button selector for click/button_down/button_up/set_buttons."
                    },
                    "buttons": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": ["left", "right", "middle", "back", "forward"]
                        },
                        "minItems": 1,
                        "uniqueItems": true,
                        "description": "Preferred multi-button selector for click/button_down/button_up/set_buttons."
                    },
                    "repeat": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_CLICK_REPEAT,
                        "description": "How many clicks to perform. Optional, defaults to 1."
                    },
                    "delay_ms": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": MAX_CLICK_DELAY_MS,
                        "description": "Delay between clicks in milliseconds. Optional, defaults to 120."
                    }
                },
                "required": ["action"],
                "additionalProperties": false
            }
        }
    })
}

pub fn build_system_prompt() -> String {
    let mut out = String::from(
        "The localcoder_service tool controls TRUEOS AI cursors. Use it only when direct pointer movement or clicking is required. Prefer normalized coordinates x_norm/y_norm unless you specifically need pixel precision. Prefer named buttons with button or buttons instead of raw bitmasks. Call spawn_cursor when you need an additional cursor; it returns a cursor_slot_id you can pass into later calls. If cursor_slot_id is omitted, the default AI cursor is used. For move_abs, provide either x_px/y_px or x_norm/y_norm; duration_ms is optional and defaults to 180. For orbit, provide either center_x_px/center_y_px or center_x_norm/center_y_norm, plus radius_px or radius_norm; loop_duration_ms defaults to 1400 and loops defaults to 1. For click, button defaults to left, repeat defaults to 1, and delay_ms defaults to 120. For drag-like behavior, call button_down, then move_abs or orbit, then button_up. set_buttons forces the full current button state. Prefer realistic timings so motion stays smooth and avoid redundant tool calls when one click or move call is enough.",
    );
    if let Some(provider) = *context_provider_cell()
        .lock()
        .expect("localcoder service context provider lock poisoned")
    {
        let extra = provider();
        if !extra.trim().is_empty() {
            out.push_str("\n\n");
            out.push_str(extra.trim());
        }
    }
    out
}

pub async fn execute_tool_call(arguments: Value) -> Result<String> {
    let command = command_from_value(&arguments)?;
    let handler = {
        let guard = handler_cell()
            .lock()
            .expect("localcoder service handler lock poisoned");
        (*guard).ok_or_else(|| anyhow!("localcoder_service handler is not registered in this runtime"))?
    };
    handler(command)
}

fn command_from_value(input: &Value) -> Result<LocalcoderServiceCommand> {
    let action = input
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("action must be a string"))?;

    match action {
        "spawn_cursor" => Ok(LocalcoderServiceCommand::SpawnCursor),
        "move_abs" => Ok(LocalcoderServiceCommand::MoveAbs {
            cursor_slot_id: optional_cursor_slot_id(input)?,
            x_px: optional_i32(input, "x_px")?,
            y_px: optional_i32(input, "y_px")?,
            x_norm: optional_norm(input, "x_norm")?,
            y_norm: optional_norm(input, "y_norm")?,
            duration_ms: optional_bounded_u32_or_default(
                input,
                "duration_ms",
                0,
                MAX_MOVE_DURATION_MS,
                DEFAULT_MOVE_DURATION_MS,
            )?,
        }),
        "orbit" => Ok(LocalcoderServiceCommand::Orbit {
            cursor_slot_id: optional_cursor_slot_id(input)?,
            center_x_px: optional_i32(input, "center_x_px")?,
            center_y_px: optional_i32(input, "center_y_px")?,
            center_x_norm: optional_norm(input, "center_x_norm")?,
            center_y_norm: optional_norm(input, "center_y_norm")?,
            radius_px: optional_bounded_u32(input, "radius_px", 1, MAX_ORBIT_RADIUS_PX)?,
            radius_norm: optional_bounded_f64(input, "radius_norm", 0.001, 1.0)?,
            loop_duration_ms: optional_bounded_u32_or_default(
                input,
                "loop_duration_ms",
                16,
                MAX_ORBIT_LOOP_DURATION_MS,
                DEFAULT_ORBIT_LOOP_DURATION_MS,
            )?,
            loops: optional_bounded_u32_or_default(
                input,
                "loops",
                1,
                MAX_ORBIT_LOOPS,
                DEFAULT_ORBIT_LOOPS,
            )?,
        }),
        "click" => Ok(LocalcoderServiceCommand::Click {
            cursor_slot_id: optional_cursor_slot_id(input)?,
            buttons_down: parse_button_mask(input, ButtonMaskKind::Down, DEFAULT_BUTTON_MASK)?,
            repeat: optional_bounded_u32_or_default(
                input,
                "repeat",
                1,
                MAX_CLICK_REPEAT,
                DEFAULT_CLICK_REPEAT,
            )?,
            delay_ms: optional_bounded_u32_or_default(
                input,
                "delay_ms",
                0,
                MAX_CLICK_DELAY_MS,
                DEFAULT_CLICK_DELAY_MS,
            )?,
        }),
        "button_down" => Ok(LocalcoderServiceCommand::ButtonDown {
            cursor_slot_id: optional_cursor_slot_id(input)?,
            buttons_down: parse_button_mask(input, ButtonMaskKind::Down, DEFAULT_BUTTON_MASK)?,
        }),
        "button_up" => Ok(LocalcoderServiceCommand::ButtonUp {
            cursor_slot_id: optional_cursor_slot_id(input)?,
            buttons_up: parse_button_mask(input, ButtonMaskKind::Up, DEFAULT_BUTTON_MASK)?,
        }),
        "set_buttons" => Ok(LocalcoderServiceCommand::SetButtons {
            cursor_slot_id: optional_cursor_slot_id(input)?,
            buttons_down: parse_button_mask(input, ButtonMaskKind::Down, DEFAULT_BUTTON_MASK)?,
        }),
        other => bail!("unsupported action: {}", other),
    }
    .and_then(validate_command)
}

fn validate_command(command: LocalcoderServiceCommand) -> Result<LocalcoderServiceCommand> {
    match &command {
        LocalcoderServiceCommand::SpawnCursor => {}
        LocalcoderServiceCommand::MoveAbs {
            x_px,
            y_px,
            x_norm,
            y_norm,
            ..
        } => validate_coordinate_mode(*x_px, *y_px, *x_norm, *y_norm, "move_abs")?,
        LocalcoderServiceCommand::Orbit {
            center_x_px,
            center_y_px,
            center_x_norm,
            center_y_norm,
            radius_px,
            radius_norm,
            ..
        } => {
            validate_coordinate_mode(
                *center_x_px,
                *center_y_px,
                *center_x_norm,
                *center_y_norm,
                "orbit center",
            )?;
            match (radius_px, radius_norm) {
                (Some(_), None) | (None, Some(_)) => {}
                _ => bail!("orbit requires exactly one of radius_px or radius_norm"),
            }
        }
        LocalcoderServiceCommand::Click { .. }
        | LocalcoderServiceCommand::ButtonDown { .. }
        | LocalcoderServiceCommand::ButtonUp { .. }
        | LocalcoderServiceCommand::SetButtons { .. } => {}
    }
    Ok(command)
}

fn validate_coordinate_mode(
    x_px: Option<i32>,
    y_px: Option<i32>,
    x_norm: Option<f64>,
    y_norm: Option<f64>,
    label: &str,
) -> Result<()> {
    let pixel_pair = x_px.is_some() || y_px.is_some();
    let norm_pair = x_norm.is_some() || y_norm.is_some();
    if pixel_pair == norm_pair {
        bail!("{} requires exactly one coordinate pair: pixels or normalized", label);
    }
    if pixel_pair && (x_px.is_none() || y_px.is_none()) {
        bail!("{} pixel coordinates require both x and y", label);
    }
    if norm_pair && (x_norm.is_none() || y_norm.is_none()) {
        bail!("{} normalized coordinates require both x and y", label);
    }
    Ok(())
}

fn optional_cursor_slot_id(input: &Value) -> Result<Option<u32>> {
    optional_bounded_u32(input, "cursor_slot_id", 1, u32::MAX)
}

fn optional_i32(input: &Value, key: &str) -> Result<Option<i32>> {
    let Some(value) = input.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_i64()
        .ok_or_else(|| anyhow!("{} must be an integer", key))?;
    Ok(Some(
        i32::try_from(value).map_err(|_| anyhow!("{} is out of i32 range", key))?,
    ))
}

fn bounded_u32(input: &Value, key: &str, min: u32, max: u32) -> Result<u32> {
    let value = input
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("{} must be an unsigned integer", key))?;
    let value = u32::try_from(value).map_err(|_| anyhow!("{} is out of u32 range", key))?;
    if value < min || value > max {
        bail!("{} must be between {} and {}", key, min, max);
    }
    Ok(value)
}

fn optional_bounded_u32(input: &Value, key: &str, min: u32, max: u32) -> Result<Option<u32>> {
    if input.get(key).is_none() {
        return Ok(None);
    }
    Ok(Some(bounded_u32(input, key, min, max)?))
}

fn optional_bounded_u32_or_default(
    input: &Value,
    key: &str,
    min: u32,
    max: u32,
    default: u32,
) -> Result<u32> {
    Ok(optional_bounded_u32(input, key, min, max)?.unwrap_or(default))
}

fn optional_norm(input: &Value, key: &str) -> Result<Option<f64>> {
    optional_bounded_f64(input, key, 0.0, 1.0)
}

fn optional_bounded_f64(input: &Value, key: &str, min: f64, max: f64) -> Result<Option<f64>> {
    let Some(value) = input.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_f64()
        .ok_or_else(|| anyhow!("{} must be a number", key))?;
    if value < min || value > max {
        bail!("{} must be between {} and {}", key, min, max);
    }
    Ok(Some(value))
}

#[derive(Copy, Clone)]
enum ButtonMaskKind {
    Down,
    Up,
}

fn parse_button_mask(input: &Value, kind: ButtonMaskKind, default: u32) -> Result<u32> {
    if let Some(mask) = parse_named_buttons(input)? {
        return Ok(mask);
    }

    let key = match kind {
        ButtonMaskKind::Down => "buttons_down",
        ButtonMaskKind::Up => "buttons_up",
    };
    Ok(optional_bounded_u32(input, key, 1, MAX_BUTTON_MASK)?.unwrap_or(default))
}

fn parse_named_buttons(input: &Value) -> Result<Option<u32>> {
    if let Some(button) = input.get("button") {
        let name = button
            .as_str()
            .ok_or_else(|| anyhow!("button must be a string"))?;
        return Ok(Some(button_name_to_mask(name)?));
    }

    let Some(buttons) = input.get("buttons") else {
        return Ok(None);
    };
    let items = buttons
        .as_array()
        .ok_or_else(|| anyhow!("buttons must be an array of strings"))?;
    if items.is_empty() {
        bail!("buttons must contain at least one entry");
    }

    let mut mask = 0u32;
    for item in items {
        let name = item
            .as_str()
            .ok_or_else(|| anyhow!("buttons entries must be strings"))?;
        mask |= button_name_to_mask(name)?;
    }
    Ok(Some(mask))
}

fn button_name_to_mask(name: &str) -> Result<u32> {
    match name {
        "left" => Ok(1),
        "right" => Ok(2),
        "middle" => Ok(4),
        "back" => Ok(8),
        "forward" => Ok(16),
        other => bail!("unsupported button name: {}", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_move_command() {
        let command = command_from_value(&json!({
            "action": "move_abs",
            "cursor_slot_id": 7,
            "x_norm": 0.5,
            "y_norm": 0.5,
            "duration_ms": 250,
        }))
        .unwrap();
        match command {
            LocalcoderServiceCommand::MoveAbs {
                cursor_slot_id,
                x_norm,
                y_norm,
                duration_ms,
                ..
            } => {
                assert_eq!(cursor_slot_id, Some(7));
                assert_eq!(x_norm, Some(0.5));
                assert_eq!(y_norm, Some(0.5));
                assert_eq!(duration_ms, 250);
            }
            _ => panic!("expected move_abs command"),
        }
    }

    #[test]
    fn rejects_click_repeat_over_limit() {
        let err = command_from_value(&json!({
            "action": "click",
            "repeat": 11,
            "delay_ms": 0,
        }))
        .unwrap_err();
        assert!(err.to_string().contains("repeat"));
    }

    #[test]
    fn parse_button_up_command() {
        let command = command_from_value(&json!({
            "action": "button_up",
            "cursor_slot_id": 9,
            "buttons_up": 8,
        }))
        .unwrap();
        match command {
            LocalcoderServiceCommand::ButtonUp {
                cursor_slot_id,
                buttons_up,
            } => {
                assert_eq!(cursor_slot_id, Some(9));
                assert_eq!(buttons_up, 8);
            }
            _ => panic!("expected button_up command"),
        }
    }

    #[test]
    fn click_defaults_to_left_button_and_single_repeat() {
        let command = command_from_value(&json!({
            "action": "click",
        }))
        .unwrap();
        match command {
            LocalcoderServiceCommand::Click {
                cursor_slot_id,
                buttons_down,
                repeat,
                delay_ms,
            } => {
                assert_eq!(cursor_slot_id, None);
                assert_eq!(buttons_down, 1);
                assert_eq!(repeat, 1);
                assert_eq!(delay_ms, 120);
            }
            _ => panic!("expected click command"),
        }
    }

    #[test]
    fn button_names_expand_to_mask() {
        let command = command_from_value(&json!({
            "action": "button_down",
            "buttons": ["left", "forward"],
        }))
        .unwrap();
        match command {
            LocalcoderServiceCommand::ButtonDown {
                cursor_slot_id,
                buttons_down,
            } => {
                assert_eq!(cursor_slot_id, None);
                assert_eq!(buttons_down, 17);
            }
            _ => panic!("expected button_down command"),
        }
    }

    #[test]
    fn parse_spawn_cursor_command() {
        let command = command_from_value(&json!({
            "action": "spawn_cursor",
        }))
        .unwrap();
        match command {
            LocalcoderServiceCommand::SpawnCursor => {}
            _ => panic!("expected spawn_cursor command"),
        }
    }
}
