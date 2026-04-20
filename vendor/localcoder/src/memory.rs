/*!
 * Memory System — S10
 *
 * Persistent memories are stored under:
 *   ~/.localcoder/projects/<project-hash>/memory/
 */

use crate::api::LLMClient;
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

const MEMORY_INDEX_FILENAME: &str = "MEMORY.md";
const MAX_MEMORY_CONTEXT_FILES: usize = 12;
const MAX_MEMORY_CONTEXT_CHARS: usize = 6_000;
const MAX_MEMORY_BODY_SNIPPET: usize = 500;
const MAX_EXTRACTION_VISIBLE_MESSAGES: usize = 8;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    User,
    Feedback,
    Project,
    Reference,
}

impl fmt::Display for MemoryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Feedback => write!(f, "feedback"),
            Self::Project => write!(f, "project"),
            Self::Reference => write!(f, "reference"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryStore {
    root: PathBuf,
    processed_visible_messages: usize,
}

#[derive(Debug, Clone)]
pub struct MemoryHeader {
    pub filename: String,
    pub name: String,
    pub description: String,
    pub memory_type: MemoryType,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct SavedMemory {
    pub filename: String,
    pub name: String,
    pub memory_type: MemoryType,
}

#[derive(Debug, Clone, Deserialize)]
struct ExtractResponse {
    #[serde(default)]
    memories: Vec<ExtractedMemory>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExtractedMemory {
    #[serde(default)]
    filename: Option<String>,
    name: String,
    description: String,
    #[serde(rename = "type")]
    memory_type: MemoryType,
    content: String,
}

impl MemoryStore {
    pub fn new(project_dir: &Path, processed_visible_messages: usize) -> Result<Self> {
        let root = memory_root(project_dir)?;
        fs::create_dir_all(&root)
            .with_context(|| format!("failed to create memory dir: {}", root.display()))?;
        Ok(Self {
            root,
            processed_visible_messages,
        })
    }

    pub fn set_processed_visible_messages(&mut self, count: usize) {
        self.processed_visible_messages = count;
    }

    pub fn build_system_prompt(&self) -> Result<Option<String>> {
        let headers = self.list_headers()?;
        if headers.is_empty() {
            return Ok(None);
        }

        let mut output = String::from("[持久记忆]\n");
        for header in headers.into_iter().take(MAX_MEMORY_CONTEXT_FILES) {
            let block = format!(
                "- [{}] {} — {}\n{}\n\n",
                header.memory_type,
                header.name,
                header.description,
                truncate(&header.content, MAX_MEMORY_BODY_SNIPPET)
            );
            if output.chars().count() + block.chars().count() > MAX_MEMORY_CONTEXT_CHARS {
                break;
            }
            output.push_str(&block);
        }

        Ok(Some(output.trim_end().to_string()))
    }

    pub fn render_memory_list(&self) -> Result<String> {
        let headers = self.list_headers()?;
        if headers.is_empty() {
            return Ok("(empty)".to_string());
        }

        Ok(headers
            .into_iter()
            .map(|m| {
                format!(
                    "- [{}] {} ({}) — {}",
                    m.memory_type, m.name, m.filename, m.description
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub fn list_headers(&self) -> Result<Vec<MemoryHeader>> {
        let mut entries = Vec::new();
        if !self.root.exists() {
            return Ok(entries);
        }

        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("failed to read memory dir: {}", self.root.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            if path.file_name().and_then(|s| s.to_str()) == Some(MEMORY_INDEX_FILENAME) {
                continue;
            }

            let raw = fs::read_to_string(&path)
                .with_context(|| format!("failed to read memory file: {}", path.display()))?;
            let (frontmatter, content) = parse_frontmatter(&raw);
            let Some(memory_type) = frontmatter
                .get("type")
                .and_then(|raw| parse_memory_type(raw))
            else {
                continue;
            };

            let name = frontmatter.get("name").cloned().unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("memory")
                    .to_string()
            });
            let description = frontmatter
                .get("description")
                .cloned()
                .unwrap_or_else(|| "No description".to_string());

            entries.push(MemoryHeader {
                filename: path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string(),
                name,
                description,
                memory_type,
                content,
            });
        }

        entries.sort_by(|a, b| a.filename.cmp(&b.filename));
        Ok(entries)
    }

    pub async fn extract_and_save(
        &mut self,
        client: &LLMClient,
        messages: &[Value],
    ) -> Result<Vec<SavedMemory>> {
        let visible = model_visible_messages(messages);
        if visible.is_empty() {
            self.processed_visible_messages = 0;
            return Ok(Vec::new());
        }

        let start = self.processed_visible_messages.min(visible.len());
        let mut slice = visible[start..].to_vec();
        if slice.is_empty() {
            self.processed_visible_messages = visible.len();
            return Ok(Vec::new());
        }

        if slice.len() > MAX_EXTRACTION_VISIBLE_MESSAGES {
            slice = slice[slice.len() - MAX_EXTRACTION_VISIBLE_MESSAGES..].to_vec();
        }

        let prompt = self.build_extract_prompt(&slice)?;
        let raw = client.complete_prompt(&prompt, 1400).await?;
        let extracted = parse_extract_response(&raw)?;
        let saved = self.persist_memories(extracted)?;
        self.processed_visible_messages = visible.len();
        Ok(saved)
    }

    fn build_extract_prompt(&self, recent_messages: &[Value]) -> Result<String> {
        let existing = self
            .list_headers()?
            .into_iter()
            .map(|m| {
                format!(
                    "- [{}] {} ({}) — {}",
                    m.memory_type, m.name, m.filename, m.description
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let existing_block = if existing.is_empty() {
            "Existing memories:\n(none)".to_string()
        } else {
            format!("Existing memories:\n{}", existing)
        };

        Ok(format!(
            "You are extracting durable memories from the most recent conversation.\n\
Only save information that is useful in future conversations and is NOT derivable from the current code, file tree, or git history.\n\n\
There are exactly 4 memory types:\n\
- user: who the user is, their experience, goals, preferences\n\
- feedback: how the assistant should behave, what to avoid, what to keep doing\n\
- project: durable context about project goals, constraints, deadlines, incidents\n\
- reference: where to find information in external systems or docs\n\n\
Do NOT save code structure, file paths, architecture that can be re-read, recent git activity, or temporary task state.\n\
If nothing should be saved, return an empty list.\n\
If an existing memory should be updated, reuse its filename. If creating a new memory, filename may be omitted.\n\n\
Return ONLY valid JSON with this exact shape:\n\
{{\"memories\":[{{\"filename\":\"optional_existing.md\",\"type\":\"user|feedback|project|reference\",\"name\":\"short title\",\"description\":\"one-line summary\",\"content\":\"markdown body\"}}]}}\n\n\
{}\n\n\
Conversation:\n{}",
            existing_block,
            format_recent_messages(recent_messages)
        ))
    }

    fn persist_memories(&self, memories: Vec<ExtractedMemory>) -> Result<Vec<SavedMemory>> {
        let mut saved = Vec::new();
        for memory in memories {
            let filename = normalize_memory_filename(
                memory.filename.as_deref(),
                &memory.name,
                memory.memory_type,
            );
            let path = self.root.join(&filename);
            let body = format!(
                "---\nname: {}\ndescription: {}\ntype: {}\n---\n\n{}\n",
                memory.name.trim(),
                memory.description.trim(),
                memory.memory_type,
                memory.content.trim()
            );

            fs::write(&path, body)
                .with_context(|| format!("failed to write memory file: {}", path.display()))?;

            saved.push(SavedMemory {
                filename,
                name: memory.name,
                memory_type: memory.memory_type,
            });
        }

        if !saved.is_empty() {
            self.rebuild_index()?;
        }

        Ok(saved)
    }

    fn rebuild_index(&self) -> Result<()> {
        let headers = self.list_headers()?;
        let index = headers
            .iter()
            .map(|m| {
                format!(
                    "- [{}]({}) — [{}] {}",
                    m.name, m.filename, m.memory_type, m.description
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(
            self.root.join(MEMORY_INDEX_FILENAME),
            format!("{}\n", index),
        )
        .with_context(|| format!("failed to write memory index in {}", self.root.display()))?;
        Ok(())
    }
}

fn memory_root(project_dir: &Path) -> Result<PathBuf> {
    memory_root_with_home(
        project_dir,
        std::env::var_os("HOME").as_deref().map(Path::new),
    )
}

fn memory_root_with_home(project_dir: &Path, home: Option<&Path>) -> Result<PathBuf> {
    let canonical = fs::canonicalize(project_dir)
        .with_context(|| format!("failed to canonicalize path: {}", project_dir.display()))?;
    let mut hasher = DefaultHasher::new();
    canonical.to_string_lossy().hash(&mut hasher);
    let hash = format!("{:016x}", hasher.finish());

    let home = home.ok_or_else(|| anyhow!("$HOME is not set"))?;
    Ok(home
        .join(".localcoder")
        .join("projects")
        .join(hash)
        .join("memory"))
}

fn model_visible_messages(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .filter(|m| matches!(m["role"].as_str(), Some("user" | "assistant")))
        .cloned()
        .collect()
}

fn format_recent_messages(messages: &[Value]) -> String {
    messages
        .iter()
        .map(|m| {
            format!(
                "{}:\n{}",
                m["role"].as_str().unwrap_or("unknown"),
                truncate(m["content"].as_str().unwrap_or_default(), 1200)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn parse_extract_response(raw: &str) -> Result<Vec<ExtractedMemory>> {
    let json_text =
        extract_json_text(raw).ok_or_else(|| anyhow!("memory extractor did not return JSON"))?;
    let response: ExtractResponse =
        serde_json::from_str(&json_text).context("failed to parse memory extractor JSON")?;
    Ok(response
        .memories
        .into_iter()
        .filter(|m| {
            !m.name.trim().is_empty()
                && !m.description.trim().is_empty()
                && !m.content.trim().is_empty()
        })
        .collect())
}

fn extract_json_text(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if serde_json::from_str::<Value>(trimmed).is_ok() {
        return Some(trimmed.to_string());
    }

    if let Some(stripped) = strip_code_fence(trimmed) {
        if serde_json::from_str::<Value>(&stripped).is_ok() {
            return Some(stripped);
        }
    }

    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    let candidate = trimmed[start..=end].trim();
    if serde_json::from_str::<Value>(candidate).is_ok() {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn strip_code_fence(raw: &str) -> Option<String> {
    if !raw.starts_with("```") {
        return None;
    }
    let inner = raw
        .trim_start_matches("```json")
        .trim_start_matches("```JSON")
        .trim_start_matches("```")
        .trim();
    let inner = inner.strip_suffix("```")?.trim();
    Some(inner.to_string())
}

fn parse_frontmatter(raw: &str) -> (std::collections::HashMap<String, String>, String) {
    let mut map = std::collections::HashMap::new();
    if !raw.starts_with("---\n") {
        return (map, raw.to_string());
    }

    let mut lines = raw.lines();
    let _ = lines.next();
    let mut in_frontmatter = true;
    let mut body = Vec::new();

    for line in lines {
        if in_frontmatter {
            if line.trim() == "---" {
                in_frontmatter = false;
                continue;
            }
            if let Some((key, value)) = line.split_once(':') {
                map.insert(key.trim().to_string(), value.trim().to_string());
            }
        } else {
            body.push(line);
        }
    }

    (map, body.join("\n").trim().to_string())
}

fn parse_memory_type(raw: &str) -> Option<MemoryType> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "user" => Some(MemoryType::User),
        "feedback" => Some(MemoryType::Feedback),
        "project" => Some(MemoryType::Project),
        "reference" => Some(MemoryType::Reference),
        _ => None,
    }
}

fn normalize_memory_filename(
    filename: Option<&str>,
    name: &str,
    memory_type: MemoryType,
) -> String {
    if let Some(filename) = filename {
        let clean = sanitize_filename(filename);
        if !clean.is_empty() {
            return ensure_md_extension(clean);
        }
    }
    ensure_md_extension(format!("{}_{}", memory_type, slugify(name)))
}

fn sanitize_filename(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
        .collect::<String>()
        .trim_matches('.')
        .to_string()
}

fn ensure_md_extension(mut filename: String) -> String {
    if !filename.ends_with(".md") {
        filename.push_str(".md");
    }
    filename
}

fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "memory".to_string()
    } else {
        trimmed.to_string()
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_extract_response_accepts_fenced_json() {
        let raw = "```json\n{\"memories\":[{\"type\":\"user\",\"name\":\"Rust background\",\"description\":\"User writes Rust\",\"content\":\"User has strong Rust experience.\"}]}\n```";
        let parsed = parse_extract_response(raw).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].memory_type, MemoryType::User);
    }

    #[test]
    fn save_memory_rebuilds_index() {
        let project = TempDir::new().unwrap();
        let fake_home = TempDir::new().unwrap();
        let store = MemoryStore {
            root: memory_root_with_home(project.path(), Some(fake_home.path())).unwrap(),
            processed_visible_messages: 0,
        };
        fs::create_dir_all(&store.root).unwrap();

        let saved = store
            .persist_memories(vec![ExtractedMemory {
                filename: None,
                name: "User profile".to_string(),
                description: "Prefers Rust".to_string(),
                memory_type: MemoryType::User,
                content: "The user prefers Rust-centric examples.".to_string(),
            }])
            .unwrap();

        assert_eq!(saved.len(), 1);
        let index = fs::read_to_string(store.root.join(MEMORY_INDEX_FILENAME)).unwrap();
        assert!(index.contains("User profile"));
        assert!(index.contains("[user]"));
    }

    #[test]
    fn build_system_prompt_includes_memory_content() {
        let project = TempDir::new().unwrap();
        let fake_home = TempDir::new().unwrap();
        let store = MemoryStore {
            root: memory_root_with_home(project.path(), Some(fake_home.path())).unwrap(),
            processed_visible_messages: 0,
        };
        fs::create_dir_all(&store.root).unwrap();

        store
            .persist_memories(vec![ExtractedMemory {
                filename: Some("user_profile.md".to_string()),
                name: "User profile".to_string(),
                description: "Senior Go engineer".to_string(),
                memory_type: MemoryType::User,
                content: "The user is a senior Go engineer and prefers concise explanations."
                    .to_string(),
            }])
            .unwrap();

        let prompt = store.build_system_prompt().unwrap().unwrap();
        assert!(prompt.contains("[持久记忆]"));
        assert!(prompt.contains("Senior Go engineer"));
    }
}
