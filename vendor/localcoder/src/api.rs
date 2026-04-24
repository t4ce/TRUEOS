/*!
 * LLM Client Module
 *
 * Pure Ollama client implementation with tool-calling support.
 *
 * NOTE:
 * The TRUEOS localcoder runtime stores conversation history in its own internal
 * shape after tool use, for example assistant messages with lightweight
 * `tool_calls` entries and tool result messages carrying `tool_name` /
 * `is_error`. OpenAI-compatible `/v1/chat/completions` backends reject that
 * history as invalid `messages`, so before sending requests to that API surface
 * we must normalize the stored messages into OpenAI chat format:
 * assistant tool calls need ids plus stringified arguments, and tool results
 * need `role="tool"` with a matching `tool_call_id`.
 */

use crate::net;
use crate::rt::{env as rt_env, fs as rt_fs};
use crate::types::{AgentResponse, OllamaChatResponse, ToolUseCall};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";
const DEFAULT_OLLAMA_MODEL: &str = "qwen3.5:4b";

#[derive(Debug, Clone, Deserialize)]
struct LLMSettings {
    ollama: OllamaSettings,
}

#[derive(Debug, Clone, Deserialize)]
struct OllamaSettings {
    url: String,
    model: String,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaTagModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagModel {
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedSettings {
    ollama: PersistedOllamaSettings,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedOllamaSettings {
    url: String,
    model: String,
}

/// Ollama client used by the REPL and agent loop.
pub struct LLMClient {
    base_url: String,
    model: String,
    max_tokens: u32,
}

#[derive(Debug, Clone)]
pub enum ClientConfigSource {
    SettingsFile(PathBuf),
    BuiltInDefaults,
}

impl LLMClient {
    fn is_placeholder_settings_cwd(path: &Path) -> bool {
        path == Path::new("/lc")
    }

    fn uses_openai_compat(&self) -> bool {
        self.base_url.ends_with("/v1")
    }

    /// Create a client from `$HOME/.localcoder/settings.json`.
    pub fn new() -> Result<Self> {
        let settings = Self::load_settings()?;
        Ok(Self::from_settings(settings))
    }

    /// Ensure the settings file exists before command handling starts.
    pub fn ensure_settings_file() -> Result<PathBuf> {
        let home = rt_env::home_dir_opt();
        Self::ensure_settings_file_with(home.as_deref())
    }

    /// Create a client, preferring a discovered settings file but falling
    /// back to built-in defaults when the kernel runtime has no writable
    /// settings location yet.
    pub fn new_with_optional_settings() -> Result<(Self, ClientConfigSource)> {
        match Self::resolve_settings_path() {
            Ok(path) => {
                let settings = Self::load_settings_from_path(&path)?;
                Ok((Self::from_settings(settings), ClientConfigSource::SettingsFile(path)))
            }
            Err(_) => Ok((Self::default_client(), ClientConfigSource::BuiltInDefaults)),
        }
    }

    pub fn home_settings_path() -> Result<PathBuf> {
        rt_env::path_from_home(Path::new(".localcoder/settings.json"))
    }

    fn from_settings(settings: LLMSettings) -> Self {
        Self {
            base_url: settings.ollama.url.trim_end_matches('/').to_string(),
            model: settings.ollama.model,
            max_tokens: 4096,
        }
    }

    fn default_client() -> Self {
        Self {
            base_url: DEFAULT_OLLAMA_URL.to_string(),
            model: DEFAULT_OLLAMA_MODEL.to_string(),
            max_tokens: 4096,
        }
    }

    fn load_settings() -> Result<LLMSettings> {
        let path = Self::resolve_settings_path()?;
        Self::load_settings_from_path(&path)
    }

    fn resolve_settings_path() -> Result<PathBuf> {
        let home = rt_env::home_dir_opt();
        Self::resolve_settings_path_with(home.as_deref())
    }

    fn resolve_settings_path_with(home: Option<&Path>) -> Result<PathBuf> {
        if let Ok(cwd) = rt_env::current_dir() {
            if !Self::is_placeholder_settings_cwd(&cwd) {
                let cwd_path = cwd.join(".localcoder/settings.json");
                if rt_fs::exists(&cwd_path) {
                    return Ok(cwd_path);
                }
            }
        }

        if let Some(home) = home {
            let home_path = home.join(".localcoder/settings.json");
            if rt_fs::exists(&home_path) {
                return Ok(home_path);
            }
        }

        Err(anyhow!(
            "missing .localcoder/settings.json in current directory and missing $HOME/.localcoder/settings.json"
        ))
    }

    fn ensure_settings_file_with(home: Option<&Path>) -> Result<PathBuf> {
        if let Ok(path) = Self::resolve_settings_path_with(home) {
            return Ok(path);
        }

        let path = if let Some(home) = home {
            home.join(".localcoder/settings.json")
        } else {
            let cwd = rt_env::current_dir()
                .context("failed to resolve current directory for settings bootstrap")?;
            if Self::is_placeholder_settings_cwd(&cwd) {
                return Err(anyhow!(
                    "missing $HOME/.localcoder/settings.json and refusing to bootstrap placeholder cwd {}",
                    cwd.display()
                ));
            }
            cwd.join(".localcoder/settings.json")
        };
        if let Some(parent) = path.parent() {
            rt_fs::create_dir_all(parent).with_context(|| {
                format!("failed to create settings directory: {}", parent.display())
            })?;
        }

        rt_fs::write(&path, Self::default_settings_json())
            .with_context(|| format!("failed to write settings file: {}", path.display()))?;

        Ok(path)
    }

    fn load_settings_from_path(path: &Path) -> Result<LLMSettings> {
        let raw = rt_fs::read_to_string(path)
            .with_context(|| format!("failed to read settings file: {}", path.display()))?;
        let settings: LLMSettings = serde_json::from_str(&raw)
            .with_context(|| format!("invalid settings JSON: {}", path.display()))?;

        if settings.ollama.url.trim().is_empty() {
            return Err(anyhow!("settings.ollama.url must not be empty"));
        }
        if settings.ollama.model.trim().is_empty() {
            return Err(anyhow!("settings.ollama.model must not be empty"));
        }

        Ok(settings)
    }

    fn default_settings_json() -> String {
        json!({
            "ollama": {
                "url": DEFAULT_OLLAMA_URL,
                "model": DEFAULT_OLLAMA_MODEL,
            }
        })
        .to_string()
    }

    /// Send a tool-aware chat request to Ollama.
    pub async fn call_with_tools(
        &self,
        messages: &[Value],
        tools: &[Value],
    ) -> Result<AgentResponse> {
        if self.uses_openai_compat() {
            let messages = normalize_openai_compat_messages(messages);
            let body = if tools.is_empty() {
                json!({
                    "model": self.model,
                    "messages": messages,
                    "stream": false,
                    "max_tokens": self.max_tokens
                })
            } else {
                json!({
                    "model": self.model,
                    "messages": messages,
                    "stream": false,
                    "tools": tools,
                    "max_tokens": self.max_tokens
                })
            };

            let response = net::post_json(&format!("{}/chat/completions", self.base_url), &body)
                .await
                .context("LM Studio chat completions request failed")?;

            if !response.is_success() {
                let status = response.status();
                let error_text = response.text().unwrap_or_default();
                anyhow::bail!("LM Studio returned error {}: {}", status, error_text);
            }

            let response: Value = response
                .json()
                .context("failed to parse LM Studio chat completions response")?;

            let message = response
                .pointer("/choices/0/message")
                .ok_or_else(|| anyhow!("LM Studio response missing choices[0].message"))?;
            let text = message
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if !text.is_empty() {
                print!("{}", text);
            }

            let tool_uses = message
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|tool_call| {
                    let function = tool_call
                        .get("function")
                        .ok_or_else(|| anyhow!("LM Studio tool call missing function"))?;
                    let name = function
                        .get("name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow!("LM Studio tool call missing function.name"))?
                        .to_string();
                    let arguments = match function.get("arguments") {
                        Some(Value::String(raw)) => {
                            serde_json::from_str(raw).with_context(|| {
                                format!("failed to parse LM Studio tool arguments for {}", name)
                            })?
                        }
                        Some(value) => value.clone(),
                        None => Value::Null,
                    };
                    Ok(ToolUseCall { name, arguments })
                })
                .collect::<Result<Vec<_>>>()?;

            let stop_reason = if tool_uses.is_empty() {
                "end_turn".to_string()
            } else {
                "tool_use".to_string()
            };

            return Ok(AgentResponse {
                text,
                stop_reason,
                tool_uses,
            });
        }

        let body = if tools.is_empty() {
            json!({
                "model": self.model,
                "messages": messages,
                "stream": false,
                "options": {
                    "num_predict": self.max_tokens
                }
            })
        } else {
            json!({
                "model": self.model,
                "messages": messages,
                "stream": false,
                "tools": tools,
                "options": {
                    "num_predict": self.max_tokens
                }
            })
        };

        let response = net::post_json(&format!("{}/api/chat", self.base_url), &body)
            .await
            .context("Ollama request failed")?;

        if !response.is_success() {
            let status = response.status();
            let error_text = response.text().unwrap_or_default();
            anyhow::bail!("Ollama returned error {}: {}", status, error_text);
        }

        let response: OllamaChatResponse =
            response.json().context("failed to parse Ollama response")?;

        let text = response.message.content.unwrap_or_default();
        if !text.is_empty() {
            print!("{}", text);
        }

        let tool_uses = response
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tool_call| ToolUseCall {
                name: tool_call.function.name,
                arguments: tool_call.function.arguments,
            })
            .collect::<Vec<_>>();

        let stop_reason = if tool_uses.is_empty() {
            "end_turn".to_string()
        } else {
            "tool_use".to_string()
        };

        Ok(AgentResponse {
            text,
            stop_reason,
            tool_uses,
        })
    }

    pub async fn summarize_messages(&self, messages: &[Value]) -> Result<String> {
        let prompt = format!(
            "以下是一段对话历史，请生成简洁摘要，保留：\n1. 已完成的任务和结果\n2. 重要文件修改\n3. 用户的关键偏好和决定\n4. 未完成的任务\n\n对话历史：\n{}",
            crate::compact::summarize_for_prompt(messages)
        );

        if self.uses_openai_compat() {
            let body = json!({
                "model": self.model,
                "messages": [
                    {
                        "role": "user",
                        "content": prompt
                    }
                ],
                "stream": false,
                "max_tokens": 1024
            });

            let response = net::post_json(&format!("{}/chat/completions", self.base_url), &body)
                .await
                .context("LM Studio summarize request failed")?;

            if !response.is_success() {
                let status = response.status();
                let error_text = response.text().unwrap_or_default();
                anyhow::bail!("LM Studio returned error {}: {}", status, error_text);
            }

            let response: Value = response
                .json()
                .context("failed to parse LM Studio summarize response")?;

            if let Some(text) = response
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
            {
                return Ok(text.to_string());
            }

            anyhow::bail!("LM Studio summarize response missing choices[0].message.content");
        }

        let body = json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "stream": false,
            "options": {
                "num_predict": 1024
            }
        });

        let response = net::post_json(&format!("{}/api/chat", self.base_url), &body)
            .await
            .context("Ollama summarize request failed")?;

        if !response.is_success() {
            let status = response.status();
            let error_text = response.text().unwrap_or_default();
            anyhow::bail!("Ollama returned error {}: {}", status, error_text);
        }

        let response: OllamaChatResponse = response
            .json()
            .context("failed to parse Ollama summarize response")?;

        Ok(response.message.content.unwrap_or_default())
    }

