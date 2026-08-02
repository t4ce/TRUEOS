use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_MESSAGES: usize = 64;
pub const MAX_TOOLS: usize = 16;
pub const MAX_PROMPT_BYTES: usize = 96 * 1024;
pub const MAX_IMAGE_ATTACHMENTS: usize = 2;
/// Aggregate decoded PNG bytes accepted across one request. The base64 form is
/// deliberately small enough to coexist with Dobby's transcript in the
/// current bounded TRUEOS JSON-POST transport.
pub const MAX_IMAGE_BYTES: usize = 80 * 1024;

#[derive(Clone, Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub tools: Option<Vec<OpenAiTool>>,
    #[serde(default)]
    pub tool_choice: Option<Value>,
    #[serde(default)]
    pub parallel_tool_calls: Option<bool>,
    #[serde(default)]
    pub max_completion_tokens: Option<u32>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<Value>,
    #[serde(default)]
    pub tool_calls: Option<Vec<Value>>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OpenAiTool {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionDefinition,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "empty_object")]
    pub parameters: Value,
    #[serde(default)]
    pub strict: Option<bool>,
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("model must be non-empty and no longer than 128 bytes")]
    Model,
    #[error("streaming chat completions are not supported")]
    Streaming,
    #[error("request must contain between 1 and {MAX_MESSAGES} messages")]
    Messages,
    #[error("request transcript exceeds {MAX_PROMPT_BYTES} bytes")]
    PromptTooLarge,
    #[error("message role or content is invalid")]
    Message,
    #[error("only bounded data:image/png;base64 image_url parts are supported")]
    Image,
    #[error(
        "request images exceed {MAX_IMAGE_ATTACHMENTS} attachments or {MAX_IMAGE_BYTES} decoded bytes"
    )]
    ImageTooLarge,
    #[error("request contains too many tools")]
    TooManyTools,
    #[error("only function tools are supported")]
    ToolKind,
    #[error("tool name is invalid")]
    ToolName,
    #[error("tool description is too long")]
    ToolDescription,
    #[error("tool parameters must be a bounded JSON object")]
    ToolParameters,
    #[error("tool_choice must be `required` when tools are supplied")]
    ToolChoice,
    #[error("reasoning_effort must be none, minimal, low, medium, high, xhigh, max, or null")]
    ReasoningEffort,
}

#[derive(Clone, Debug)]
pub struct NormalizedChat {
    pub requested_model: String,
    pub system_prompt: String,
    pub prompt: String,
    pub tools: Vec<OpenAiTool>,
    pub attachments: Vec<PngAttachment>,
    pub parallel_tool_calls: bool,
    pub max_completion_tokens: u32,
    pub reasoning_effort: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PngAttachment {
    pub data_base64: String,
    pub display_name: String,
    pub decoded_bytes: usize,
}

#[derive(Default)]
struct ContentImages {
    attachments: Vec<PngAttachment>,
    decoded_bytes: usize,
}

impl ChatCompletionRequest {
    pub fn normalize(self) -> Result<NormalizedChat, ValidationError> {
        let model = self.model.trim();
        if model.is_empty()
            || model.len() > 128
            || model.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(ValidationError::Model);
        }
        if self.stream.unwrap_or(false) {
            return Err(ValidationError::Streaming);
        }
        if self.messages.is_empty() || self.messages.len() > MAX_MESSAGES {
            return Err(ValidationError::Messages);
        }

        let tools = self.tools.unwrap_or_default();
        validate_tools(&tools)?;
        let parallel_tool_calls = self.parallel_tool_calls.unwrap_or(false) && !tools.is_empty();
        if !tools.is_empty()
            && self
                .tool_choice
                .as_ref()
                .and_then(Value::as_str)
                .is_some_and(|choice| choice != "required")
        {
            return Err(ValidationError::ToolChoice);
        }

        let mut system_parts = Vec::new();
        let mut transcript = String::new();
        let mut images = ContentImages::default();
        for message in self.messages {
            let role = message.role.trim();
            if !matches!(role, "system" | "developer" | "user" | "assistant" | "tool") {
                return Err(ValidationError::Message);
            }
            let content = content_text(message.content.as_ref(), &mut images)?;
            if matches!(role, "system" | "developer") {
                if !content.is_empty() {
                    system_parts.push(content);
                }
                continue;
            }

            match role {
                "user" => push_transcript(&mut transcript, "USER", &content),
                "assistant" => {
                    if !content.is_empty() {
                        push_transcript(&mut transcript, "ASSISTANT", &content);
                    }
                    if let Some(calls) = message.tool_calls {
                        for call in calls {
                            push_transcript(
                                &mut transcript,
                                "ASSISTANT TOOL CALL",
                                &compact_json(&call),
                            );
                        }
                    }
                }
                "tool" => {
                    let label = message
                        .tool_call_id
                        .as_deref()
                        .map(|id| format!("TOOL RESULT {id}"))
                        .unwrap_or_else(|| "TOOL RESULT".to_string());
                    push_transcript(&mut transcript, &label, &content);
                }
                _ => unreachable!(),
            }
        }

