use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use github_copilot_sdk::session::Session;
use github_copilot_sdk::session_events::{
    AssistantMessageData, ExternalToolRequestedData, SessionEventType,
};
use github_copilot_sdk::types::{
    Attachment, DeferMode, MessageOptions, SessionConfig, SystemMessageConfig, Tool,
};
use github_copilot_sdk::{Client, ClientMode, ClientOptions, LogLevel};
use serde_json::Value;
use tokio::sync::{Semaphore, TryAcquireError, oneshot};

use crate::openai::{NormalizedChat, OpenAiTool};

pub const MAX_ORDERED_TOOL_CALLS: usize = 8;
const BATCH_TOOL_NAME: &str = "remoteai_ordered_tool_batch_v1";
const MAX_TOOL_ARGUMENT_BYTES: usize = 4 * 1024;
const MAX_BATCH_ARGUMENT_BYTES: usize = MAX_ORDERED_TOOL_CALLS * MAX_TOOL_ARGUMENT_BYTES + 4 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct CapturedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BackendReply {
    Text(String),
    Tool(CapturedToolCall),
    Tools(Vec<CapturedToolCall>),
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("Copilot runtime is unavailable: {0}")]
    Runtime(String),
    #[error("Copilot session failed: {0}")]
    Session(String),
    #[error("Copilot returned no assistant content or tool call")]
    Empty,
    #[error("remoteAI is shutting down")]
    Closed,
    #[error("another completion is already in progress")]
    Busy,
    #[error("completion caller disconnected")]
    Cancelled,
}

#[async_trait]
pub trait ChatBackend: Send + Sync + 'static {
    async fn complete(&self, chat: NormalizedChat) -> Result<BackendReply, BackendError>;

    async fn shutdown(&self) -> Result<(), BackendError> {
        Ok(())
    }
}

pub struct CopilotBackend {
    client: Client,
    model: String,
    timeout: Duration,
    single_flight: Arc<Semaphore>,
}

impl CopilotBackend {
    pub async fn start(
        model: String,
        timeout: Duration,
        base_directory: PathBuf,
        github_token: Option<String>,
    ) -> Result<Self, BackendError> {
        std::fs::create_dir_all(&base_directory)
            .map_err(|error| BackendError::Runtime(error.to_string()))?;
        secure_directory(&base_directory)
            .map_err(|error| BackendError::Runtime(error.to_string()))?;
        let bundled_cli_directory = base_directory.join("bundled-cli");
        std::fs::create_dir_all(&bundled_cli_directory)
            .map_err(|error| BackendError::Runtime(error.to_string()))?;

        let mut options = ClientOptions::new()
            .with_mode(ClientMode::Empty)
            .with_base_directory(base_directory)
            .with_bundled_cli_extract_dir(bundled_cli_directory)
            .with_log_level(LogLevel::Error)
            .with_session_idle_timeout_seconds(timeout.as_secs().max(1));
        options = match github_token {
            Some(token) => options.with_github_token(token),
            None => options.with_use_logged_in_user(true),
        };
        let client = Client::start(options)
            .await
            .map_err(|error| BackendError::Runtime(error.to_string()))?;

        Ok(Self {
            client,
            model,
            timeout,
            single_flight: Arc::new(Semaphore::new(1)),
        })
    }
}

#[async_trait]
impl ChatBackend for CopilotBackend {
    async fn complete(&self, chat: NormalizedChat) -> Result<BackendReply, BackendError> {
        let permit =
            self.single_flight
                .clone()
                .try_acquire_owned()
                .map_err(|error| match error {
                    TryAcquireError::Closed => BackendError::Closed,
                    TryAcquireError::NoPermits => BackendError::Busy,
                })?;
        let client = self.client.clone();
        let model = self.model.clone();
        let timeout = self.timeout;
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let mut cancel_on_drop = CancelOnDrop(Some(cancel_tx));

        let result = tokio::spawn(async move {
            let _permit = permit;
            complete_one(client, model, timeout, chat, cancel_rx).await
        })
        .await
        .map_err(|error| BackendError::Runtime(format!("completion task failed: {error}")))?;
        cancel_on_drop.disarm();
        result
    }

    async fn shutdown(&self) -> Result<(), BackendError> {
        match tokio::time::timeout(Duration::from_secs(5), self.client.stop()).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(BackendError::Runtime(error.to_string())),
            Err(_) => {
                self.client.force_stop();
                Err(BackendError::Runtime("Copilot runtime shutdown timed out".to_string()))
            }
        }
    }
}

