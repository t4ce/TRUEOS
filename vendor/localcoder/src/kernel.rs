use crate::api::LLMClient;
use crate::localcoder_service;
use crate::resume::ResumeTarget;
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

#[cfg(feature = "trueos-net")]
const TRUEOS_REMOTE_OLLAMA_URL: &str = "http://192.168.178.112:1234/v1";
#[cfg(feature = "trueos-net")]
const TRUEOS_REMOTE_MODEL: &str = "google/gemma-4-e4b";

#[derive(Debug, Clone)]
pub struct BasicPromptRequest {
    pub resume_target: ResumeTarget,
    pub prompt: String,
    pub max_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct BasicPromptResponse {
    pub model: String,
    pub base_url: String,
    pub settings_path: String,
    pub text: String,
}

impl BasicPromptRequest {
    pub fn validate(&self) -> Result<()> {
        if self.prompt.trim().is_empty() {
            bail!("prompt must not be empty");
        }

        if !matches!(self.resume_target, ResumeTarget::New) {
            bail!("resume/continue is not wired for the first kernel localcoder example");
        }

        if self.max_tokens == 0 {
            bail!("max_tokens must be greater than zero");
        }

        Ok(())
    }
}

pub fn describe_runtime_surface() -> &'static str {
    "localcoder kernel example uses TRUEOS net/fs/env facades plus a one-shot async LLM prompt path with optional TRUEOS cursor tooling"
}

fn endpoint_soft_warning(base_url: &str, err: &anyhow::Error) -> Option<String> {
    let text = err.to_string();
    let looks_unreachable = text.contains("request failed for ")
        || text.contains("start failed rc=")
        || text.contains("wait failed rc=")
        || text.contains("LM Studio chat completions request failed")
        || text.contains("Ollama prompt request failed");
    if !looks_unreachable {
        return None;
    }

    Some(format!(
        "warning: localcoder endpoint {} is unavailable right now",
        base_url
    ))
}

pub async fn run_basic_prompt(request: &BasicPromptRequest) -> Result<BasicPromptResponse> {
    request.validate()?;

    let settings_path = LLMClient::ensure_settings_file()
        .map_err(|e| anyhow!("localcoder settings bootstrap failed: {}", e))?;
    let mut client =
        LLMClient::new().map_err(|e| anyhow!("localcoder client init failed: {}", e))?;
    #[cfg(feature = "trueos-net")]
    client.set_base_url(TRUEOS_REMOTE_OLLAMA_URL.to_string());
    #[cfg(feature = "trueos-net")]
    client.set_model(TRUEOS_REMOTE_MODEL.to_string());
    let model = client.model().to_string();
    let base_url = client.base_url().to_string();
    let text = if localcoder_service::is_registered() {
        run_prompt_with_localcoder_service(&client, request).await
    } else {
        client
            .complete_prompt(request.prompt.trim(), request.max_tokens)
            .await
    }
    .map_err(|e| {
        if let Some(warning) = endpoint_soft_warning(base_url.as_str(), &e) {
            return anyhow!(warning);
        }
        anyhow!(
            "localcoder prompt failed: {:#} (endpoint={} model={} settings={} hint=for LM Studio enable Serve on Local Network and verify the server is listening on that host:port)",
            e,
            base_url,
            model,
            settings_path.display()
        )
    })?;

    Ok(BasicPromptResponse {
        model,
        base_url,
        settings_path: settings_path.display().to_string(),
        text,
    })
}

async fn run_prompt_with_localcoder_service(
    client: &LLMClient,
    request: &BasicPromptRequest,
) -> Result<String> {
    let tool = localcoder_service::tool_definition();
    let mut messages = vec![
        json!({"role": "system", "content": localcoder_service::build_system_prompt()}),
        json!({"role": "user", "content": request.prompt.trim()}),
    ];

    loop {
        let response = client
            .call_with_tools(&messages, core::slice::from_ref(&tool))
            .await?;
        let text = response.text.clone();
        messages.push(build_assistant_message(&response.text, &response.tool_uses));

        if response.stop_reason != "tool_use" || response.tool_uses.is_empty() {
            return Ok(text);
        }

        for call in response.tool_uses {
            let (content, is_error) = match localcoder_service::execute_tool_call(call.arguments).await
            {
                Ok(result) => (result, false),
                Err(err) => (err.to_string(), true),
            };

            messages.push(json!({
                "role": "tool",
                "tool_name": call.name,
                "content": content,
                "is_error": is_error,
            }));
        }
    }
}

fn build_assistant_message(text: &str, tool_uses: &[crate::types::ToolUseCall]) -> Value {
    let mut message = json!({
        "role": "assistant",
        "content": text,
    });

    if !tool_uses.is_empty() {
        let tool_calls: Vec<Value> = tool_uses
            .iter()
            .map(|call| {
                json!({
                    "function": {
                        "name": call.name,
                        "arguments": call.arguments,
                    }
                })
            })
            .collect();
        message["tool_calls"] = Value::Array(tool_calls);
    }

    message
}
