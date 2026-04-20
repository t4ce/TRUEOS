use anyhow::{Context, Result, anyhow};
use ignore::WalkBuilder;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspOperation {
    GoToDefinition,
    FindReferences,
    Hover,
    DocumentSymbols,
    WorkspaceSymbols,
    CallHierarchy,
}

impl LspOperation {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "go_to_definition" | "goto_definition" | "definition" => Some(Self::GoToDefinition),
            "find_references" | "references" => Some(Self::FindReferences),
            "hover" => Some(Self::Hover),
            "document_symbols" | "document_symbol" | "symbols" => Some(Self::DocumentSymbols),
            "workspace_symbols" | "workspace_symbol" => Some(Self::WorkspaceSymbols),
            "call_hierarchy" | "calls" => Some(Self::CallHierarchy),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GoToDefinition => "go_to_definition",
            Self::FindReferences => "find_references",
            Self::Hover => "hover",
            Self::DocumentSymbols => "document_symbols",
            Self::WorkspaceSymbols => "workspace_symbols",
            Self::CallHierarchy => "call_hierarchy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallHierarchyDirection {
    Incoming,
    Outgoing,
}

impl CallHierarchyDirection {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "incoming" | "in" => Some(Self::Incoming),
            "outgoing" | "out" => Some(Self::Outgoing),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Incoming => "incoming",
            Self::Outgoing => "outgoing",
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct LspServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub language_id: Option<String>,
}

impl LspServerConfig {
    pub fn matches_extension(&self, extension: &str) -> bool {
        let extension = normalize_extension(extension);
        self.extensions
            .iter()
            .map(|ext| normalize_extension(ext))
            .any(|ext| ext == extension)
    }

    pub fn language_id_for_path(&self, path: &Path) -> String {
        if let Some(language_id) = self
            .language_id
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            return language_id.trim().to_string();
        }

        let extension = canonical_extension(path).unwrap_or_default();
        default_language_id(&extension)
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SettingsRoot {
    #[serde(default)]
    lsp: Option<LspSettings>,
}

#[derive(Debug, Clone, Deserialize)]
struct LspSettings {
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    servers: Vec<LspServerConfig>,
}

impl Default for LspSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            servers: Vec::new(),
        }
    }
}

fn default_enabled() -> bool {
    true
}

pub fn default_servers() -> Vec<LspServerConfig> {
    vec![
        LspServerConfig {
            name: "rust-analyzer".to_string(),
            command: "rust-analyzer".to_string(),
            args: Vec::new(),
            extensions: vec![".rs".to_string()],
            language_id: Some("rust".to_string()),
        },
        LspServerConfig {
            name: "typescript-language-server".to_string(),
            command: "typescript-language-server".to_string(),
            args: vec!["--stdio".to_string()],
            extensions: vec![
                ".ts".to_string(),
                ".tsx".to_string(),
                ".js".to_string(),
                ".jsx".to_string(),
                ".mjs".to_string(),
                ".cjs".to_string(),
            ],
            language_id: Some("typescript".to_string()),
        },
        LspServerConfig {
            name: "pyright-langserver".to_string(),
            command: "pyright-langserver".to_string(),
            args: vec!["--stdio".to_string()],
            extensions: vec![".py".to_string()],
            language_id: Some("python".to_string()),
        },
        LspServerConfig {
            name: "gopls".to_string(),
            command: "gopls".to_string(),
            args: Vec::new(),
            extensions: vec![".go".to_string()],
            language_id: Some("go".to_string()),
        },
    ]
}

pub fn load_lsp_server_configs(project_dir: &Path) -> Result<Vec<LspServerConfig>> {
    let path = resolve_settings_path(project_dir);
    if let Some(path) = path {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read settings file: {}", path.display()))?;
        let root: SettingsRoot = serde_json::from_str(&raw)
            .with_context(|| format!("invalid settings JSON: {}", path.display()))?;

        if let Some(lsp) = root.lsp {
            if !lsp.enabled {
                return Ok(Vec::new());
            }
            if !lsp.servers.is_empty() {
                return normalize_configs(lsp.servers);
            }
        }
    }

    Ok(default_servers())
}

pub fn detect_workspace_extensions(project_dir: &Path) -> HashSet<String> {
    let mut extensions = HashSet::new();
    let walker = WalkBuilder::new(project_dir)
        .hidden(false)
        .standard_filters(true)
        .build();

    for entry in walker.flatten() {
        if !entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false)
        {
            continue;
        }

        if let Some(ext) = canonical_extension(entry.path()) {
            extensions.insert(ext);
        }
    }

    extensions
}

