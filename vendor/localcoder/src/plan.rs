/*!
 * Plan Mode — S12
 *
 * Persistent project-scoped plan state with todo tracking.
 */

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const PLAN_STATE_FILENAME: &str = "state.json";
const PLAN_MARKDOWN_FILENAME: &str = "TODO.md";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlanMode {
    #[default]
    Off,
    Planning,
}

impl fmt::Display for PlanMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off => write!(f, "off"),
            Self::Planning => write!(f, "planning"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
}

impl fmt::Display for TodoStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoItem {
    #[serde(default)]
    pub id: u32,
    #[serde(default)]
    pub status: TodoStatus,
    #[serde(alias = "text", alias = "step")]
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PlanState {
    #[serde(default)]
    mode: PlanMode,
    #[serde(default)]
    todos: Vec<TodoItem>,
}

#[derive(Clone)]
pub struct PlanManager {
    root: PathBuf,
    runtime: Arc<Mutex<PlanState>>,
}

impl PlanManager {
    pub fn new(project_dir: &Path) -> Result<Self> {
        let root = plan_root(project_dir)?;
        fs::create_dir_all(&root)
            .with_context(|| format!("failed to create plan dir: {}", root.display()))?;
        let state = load_state(&root)?;
        let manager = Self {
            root,
            runtime: Arc::new(Mutex::new(state)),
        };
        manager.persist()?;
        Ok(manager)
    }

    pub fn mode(&self) -> PlanMode {
        self.runtime
            .lock()
            .expect("plan runtime lock poisoned")
            .mode
    }

    pub fn is_planning(&self) -> bool {
        self.mode() == PlanMode::Planning
    }

    pub fn enter_mode(&self, reason: Option<&str>) -> Result<String> {
        {
            let mut state = self.runtime.lock().expect("plan runtime lock poisoned");
            state.mode = PlanMode::Planning;
        }
        self.persist()?;

        let mut message = String::from(
            "Entered plan mode. Only read-only tools and planning tools are allowed now.",
        );
        if let Some(reason) = reason.map(str::trim).filter(|reason| !reason.is_empty()) {
            message.push_str(&format!("\nReason: {}", reason));
        }
        message.push_str("\n\n");
        message.push_str(&self.render_status());
        Ok(message)
    }

    pub fn exit_mode(&self, reason: Option<&str>) -> Result<String> {
        {
            let mut state = self.runtime.lock().expect("plan runtime lock poisoned");
            state.mode = PlanMode::Off;
        }
        self.persist()?;

        let mut message = String::from("Exited plan mode. Normal tool access is restored.");
        if let Some(reason) = reason.map(str::trim).filter(|reason| !reason.is_empty()) {
            message.push_str(&format!("\nReason: {}", reason));
        }
        message.push_str("\n\n");
        message.push_str(&self.render_status());
        Ok(message)
    }

    pub fn replace_todos(&self, todos: Vec<TodoItem>) -> Result<String> {
        let normalized = normalize_todos(todos)?;
        {
            let mut state = self.runtime.lock().expect("plan runtime lock poisoned");
            state.todos = normalized;
        }
        self.persist()?;
        Ok(self.render_todos())
    }

    pub fn clear_todos(&self) -> Result<String> {
        {
            let mut state = self.runtime.lock().expect("plan runtime lock poisoned");
            state.todos.clear();
        }
        self.persist()?;
        Ok(self.render_status())
    }

    pub fn render_status(&self) -> String {
        let state = self.runtime.lock().expect("plan runtime lock poisoned");
        let mut out = format!("Mode: {}", state.mode);
        out.push_str("\n");
        out.push_str(&render_todos(&state.todos));
        out
    }

    pub fn render_todos(&self) -> String {
        let state = self.runtime.lock().expect("plan runtime lock poisoned");
        render_todos(&state.todos)
    }

    pub fn build_system_prompt(&self) -> Option<String> {
        let state = self.runtime.lock().expect("plan runtime lock poisoned");
        if state.mode == PlanMode::Off && state.todos.is_empty() {
            return None;
        }

        let mut out = String::from("[计划状态]\n");
        out.push_str(&format!("mode: {}\n", state.mode));

        if state.mode == PlanMode::Planning {
            out.push_str(
                "当前处于计划模式。先分析问题并维护 todo，禁止使用 Edit、Write、Bash 等写工具，直到调用 ExitPlanMode。\n",
            );
        } else {
            out.push_str("如果存在 todo，请在执行过程中用 TodoWrite 保持状态更新。\n");
        }

        out.push_str("\nCurrent todos:\n");
        out.push_str(&render_todos(&state.todos));
        Some(out)
    }

    pub fn allowed_tools(&self) -> Option<HashSet<String>> {
        if !self.is_planning() {
            return None;
        }

        Some(
            [
                "Read",
                "Glob",
                "Grep",
                "WebFetch",
                "WebSearch",
                "EnterPlanMode",
                "ExitPlanMode",
                "TodoWrite",
                "skill_tool",
                "echo_tool",
            ]
            .into_iter()
            .map(|tool| tool.to_ascii_lowercase())
            .collect(),
        )
    }

