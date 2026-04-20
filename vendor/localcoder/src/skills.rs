/*!
 * Skills System — S13
 *
 * Loads reusable skills from bundled assets, user config, and project-local
 * `.claude/skills/<name>/SKILL.md` directories.
 */

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde::de::{Deserializer, Error as DeError};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const MAX_SKILL_LISTING_CHARS: usize = 6_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillContext {
    Inline,
    Fork,
}

impl fmt::Display for SkillContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inline => write!(f, "inline"),
            Self::Fork => write!(f, "fork"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadedFrom {
    Bundled,
    User,
    Project,
}

impl fmt::Display for LoadedFrom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bundled => write!(f, "bundled"),
            Self::User => write!(f, "user"),
            Self::Project => write!(f, "project"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    pub allowed_tools: Vec<String>,
    pub context: SkillContext,
    pub paths: Vec<String>,
    pub user_invocable: bool,
    pub argument_hint: Option<String>,
    pub content: String,
    pub loaded_from: LoadedFrom,
    pub skill_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ResolvedSkill {
    pub name: String,
    pub prompt: String,
    pub context: SkillContext,
    pub allowed_tools: Vec<String>,
    pub loaded_from: LoadedFrom,
}

impl ResolvedSkill {
    pub fn default_user_message(&self, args: &str) -> String {
        let args = args.trim();
        if args.is_empty() {
            format!(
                "Please execute the active skill `{}` for this turn.",
                self.name
            )
        } else {
            format!("Skill arguments:\n{}", args)
        }
    }
}

#[derive(Debug, Clone, Default)]
struct SkillRegistry {
    skills: HashMap<String, Skill>,
}

#[derive(Debug, Clone)]
struct ActiveSkill {
    prompt: String,
    allowed_tools: Option<HashSet<String>>,
}

#[derive(Debug, Default)]
struct SkillRuntime {
    session_id: Option<String>,
    active_skill: Option<ActiveSkill>,
}

#[derive(Clone)]
pub struct SkillManager {
    cwd: PathBuf,
    registry: Arc<Mutex<SkillRegistry>>,
    runtime: Arc<Mutex<SkillRuntime>>,
}

#[derive(Debug, Deserialize, Default)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    when_to_use: Option<String>,
    #[serde(
        rename = "allowed-tools",
        default,
        deserialize_with = "deserialize_string_list"
    )]
    allowed_tools: Vec<String>,
    context: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_list")]
    paths: Vec<String>,
    #[serde(rename = "user-invocable")]
    user_invocable: Option<bool>,
    #[serde(rename = "argument-hint")]
    argument_hint: Option<String>,
}

impl SkillManager {
    pub fn new(cwd: &Path) -> Result<Self> {
        let registry = load_skill_registry(cwd)?;
        Ok(Self {
            cwd: cwd.to_path_buf(),
            registry: Arc::new(Mutex::new(registry)),
            runtime: Arc::new(Mutex::new(SkillRuntime::default())),
        })
    }

    pub fn reload(&self) -> Result<()> {
        let loaded = load_skill_registry(&self.cwd)?;
        *self.registry.lock().expect("skill registry lock poisoned") = loaded;
        Ok(())
    }

    pub fn set_session_id(&self, session_id: Option<&str>) {
        self.runtime
            .lock()
            .expect("skill runtime lock poisoned")
            .session_id = session_id.map(|s| s.to_string());
    }

    pub fn clear_active(&self) {
        self.runtime
            .lock()
            .expect("skill runtime lock poisoned")
            .active_skill = None;
    }

    pub fn active_prompt(&self) -> Option<String> {
        self.runtime
            .lock()
            .expect("skill runtime lock poisoned")
            .active_skill
            .as_ref()
            .map(|skill| skill.prompt.clone())
    }

    pub fn active_allowed_tools(&self) -> Option<HashSet<String>> {
        self.runtime
            .lock()
            .expect("skill runtime lock poisoned")
            .active_skill
            .as_ref()
            .and_then(|skill| skill.allowed_tools.clone())
    }

