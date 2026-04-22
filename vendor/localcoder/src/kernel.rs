use crate::api::LLMClient;
use crate::compact;
use crate::engine;
use crate::localcoder_service;
use crate::resume::ResumeTarget;
use crate::rt::fs as rt_fs;
use crate::rt::env as rt_env;
use crate::tools::{
    EchoTool, EditTool, EnterPlanModeTool, ExitPlanModeTool, GlobTool, ReadTool, SkillTool,
    TodoWriteTool, Tool, ToolRegistry, WriteTool,
};
use crate::ui2_window_controller;
use crate::ui2_window_observer;
use crate::{plan, skills};
use anyhow::{Result, anyhow, bail};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use core::sync::atomic::{AtomicU64, Ordering};

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);
const TRUEOS_FORCED_MODEL: &str = "google/gemma-4-e4b";

#[derive(Debug, Clone)]
pub struct BasicPromptRequest {
    pub session_scope: Option<String>,
    pub resume_target: ResumeTarget,
    pub prompt: String,
    pub max_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct BasicPromptResponse {
    pub model: String,
    pub base_url: String,
    pub settings_path: String,
    pub session_id: String,
    pub session_action: String,
    pub text: String,
}

impl BasicPromptRequest {
    pub fn validate(&self) -> Result<()> {
        if self.prompt.trim().is_empty() {
            bail!("prompt must not be empty");
        }

        if self.max_tokens == 0 {
            bail!("max_tokens must be greater than zero");
        }

        Ok(())
    }
}

pub fn describe_runtime_surface() -> &'static str {
    "localcoder TRUEOS one-shot mode uses the shared ToolRegistry and agent loop with a TRUEOS-specific tool set"
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

struct PromptSession {
    id: String,
    path: PathBuf,
    action: &'static str,
    messages: Vec<Value>,
}

fn init_prompt_session(
    cwd: &Path,
    session_scope: Option<&str>,
    resume_target: &ResumeTarget,
) -> Result<PromptSession> {
    match resume_target {
        ResumeTarget::New => create_prompt_session(cwd, session_scope),
        ResumeTarget::ContinueLatest => {
            if let Some(session_id) = load_latest_session_id(cwd, session_scope)? {
                load_prompt_session(cwd, session_scope, &session_id, "continued")
            } else {
                create_prompt_session(cwd, session_scope)
            }
        }
        ResumeTarget::ResumeId(session_id) => {
            load_prompt_session(cwd, session_scope, session_id, "resumed")
        }
    }
}

fn create_prompt_session(cwd: &Path, session_scope: Option<&str>) -> Result<PromptSession> {
    let path = session_file_path(cwd, session_scope, &generate_session_id())?;
    let id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| anyhow!("invalid generated session path: {}", path.display()))?
        .to_string();
    Ok(PromptSession {
        id,
        path,
        action: "new",
        messages: Vec::new(),
    })
}

fn load_prompt_session(
    cwd: &Path,
    session_scope: Option<&str>,
    session_id: &str,
    action: &'static str,
) -> Result<PromptSession> {
    let path = session_file_path(cwd, session_scope, session_id)?;
    if !rt_fs::exists(&path) {
        bail!("session not found: {}", session_id);
    }

    let raw = rt_fs::read_to_string(&path)
        .map_err(|e| anyhow!("failed to read session {}: {}", session_id, e))?;
    let messages: Vec<Value> = if raw.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(&raw)
            .map_err(|e| anyhow!("invalid session {}: {}", session_id, e))?
    };

    Ok(PromptSession {
        id: session_id.to_string(),
        path,
        action,
        messages,
    })
}

fn save_prompt_session(cwd: &Path, session_scope: Option<&str>, session: &PromptSession) -> Result<()> {
    let base = project_session_dir(cwd, session_scope)?;
    rt_fs::create_dir_all(&base)
        .map_err(|e| anyhow!("failed to create session directory {}: {}", base.display(), e))?;
    let raw = serde_json::to_string(&session.messages)
        .map_err(|e| anyhow!("failed to serialize session {}: {}", session.id, e))?;
    rt_fs::write(&session.path, raw.as_bytes())
        .map_err(|e| anyhow!("failed to write session {}: {}", session.path.display(), e))?;
    let latest_path = latest_session_pointer_path(cwd, session_scope)?;
    rt_fs::write(&latest_path, session.id.as_bytes()).map_err(|e| {
        anyhow!(
            "failed to write latest session pointer {}: {}",
            latest_path.display(),
            e
        )
    })?;
    Ok(())
}