        if transcript.is_empty() || transcript.len() > MAX_PROMPT_BYTES {
            return Err(if transcript.is_empty() {
                ValidationError::Message
            } else {
                ValidationError::PromptTooLarge
            });
        }
        if !tools.is_empty() {
            if parallel_tool_calls {
                transcript.push_str(concat!(
                    "\nINSTRUCTION: Continue the transcript. Call the one available batch tool exactly once. ",
                    "Put between one and eight intended custom-tool calls in execution order in its calls array.\n",
                ));
            } else {
                transcript.push_str(
                    "\nINSTRUCTION: Continue the transcript. Call exactly one available custom tool now; do not call a second tool.\n",
                );
            }
        }
        if transcript.len() > MAX_PROMPT_BYTES {
            return Err(ValidationError::PromptTooLarge);
        }

        let reasoning_effort = self
            .reasoning_effort
            .map(|effort| effort.trim().to_string())
            .filter(|effort| !effort.is_empty());
        if reasoning_effort.as_deref().is_some_and(|effort| {
            !matches!(effort, "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max")
        }) {
            return Err(ValidationError::ReasoningEffort);
        }

        Ok(NormalizedChat {
            requested_model: model.to_string(),
            system_prompt: system_parts.join("\n\n"),
            prompt: transcript,
            tools,
            attachments: images.attachments,
            parallel_tool_calls,
            max_completion_tokens: self.max_completion_tokens.unwrap_or(500).clamp(1, 2_048),
            reasoning_effort,
        })
    }
}

fn validate_tools(tools: &[OpenAiTool]) -> Result<(), ValidationError> {
    if tools.len() > MAX_TOOLS {
        return Err(ValidationError::TooManyTools);
    }
    for tool in tools {
        if tool.kind != "function" {
            return Err(ValidationError::ToolKind);
        }
        let name = tool.function.name.as_str();
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(ValidationError::ToolName);
        }
        if tool.function.description.len() > 2_048 {
            return Err(ValidationError::ToolDescription);
        }
        if !tool.function.parameters.is_object()
            || compact_json(&tool.function.parameters).len() > 32 * 1024
        {
            return Err(ValidationError::ToolParameters);
        }
    }
    Ok(())
}

fn content_text(
    content: Option<&Value>,
    images: &mut ContentImages,
) -> Result<String, ValidationError> {
    match content {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) if text.len() <= MAX_PROMPT_BYTES => Ok(text.clone()),
        Some(Value::Array(parts)) => {
            let mut out = String::new();
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    None | Some("text") => {
                        let Some(text) = part.get("text").and_then(Value::as_str) else {
                            return Err(ValidationError::Message);
                        };
                        append_content_line(&mut out, text)?;
                    }
                    Some("image_url") => {
                        let url = part
                            .get("image_url")
                            .and_then(Value::as_object)
                            .and_then(|image| image.get("url"))
                            .and_then(Value::as_str)
                            .ok_or(ValidationError::Image)?;
                        let attachment = decode_png_data_url(url, images)?;
                        let marker = format!("[attached PNG: {}]", attachment.display_name);
                        append_content_line(&mut out, marker.as_str())?;
                        images.decoded_bytes = images
                            .decoded_bytes
                            .saturating_add(attachment.decoded_bytes);
                        images.attachments.push(attachment);
                    }
                    _ => return Err(ValidationError::Message),
                }
            }
            Ok(out)
        }
        _ => Err(ValidationError::Message),
    }
}

fn append_content_line(out: &mut String, text: &str) -> Result<(), ValidationError> {
    let separator = usize::from(!out.is_empty());
    if out
        .len()
        .saturating_add(separator)
        .saturating_add(text.len())
        > MAX_PROMPT_BYTES
    {
        return Err(ValidationError::PromptTooLarge);
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(text);
    Ok(())
}

fn decode_png_data_url(
    url: &str,
    images: &ContentImages,
) -> Result<PngAttachment, ValidationError> {
    const PREFIX: &str = "data:image/png;base64,";
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

    if images.attachments.len() >= MAX_IMAGE_ATTACHMENTS {
        return Err(ValidationError::ImageTooLarge);
    }
    let encoded = url.strip_prefix(PREFIX).ok_or(ValidationError::Image)?;
    let maximum_encoded_len = MAX_IMAGE_BYTES.div_ceil(3).saturating_mul(4);
    if encoded.is_empty() || encoded.len() > maximum_encoded_len {
        return Err(ValidationError::ImageTooLarge);
    }
    let decoded = BASE64_STANDARD
        .decode(encoded)
        .map_err(|_| ValidationError::Image)?;
    if !decoded.starts_with(PNG_SIGNATURE) {
        return Err(ValidationError::Image);
    }
    let total = images
        .decoded_bytes
        .checked_add(decoded.len())
        .ok_or(ValidationError::ImageTooLarge)?;
    if total > MAX_IMAGE_BYTES {
        return Err(ValidationError::ImageTooLarge);
    }
    let index = images.attachments.len() + 1;
    Ok(PngAttachment {
        data_base64: encoded.to_string(),
        display_name: format!("remoteai-image-{index:02}.png"),
        decoded_bytes: decoded.len(),
    })
}

fn push_transcript(out: &mut String, label: &str, content: &str) {
    out.push_str(label);
    out.push_str(": ");
    out.push_str(content);
    out.push('\n');
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    pub index: u32,
    pub message: AssistantMessage,
    pub finish_reason: &'static str,
}

#[derive(Debug, Serialize)]
pub struct AssistantMessage {
    pub role: &'static str,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<AssistantToolCall>>,
}

#[derive(Debug, Serialize)]
pub struct AssistantToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: AssistantFunctionCall,
}