    pub fn build_system_prompt(&self) -> Result<Option<String>> {
        self.reload()?;
        let registry = self.registry.lock().expect("skill registry lock poisoned");
        if registry.skills.is_empty() {
            return Ok(None);
        }

        let mut body = String::from(
            "[技能系统]\n当用户请求明显匹配某个技能，或者明确提到 `/<skill>` 斜杠命令时，先调用 `skill_tool`，再继续执行任务。\n可用技能:\n",
        );

        let mut skills = registry.skills.values().cloned().collect::<Vec<_>>();
        skills.sort_by(|a, b| a.name.cmp(&b.name));

        for skill in skills {
            let mut line = format!("- {}", skill.name);
            if !skill.description.trim().is_empty() {
                line.push_str(": ");
                line.push_str(skill.description.trim());
            }
            if let Some(when) = &skill.when_to_use {
                if !when.trim().is_empty() {
                    line.push_str(" | ");
                    line.push_str(when.trim());
                }
            }
            line.push('\n');

            if body.chars().count() + line.chars().count() > MAX_SKILL_LISTING_CHARS {
                break;
            }
            body.push_str(&line);
        }

        Ok(Some(body.trim_end().to_string()))
    }

    pub fn render_user_invocable_list(&self) -> Result<String> {
        self.reload()?;
        let registry = self.registry.lock().expect("skill registry lock poisoned");
        let mut skills = registry
            .skills
            .values()
            .filter(|skill| skill.user_invocable)
            .cloned()
            .collect::<Vec<_>>();

        if skills.is_empty() {
            return Ok("(empty)".to_string());
        }

        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(skills
            .into_iter()
            .map(|skill| {
                let hint = skill
                    .argument_hint
                    .as_deref()
                    .filter(|hint| !hint.trim().is_empty())
                    .map(|hint| format!(" {}", hint))
                    .unwrap_or_default();
                format!(
                    "- /{}{} — {} [{} / {}]",
                    skill.name, hint, skill.description, skill.loaded_from, skill.context
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub fn has_user_invocable(&self, skill_name: &str) -> Result<bool> {
        self.reload()?;
        let registry = self.registry.lock().expect("skill registry lock poisoned");
        Ok(registry
            .skills
            .get(&canonical_skill_key(skill_name))
            .map(|skill| skill.user_invocable)
            .unwrap_or(false))
    }

    pub fn resolve_and_activate(&self, skill_name: &str, args: &str) -> Result<ResolvedSkill> {
        self.reload()?;
        let skill = {
            let registry = self.registry.lock().expect("skill registry lock poisoned");
            registry
                .skills
                .get(&canonical_skill_key(skill_name))
                .cloned()
                .ok_or_else(|| anyhow!("skill not found: {}", skill_name))?
        };

        let session_id = self
            .runtime
            .lock()
            .expect("skill runtime lock poisoned")
            .session_id
            .clone()
            .unwrap_or_default();

        let rendered_body = render_skill_body(&skill, args, &session_id);
        let mut prompt = format!(
            "[技能 {}]\n来源: {}\n执行方式: {}",
            skill.name, skill.loaded_from, skill.context
        );

        if !skill.allowed_tools.is_empty() {
            prompt.push_str(&format!(
                "\nallowed-tools: {}",
                skill.allowed_tools.join(", ")
            ));
        }

        if !skill.paths.is_empty() {
            prompt.push_str(&format!("\npaths: {}", skill.paths.join(", ")));
        }

        if skill.context == SkillContext::Fork {
            prompt.push_str("\n注意: 当前版本尚未实现 fork 子代理，本次降级为 inline 执行。");
        }

        prompt.push_str("\n\n请在当前任务中严格遵循以下技能说明：\n");
        prompt.push_str(&rendered_body);

        let allowed_tools = normalize_allowed_tools(&skill.allowed_tools);
        self.runtime
            .lock()
            .expect("skill runtime lock poisoned")
            .active_skill = Some(ActiveSkill {
            prompt: prompt.clone(),
            allowed_tools,
        });

        Ok(ResolvedSkill {
            name: skill.name,
            prompt,
            context: skill.context,
            allowed_tools: skill.allowed_tools,
            loaded_from: skill.loaded_from,
        })
    }
}

fn load_skill_registry(cwd: &Path) -> Result<SkillRegistry> {
    load_skill_registry_with_home(cwd, std::env::var_os("HOME").as_deref().map(Path::new))
}

fn load_skill_registry_with_home(cwd: &Path, home: Option<&Path>) -> Result<SkillRegistry> {
    let mut skills = HashMap::new();

    for skill in bundled_skills() {
        skills.insert(canonical_skill_key(&skill.name), skill);
    }

    if let Some(home) = home {
        load_skills_from_dir(
            &home.join(".localcoder").join("skills"),
            LoadedFrom::User,
            &mut skills,
        )?;
    }

    load_skills_from_dir(
        &cwd.join(".claude").join("skills"),
        LoadedFrom::Project,
        &mut skills,
    )?;
    Ok(SkillRegistry { skills })
}

fn load_skills_from_dir(
    dir: &Path,
    loaded_from: LoadedFrom,
    skills: &mut HashMap<String, Skill>,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)
        .with_context(|| format!("failed to read skills directory: {}", dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let skill_md = entry.path().join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }

        match parse_skill_md(&skill_md, loaded_from) {
            Ok(skill) => {
                skills.insert(canonical_skill_key(&skill.name), skill);
            }
            Err(err) => {
                eprintln!("failed to load skill {}: {}", skill_md.display(), err);
            }
        }
    }

    Ok(())
}

fn parse_skill_md(path: &Path, loaded_from: LoadedFrom) -> Result<Skill> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read skill file: {}", path.display()))?;
    let fallback_name = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("skill");
    parse_skill_text(
        &raw,
        fallback_name,
        loaded_from,
        path.parent().map(|dir| dir.to_path_buf()),
    )
    .with_context(|| format!("failed to parse skill file: {}", path.display()))
}

fn parse_skill_text(
    raw: &str,
    fallback_name: &str,
    loaded_from: LoadedFrom,
    skill_dir: Option<PathBuf>,
) -> Result<Skill> {
    let normalized = raw.replace("\r\n", "\n");
    let (frontmatter, content) = split_frontmatter(&normalized)?;
    let description = frontmatter
        .description
        .clone()
        .or_else(|| extract_description(&content))
        .unwrap_or_else(|| "No description".to_string());
    let name = frontmatter
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(fallback_name)
        .trim()
        .to_string();

    Ok(Skill {
        name,
        description,
        when_to_use: frontmatter.when_to_use.filter(|s| !s.trim().is_empty()),
        allowed_tools: dedup_strings(frontmatter.allowed_tools),
        context: match frontmatter.context.as_deref() {
            Some(raw) if raw.eq_ignore_ascii_case("fork") => SkillContext::Fork,
            _ => SkillContext::Inline,
        },
        paths: dedup_strings(frontmatter.paths),
        user_invocable: frontmatter.user_invocable.unwrap_or(true),
        argument_hint: frontmatter.argument_hint.filter(|s| !s.trim().is_empty()),
        content: content.trim().to_string(),
        loaded_from,
        skill_dir,
    })
}

fn bundled_skills() -> Vec<Skill> {
    vec![
        parse_skill_text(
            include_str!("../skills/bundled/simplify/SKILL.md"),
            "simplify",
            LoadedFrom::Bundled,
            None,
        )
        .expect("invalid bundled skill: simplify"),
        parse_skill_text(
            include_str!("../skills/bundled/review-diff/SKILL.md"),
            "review-diff",
            LoadedFrom::Bundled,
            None,
        )
        .expect("invalid bundled skill: review-diff"),
        parse_skill_text(
            include_str!("../skills/bundled/commit-message/SKILL.md"),
            "commit-message",
            LoadedFrom::Bundled,
            None,
        )
        .expect("invalid bundled skill: commit-message"),
    ]
}

fn split_frontmatter(raw: &str) -> Result<(SkillFrontmatter, String)> {
    if !raw.starts_with("---\n") {
        return Ok((SkillFrontmatter::default(), raw.to_string()));
    }

    let rest = &raw[4..];
    let Some(end) = rest.find("\n---\n") else {
        return Err(anyhow!("unterminated YAML frontmatter"));
    };

    let frontmatter_text = &rest[..end];
    let body = &rest[end + 5..];
    let frontmatter = if frontmatter_text.trim().is_empty() {
        SkillFrontmatter::default()
    } else {
        serde_yaml::from_str(frontmatter_text).context("invalid YAML frontmatter")?
    };

    Ok((frontmatter, body.to_string()))
}

fn deserialize_string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_yaml::Value::deserialize(deserializer)?;
    match value {
        serde_yaml::Value::Null => Ok(Vec::new()),
        serde_yaml::Value::String(value) => Ok(value
            .split(',')
            .map(|item| item.trim().trim_matches('"').trim_matches('\''))
            .filter(|item| !item.is_empty())
            .map(|item| item.to_string())
            .collect()),
        serde_yaml::Value::Sequence(values) => values
            .into_iter()
            .map(|value| match value {
                serde_yaml::Value::String(s) => Ok(s.trim().to_string()),
                other => Err(D::Error::custom(format!(
                    "expected string item in list, got {:?}",
                    other
                ))),
            })
            .collect(),
        other => Err(D::Error::custom(format!(
            "expected string or list of strings, got {:?}",
            other
        ))),
    }
}

fn extract_description(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| {
            line.trim_start_matches('#')
                .trim()
                .trim_matches('`')
                .to_string()
        })
        .filter(|line| !line.is_empty())
}

fn render_skill_body(skill: &Skill, args: &str, session_id: &str) -> String {
    let skill_dir = skill
        .skill_dir
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();

    skill
        .content
        .replace("${CLAUDE_SKILL_DIR}", &skill_dir)
        .replace("${CLAUDE_SESSION_ID}", session_id)
        .replace("$ARGUMENTS", args.trim())
}

fn normalize_allowed_tools(raw: &[String]) -> Option<HashSet<String>> {
    let set = raw
        .iter()
        .map(|tool| tool.trim().to_ascii_lowercase())
        .filter(|tool| !tool.is_empty())
        .collect::<HashSet<_>>();

    if set.is_empty() { None } else { Some(set) }
}

fn dedup_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn canonical_skill_key(skill_name: &str) -> String {
    skill_name
        .trim()
        .trim_start_matches('/')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_skill_text_reads_frontmatter_fields() {
        let skill = parse_skill_text(
            "---\nname: review-rust\ndescription: Review Rust code\nwhen_to_use: When reviewing patches\nallowed-tools: [Read, Grep, Bash]\ncontext: fork\npaths: [\"src/**/*.rs\"]\nuser-invocable: true\nargument-hint: \"[path]\"\n---\n\nUse the skill body.\n$ARGUMENTS\n",
            "review-rust",
            LoadedFrom::Project,
            Some(PathBuf::from("/tmp/review-rust")),
        )
        .unwrap();

        assert_eq!(skill.name, "review-rust");
        assert_eq!(skill.context, SkillContext::Fork);
        assert_eq!(skill.allowed_tools, vec!["Read", "Grep", "Bash"]);
        assert_eq!(skill.argument_hint.as_deref(), Some("[path]"));
        assert_eq!(skill.paths, vec!["src/**/*.rs"]);
    }

    #[test]
    fn project_skill_overrides_bundled() {
        let project = TempDir::new().unwrap();
        let fake_home = TempDir::new().unwrap();
        let project_skill_dir = project.path().join(".claude/skills/simplify");
        fs::create_dir_all(&project_skill_dir).unwrap();
        fs::write(
            project_skill_dir.join("SKILL.md"),
            "---\nname: simplify\ndescription: Project override\n---\n\nProject custom simplify skill.\n",
        )
        .unwrap();

        let loaded = load_skill_registry_with_home(project.path(), Some(fake_home.path())).unwrap();
        let skill = loaded.skills.get("simplify").unwrap();
        assert_eq!(skill.loaded_from, LoadedFrom::Project);
        assert_eq!(skill.description, "Project override");
    }

    #[test]
    fn resolve_and_activate_substitutes_arguments_and_session() {
        let project = TempDir::new().unwrap();
        let project_skill_dir = project.path().join(".claude/skills/explain");
        fs::create_dir_all(&project_skill_dir).unwrap();
        fs::write(
            project_skill_dir.join("SKILL.md"),
            "---\nname: explain\nallowed-tools: [Read, Glob]\n---\n\nSession=${CLAUDE_SESSION_ID}\nDir=${CLAUDE_SKILL_DIR}\nArgs=$ARGUMENTS\n",
        )
        .unwrap();

        let manager = SkillManager::new(project.path()).unwrap();
        manager.set_session_id(Some("s123"));

        let resolved = manager
            .resolve_and_activate("explain", "src/main.rs")
            .unwrap();
        assert!(resolved.prompt.contains("Session=s123"));
        assert!(resolved.prompt.contains("Args=src/main.rs"));

        let allowed = manager.active_allowed_tools().unwrap();
        assert!(allowed.contains("read"));
        assert!(allowed.contains("glob"));
    }
}