    pub async fn complete_prompt(&self, prompt: &str, max_tokens: u32) -> Result<String> {
        if self.uses_openai_compat() {
            let body = json!({
                "model": self.model,
                "messages": [
                    {
                        "role": "user",
                        "content": prompt
                    }
                ],
                "stream": false,
                "max_tokens": max_tokens
            });

            let response = net::post_json(&format!("{}/chat/completions", self.base_url), &body)
                .await
                .context("LM Studio chat completions request failed")?;

            if !response.is_success() {
                let status = response.status();
                let error_text = response.text().unwrap_or_default();
                anyhow::bail!("LM Studio returned error {}: {}", status, error_text);
            }

            let response: Value = response
                .json()
                .context("failed to parse LM Studio chat completions response")?;

            if let Some(text) = response
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
            {
                return Ok(text.to_string());
            }

            anyhow::bail!("LM Studio response missing choices[0].message.content");
        }

        let body = json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "stream": false,
            "options": {
                "num_predict": max_tokens
            }
        });

        let response = net::post_json(&format!("{}/api/chat", self.base_url), &body)
            .await
            .context("Ollama prompt request failed")?;

        if !response.is_success() {
            let status = response.status();
            let error_text = response.text().unwrap_or_default();
            anyhow::bail!("Ollama returned error {}: {}", status, error_text);
        }

        let response: OllamaChatResponse = response
            .json()
            .context("failed to parse Ollama prompt response")?;

        Ok(response.message.content.unwrap_or_default())
    }

    /// Set model.
    pub fn set_model(&mut self, model: String) {
        self.model = model;
    }

    /// Set base URL.
    pub fn set_base_url(&mut self, base_url: String) {
        self.base_url = base_url.trim_end_matches('/').to_string();
    }

    /// Set max tokens.
    pub fn set_max_tokens(&mut self, max_tokens: u32) {
        self.max_tokens = max_tokens;
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn list_models(&self) -> Result<Vec<String>> {
        let response = net::get(&format!("{}/api/tags", self.base_url))
            .await
            .context("failed to fetch Ollama model tags")?;

        if !response.is_success() {
            let status = response.status();
            let error_text = response.text().unwrap_or_default();
            anyhow::bail!("Ollama returned error {}: {}", status, error_text);
        }

        let response: OllamaTagsResponse = response
            .json()
            .context("failed to parse Ollama tag response")?;

        let mut models = response
            .models
            .into_iter()
            .map(|model| model.name)
            .collect::<Vec<_>>();
        models.sort();
        models.dedup();
        Ok(models)
    }

    pub fn persist_model_to_home(&self, model: &str) -> Result<PathBuf> {
        let home_path = Self::home_settings_path()?;
        Self::persist_model_to_path(&home_path, &self.base_url, model)?;
        Ok(home_path)
    }

    fn persist_model_to_path(path: &Path, base_url: &str, model: &str) -> Result<()> {
        let model = model.trim();
        if model.is_empty() {
            return Err(anyhow!("model must not be empty"));
        }

        let settings = if rt_fs::exists(path) {
            let raw = rt_fs::read_to_string(path)
                .with_context(|| format!("failed to read settings file: {}", path.display()))?;
            let mut settings: PersistedSettings = serde_json::from_str(&raw)
                .with_context(|| format!("invalid settings JSON: {}", path.display()))?;
            settings.ollama.model = model.to_string();
            if settings.ollama.url.trim().is_empty() {
                settings.ollama.url = base_url.to_string();
            }
            settings
        } else {
            PersistedSettings {
                ollama: PersistedOllamaSettings {
                    url: base_url.to_string(),
                    model: model.to_string(),
                },
            }
        };

        if let Some(parent) = path.parent() {
            rt_fs::create_dir_all(parent).with_context(|| {
                format!("failed to create settings directory: {}", parent.display())
            })?;
        }

        let raw = serde_json::to_string_pretty(&settings)
            .context("failed to serialize updated settings")?;
        rt_fs::write(path, raw)
            .with_context(|| format!("failed to write settings file: {}", path.display()))?;

        Ok(())
    }
}

