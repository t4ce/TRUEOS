use super::client::LspClient;
use super::types::{
    LspServerConfig, canonical_extension, detect_workspace_extensions, file_uri,
    load_lsp_server_configs,
};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

pub struct LspServerManager {
    cwd: PathBuf,
    root_uri: String,
    configs: Vec<LspServerConfig>,
    workspace_extensions: HashSet<String>,
    state: Mutex<ManagerState>,
}

struct ManagerState {
    servers: HashMap<String, RunningServer>,
}

struct RunningServer {
    config: LspServerConfig,
    client: LspClient,
    documents: HashMap<PathBuf, OpenDocument>,
}

struct OpenDocument {
    uri: String,
    version: i32,
    text: String,
}

pub struct WorkspaceSymbolBatch {
    pub results: Vec<(String, Value)>,
    pub errors: Vec<String>,
}

impl LspServerManager {
    pub fn new(cwd: &Path) -> Result<Self> {
        let cwd = cwd
            .canonicalize()
            .with_context(|| format!("failed to resolve workspace root: {}", cwd.display()))?;
        let root_uri = file_uri(&cwd)?;
        let configs = load_lsp_server_configs(&cwd)?;
        let workspace_extensions = detect_workspace_extensions(&cwd);

        Ok(Self {
            cwd,
            root_uri,
            configs,
            workspace_extensions,
            state: Mutex::new(ManagerState {
                servers: HashMap::new(),
            }),
        })
    }

    pub fn has_servers(&self) -> bool {
        !self.configs.is_empty()
    }

    pub fn workspace_root(&self) -> &Path {
        &self.cwd
    }

    pub fn render_status(&self) -> String {
        if self.configs.is_empty() {
            return "LSP disabled or no language servers configured".to_string();
        }

        self.configs
            .iter()
            .map(|config| {
                format!(
                    "- {} [{}] -> {} {}",
                    config.name,
                    config.extensions.join(", "),
                    config.command,
                    config.args.join(" ")
                )
                .trim_end()
                .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub async fn request_for_file(
        &self,
        file_path: &Path,
        method: &str,
        params: Value,
    ) -> Result<Option<(String, Value)>> {
        let absolute = self.resolve_path(file_path)?;
        let extension = canonical_extension(&absolute)
            .ok_or_else(|| anyhow!("file has no extension: {}", absolute.display()))?;
        let Some(config) = self.config_for_extension(&extension).cloned() else {
            return Ok(None);
        };

        let mut state = self.state.lock().await;
        let server = self
            .ensure_server_started(&mut state, config)
            .await
            .with_context(|| {
                format!("failed to initialize LSP server for {}", absolute.display())
            })?;
        Self::sync_document(server, &absolute).await?;
        let result = server
            .client
            .send_request(method, params)
            .await
            .with_context(|| format!("LSP request '{}' failed", method))?;
        Ok(Some((server.config.name.clone(), result)))
    }

    pub async fn request_workspace_symbols(&self, query: &str) -> Result<WorkspaceSymbolBatch> {
        let mut state = self.state.lock().await;
        let mut results = Vec::new();
        let mut errors = Vec::new();

        let configs = self.workspace_configs();
        for config in configs {
            match self.ensure_server_started(&mut state, config.clone()).await {
                Ok(server) => match server
                    .client
                    .send_request("workspace/symbol", json!({"query": query}))
                    .await
                {
                    Ok(value) => results.push((server.config.name.clone(), value)),
                    Err(err) => errors.push(format!("{}: {}", config.name, err)),
                },
                Err(err) => errors.push(format!("{}: {}", config.name, err)),
            }
        }

        Ok(WorkspaceSymbolBatch { results, errors })
    }

    pub async fn shutdown(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        for server in state.servers.values_mut() {
            for document in server.documents.values() {
                let _ = server
                    .client
                    .send_notification(
                        "textDocument/didClose",
                        json!({
                            "textDocument": {"uri": document.uri}
                        }),
                    )
                    .await;
            }
            server.client.shutdown().await?;
        }
        state.servers.clear();
        Ok(())
    }

    fn resolve_path(&self, path: &Path) -> Result<PathBuf> {
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        };
        candidate
            .canonicalize()
            .with_context(|| format!("failed to resolve path: {}", candidate.display()))
    }

    fn config_for_extension(&self, extension: &str) -> Option<&LspServerConfig> {
        self.configs
            .iter()
            .find(|config| config.matches_extension(extension))
    }

    fn workspace_configs(&self) -> Vec<LspServerConfig> {
        let configs = self
            .configs
            .iter()
            .filter(|config| {
                self.workspace_extensions.is_empty()
                    || config
                        .extensions
                        .iter()
                        .any(|ext| self.workspace_extensions.contains(ext))
            })
            .cloned()
            .collect::<Vec<_>>();

        if configs.is_empty() {
            self.configs.clone()
        } else {
            configs
        }
    }

    async fn ensure_server_started<'a>(
        &'a self,
        state: &'a mut ManagerState,
        config: LspServerConfig,
    ) -> Result<&'a mut RunningServer> {
        if !state.servers.contains_key(&config.name) {
            let mut client =
                LspClient::start(&config.name, &config.command, &config.args, &self.cwd)
                    .await
                    .map_err(|err| with_install_hint(err, &config))?;
            client
                .initialize(initialization_params(&self.cwd, &self.root_uri))
                .await
                .map_err(|err| with_install_hint(err, &config))?;
            state.servers.insert(
                config.name.clone(),
                RunningServer {
                    config: config.clone(),
                    client,
                    documents: HashMap::new(),
                },
            );
        }

        state
            .servers
            .get_mut(&config.name)
            .ok_or_else(|| anyhow!("LSP server '{}' failed to start", config.name))
    }

