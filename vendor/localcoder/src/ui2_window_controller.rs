use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::sync::{Mutex, OnceLock};

const DEFAULT_MOVE_DURATION_MS: u32 = 240;
const MAX_MOVE_DURATION_MS: u32 = 120_000;

type Ui2WindowControllerHandler = fn(Ui2WindowControllerCommand) -> Result<String>;

pub type Ui2WindowControllerResult = Result<String>;
pub type Ui2WindowControllerError = anyhow::Error;

static UI2_WINDOW_CONTROLLER_HANDLER: OnceLock<Mutex<Option<Ui2WindowControllerHandler>>> =
    OnceLock::new();

#[derive(Debug, Clone, PartialEq)]
pub enum Ui2WindowControllerCommand {
    Focus(Ui2WindowSelector),
    Minimize(Ui2WindowSelector),
    Maximize(Ui2WindowSelector),
    Restore(Ui2WindowSelector),
    Close(Ui2WindowSelector),
    Move {
        selector: Ui2WindowSelector,
        x_px: Option<i32>,
        y_px: Option<i32>,
        x_norm: Option<f64>,
        y_norm: Option<f64>,
        duration_ms: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ui2WindowSelector {
    pub window_id: Option<u32>,
    pub title_contains: Option<String>,
}

fn handler_cell() -> &'static Mutex<Option<Ui2WindowControllerHandler>> {
    UI2_WINDOW_CONTROLLER_HANDLER.get_or_init(|| Mutex::new(None))
}

pub fn register_handler(handler: Ui2WindowControllerHandler) {
    let mut guard = handler_cell()
        .lock()
        .expect("ui2_window_controller handler lock poisoned");
    *guard = Some(handler);
}

pub fn controller_error(message: impl Into<String>) -> Ui2WindowControllerError {
    anyhow!(message.into())
}

pub fn is_registered() -> bool {
    handler_cell()
        .lock()
        .expect("ui2_window_controller handler lock poisoned")
        .is_some()
}

pub fn tool_name() -> &'static str {
    "ui2_window_controller"
}

pub fn tool_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool_name(),
            "description": "Control UI2 windows using shell-level semantics. Standard shell actions like focus, minimize, maximize, restore, and close go through the UI2 kernel service. Move resolves shell geometry and drives the cursor movement tool underneath.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["focus", "minimize", "maximize", "restore", "close", "move"],
                        "description": "Window shell action to perform."
                    },
                    "window_id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Optional exact window id selector."
                    },
                    "title_contains": {
                        "type": "string",
                        "description": "Optional case-insensitive title substring selector."
                    },
                    "x_px": {
                        "type": "integer",
                        "description": "Target X pixel for action=move."
                    },
                    "y_px": {
                        "type": "integer",
                        "description": "Target Y pixel for action=move."
                    },
                    "x_norm": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "description": "Normalized target X for action=move."
                    },
                    "y_norm": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "description": "Normalized target Y for action=move."
                    },
                    "duration_ms": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": MAX_MOVE_DURATION_MS,
                        "description": "Optional move duration in milliseconds. Defaults to 240."
                    }
                },
                "required": ["action"],
                "additionalProperties": false
            }
        }
    })
}

pub fn build_system_prompt() -> &'static str {
    "Use ui2_window_controller for shell-level window actions only. Select the target window by window_id when known, otherwise by title_contains. focus, minimize, maximize, restore, and close go through direct UI2 kernel window controls. For move, provide either x_px/y_px or x_norm/y_norm; move resolves the proper shell target and then drives the cursor movement service for human-like interaction."
}

pub async fn execute_tool_call(arguments: Value) -> Result<String> {
    let command = command_from_value(&arguments)?;
    let handler = {
        let guard = handler_cell()
            .lock()
            .expect("ui2_window_controller handler lock poisoned");
        (*guard).ok_or_else(|| anyhow!("ui2_window_controller handler is not registered in this runtime"))?
    };
    handler(command)
}