fn normalize_openai_compat_messages(messages: &[Value]) -> Vec<Value> {
    let mut normalized = Vec::with_capacity(messages.len());
    let mut pending_tool_calls: VecDeque<(String, String)> = VecDeque::new();
    let mut next_tool_call_id: u64 = 1;

    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match role {
            "system" | "user" => {
                normalized.push(json!({
                    "role": role,
                    "content": message
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                }));
            }
            "assistant" => {
                let content = message
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let mut assistant = json!({
                    "role": "assistant",
                    "content": content
                });
                if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
                    let mut normalized_tool_calls = Vec::with_capacity(tool_calls.len());
                    for tool_call in tool_calls {
                        let function = tool_call
                            .get("function")
                            .cloned()
                            .unwrap_or_else(|| json!({}));
                        let name = function
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let arguments = function.get("arguments").cloned().unwrap_or(Value::Null);
                        let call_id = format!("call_{}", next_tool_call_id);
                        next_tool_call_id += 1;
                        pending_tool_calls.push_back((call_id.clone(), name.clone()));
                        normalized_tool_calls.push(json!({
                            "id": call_id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": stringify_tool_arguments(&arguments),
                            }
                        }));
                    }
                    assistant["tool_calls"] = Value::Array(normalized_tool_calls);
                }
                normalized.push(assistant);
            }
            "tool" => {
                let tool_name = message
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let tool_call_id = take_pending_tool_call_id(&mut pending_tool_calls, tool_name)
                    .unwrap_or_else(|| {
                        let call_id = format!("call_{}", next_tool_call_id);
                        next_tool_call_id += 1;
                        call_id
                    });
                normalized.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_call_id,
                    "content": message
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                }));
            }
            _ => {
                normalized.push(json!({
                    "role": role,
                    "content": message
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                }));
            }
        }
    }

    normalized
}