    async fn sync_document(server: &mut RunningServer, path: &Path) -> Result<()> {
        let uri = file_uri(path)?;
        let text = fs::read_to_string(path).with_context(|| {
            format!(
                "failed to read source file for LSP sync: {}",
                path.display()
            )
        })?;

        match server.documents.get_mut(path) {
            Some(document) => {
                if document.text != text {
                    document.version += 1;
                    document.text = text.clone();
                    server
                        .client
                        .send_notification(
                            "textDocument/didChange",
                            json!({
                                "textDocument": {
                                    "uri": document.uri,
                                    "version": document.version,
                                },
                                "contentChanges": [{"text": text}],
                            }),
                        )
                        .await?;
                    server
                        .client
                        .send_notification(
                            "textDocument/didSave",
                            json!({
                                "textDocument": {"uri": document.uri},
                                "text": document.text,
                            }),
                        )
                        .await?;
                }
            }
            None => {
                server
                    .client
                    .send_notification(
                        "textDocument/didOpen",
                        json!({
                            "textDocument": {
                                "uri": uri,
                                "languageId": server.config.language_id_for_path(path),
                                "version": 1,
                                "text": text,
                            }
                        }),
                    )
                    .await?;
                server
                    .client
                    .send_notification(
                        "textDocument/didSave",
                        json!({
                            "textDocument": {"uri": uri},
                            "text": text,
                        }),
                    )
                    .await?;
                server.documents.insert(
                    path.to_path_buf(),
                    OpenDocument {
                        uri,
                        version: 1,
                        text,
                    },
                );
            }
        }

        Ok(())
    }
}

fn with_install_hint(error: anyhow::Error, config: &LspServerConfig) -> anyhow::Error {
    let mut message = error.to_string();
    if message.contains("No such file or directory") || message.contains("not found") {
        message.push_str(&format!(
            ". Install '{}' or override lsp.servers in .localcoder/settings.json",
            config.command
        ));
    }
    anyhow!(message)
}

fn initialization_params(cwd: &Path, root_uri: &str) -> Value {
    json!({
        "processId": std::process::id(),
        "rootUri": root_uri,
        "rootPath": cwd,
        "workspaceFolders": [{
            "uri": root_uri,
            "name": cwd.file_name().and_then(|name| name.to_str()).unwrap_or("workspace")
        }],
        "capabilities": {
            "workspace": {
                "configuration": false,
                "workspaceFolders": false
            },
            "textDocument": {
                "synchronization": {
                    "dynamicRegistration": false,
                    "didSave": true,
                    "willSave": false,
                    "willSaveWaitUntil": false
                },
                "hover": {
                    "dynamicRegistration": false,
                    "contentFormat": ["markdown", "plaintext"]
                },
                "definition": {
                    "dynamicRegistration": false,
                    "linkSupport": true
                },
                "references": {
                    "dynamicRegistration": false
                },
                "documentSymbol": {
                    "dynamicRegistration": false,
                    "hierarchicalDocumentSymbolSupport": true
                },
                "callHierarchy": {
                    "dynamicRegistration": false
                }
            },
            "general": {
                "positionEncodings": ["utf-16"]
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn render_status_lists_servers() {
        let temp = TempDir::new().unwrap();
        let manager = LspServerManager::new(temp.path()).unwrap();
        let status = manager.render_status();
        assert!(status.contains("rust-analyzer"));
        assert!(status.contains("typescript-language-server"));
    }

    #[test]
    fn resolve_path_handles_relative_inputs() {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join("src")).unwrap();
        fs::write(temp.path().join("src/lib.rs"), "pub fn demo() {}\n").unwrap();

        let manager = LspServerManager::new(temp.path()).unwrap();
        let resolved = manager.resolve_path(Path::new("src/lib.rs")).unwrap();
        assert!(resolved.ends_with("src/lib.rs"));
    }

    #[test]
    fn workspace_configs_fall_back_when_no_files_detected() {
        let temp = TempDir::new().unwrap();
        let manager = LspServerManager::new(temp.path()).unwrap();
        assert_eq!(manager.workspace_configs().len(), manager.configs.len());
    }
}