    fn persist(&self) -> Result<()> {
        let state = self
            .runtime
            .lock()
            .expect("plan runtime lock poisoned")
            .clone();
        let json_path = self.root.join(PLAN_STATE_FILENAME);
        let markdown_path = self.root.join(PLAN_MARKDOWN_FILENAME);

        fs::write(
            &json_path,
            serde_json::to_string_pretty(&state).context("failed to serialize plan state")?,
        )
        .with_context(|| format!("failed to write plan state: {}", json_path.display()))?;

        fs::write(&markdown_path, render_markdown(&state)).with_context(|| {
            format!("failed to write todo markdown: {}", markdown_path.display())
        })?;
        Ok(())
    }
}

fn load_state(root: &Path) -> Result<PlanState> {
    let path = root.join(PLAN_STATE_FILENAME);
    if !path.exists() {
        return Ok(PlanState::default());
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read plan state: {}", path.display()))?;
    let state: PlanState = serde_json::from_str(&raw)
        .with_context(|| format!("invalid plan state: {}", path.display()))?;
    Ok(state)
}

fn normalize_todos(todos: Vec<TodoItem>) -> Result<Vec<TodoItem>> {
    let in_progress = todos
        .iter()
        .filter(|todo| todo.status == TodoStatus::InProgress)
        .count();
    if in_progress > 1 {
        return Err(anyhow!("TodoWrite: at most one todo may be in_progress"));
    }

    let mut seen = HashSet::new();
    let mut next_id = todos.iter().map(|todo| todo.id).max().unwrap_or(0) + 1;
    let mut normalized = Vec::with_capacity(todos.len());

    for mut todo in todos {
        todo.content = todo.content.trim().to_string();
        if todo.content.is_empty() {
            return Err(anyhow!("TodoWrite: todo content must not be empty"));
        }
        if todo.id == 0 {
            todo.id = next_id;
            next_id += 1;
        }
        if !seen.insert(todo.id) {
            return Err(anyhow!("TodoWrite: duplicate todo id {}", todo.id));
        }
        normalized.push(todo);
    }

    Ok(normalized)
}

fn render_todos(todos: &[TodoItem]) -> String {
    if todos.is_empty() {
        return "(empty)".to_string();
    }

    todos
        .iter()
        .map(|todo| format!("- {} {}. {}", checkbox(todo.status), todo.id, todo.content))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_markdown(state: &PlanState) -> String {
    format!(
        "# Plan\n\nMode: {}\n\n{}\n",
        state.mode,
        render_todos(&state.todos)
    )
}

fn checkbox(status: TodoStatus) -> &'static str {
    match status {
        TodoStatus::Pending => "[ ]",
        TodoStatus::InProgress => "[>]",
        TodoStatus::Completed => "[x]",
    }
}

fn plan_root(project_dir: &Path) -> Result<PathBuf> {
    plan_root_with_home(
        project_dir,
        std::env::var_os("HOME").as_deref().map(Path::new),
    )
}

fn plan_root_with_home(project_dir: &Path, home: Option<&Path>) -> Result<PathBuf> {
    let canonical = fs::canonicalize(project_dir)
        .with_context(|| format!("failed to canonicalize path: {}", project_dir.display()))?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical.to_string_lossy().hash(&mut hasher);
    let hash = format!("{:016x}", hasher.finish());

    let home = home.ok_or_else(|| anyhow!("$HOME is not set"))?;
    Ok(home
        .join(".localcoder")
        .join("projects")
        .join(hash)
        .join("plan"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn replace_todos_assigns_ids_and_renders() {
        let project = TempDir::new().unwrap();
        let fake_home = TempDir::new().unwrap();
        let manager = PlanManager {
            root: plan_root_with_home(project.path(), Some(fake_home.path())).unwrap(),
            runtime: Arc::new(Mutex::new(PlanState::default())),
        };
        fs::create_dir_all(&manager.root).unwrap();

        let rendered = manager
            .replace_todos(vec![TodoItem {
                id: 0,
                status: TodoStatus::Pending,
                content: "Inspect auth flow".to_string(),
            }])
            .unwrap();

        assert!(rendered.contains("1. Inspect auth flow"));
        let state = fs::read_to_string(manager.root.join(PLAN_STATE_FILENAME)).unwrap();
        assert!(state.contains("Inspect auth flow"));
    }

    #[test]
    fn enter_mode_limits_tools() {
        let project = TempDir::new().unwrap();
        let manager = PlanManager::new(project.path()).unwrap();
        manager.enter_mode(Some("Need to inspect first")).unwrap();

        let allowed = manager.allowed_tools().unwrap();
        assert!(allowed.contains("read"));
        assert!(!allowed.contains("edit"));
    }

    #[test]
    fn build_system_prompt_includes_todos() {
        let project = TempDir::new().unwrap();
        let manager = PlanManager::new(project.path()).unwrap();
        manager.enter_mode(None).unwrap();
        manager
            .replace_todos(vec![TodoItem {
                id: 1,
                status: TodoStatus::InProgress,
                content: "Read existing code".to_string(),
            }])
            .unwrap();

        let prompt = manager.build_system_prompt().unwrap();
        assert!(prompt.contains("[计划状态]"));
        assert!(prompt.contains("Read existing code"));
    }
}