fn stringify_tool_arguments(arguments: &Value) -> String {
    match arguments {
        Value::String(raw) => raw.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_string()),
    }
}

fn take_pending_tool_call_id(
    pending_tool_calls: &mut VecDeque<(String, String)>,
    tool_name: &str,
) -> Option<String> {
    if tool_name.is_empty() {
        return pending_tool_calls.pop_front().map(|(call_id, _)| call_id);
    }

    if let Some(index) = pending_tool_calls
        .iter()
        .position(|(_, pending_name)| pending_name == tool_name)
    {
        return pending_tool_calls.remove(index).map(|(call_id, _)| call_id);
    }

    pending_tool_calls.pop_front().map(|(call_id, _)| call_id)
}

#[cfg(test)]
impl LLMClient {
    fn max_tokens(&self) -> u32 {
        self.max_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn ensure_settings_file_with_creates_default_settings_in_home() {
        let home = tempdir().unwrap();
        let path = LLMClient::ensure_settings_file_with(Some(home.path())).unwrap();

        assert_eq!(path, home.path().join(".localcoder/settings.json"));

        let settings = LLMClient::load_settings_from_path(&path).unwrap();
        assert_eq!(settings.ollama.url, DEFAULT_OLLAMA_URL);
        assert_eq!(settings.ollama.model, DEFAULT_OLLAMA_MODEL);
    }

    #[test]
    fn ensure_settings_file_with_prefers_existing_home_settings() {
        let home = tempdir().unwrap();
        let home_settings = home.path().join(".localcoder/settings.json");

        fs::create_dir_all(home_settings.parent().unwrap()).unwrap();
        fs::write(
            &home_settings,
            r#"{"ollama":{"url":"http://remote-host:11434","model":"qwen2.5-coder:7b"}}"#,
        )
        .unwrap();

        let path = LLMClient::ensure_settings_file_with(Some(home.path())).unwrap();

        assert_eq!(path, home_settings);
    }

    #[test]
    fn resolve_settings_path_with_requires_home_settings() {
        let err = LLMClient::resolve_settings_path_with(None).unwrap_err();
        assert!(err.to_string().contains("$HOME/.localcoder/settings.json"));
    }

    #[test]
    fn load_settings_from_path_reads_ollama_values() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("setting.json");
        fs::write(
            &path,
            r#"{"ollama":{"url":"http://localhost:11434","model":"qwen2.5-coder:7b"}}"#,
        )
        .unwrap();

        let settings = LLMClient::load_settings_from_path(&path).unwrap();
        assert_eq!(settings.ollama.url, "http://localhost:11434");
        assert_eq!(settings.ollama.model, "qwen2.5-coder:7b");
    }

