/*!
 * Output Styles — S18
 *
 * Loads markdown output styles from bundled, user, and project locations and
 * injects the selected style into the system prompt.
 */

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStyleSource {
    BuiltIn,
    User,
    Project,
}

impl fmt::Display for OutputStyleSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuiltIn => write!(f, "built-in"),
            Self::User => write!(f, "user"),
            Self::Project => write!(f, "project"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OutputStyle {
    pub name: String,
    pub description: String,
    pub content: String,
    pub keep_coding_instructions: bool,
    pub source: OutputStyleSource,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct StyleFrontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(rename = "keep-coding-instructions")]
    keep_coding_instructions: Option<bool>,
}

#[derive(Clone)]
pub struct OutputStyleManager {
    cwd: PathBuf,
}

impl OutputStyleManager {
    pub fn new(cwd: &Path) -> Self {
        Self {
            cwd: cwd.to_path_buf(),
        }
    }

    pub fn list_styles(&self) -> Result<Vec<OutputStyle>> {
        load_styles(&self.cwd)
    }

    pub fn has_style(&self, name: &str) -> Result<bool> {
        Ok(self.get_style(name)?.is_some())
    }

    pub fn render_style_list(&self, current: &str) -> Result<String> {
        let current = canonical_name(current);
        let mut styles = self.list_styles()?;
        styles.sort_by(style_sort_key);

        Ok(styles
            .into_iter()
            .map(|style| {
                let marker = if canonical_name(&style.name) == current {
                    " (current)"
                } else {
                    ""
                };
                format!(
                    "- {} [{}] — {}{}",
                    style.name, style.source, style.description, marker
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub fn apply_selected_style(
        &self,
        style_name: &str,
        base_prompt: Option<String>,
    ) -> Result<Option<String>> {
        let Some(style) = self.get_style(style_name)? else {
            return Ok(base_prompt);
        };

        if canonical_name(&style.name) == "default" {
            return Ok(base_prompt);
        }

        let style_block = format!("[输出样式: {}]\n{}", style.name, style.content.trim());

        Ok(match base_prompt {
            Some(base) if !base.trim().is_empty() => {
                if style.keep_coding_instructions {
                    Some(format!("{}\n\n{}", base, style_block))
                } else {
                    Some(format!("{}\n\n{}", style_block, base))
                }
            }
            _ => Some(style_block),
        })
    }

    fn get_style(&self, name: &str) -> Result<Option<OutputStyle>> {
        let target = canonical_name(name);
        Ok(self
            .list_styles()?
            .into_iter()
            .find(|style| canonical_name(&style.name) == target))
    }
}

fn load_styles(cwd: &Path) -> Result<Vec<OutputStyle>> {
    load_styles_with_home(cwd, std::env::var_os("HOME").as_deref().map(Path::new))
}

fn load_styles_with_home(cwd: &Path, home: Option<&Path>) -> Result<Vec<OutputStyle>> {
    let mut styles = HashMap::new();

    for style in bundled_styles() {
        styles.insert(canonical_name(&style.name), style);
    }

    if let Some(home) = home {
        load_styles_from_dir(
            &home.join(".localcoder").join("output-styles"),
            OutputStyleSource::User,
            &mut styles,
        )?;
    }

    load_styles_from_dir(
        &cwd.join(".claude").join("output-styles"),
        OutputStyleSource::Project,
        &mut styles,
    )?;

    Ok(styles.into_values().collect())
}

fn load_styles_from_dir(
    dir: &Path,
    source: OutputStyleSource,
    styles: &mut HashMap<String, OutputStyle>,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)
        .with_context(|| format!("failed to read output styles dir: {}", dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }

        match parse_style_file(&path, source) {
            Ok(style) => {
                styles.insert(canonical_name(&style.name), style);
            }
            Err(err) => {
                eprintln!("failed to load output style {}: {}", path.display(), err);
            }
        }
    }

    Ok(())
}

fn parse_style_file(path: &Path, source: OutputStyleSource) -> Result<OutputStyle> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read output style: {}", path.display()))?;
    let fallback_name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("style");
    parse_style_text(&raw, fallback_name, source)
        .with_context(|| format!("failed to parse output style: {}", path.display()))
}

fn parse_style_text(
    raw: &str,
    fallback_name: &str,
    source: OutputStyleSource,
) -> Result<OutputStyle> {
    let normalized = raw.replace("\r\n", "\n");
    let (frontmatter, content) = split_frontmatter(&normalized)?;
    let name = frontmatter
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(fallback_name)
        .trim()
        .to_string();
    let description = frontmatter
        .description
        .clone()
        .or_else(|| extract_description(&content))
        .unwrap_or_else(|| "No description".to_string());

    Ok(OutputStyle {
        name,
        description,
        content: content.trim().to_string(),
        keep_coding_instructions: frontmatter.keep_coding_instructions.unwrap_or(true),
        source,
    })
}

fn bundled_styles() -> Vec<OutputStyle> {
    vec![
        OutputStyle {
            name: "default".to_string(),
            description: "标准 Localcoder 回复风格".to_string(),
            content: String::new(),
            keep_coding_instructions: true,
            source: OutputStyleSource::BuiltIn,
        },
        parse_style_text(
            include_str!("../output-styles/bundled/concise.md"),
            "concise",
            OutputStyleSource::BuiltIn,
        )
        .expect("invalid bundled output style: concise"),
        parse_style_text(
            include_str!("../output-styles/bundled/detailed.md"),
            "detailed",
            OutputStyleSource::BuiltIn,
        )
        .expect("invalid bundled output style: detailed"),
    ]
}

fn split_frontmatter(raw: &str) -> Result<(StyleFrontmatter, String)> {
    if !raw.starts_with("---\n") {
        return Ok((StyleFrontmatter::default(), raw.to_string()));
    }

    let rest = &raw[4..];
    let Some(end) = rest.find("\n---\n") else {
        return Err(anyhow!("unterminated YAML frontmatter"));
    };

    let frontmatter_text = &rest[..end];
    let body = &rest[end + 5..];
    let frontmatter = if frontmatter_text.trim().is_empty() {
        StyleFrontmatter::default()
    } else {
        serde_yaml::from_str(frontmatter_text).context("invalid YAML frontmatter")?
    };

    Ok((frontmatter, body.to_string()))
}

fn extract_description(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.trim_start_matches('#').trim().to_string())
        .filter(|line| !line.is_empty())
}

fn canonical_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn style_sort_key(style: &OutputStyle, other: &OutputStyle) -> std::cmp::Ordering {
    let left = (style.name != "default", canonical_name(&style.name));
    let right = (other.name != "default", canonical_name(&other.name));
    left.cmp(&right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_style_text_reads_frontmatter() {
        let style = parse_style_text(
            "---\nname: concise\ndescription: Keep it short\nkeep-coding-instructions: false\n---\n\nReply briefly.\n",
            "concise",
            OutputStyleSource::Project,
        )
        .unwrap();

        assert_eq!(style.name, "concise");
        assert_eq!(style.description, "Keep it short");
        assert!(!style.keep_coding_instructions);
    }

    #[test]
    fn project_style_overrides_built_in() {
        let project = TempDir::new().unwrap();
        let style_dir = project.path().join(".claude/output-styles");
        fs::create_dir_all(&style_dir).unwrap();
        fs::write(
            style_dir.join("concise.md"),
            "---\nname: concise\ndescription: Project concise\n---\n\nProject style.\n",
        )
        .unwrap();

        let styles = load_styles_with_home(project.path(), None).unwrap();
        let concise = styles
            .into_iter()
            .find(|style| style.name == "concise")
            .unwrap();
        assert_eq!(concise.source, OutputStyleSource::Project);
        assert_eq!(concise.description, "Project concise");
    }

    #[test]
    fn apply_selected_style_appends_when_keep_is_true() {
        let project = TempDir::new().unwrap();
        let manager = OutputStyleManager::new(project.path());
        let prompt = manager
            .apply_selected_style("concise", Some("base prompt".to_string()))
            .unwrap()
            .unwrap();
        assert!(prompt.contains("base prompt"));
        assert!(prompt.contains("[输出样式: concise]"));
    }
}
