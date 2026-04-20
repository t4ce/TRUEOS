/*!
 * Config System — S06
 *
 * Stores user-facing REPL preferences in .localcoder/settings.json:
 *   ui.theme
 *   ui.tips
 */

use crate::rt::{env as rt_env, fs as rt_fs};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt;
use std::path::{Path, PathBuf};

const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";
const DEFAULT_OLLAMA_MODEL: &str = "qwen3.5:4b";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Default,
    Light,
    Dark,
}

impl fmt::Display for Theme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Theme::Default => write!(f, "default"),
            Theme::Light => write!(f, "light"),
            Theme::Dark => write!(f, "dark"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub theme: Theme,
    #[serde(default = "default_tips")]
    pub tips: bool,
    #[serde(default = "default_output_style")]
    pub output_style: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self::Default
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: Theme::Default,
            tips: true,
            output_style: default_output_style(),
        }
    }
}

fn default_tips() -> bool {
    true
}

fn default_output_style() -> String {
    "default".to_string()
}

impl AppConfig {
    pub fn load(project_dir: &Path) -> Result<Self> {
        let path = resolve_settings_path(project_dir)?;
        load_from_path(&path)
    }

    pub fn save(&self, project_dir: &Path) -> Result<PathBuf> {
        let path = resolve_settings_path(project_dir)?;
        save_to_path(self, &path)?;
        Ok(path)
    }
}

fn load_from_path(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let raw = rt_fs::read_to_string(&path)
        .with_context(|| format!("failed to read settings file: {}", path.display()))?;
    let root: Value = serde_json::from_str(&raw)
        .with_context(|| format!("invalid settings JSON: {}", path.display()))?;

    let ui = root.get("ui").cloned().unwrap_or_else(|| json!({}));
    let cfg = serde_json::from_value::<AppConfig>(ui).unwrap_or_default();
    Ok(cfg)
}

fn default_settings_json() -> Value {
    json!({
        "ollama": {
            "url": DEFAULT_OLLAMA_URL,
            "model": DEFAULT_OLLAMA_MODEL
        }
    })
}

fn resolve_settings_path(project_dir: &Path) -> Result<PathBuf> {
    resolve_settings_path_with_home(
        project_dir,
        rt_env::home_dir_opt().as_deref(),
    )
}

fn resolve_settings_path_with_home(project_dir: &Path, home: Option<&Path>) -> Result<PathBuf> {
    let cwd_path = project_dir.join(".localcoder/settings.json");
    if cwd_path.exists() {
        return Ok(cwd_path);
    }

    if let Some(home) = home {
        let home_path = home.join(".localcoder/settings.json");
        if home_path.exists() {
            return Ok(home_path);
        }
    }

    Ok(cwd_path)
}

fn save_to_path(config: &AppConfig, path: &Path) -> Result<()> {
    let mut root: Value = if path.exists() {
        let raw = rt_fs::read_to_string(path)
            .with_context(|| format!("failed to read settings file: {}", path.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("invalid settings JSON: {}", path.display()))?
    } else {
        default_settings_json()
    };

    root["ui"] = serde_json::to_value(config).context("failed to serialize ui config")?;

    if let Some(parent) = path.parent() {
        rt_fs::create_dir_all(parent)
            .with_context(|| format!("failed to create settings dir: {}", parent.display()))?;
    }

    rt_fs::write(
        path,
        serde_json::to_string_pretty(&root).context("failed to serialize settings")?,
    )
    .with_context(|| format!("failed to write settings file: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_defaults_when_missing_ui() {
        let project = TempDir::new().unwrap();
        let dir = project.path().join(".localcoder");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.json"),
            r#"{"ollama":{"url":"http://localhost:11434","model":"qwen"}}"#,
        )
        .unwrap();

        let path = resolve_settings_path_with_home(project.path(), None).unwrap();
        let cfg = load_from_path(&path).unwrap();
        assert_eq!(cfg.theme, Theme::Default);
        assert!(cfg.tips);
        assert_eq!(cfg.output_style, "default");
    }

    #[test]
    fn save_and_reload_ui_config() {
        let project = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let cfg = AppConfig {
            theme: Theme::Dark,
            tips: false,
            output_style: "concise".to_string(),
        };

        let path = resolve_settings_path_with_home(project.path(), Some(home.path())).unwrap();
        save_to_path(&cfg, &path).unwrap();
        assert!(path.exists());

        let loaded = load_from_path(&path).unwrap();
        assert_eq!(loaded.theme, Theme::Dark);
        assert!(!loaded.tips);
        assert_eq!(loaded.output_style, "concise");
    }
}