fn load_latest_session_id(cwd: &Path, session_scope: Option<&str>) -> Result<Option<String>> {
    let latest_path = latest_session_pointer_path(cwd, session_scope)?;
    if !rt_fs::exists(&latest_path) {
        return Ok(None);
    }

    let latest = rt_fs::read_to_string(&latest_path).map_err(|e| {
        anyhow!(
            "failed to read latest session pointer {}: {}",
            latest_path.display(),
            e
        )
    })?;
    let trimmed = latest.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(trimmed.to_string()))
}

fn session_file_path(cwd: &Path, session_scope: Option<&str>, session_id: &str) -> Result<PathBuf> {
    Ok(project_session_dir(cwd, session_scope)?.join(format!("{}.json", session_id)))
}

fn latest_session_pointer_path(cwd: &Path, session_scope: Option<&str>) -> Result<PathBuf> {
    Ok(project_session_dir(cwd, session_scope)?.join("latest"))
}

fn project_session_dir(cwd: &Path, session_scope: Option<&str>) -> Result<PathBuf> {
    let mut hasher = DefaultHasher::new();
    cwd.to_string_lossy().hash(&mut hasher);
    let project_hash = format!("{:016x}", hasher.finish());
    let root = match rt_env::home_dir_opt() {
        Some(home) => home.join(".localcoder").join("sessions"),
        None => cwd.join(".localcoder").join("sessions"),
    };
    let scoped = match normalize_session_scope(session_scope) {
        Some(scope) => root.join(project_hash).join(scope),
        None => root.join(project_hash),
    };
    Ok(scoped)
}

fn normalize_session_scope(session_scope: Option<&str>) -> Option<String> {
    let scope = session_scope?.trim();
    if scope.is_empty() {
        return None;
    }

    let mut normalized = String::with_capacity(scope.len());
    for ch in scope.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => normalized.push(ch),
            _ => normalized.push('_'),
        }
    }

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn generate_session_id() -> String {
    let ts = crate::time::unix_time_seconds();
    let seq = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("s{}-{}", ts, seq)
}

pub async fn run_basic_prompt(request: &BasicPromptRequest) -> Result<BasicPromptResponse> {
    request.validate()?;

    let settings_path = LLMClient::ensure_settings_file()
        .map_err(|e| anyhow!("localcoder settings bootstrap failed: {}", e))?;
    let mut client =
        LLMClient::new().map_err(|e| anyhow!("localcoder client init failed: {}", e))?;
    client.set_model(TRUEOS_FORCED_MODEL.to_string());
    client.set_max_tokens(request.max_tokens);

    let session_scope = request.session_scope.as_deref();
    let cwd = rt_env::current_dir().map_err(|e| anyhow!("failed to resolve project directory: {}", e))?;
    let model = client.model().to_string();
    let base_url = client.base_url().to_string();
    let registry = build_trueos_registry(&cwd)?;
    let system_prompt = build_trueos_system_prompt();
    let mut session = init_prompt_session(&cwd, session_scope, &request.resume_target)?;
    session.messages.push(serde_json::json!({
        "role": "user",
        "content": request.prompt.trim(),
    }));
    save_prompt_session(&cwd, session_scope, &session)?;

    let mut messages = session.messages.clone();
    if compact::maybe_compact(&client, &mut messages).await? {
        session.messages = messages.clone();
        save_prompt_session(&cwd, session_scope, &session)?;
    }

    let text = match engine::run_agent_loop_with_system_prompt(
        &client,
        &registry,
        &mut messages,
        system_prompt.as_deref(),
    )
    .await
    {
        Ok(text) => text,
        Err(err) if err.to_string().contains("Context size has been exceeded") => {
            if compact::force_compact(&client, &mut messages).await? {
                session.messages = messages.clone();
                save_prompt_session(&cwd, session_scope, &session)?;
            }
            engine::run_agent_loop_with_system_prompt(
                &client,
                &registry,
                &mut messages,
                system_prompt.as_deref(),
            )
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
            })?
        }
        Err(err) => {
            return Err(if let Some(warning) = endpoint_soft_warning(base_url.as_str(), &err) {
                anyhow!(warning)
            } else {
                anyhow!(
                    "localcoder prompt failed: {:#} (endpoint={} model={} settings={} hint=for LM Studio enable Serve on Local Network and verify the server is listening on that host:port)",
                    err,
                    base_url,
                    model,
                    settings_path.display()
                )
            });
        }
    };

    session.messages = messages;
    save_prompt_session(&cwd, session_scope, &session)?;

    Ok(BasicPromptResponse {
        model,
        base_url,
        settings_path: settings_path.display().to_string(),
        session_id: session.id,
        session_action: session.action.to_string(),
        text,
    })
}