async fn complete_one(
    client: Client,
    model: String,
    timeout: Duration,
    chat: NormalizedChat,
    mut cancellation: oneshot::Receiver<()>,
) -> Result<BackendReply, BackendError> {
    let tools = copilot_tools(&chat);
    let available_tools = if chat.parallel_tool_calls {
        vec![format!("custom:{BATCH_TOOL_NAME}")]
    } else {
        chat.tools
            .iter()
            .map(|tool| format!("custom:{}", tool.function.name))
            .collect()
    };
    let system_prompt = if chat.system_prompt.trim().is_empty() {
        "Follow the user's request precisely. Do not use host capabilities.".to_string()
    } else {
        chat.system_prompt.clone()
    };
    let mut config = SessionConfig::default()
        .with_model(model)
        .with_streaming(false)
        .with_system_message(
            SystemMessageConfig::new()
                .with_mode("replace")
                .with_content(system_prompt),
        )
        .with_tools(tools)
        .with_available_tools(available_tools)
        .deny_all_permissions()
        .with_enable_config_discovery(false)
        .with_skip_embedding_retrieval(true)
        .with_enable_on_demand_instruction_discovery(false)
        .with_enable_file_hooks(false)
        .with_enable_host_git_operations(false)
        .with_enable_session_store(false)
        .with_enable_skills(false)
        .with_enable_session_telemetry(false)
        .with_skip_custom_instructions(true);
    if let Some(reasoning_effort) = chat.reasoning_effort.as_deref() {
        config = config.with_reasoning_effort(reasoning_effort);
    }
    let session = client
        .create_session(config)
        .await
        .map_err(|error| BackendError::Session(error.to_string()))?;
    let session_id = session.id().clone();

    let result = tokio::select! {
        biased;
        _ = &mut cancellation => Err(BackendError::Cancelled),
        result = async {
            if chat.tools.is_empty() {
                complete_summary(&session, &chat, timeout).await
            } else {
                complete_tool_turn(&session, &chat, timeout).await
            }
        } => result,
    };
    if result.is_err() || matches!(result, Ok(BackendReply::Tool(_) | BackendReply::Tools(_))) {
        let _ = session.abort().await;
    }
    let disconnect_result = session.disconnect().await;
    let _ = client.delete_session(&session_id).await;

    match (result, disconnect_result) {
        (Ok(reply), Ok(())) => Ok(reply),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(BackendError::Session(error.to_string())),
    }
}

struct CancelOnDrop(Option<oneshot::Sender<()>>);

impl CancelOnDrop {
    fn disarm(&mut self) {
        self.0.take();
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

async fn complete_summary(
    session: &Session,
    chat: &NormalizedChat,
    timeout: Duration,
) -> Result<BackendReply, BackendError> {
    let options = message_options(chat).with_wait_timeout(timeout);
    let event = session
        .send_and_wait(options)
        .await
        .map_err(|error| BackendError::Session(error.to_string()))?
        .ok_or(BackendError::Empty)?;
    let content = event
        .typed_data::<AssistantMessageData>()
        .map(|data| bounded_text(&data.content, chat.max_completion_tokens))
        .filter(|text| !text.trim().is_empty())
        .ok_or(BackendError::Empty)?;
    Ok(BackendReply::Text(content))
}

async fn complete_tool_turn(
    session: &Session,
    chat: &NormalizedChat,
    timeout: Duration,
) -> Result<BackendReply, BackendError> {
    let mut events = session.subscribe();
    let options = message_options(chat);
    session
        .send(options)
        .await
        .map_err(|error| BackendError::Session(error.to_string()))?;

    let result = tokio::time::timeout(timeout, async {
        let mut assistant_text = None;
        loop {
            let event = events
                .recv()
                .await
                .map_err(|error| BackendError::Session(error.to_string()))?;
            match event.parsed_type() {
                SessionEventType::ExternalToolRequested => {
                    let data =
                        event
                            .typed_data::<ExternalToolRequestedData>()
                            .ok_or_else(|| {
                                BackendError::Session("malformed external tool request".to_string())
                            })?;
                    break sanitize_tool_reply(chat, data);
                }
                SessionEventType::AssistantMessage => {
                    assistant_text = event
                        .typed_data::<AssistantMessageData>()
                        .map(|data| data.content);
                }
                SessionEventType::SessionIdle => {
                    let fallback = assistant_text
                        .as_deref()
                        .and_then(|text| fallback_text_tool(&chat.tools, text))
                        .map(|call| {
                            if chat.parallel_tool_calls {
                                BackendReply::Tools(vec![call])
                            } else {
                                BackendReply::Tool(call)
                            }
                        })
                        .ok_or(BackendError::Empty);
                    break fallback;
                }
                SessionEventType::SessionError => {
                    break Err(BackendError::Session(
                        "Copilot session ended before requesting a tool".to_string(),
                    ));
                }
                _ => {}
            }
        }
    })
    .await
    .map_err(|_| BackendError::Session("Copilot request timed out".to_string()))?;

    // Let the abort event settle briefly so session.destroy cannot race an
    // active model/tool turn. Failure here is harmless; cleanup still runs.
    let _ = session.abort().await;
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while let Ok(event) = events.recv().await {
            if matches!(
                event.parsed_type(),
                SessionEventType::SessionIdle | SessionEventType::SessionError
            ) {
                break;
            }
        }
    })
    .await;
    result
}