    #[test]
    fn from_settings_sets_defaults() {
        let client = LLMClient::from_settings(LLMSettings {
            ollama: OllamaSettings {
                url: "http://localhost:11434/".to_string(),
                model: "qwen2.5-coder:7b".to_string(),
            },
        });

        assert_eq!(client.base_url(), "http://localhost:11434");
        assert_eq!(client.model(), "qwen2.5-coder:7b");
        assert_eq!(client.max_tokens(), 4096);
    }

    #[test]
    fn set_model_updates_model() {
        let mut client = LLMClient::from_settings(LLMSettings {
            ollama: OllamaSettings {
                url: "http://localhost:11434".to_string(),
                model: "qwen2.5-coder:7b".to_string(),
            },
        });

        client.set_model("llama3.2".to_string());
        assert_eq!(client.model(), "llama3.2");
    }

    #[test]
    fn set_max_tokens_updates_value() {
        let mut client = LLMClient::from_settings(LLMSettings {
            ollama: OllamaSettings {
                url: "http://localhost:11434".to_string(),
                model: "qwen2.5-coder:7b".to_string(),
            },
        });

        client.set_max_tokens(2048);
        assert_eq!(client.max_tokens(), 2048);
    }

    #[test]
    fn persist_model_to_path_creates_home_settings() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(".localcoder/settings.json");

