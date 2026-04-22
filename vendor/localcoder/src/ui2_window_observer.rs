use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::sync::{Mutex, OnceLock};

type Ui2WindowObserverHandler = fn(Ui2WindowObserverRequest) -> Result<String>;

pub type Ui2WindowObserverResult = Result<String>;
pub type Ui2WindowObserverError = anyhow::Error;

static UI2_WINDOW_OBSERVER_HANDLER: OnceLock<Mutex<Option<Ui2WindowObserverHandler>>> =
    OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ui2WindowObserverRequest {
    pub window_id: Option<u32>,
    pub title_contains: Option<String>,
    pub include_hidden: bool,
}

fn handler_cell() -> &'static Mutex<Option<Ui2WindowObserverHandler>> {
    UI2_WINDOW_OBSERVER_HANDLER.get_or_init(|| Mutex::new(None))
}

pub fn register_handler(handler: Ui2WindowObserverHandler) {
    let mut guard = handler_cell()
        .lock()
        .expect("ui2_window_observer handler lock poisoned");
    *guard = Some(handler);
}

pub fn observer_error(message: impl Into<String>) -> Ui2WindowObserverError {
    anyhow!(message.into())
}

pub fn is_registered() -> bool {
    handler_cell()
        .lock()
        .expect("ui2_window_observer handler lock poisoned")
        .is_some()
}

pub fn tool_name() -> &'static str {
    "ui2_window_observer"
}

pub fn tool_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool_name(),
            "description": "Inspect UI2 windows and their shell geometry only: frame, titlebar, window controls, resize handle, and content rect. This does not expose app-internal widgets.",
            "parameters": {
                "type": "object",
                "properties": {
                    "window_id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Optional exact window id to inspect."
                    },
                    "title_contains": {
                        "type": "string",
                        "description": "Optional case-insensitive title substring filter."
                    },
                    "include_hidden": {
                        "type": "boolean",
                        "description": "Optional. When false, only visible windows are returned. Defaults to true."
                    }
                },
                "required": [],
                "additionalProperties": false
            }
        }
    })
}

pub fn build_system_prompt() -> &'static str {
    "Use ui2_window_observer when you need UI2 window titles, window positions, or shell-level targets such as titlebars and system buttons. It does not reveal app-internal layouts or content widgets. Prefer this tool before ui2_window_controller when you are uncertain which window to target."
}

pub async fn execute_tool_call(arguments: Value) -> Result<String> {
    let request = request_from_value(&arguments)?;
    let handler = {
        let guard = handler_cell()
            .lock()
            .expect("ui2_window_observer handler lock poisoned");
        (*guard).ok_or_else(|| anyhow!("ui2_window_observer handler is not registered in this runtime"))?
    };
    handler(request)
}

fn request_from_value(input: &Value) -> Result<Ui2WindowObserverRequest> {
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

    let include_hidden = input
        .get("include_hidden")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    Ok(Ui2WindowObserverRequest {
        window_id,
        title_contains,
        include_hidden,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_include_hidden() {
        let request = request_from_value(&json!({})).unwrap();
        assert_eq!(request.window_id, None);
        assert_eq!(request.title_contains, None);
        assert!(request.include_hidden);
    }
}