fn message_options(chat: &NormalizedChat) -> MessageOptions {
    let mut options = MessageOptions::new(chat.prompt.clone());
    if !chat.attachments.is_empty() {
        options = options.with_attachments(
            chat.attachments
                .iter()
                .map(|attachment| Attachment::Blob {
                    data: attachment.data_base64.clone(),
                    mime_type: "image/png".to_string(),
                    display_name: Some(attachment.display_name.clone()),
                })
                .collect(),
        );
    }
    options
}

fn copilot_tools(chat: &NormalizedChat) -> Vec<Tool> {
    if chat.parallel_tool_calls {
        return vec![batch_tool(&chat.tools)];
    }
    chat.tools
        .iter()
        .map(|definition| {
            Tool::new(definition.function.name.as_str())
                .with_description(definition.function.description.as_str())
                .with_parameters(definition.function.parameters.clone())
                .with_skip_permission(true)
                .with_defer(DeferMode::Never)
        })
        .collect()
}

fn batch_tool(definitions: &[OpenAiTool]) -> Tool {
    let variants: Vec<Value> = definitions
        .iter()
        .map(|definition| {
            serde_json::json!({
                "type": "object",
                "description": definition.function.description,
                "properties": {
                    "name": { "type": "string", "const": definition.function.name },
                    "arguments": definition.function.parameters,
                },
                "required": ["name", "arguments"],
                "additionalProperties": false,
            })
        })
        .collect();
    let parameters = serde_json::json!({
        "type": "object",
        "properties": {
            "calls": {
                "type": "array",
                "description": "Original custom-tool calls in the exact order they must execute.",
                "minItems": 1,
                "maxItems": MAX_ORDERED_TOOL_CALLS,
                "items": { "oneOf": variants },
            }
        },
        "required": ["calls"],
        "additionalProperties": false,
    });
    Tool::new(BATCH_TOOL_NAME)
        .with_description(
            "Submit one bounded ordered batch of the caller's custom tools. This relay never executes them.",
        )
        .with_parameters(parameters)
        .with_skip_permission(true)
        .with_defer(DeferMode::Never)
}

fn sanitize_tool_reply(
    chat: &NormalizedChat,
    data: ExternalToolRequestedData,
) -> Result<BackendReply, BackendError> {
    if chat.parallel_tool_calls {
        if data.tool_name != BATCH_TOOL_NAME {
            return Err(BackendError::Session(
                "Copilot requested a tool outside the internal batch boundary".to_string(),
            ));
        }
        let calls = expand_batch_calls(
            &chat.tools,
            data.tool_call_id.as_str(),
            data.arguments
                .unwrap_or_else(|| Value::Object(serde_json::Map::new())),
        )?;
        return Ok(BackendReply::Tools(calls));
    }
    sanitize_tool_call(&chat.tools, data).map(BackendReply::Tool)
}