        LLMClient::persist_model_to_path(&path, "http://localhost:11434", "llama3.2").unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        let settings: PersistedSettings = serde_json::from_str(&raw).unwrap();
        assert_eq!(settings.ollama.url, "http://localhost:11434");
        assert_eq!(settings.ollama.model, "llama3.2");
    }

    #[test]
    fn persist_model_to_path_preserves_existing_url() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(".localcoder/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"ollama":{"url":"http://remote-host:11434","model":"qwen2.5-coder:7b"}}"#,
        )
        .unwrap();

        LLMClient::persist_model_to_path(&path, "http://localhost:11434", "deepseek-r1:8b")
            .unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        let settings: PersistedSettings = serde_json::from_str(&raw).unwrap();
        assert_eq!(settings.ollama.url, "http://remote-host:11434");
        assert_eq!(settings.ollama.model, "deepseek-r1:8b");
    }

    #[test]
    fn normalize_openai_compat_messages_converts_tool_history() {
        let messages = vec![
            json!({"role":"system","content":"sys"}),
            json!({"role":"user","content":"orbit"}),
            json!({
                "role":"assistant",
                "content":"",
                "tool_calls":[
                    {
                        "function": {
                            "name":"localcoder_service",
                            "arguments":{"action":"orbit","center_x_norm":0.5}
                        }
                    }
                ]
            }),
            json!({
                "role":"tool",
                "tool_name":"localcoder_service",
                "content":"queued orbit",
                "is_error": false
            }),
        ];

        let normalized = normalize_openai_compat_messages(&messages);
        assert_eq!(normalized.len(), 4);
        assert_eq!(normalized[0]["role"], "system");
        assert_eq!(normalized[1]["role"], "user");
        assert_eq!(normalized[2]["role"], "assistant");
        assert_eq!(normalized[3]["role"], "tool");

        let tool_call_id = normalized[2]["tool_calls"][0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(normalized[2]["tool_calls"][0]["type"], "function");
        assert_eq!(
            normalized[2]["tool_calls"][0]["function"]["arguments"],
            "{\"action\":\"orbit\",\"center_x_norm\":0.5}"
        );
        assert_eq!(normalized[3]["tool_call_id"], tool_call_id);
        assert_eq!(normalized[3]["content"], "queued orbit");
        assert!(normalized[3].get("tool_name").is_none());
        assert!(normalized[3].get("is_error").is_none());
    }
}