#[derive(Debug, Serialize)]
pub struct AssistantFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn normalizes_dobby_history_and_tools() {
        let request: ChatCompletionRequest = serde_json::from_value(json!({
            "model": "auto",
            "messages": [
                {"role":"system","content":"Be Dobby."},
                {"role":"user","content":"Move."},
                {"role":"assistant","content":null,"tool_calls":[{"id":"one","function":{"name":"move","arguments":"{\\\"x\\\":0.2}"}}]},
                {"role":"tool","tool_call_id":"one","content":"moved"},
                {"role":"user","content":"Again."}
            ],
            "tools": [{"type":"function","function":{"name":"move","parameters":{"type":"object"}}}],
            "tool_choice": "required",
            "parallel_tool_calls": false
        })).unwrap();

        let normalized = request.normalize().unwrap();
        assert_eq!(normalized.system_prompt, "Be Dobby.");
        assert!(normalized.prompt.contains("TOOL RESULT one: moved"));
        assert_eq!(normalized.tools.len(), 1);
    }

    #[test]
    fn rejects_streaming_and_ambient_tool_names() {
        let request: ChatCompletionRequest = serde_json::from_value(json!({
            "model": "auto",
            "messages": [{"role":"user","content":"hello"}],
            "stream": true
        }))
        .unwrap();
        assert!(matches!(request.normalize(), Err(ValidationError::Streaming)));
    }

    #[test]
    fn normalizes_supported_reasoning_effort_and_rejects_unknown_values() {
        let request: ChatCompletionRequest = serde_json::from_value(json!({
            "model": "auto",
            "messages": [{"role":"user","content":"hello"}],
            "reasoning_effort": " low "
        }))
        .unwrap();
        assert_eq!(request.normalize().unwrap().reasoning_effort.as_deref(), Some("low"));

        let request: ChatCompletionRequest = serde_json::from_value(json!({
            "model": "auto",
            "messages": [{"role":"user","content":"hello"}],
            "reasoning_effort": "turbo"
        }))
        .unwrap();
        assert!(matches!(request.normalize(), Err(ValidationError::ReasoningEffort)));
    }

    #[test]
    fn normalizes_text_and_bounded_png_content_parts() {
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let request: ChatCompletionRequest = serde_json::from_value(json!({
            "model": "auto",
            "messages": [{
                "role": "user",
                "content": [
                    {"type":"text", "text":"Inspect the selected window."},
                    {"type":"image_url", "image_url":{"url":format!("data:image/png;base64,{png}"), "detail":"low"}}
                ]
            }]
        }))
        .unwrap();

        let normalized = request.normalize().unwrap();
        assert!(normalized.prompt.contains("Inspect the selected window."));
        assert!(
            normalized
                .prompt
                .contains("[attached PNG: remoteai-image-01.png]")
        );
        assert_eq!(normalized.attachments.len(), 1);
        assert_eq!(normalized.attachments[0].data_base64, png);
        assert_eq!(normalized.attachments[0].decoded_bytes, 70);
    }

    #[test]
    fn rejects_remote_non_png_and_oversized_image_parts() {
        for url in [
            "https://example.test/window.png".to_string(),
            "data:image/png;base64,SGVsbG8=".to_string(),
            format!("data:image/png;base64,{}", "A".repeat(MAX_IMAGE_BYTES.div_ceil(3) * 4 + 1)),
        ] {
            let request: ChatCompletionRequest = serde_json::from_value(json!({
                "model": "auto",
                "messages": [{"role":"user", "content":[
                    {"type":"image_url", "image_url":{"url":url}}
                ]}]
            }))
            .unwrap();
            assert!(request.normalize().is_err());
        }
    }

    #[test]
    fn parallel_tool_mode_requests_one_ordered_batch() {
        let request: ChatCompletionRequest = serde_json::from_value(json!({
            "model": "auto",
            "messages": [{"role":"user","content":"Move and react."}],
            "tools": [
                {"type":"function","function":{"name":"move","parameters":{"type":"object"}}},
                {"type":"function","function":{"name":"play_emotion","parameters":{"type":"object"}}}
            ],
            "tool_choice": "required",
            "parallel_tool_calls": true
        }))
        .unwrap();

        let normalized = request.normalize().unwrap();
        assert!(normalized.parallel_tool_calls);
        assert!(
            normalized
                .prompt
                .contains("one available batch tool exactly once")
        );
        assert_eq!(normalized.tools.len(), 2);
    }
}