fn sanitize_tool_call(
    definitions: &[OpenAiTool],
    data: ExternalToolRequestedData,
) -> Result<CapturedToolCall, BackendError> {
    let arguments = data
        .arguments
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    let arguments = sanitize_arguments(definitions, data.tool_name.as_str(), arguments, false)?;
    // Keep the original one-tool response contract byte-for-byte when batch
    // mode was not requested.
    let id = if data.tool_call_id.is_empty() || data.tool_call_id.len() > 256 {
        "call_remoteai_1".to_string()
    } else {
        data.tool_call_id
    };
    Ok(CapturedToolCall {
        id,
        name: data.tool_name,
        arguments,
    })
}

fn expand_batch_calls(
    definitions: &[OpenAiTool],
    batch_id: &str,
    arguments: Value,
) -> Result<Vec<CapturedToolCall>, BackendError> {
    let batch_bytes =
        serde_json::to_vec(&arguments).map_err(|error| BackendError::Session(error.to_string()))?;
    if batch_bytes.len() > MAX_BATCH_ARGUMENT_BYTES {
        return Err(BackendError::Session("Copilot ordered tool batch is too large".to_string()));
    }
    let object = arguments.as_object().ok_or_else(|| {
        BackendError::Session("Copilot ordered tool batch must be an object".to_string())
    })?;
    if object.len() != 1 {
        return Err(BackendError::Session(
            "Copilot ordered tool batch has unexpected fields".to_string(),
        ));
    }
    let calls = object
        .get("calls")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            BackendError::Session("Copilot ordered tool batch has no calls array".to_string())
        })?;
    if calls.is_empty() || calls.len() > MAX_ORDERED_TOOL_CALLS {
        return Err(BackendError::Session(format!(
            "Copilot ordered tool batch must contain 1..={MAX_ORDERED_TOOL_CALLS} calls"
        )));
    }

    let base_id = bounded_call_id(batch_id, "call_remoteai_batch");
    let mut expanded = Vec::with_capacity(calls.len());
    for (index, call) in calls.iter().enumerate() {
        let call = call.as_object().ok_or_else(|| {
            BackendError::Session("Copilot ordered batch call must be an object".to_string())
        })?;
        if call.len() != 2 {
            return Err(BackendError::Session(
                "Copilot ordered batch call has unexpected fields".to_string(),
            ));
        }
        let name = call.get("name").and_then(Value::as_str).ok_or_else(|| {
            BackendError::Session("Copilot ordered batch call has no name".to_string())
        })?;
        let arguments = call.get("arguments").cloned().ok_or_else(|| {
            BackendError::Session("Copilot ordered batch call has no arguments".to_string())
        })?;
        let arguments = sanitize_arguments(definitions, name, arguments, true)?;
        let suffix = index + 1;
        expanded.push(CapturedToolCall {
            id: format!("{}_{suffix}", truncate_utf8(base_id.as_str(), 240)),
            name: name.to_string(),
            arguments,
        });
    }
    Ok(expanded)
}

fn sanitize_arguments(
    definitions: &[OpenAiTool],
    name: &str,
    mut arguments: Value,
    require_object: bool,
) -> Result<Value, BackendError> {
    if !definitions.iter().any(|tool| tool.function.name == name) {
        return Err(BackendError::Session(
            "Copilot requested a tool outside the request allowlist".to_string(),
        ));
    }
    if require_object && !arguments.is_object() {
        return Err(BackendError::Session(
            "Copilot ordered batch tool arguments must be an object".to_string(),
        ));
    }
    if name == "text"
        && let Some(text) = arguments.get_mut("text")
        && let Some(raw) = text.as_str()
    {
        *text = Value::String(truncate_utf8(raw.trim(), 96));
    }
    let argument_bytes =
        serde_json::to_vec(&arguments).map_err(|error| BackendError::Session(error.to_string()))?;
    if argument_bytes.len() > MAX_TOOL_ARGUMENT_BYTES {
        return Err(BackendError::Session(
            "Copilot tool arguments exceed Dobby's 4 KiB limit".to_string(),
        ));
    }
    Ok(arguments)
}

fn bounded_call_id(raw: &str, fallback: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        fallback.to_string()
    } else {
        truncate_utf8(raw, 256)
    }
}

fn fallback_text_tool(tools: &[OpenAiTool], text: &str) -> Option<CapturedToolCall> {
    tools.iter().find(|tool| tool.function.name == "text")?;
    let text = truncate_utf8(text.trim(), 96);
    if text.is_empty() {
        return None;
    }
    Some(CapturedToolCall {
        id: "call_remoteai_fallback".to_string(),
        name: "text".to_string(),
        arguments: serde_json::json!({ "text": text }),
    })
}

