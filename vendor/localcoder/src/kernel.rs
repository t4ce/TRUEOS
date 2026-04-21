use crate::api::LLMClient;
use crate::resume::ResumeTarget;
use anyhow::{Result, anyhow, bail};

#[cfg(feature = "trueos-net")]
const TRUEOS_REMOTE_OLLAMA_URL: &str = "http://192.168.178.111:1234/v1";
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
    "localcoder kernel example uses TRUEOS net/fs/env facades plus a one-shot async LLM prompt path"
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
    let text = client
        .complete_prompt(request.prompt.trim(), request.max_tokens)
        .await
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