fn build_trueos_system_prompt() -> Option<String> {
    let mut parts = Vec::new();
    if localcoder_service::is_registered() {
        parts.push(localcoder_service::build_system_prompt());
    }
    if ui2_window_observer::is_registered() {
        parts.push(ui2_window_observer::build_system_prompt().to_string());
    }
    if ui2_window_controller::is_registered() {
        parts.push(ui2_window_controller::build_system_prompt().to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

fn build_trueos_registry(cwd: &std::path::Path) -> Result<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(EchoTool);
    registry.register(ReadTool);
    registry.register(EditTool);
    registry.register(WriteTool);
    registry.register(GlobTool);

    if let Ok(plan_manager) = plan::PlanManager::new(cwd) {
        registry.attach_plan_manager(plan_manager.clone());
        registry.register(EnterPlanModeTool::new(plan_manager.clone()));
        registry.register(ExitPlanModeTool::new(plan_manager.clone()));
        registry.register(TodoWriteTool::new(plan_manager));
    }

    if let Ok(skill_manager) = skills::SkillManager::new(cwd) {
        registry.attach_skill_manager(skill_manager.clone());
        registry.register(SkillTool::new(skill_manager));
    }

    if localcoder_service::is_registered() {
        registry.register(LocalcoderServiceTool);
    }
    if ui2_window_observer::is_registered() {
        registry.register(Ui2WindowObserverTool);
    }
    if ui2_window_controller::is_registered() {
        registry.register(Ui2WindowControllerTool);
    }

    Ok(registry)
}

struct LocalcoderServiceTool;

impl Tool for LocalcoderServiceTool {
    fn name(&self) -> &str {
        localcoder_service::tool_name()
    }

    fn description(&self) -> &str {
        "Drive the TRUEOS AI cursor with smooth absolute motion, orbit motion, clicks, and explicit button state changes."
    }

    fn schema(&self) -> Value {
        localcoder_service::tool_definition()["function"]["parameters"].clone()
    }

    async fn execute(&self, input: Value) -> Result<String> {
        localcoder_service::execute_tool_call(input).await
    }
}

struct Ui2WindowObserverTool;

impl Tool for Ui2WindowObserverTool {
    fn name(&self) -> &str {
        ui2_window_observer::tool_name()
    }

    fn description(&self) -> &str {
        "Inspect UI2 windows and their shell geometry only: frame, titlebar, window controls, resize handle, and content rect. This does not expose app-internal widgets."
    }

    fn schema(&self) -> Value {
        ui2_window_observer::tool_definition()["function"]["parameters"].clone()
    }

    async fn execute(&self, input: Value) -> Result<String> {
        ui2_window_observer::execute_tool_call(input).await
    }
}

struct Ui2WindowControllerTool;

impl Tool for Ui2WindowControllerTool {
    fn name(&self) -> &str {
        ui2_window_controller::tool_name()
    }

    fn description(&self) -> &str {
        "Control UI2 windows using shell-level semantics. This bridge resolves UI2 window shell targets and then drives the cursor movement tool underneath."
    }

    fn schema(&self) -> Value {
        ui2_window_controller::tool_definition()["function"]["parameters"].clone()
    }

    async fn execute(&self, input: Value) -> Result<String> {
        ui2_window_controller::execute_tool_call(input).await
    }
}
