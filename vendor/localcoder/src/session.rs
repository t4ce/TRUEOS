/*!
 * Session Persistence — S05
 *
 * Stores conversation messages as JSONL and supports session resume.
 */

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::collections::hash_map::DefaultHasher;
use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct SessionStore {
    pub id: String,
    pub path: PathBuf,
}

impl SessionStore {
    pub fn list(project_dir: &Path) -> Result<Vec<Self>> {
        let base = sessions_project_dir(project_dir)?;
        if !base.exists() {
            return Ok(Vec::new());
        }

        let mut candidates: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
        for entry in fs::read_dir(&base)
            .with_context(|| format!("failed to read sessions directory: {}", base.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            candidates.push((path, modified));
        }

        candidates.sort_by(|a, b| b.1.cmp(&a.1));

        let sessions = candidates
            .into_iter()
            .map(|(path, _)| {
                let id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| anyhow!("invalid session filename: {}", path.display()))?
                    .to_string();
                Ok(Self { id, path })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(sessions)
    }

    pub fn create(project_dir: &Path) -> Result<Self> {
        let base = sessions_project_dir(project_dir)?;
        fs::create_dir_all(&base)
            .with_context(|| format!("failed to create sessions directory: {}", base.display()))?;

        let id = generate_session_id();
        let path = base.join(format!("{}.jsonl", id));

        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to create session file: {}", path.display()))?;

        Ok(Self { id, path })
    }

    pub fn load(project_dir: &Path, session_id: &str) -> Result<Self> {
        let base = sessions_project_dir(project_dir)?;
        let path = base.join(format!("{}.jsonl", session_id));
        if !path.exists() {
            return Err(anyhow!("session not found: {}", session_id));
        }
        Ok(Self {
            id: session_id.to_string(),
            path,
        })
    }

    pub fn load_latest(project_dir: &Path) -> Result<Option<Self>> {
        Ok(Self::list(project_dir)?.into_iter().next())
    }

    pub fn append_message(&self, message: &Value) -> Result<()> {
        let event = message_to_event(message)?;
        self.append_event(event)
    }

    pub fn append_messages(&self, messages: &[Value]) -> Result<()> {
        for m in messages {
            self.append_message(m)?;
        }
        Ok(())
    }

    pub fn replace_messages(&self, messages: &[Value]) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)
            .with_context(|| format!("failed to rewrite session file: {}", self.path.display()))?;

        for message in messages {
            let event = message_to_event(message)?;
            let line =
                serde_json::to_string(&event).context("failed to serialize session event")?;
            writeln!(file, "{}", line).context("failed to rewrite session line")?;
        }

        file.flush()
            .context("failed to flush rewritten session file")?;
        Ok(())
    }

    pub fn load_messages(&self) -> Result<Vec<Value>> {
        let file = OpenOptions::new()
            .read(true)
            .open(&self.path)
            .with_context(|| format!("failed to open session file: {}", self.path.display()))?;

        let mut out = Vec::new();
        let reader = BufReader::new(file);

        for (line_no, line) in reader.lines().enumerate() {
            let line = line.with_context(|| {
                format!(
                    "failed to read line {} from {}",
                    line_no + 1,
                    self.path.display()
                )
            })?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let event: Value = serde_json::from_str(line).with_context(|| {
                format!(
                    "invalid JSONL at line {} in {}",
                    line_no + 1,
                    self.path.display()
                )
            })?;

            if let Some(message) = event_to_message(&event) {
                out.push(message);
            }
        }

        Ok(out)
    }
}

fn append_jsonl(path: &Path, value: &Value) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open session file: {}", path.display()))?;

    let line = serde_json::to_string(value).context("failed to serialize session event")?;
    writeln!(file, "{}", line).context("failed to append session line")?;
    file.flush().context("failed to flush session file")?;
    Ok(())
}

impl SessionStore {
    fn append_event(&self, event: Value) -> Result<()> {
        append_jsonl(&self.path, &event)
    }
}

fn message_to_event(message: &Value) -> Result<Value> {
    let timestamp = now_ts();
    let role = message["role"]
        .as_str()
        .ok_or_else(|| anyhow!("message missing role"))?;

    match role {
        "user" => Ok(json!({
            "type": "user",
            "content": message["content"].as_str().unwrap_or_default(),
            "timestamp": timestamp,
        })),
        "assistant" => Ok(json!({
            "type": "assistant",
            "content": message["content"].as_str().unwrap_or_default(),
            "tool_calls": message.get("tool_calls").cloned().unwrap_or(Value::Null),
            "timestamp": timestamp,
        })),
        "system" => Ok(json!({
            "type": "system",
            "content": message["content"].as_str().unwrap_or_default(),
            "timestamp": timestamp,
        })),
        "tool" => Ok(json!({
            "type": "tool_result",
            "tool_name": message["tool_name"].as_str().unwrap_or_default(),
            "content": message["content"].as_str().unwrap_or_default(),
            "is_error": message["is_error"].as_bool().unwrap_or(false),
            "timestamp": timestamp,
        })),
        _ => Err(anyhow!("unsupported message role: {}", role)),
    }
}