fn bounded_text(text: &str, max_tokens: u32) -> String {
    let byte_cap = (max_tokens as usize).saturating_mul(4).clamp(64, 8 * 1024);
    truncate_utf8(text.trim(), byte_cap)
}

fn truncate_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].trim_end().to_string()
}

#[cfg(unix)]
fn secure_directory(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn secure_directory(_path: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::{FunctionDefinition, PngAttachment};

    fn tool(name: &str) -> OpenAiTool {
        OpenAiTool {
            kind: "function".to_string(),
            function: FunctionDefinition {
                name: name.to_string(),
                description: format!("Use {name}."),
                parameters: serde_json::json!({
                    "type": "object",
                    "additionalProperties": false
                }),
                strict: Some(true),
            },
        }
    }

    fn chat(parallel_tool_calls: bool) -> NormalizedChat {
        NormalizedChat {
            requested_model: "auto".to_string(),
            system_prompt: "Be Dobby.".to_string(),
            prompt: "USER: Act.\n".to_string(),
            tools: vec![tool("move"), tool("text")],
            attachments: Vec::new(),
            parallel_tool_calls,
            max_completion_tokens: 256,
            reasoning_effort: None,
        }
    }

    #[tokio::test]
    async fn dropping_request_guard_signals_background_cleanup() {
        let (sender, receiver) = oneshot::channel();
        let guard = CancelOnDrop(Some(sender));

        drop(guard);

        tokio::time::timeout(Duration::from_secs(1), receiver)
            .await
            .expect("cancellation signal timed out")
            .expect("cancellation sender dropped without signaling");
    }

    #[test]
    fn pngs_become_sdk_blob_attachments() {
        let mut chat = chat(false);
        chat.attachments.push(PngAttachment {
            data_base64: "iVBORw0KGgo=".to_string(),
            display_name: "selected-window.png".to_string(),
            decoded_bytes: 8,
        });

        let options = message_options(&chat);
        let attachments = options.attachments.expect("one attachment");
        assert_eq!(attachments.len(), 1);
        match &attachments[0] {
            Attachment::Blob {
                data,
                mime_type,
                display_name,
            } => {
                assert_eq!(data, "iVBORw0KGgo=");
                assert_eq!(mime_type, "image/png");
                assert_eq!(display_name.as_deref(), Some("selected-window.png"));
            }
            _ => panic!("PNG was not forwarded as a blob attachment"),
        }
    }

    #[test]
    fn parallel_mode_exposes_only_the_synthetic_batch_tool() {
        let chat = chat(true);
        let tools = copilot_tools(&chat);

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, BATCH_TOOL_NAME);
        let schema = serde_json::to_value(&tools[0].parameters).unwrap();
        assert_eq!(schema["properties"]["calls"]["maxItems"], MAX_ORDERED_TOOL_CALLS);
        assert_eq!(
            schema["properties"]["calls"]["items"]["oneOf"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn ordered_batch_expands_allowlisted_calls_in_order() {
        let definitions = vec![tool("move"), tool("text")];
        let calls = expand_batch_calls(
            definitions.as_slice(),
            "batch_7",
            serde_json::json!({
                "calls": [
                    {"name":"move", "arguments":{"x":0.25,"y":0.75}},
                    {"name":"text", "arguments":{"text":"  hello from Dobby  "}}
                ]
            }),
        )
        .unwrap();

        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "batch_7_1");
        assert_eq!(calls[0].name, "move");
        assert_eq!(calls[1].id, "batch_7_2");
        assert_eq!(calls[1].name, "text");
        assert_eq!(calls[1].arguments["text"], "hello from Dobby");
    }

    #[test]
    fn ordered_batch_rejects_unknown_and_more_than_eight_calls() {
        let definitions = vec![tool("move")];
        assert!(
            expand_batch_calls(
                definitions.as_slice(),
                "batch",
                serde_json::json!({"calls":[{"name":"shell", "arguments":{}}]}),
            )
            .is_err()
        );

        let calls: Vec<Value> = (0..=MAX_ORDERED_TOOL_CALLS)
            .map(|_| serde_json::json!({"name":"move", "arguments":{}}))
            .collect();
        assert!(
            expand_batch_calls(
                definitions.as_slice(),
                "batch",
                serde_json::json!({"calls":calls}),
            )
            .is_err()
        );
    }
}