pub fn canonical_extension(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_string_lossy();
    let ext = ext.trim();
    if ext.is_empty() {
        None
    } else {
        Some(format!(".{}", ext.to_ascii_lowercase()))
    }
}

pub fn normalize_extension(value: &str) -> String {
    let trimmed = value.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!(".{}", trimmed.to_ascii_lowercase())
    }
}

pub fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| anyhow!("failed to convert path to file URI: {}", path.display()))
}

pub fn file_path_from_uri(uri: &str) -> Result<PathBuf> {
    Url::parse(uri)
        .with_context(|| format!("invalid file URI: {}", uri))?
        .to_file_path()
        .map_err(|_| anyhow!("unsupported file URI: {}", uri))
}

fn resolve_settings_path(project_dir: &Path) -> Option<PathBuf> {
    let local = project_dir.join(".localcoder/settings.json");
    if local.exists() {
        return Some(local);
    }

    let home = std::env::var_os("HOME")?;
    let home_path = PathBuf::from(home).join(".localcoder/settings.json");
    home_path.exists().then_some(home_path)
}

fn normalize_configs(configs: Vec<LspServerConfig>) -> Result<Vec<LspServerConfig>> {
    configs
        .into_iter()
        .map(|mut config| {
            if config.name.trim().is_empty() {
                return Err(anyhow!("lsp.servers[].name must not be empty"));
            }
            if config.command.trim().is_empty() {
                return Err(anyhow!("lsp.servers[].command must not be empty"));
            }
            if config.extensions.is_empty() {
                return Err(anyhow!(
                    "lsp.servers[{}].extensions must contain at least one extension",
                    config.name
                ));
            }
            config.extensions = config
                .extensions
                .iter()
                .map(|ext| normalize_extension(ext))
                .filter(|ext| !ext.is_empty())
                .collect();
            if config.extensions.is_empty() {
                return Err(anyhow!(
                    "lsp.servers[{}].extensions resolved to an empty list",
                    config.name
                ));
            }
            Ok(config)
        })
        .collect()
}

fn default_language_id(extension: &str) -> String {
    match extension {
        ".rs" => "rust".to_string(),
        ".ts" | ".tsx" => "typescript".to_string(),
        ".js" | ".jsx" | ".mjs" | ".cjs" => "javascript".to_string(),
        ".py" => "python".to_string(),
        ".go" => "go".to_string(),
        other if !other.is_empty() => other.trim_start_matches('.').to_string(),
        _ => "plaintext".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_defaults_when_settings_missing() {
        let temp = TempDir::new().unwrap();
        let configs = load_lsp_server_configs(temp.path()).unwrap();
        assert!(configs.iter().any(|cfg| cfg.command == "rust-analyzer"));
        assert!(
            configs
                .iter()
                .any(|cfg| cfg.command == "typescript-language-server")
        );
    }

    #[test]
    fn load_custom_lsp_servers_from_settings() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().join(".localcoder");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.json"),
            r#"{
              "ollama": {"url": "http://localhost:11434", "model": "qwen"},
              "lsp": {
                "servers": [
                  {
                    "name": "lua-language-server",
                    "command": "lua-language-server",
                    "args": [],
                    "extensions": ["lua"],
                    "language_id": "lua"
                  }
                ]
              }
            }"#,
        )
        .unwrap();

        let configs = load_lsp_server_configs(temp.path()).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "lua-language-server");
        assert_eq!(configs[0].extensions, vec![".lua"]);
    }

    #[test]
    fn disabled_lsp_returns_no_servers() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().join(".localcoder");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.json"),
            r#"{
              "ollama": {"url": "http://localhost:11434", "model": "qwen"},
              "lsp": {"enabled": false}
            }"#,
        )
        .unwrap();

        assert!(load_lsp_server_configs(temp.path()).unwrap().is_empty());
    }

    #[test]
    fn detect_workspace_extensions_collects_gitignored_aware_set() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("main.rs"), "fn main() {}\n").unwrap();
        fs::create_dir_all(temp.path().join("src")).unwrap();
        fs::write(temp.path().join("src/app.ts"), "export const ok = true;\n").unwrap();

        let extensions = detect_workspace_extensions(temp.path());
        assert!(extensions.contains(".rs"));
        assert!(extensions.contains(".ts"));
    }

    #[test]
    fn file_uri_roundtrip_preserves_path() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("src/test.rs");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "fn main() {}\n").unwrap();

        let uri = file_uri(&file).unwrap();
        let roundtrip = file_path_from_uri(&uri).unwrap();
        assert_eq!(roundtrip, file);
    }
}