fn event_to_message(event: &Value) -> Option<Value> {
    match event["type"].as_str()? {
        "user" => Some(json!({
            "role": "user",
            "content": event["content"].as_str().unwrap_or_default(),
        })),
        "assistant" => {
            let mut msg = json!({
                "role": "assistant",
                "content": event["content"].as_str().unwrap_or_default(),
            });
            if event.get("tool_calls").is_some() && !event["tool_calls"].is_null() {
                msg["tool_calls"] = event["tool_calls"].clone();
            }
            Some(msg)
        }
        "system" => Some(json!({
            "role": "system",
            "content": event["content"].as_str().unwrap_or_default(),
        })),
        "tool_result" => Some(json!({
            "role": "tool",
            "tool_name": event["tool_name"].as_str().unwrap_or_default(),
            "content": event["content"].as_str().unwrap_or_default(),
            "is_error": event["is_error"].as_bool().unwrap_or(false),
        })),
        _ => None,
    }
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn sessions_project_dir(project_dir: &Path) -> Result<PathBuf> {
    sessions_project_dir_with_home(
        project_dir,
        std::env::var_os("HOME").as_deref().map(Path::new),
    )
}

fn sessions_project_dir_with_home(project_dir: &Path, home: Option<&Path>) -> Result<PathBuf> {
    let canonical = fs::canonicalize(project_dir)
        .with_context(|| format!("failed to canonicalize path: {}", project_dir.display()))?;
    let mut hasher = DefaultHasher::new();
    canonical.to_string_lossy().hash(&mut hasher);
    let hash = format!("{:016x}", hasher.finish());

    Ok(localcoder_home_with_home(home)?.join("sessions").join(hash))
}

fn localcoder_home() -> Result<PathBuf> {
    localcoder_home_with_home(std::env::var_os("HOME").as_deref().map(Path::new))
}

fn localcoder_home_with_home(home: Option<&Path>) -> Result<PathBuf> {
    let home = home.ok_or_else(|| anyhow!("$HOME is not set"))?;
    Ok(home.join(".localcoder"))
}

fn generate_session_id() -> String {
    let ts = now_ts();
    let pid = std::process::id();
    format!("s{}-{}", ts, pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn message_event_roundtrip_user_and_assistant() {
        let user = json!({"role":"user","content":"hello"});
        let assistant = json!({"role":"assistant","content":"hi"});

        let user_event = message_to_event(&user).unwrap();
        let assistant_event = message_to_event(&assistant).unwrap();

        assert_eq!(event_to_message(&user_event).unwrap()["role"], "user");
        assert_eq!(
            event_to_message(&assistant_event).unwrap()["role"],
            "assistant"
        );
    }

    #[test]
    fn store_create_append_and_load() {
        let project = TempDir::new().unwrap();
        let fake_home = TempDir::new().unwrap();
        let path = sessions_project_dir_with_home(project.path(), Some(fake_home.path())).unwrap();
        fs::create_dir_all(&path).unwrap();
        let store = SessionStore {
            id: "test".to_string(),
            path: path.join("test.jsonl"),
        };
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&store.path)
            .unwrap();

        store
            .append_message(&json!({"role":"user","content":"ping"}))
            .unwrap();
        store
            .append_message(&json!({"role":"assistant","content":"pong"}))
            .unwrap();

        let loaded = store.load_messages().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0]["role"], "user");
        assert_eq!(loaded[1]["role"], "assistant");
    }

    #[test]
    fn load_latest_returns_some_after_create() {
        let project = TempDir::new().unwrap();
        let fake_home = TempDir::new().unwrap();
        let path = sessions_project_dir_with_home(project.path(), Some(fake_home.path())).unwrap();
        fs::create_dir_all(&path).unwrap();
        let store = SessionStore {
            id: "latest".to_string(),
            path: path.join("latest.jsonl"),
        };
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&store.path)
            .unwrap();
        let latest = list_with_home(project.path(), Some(fake_home.path()))
            .unwrap()
            .into_iter()
            .next()
            .unwrap();

        assert_eq!(latest.id, store.id);
    }

    #[test]
    fn list_returns_sessions() {
        let project = TempDir::new().unwrap();
        let fake_home = TempDir::new().unwrap();
        let path = sessions_project_dir_with_home(project.path(), Some(fake_home.path())).unwrap();
        fs::create_dir_all(&path).unwrap();
        let store = SessionStore {
            id: "list".to_string(),
            path: path.join("list.jsonl"),
        };
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&store.path)
            .unwrap();
        let sessions = list_with_home(project.path(), Some(fake_home.path())).unwrap();

        assert!(!sessions.is_empty());
        assert_eq!(sessions[0].id, store.id);
    }

    #[test]
    fn replace_messages_rewrites_file_with_system_message() {
        let project = TempDir::new().unwrap();
        let fake_home = TempDir::new().unwrap();
        let path = sessions_project_dir_with_home(project.path(), Some(fake_home.path())).unwrap();
        fs::create_dir_all(&path).unwrap();
        let store = SessionStore {
            id: "rewrite".to_string(),
            path: path.join("rewrite.jsonl"),
        };
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&store.path)
            .unwrap();
        store
            .append_message(&json!({"role":"user","content":"old"}))
            .unwrap();

        store
            .replace_messages(&[
                json!({"role":"system","content":"[对话摘要]\\nsummary"}),
                json!({"role":"user","content":"new"}),
            ])
            .unwrap();

        let loaded = store.load_messages().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0]["role"], "system");
        assert_eq!(loaded[1]["content"], "new");
    }

    fn list_with_home(project_dir: &Path, home: Option<&Path>) -> Result<Vec<SessionStore>> {
        let base = sessions_project_dir_with_home(project_dir, home)?;
        if !base.exists() {
            return Ok(Vec::new());
        }

        let mut candidates: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
        for entry in fs::read_dir(&base)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            candidates.push((path, modified));
        }

        candidates.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(candidates
            .into_iter()
            .map(|(path, _)| SessionStore {
                id: path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string(),
                path,
            })
            .collect())
    }
}