fn command_from_value(input: &Value) -> Result<Ui2WindowControllerCommand> {
    let action = input
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("action must be a string"))?;
    let selector = selector_from_value(input)?;
    selector.validate()?;

    match action {
        "focus" => Ok(Ui2WindowControllerCommand::Focus(selector)),
        "minimize" => Ok(Ui2WindowControllerCommand::Minimize(selector)),
        "maximize" => Ok(Ui2WindowControllerCommand::Maximize(selector)),
        "restore" => Ok(Ui2WindowControllerCommand::Restore(selector)),
        "close" => Ok(Ui2WindowControllerCommand::Close(selector)),
        "move" => {
            let x_px = optional_i32(input, "x_px")?;
            let y_px = optional_i32(input, "y_px")?;
            let x_norm = optional_bounded_f64(input, "x_norm", 0.0, 1.0)?;
            let y_norm = optional_bounded_f64(input, "y_norm", 0.0, 1.0)?;
            validate_coordinate_mode(x_px, y_px, x_norm, y_norm)?;
            Ok(Ui2WindowControllerCommand::Move {
                selector,
                x_px,
                y_px,
                x_norm,
                y_norm,
                duration_ms: optional_bounded_u32_or_default(
                    input,
                    "duration_ms",
                    0,
                    MAX_MOVE_DURATION_MS,
                    DEFAULT_MOVE_DURATION_MS,
                )?,
            })
        }
        other => bail!("unsupported action: {}", other),
    }
}

fn selector_from_value(input: &Value) -> Result<Ui2WindowSelector> {
    let window_id = input
        .get("window_id")
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| anyhow!("window_id must be an unsigned integer"))
                .and_then(|value| {
                    u32::try_from(value).map_err(|_| anyhow!("window_id is out of u32 range"))
                })
        })
        .transpose()?;
    let title_contains = input
        .get("title_contains")
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| anyhow!("title_contains must be a string"))
        })
        .transpose()?;
    Ok(Ui2WindowSelector {
        window_id,
        title_contains,
    })
}

impl Ui2WindowSelector {
    fn validate(&self) -> Result<()> {
        let have_id = self.window_id.is_some();
        let have_title = self.title_contains.as_ref().is_some_and(|value| !value.trim().is_empty());
        if have_id == have_title {
            bail!("provide exactly one of window_id or title_contains");
        }
        Ok(())
    }
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

fn optional_bounded_u32_or_default(
    input: &Value,
    key: &str,
    min: u32,
    max: u32,
    default: u32,
) -> Result<u32> {
    Ok(optional_bounded_u32(input, key, min, max)?.unwrap_or(default))
}

fn optional_bounded_u32(input: &Value, key: &str, min: u32, max: u32) -> Result<Option<u32>> {
    let Some(value) = input.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_u64()
        .ok_or_else(|| anyhow!("{} must be an unsigned integer", key))?;
    let value = u32::try_from(value).map_err(|_| anyhow!("{} is out of u32 range", key))?;
    if value < min || value > max {
        bail!("{} must be between {} and {}", key, min, max);
    }
    Ok(Some(value))
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

fn validate_coordinate_mode(
    x_px: Option<i32>,
    y_px: Option<i32>,
    x_norm: Option<f64>,
    y_norm: Option<f64>,
) -> Result<()> {
    let pixel_pair = x_px.is_some() || y_px.is_some();
    let norm_pair = x_norm.is_some() || y_norm.is_some();
    if pixel_pair == norm_pair {
        bail!("move requires exactly one coordinate pair: pixels or normalized");
    }
    if pixel_pair && (x_px.is_none() || y_px.is_none()) {
        bail!("pixel coordinates require both x_px and y_px");
    }
    if norm_pair && (x_norm.is_none() || y_norm.is_none()) {
        bail!("normalized coordinates require both x_norm and y_norm");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_focus_by_title() {
        let command = command_from_value(&json!({
            "action": "focus",
            "title_contains": "Explorer",
        }))
        .unwrap();
        match command {
            Ui2WindowControllerCommand::Focus(selector) => {
                assert_eq!(selector.window_id, None);
                assert_eq!(selector.title_contains.as_deref(), Some("Explorer"));
            }
            _ => panic!("expected focus command"),
        }
    }
}
