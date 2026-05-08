//! Backend adapter for invoking a local `localcoder` CLI.
//!
//! This module is intentionally route-agnostic: Axum handlers can serialize the
//! public request/response structs and call `status()` or `chat()` when the
//! backend crate wires this in.

use serde::{Deserialize, Serialize};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::fs as async_fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub const LOCALCODER_BIN_ENV: &str = "LOCALCODER_BIN";
pub const LOCALCODER_ARGS_ENV: &str = "LOCALCODER_ARGS";
pub const DEFAULT_CHAT_TIMEOUT_MS: u64 = 120_000;

const PROMPT_PLACEHOLDER: &str = "{prompt}";
const PATH_BINARY_NAME: &str = "localcoder";
const RELATIVE_BIN_CANDIDATES: &[&str] = &[
    "./localcoder",
    "../localcoder/target/debug/localcoder",
    "../localcoder/target/release/localcoder",
    "../LocalCoder/target/debug/localcoder",
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LocalCoderChatRequest {
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LocalCoderChatResponse {
    pub ok: bool,
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stderr: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub bin_path: String,
    pub strategy: LocalCoderCommandStrategy,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LocalCoderStatusResponse {
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bin_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<LocalCoderBinSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub checked_candidates: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LocalCoderError {
    pub kind: LocalCoderErrorKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalCoderBinSource {
    EnvVar,
    RelativeCandidate,
    Path,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalCoderCommandStrategy {
    DefaultStdin,
    EnvArgsStdin,
    EnvArgsPromptArg,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalCoderErrorKind {
    Unavailable,
    InvalidArgs,
    SpawnFailed,
    Io,
    TimedOut,
    ExitFailed,
}

impl std::fmt::Display for LocalCoderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.detail {
            Some(detail) => write!(f, "{}: {}", self.message, detail),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for LocalCoderError {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DiscoveredBinary {
    path: PathBuf,
    source: LocalCoderBinSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DiscoveryReport {
    binary: Option<DiscoveredBinary>,
    checked_candidates: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CommandPlan {
    args: Vec<String>,
    stdin_prompt: bool,
    strategy: LocalCoderCommandStrategy,
}

pub async fn status() -> LocalCoderStatusResponse {
    let report = discover_binary_async().await;
    match report.binary {
        Some(binary) => LocalCoderStatusResponse {
            available: true,
            bin_path: Some(path_to_string(&binary.path)),
            source: Some(binary.source),
            message: None,
            checked_candidates: report.checked_candidates,
        },
        None => LocalCoderStatusResponse {
            available: false,
            bin_path: None,
            source: None,
            message: Some("localcoder executable not found".to_string()),
            checked_candidates: report.checked_candidates,
        },
    }
}

pub fn status_sync() -> LocalCoderStatusResponse {
    let report = discover_binary();
    match report.binary {
        Some(binary) => LocalCoderStatusResponse {
            available: true,
            bin_path: Some(path_to_string(&binary.path)),
            source: Some(binary.source),
            message: None,
            checked_candidates: report.checked_candidates,
        },
        None => LocalCoderStatusResponse {
            available: false,
            bin_path: None,
            source: None,
            message: Some("localcoder executable not found".to_string()),
            checked_candidates: report.checked_candidates,
        },
    }
}

pub async fn chat(
    request: LocalCoderChatRequest,
) -> Result<LocalCoderChatResponse, LocalCoderError> {
    let report = discover_binary_async().await;
    let Some(binary) = report.binary else {
        return Err(LocalCoderError {
            kind: LocalCoderErrorKind::Unavailable,
            message: "localcoder executable not found".to_string(),
            detail: Some(format!("checked {} candidate(s)", report.checked_candidates.len())),
        });
    };

    let plan = command_plan(&request.prompt)?;
    let output = run_localcoder(&binary.path, &request.prompt, &plan, request.timeout_ms).await?;

    if !output.status.success() {
        return Err(LocalCoderError {
            kind: LocalCoderErrorKind::ExitFailed,
            message: "localcoder exited with a non-zero status".to_string(),
            detail: Some(format!(
                "exit_code={:?}; stderr={}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            )),
        });
    }

    Ok(LocalCoderChatResponse {
        ok: true,
        text: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        exit_code: output.status.code(),
        bin_path: path_to_string(&binary.path),
        strategy: plan.strategy,
    })
}

async fn run_localcoder(
    bin_path: &Path,
    prompt: &str,
    plan: &CommandPlan,
    timeout_ms: Option<u64>,
) -> Result<std::process::Output, LocalCoderError> {
    let mut command = Command::new(bin_path);
    command
        .args(&plan.args)
        .stdin(if plan.stdin_prompt {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command.spawn().map_err(|err| LocalCoderError {
        kind: LocalCoderErrorKind::SpawnFailed,
        message: "failed to spawn localcoder".to_string(),
        detail: Some(err.to_string()),
    })?;

    if plan.stdin_prompt {
        let Some(mut stdin) = child.stdin.take() else {
            return Err(LocalCoderError {
                kind: LocalCoderErrorKind::Io,
                message: "failed to open localcoder stdin".to_string(),
                detail: None,
            });
        };

        stdin
            .write_all(prompt.as_bytes())
            .await
            .map_err(io_error("failed to write prompt to localcoder stdin"))?;
        if !prompt.ends_with('\n') {
            stdin
                .write_all(b"\n")
                .await
                .map_err(io_error("failed to finish localcoder stdin"))?;
        }
        drop(stdin);
    }

    let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_CHAT_TIMEOUT_MS));
    tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| LocalCoderError {
            kind: LocalCoderErrorKind::TimedOut,
            message: "localcoder timed out".to_string(),
            detail: Some(format!("timeout_ms={}", timeout.as_millis())),
        })?
        .map_err(io_error("failed to read localcoder output"))
}

fn command_plan(prompt: &str) -> Result<CommandPlan, LocalCoderError> {
    let Some(raw_args) = env_string(LOCALCODER_ARGS_ENV) else {
        return Ok(CommandPlan {
            args: Vec::new(),
            stdin_prompt: true,
            strategy: LocalCoderCommandStrategy::DefaultStdin,
        });
    };

    let raw_args = raw_args.trim();
    if raw_args.is_empty() {
        return Ok(CommandPlan {
            args: Vec::new(),
            stdin_prompt: true,
            strategy: LocalCoderCommandStrategy::DefaultStdin,
        });
    }

    let mut saw_prompt_placeholder = false;
    let args = split_args(raw_args)
        .map_err(|detail| LocalCoderError {
            kind: LocalCoderErrorKind::InvalidArgs,
            message: format!("failed to parse {}", LOCALCODER_ARGS_ENV),
            detail: Some(detail),
        })?
        .into_iter()
        .map(|arg| {
            if arg.contains(PROMPT_PLACEHOLDER) {
                saw_prompt_placeholder = true;
                arg.replace(PROMPT_PLACEHOLDER, prompt)
            } else {
                arg
            }
        })
        .collect();

    Ok(CommandPlan {
        args,
        stdin_prompt: !saw_prompt_placeholder,
        strategy: if saw_prompt_placeholder {
            LocalCoderCommandStrategy::EnvArgsPromptArg
        } else {
            LocalCoderCommandStrategy::EnvArgsStdin
        },
    })
}

fn discover_binary() -> DiscoveryReport {
    let mut checked_candidates = Vec::new();

    if let Some(configured) = env_os_nonempty(LOCALCODER_BIN_ENV) {
        for path in executable_candidates_for_path(PathBuf::from(configured)) {
            checked_candidates.push(path_to_string(&path));
            if is_executable_file(&path) {
                return DiscoveryReport {
                    binary: Some(DiscoveredBinary {
                        path: canonical_or_original(path),
                        source: LocalCoderBinSource::EnvVar,
                    }),
                    checked_candidates,
                };
            }
        }
    }

    for candidate in RELATIVE_BIN_CANDIDATES {
        for path in executable_candidates_for_path(PathBuf::from(candidate)) {
            checked_candidates.push(path_to_string(&path));
            if is_executable_file(&path) {
                return DiscoveryReport {
                    binary: Some(DiscoveredBinary {
                        path: canonical_or_original(path),
                        source: LocalCoderBinSource::RelativeCandidate,
                    }),
                    checked_candidates,
                };
            }
        }
    }

    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            for name in path_binary_names() {
                let path = dir.join(name);
                checked_candidates.push(path_to_string(&path));
                if is_executable_file(&path) {
                    return DiscoveryReport {
                        binary: Some(DiscoveredBinary {
                            path: canonical_or_original(path),
                            source: LocalCoderBinSource::Path,
                        }),
                        checked_candidates,
                    };
                }
            }
        }
    }

    DiscoveryReport {
        binary: None,
        checked_candidates,
    }
}

async fn discover_binary_async() -> DiscoveryReport {
    let mut checked_candidates = Vec::new();

    if let Some(configured) = env_os_nonempty(LOCALCODER_BIN_ENV) {
        for path in executable_candidates_for_path(PathBuf::from(configured)) {
            checked_candidates.push(path_to_string(&path));
            if is_executable_file_async(&path).await {
                return DiscoveryReport {
                    binary: Some(DiscoveredBinary {
                        path: canonical_or_original_async(path).await,
                        source: LocalCoderBinSource::EnvVar,
                    }),
                    checked_candidates,
                };
            }
        }
    }

    for candidate in RELATIVE_BIN_CANDIDATES {
        for path in executable_candidates_for_path(PathBuf::from(candidate)) {
            checked_candidates.push(path_to_string(&path));
            if is_executable_file_async(&path).await {
                return DiscoveryReport {
                    binary: Some(DiscoveredBinary {
                        path: canonical_or_original_async(path).await,
                        source: LocalCoderBinSource::RelativeCandidate,
                    }),
                    checked_candidates,
                };
            }
        }
    }

    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            for name in path_binary_names() {
                let path = dir.join(name);
                checked_candidates.push(path_to_string(&path));
                if is_executable_file_async(&path).await {
                    return DiscoveryReport {
                        binary: Some(DiscoveredBinary {
                            path: canonical_or_original_async(path).await,
                            source: LocalCoderBinSource::Path,
                        }),
                        checked_candidates,
                    };
                }
            }
        }
    }

    DiscoveryReport {
        binary: None,
        checked_candidates,
    }
}

fn split_args(input: &str) -> Result<Vec<String>, String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote = Quote::None;
    let mut arg_started = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match quote {
            Quote::None => match ch {
                c if c.is_whitespace() => {
                    if arg_started {
                        args.push(std::mem::take(&mut current));
                        arg_started = false;
                    }
                }
                '\'' => {
                    quote = Quote::Single;
                    arg_started = true;
                }
                '"' => {
                    quote = Quote::Double;
                    arg_started = true;
                }
                '\\' => {
                    let Some(next) = chars.next() else {
                        return Err("trailing backslash".to_string());
                    };
                    current.push(next);
                    arg_started = true;
                }
                _ => {
                    current.push(ch);
                    arg_started = true;
                }
            },
            Quote::Single => {
                if ch == '\'' {
                    quote = Quote::None;
                } else {
                    current.push(ch);
                }
            }
            Quote::Double => match ch {
                '"' => quote = Quote::None,
                '\\' => {
                    let Some(next) = chars.next() else {
                        return Err("trailing backslash in double quotes".to_string());
                    };
                    current.push(next);
                }
                _ => current.push(ch),
            },
        }
    }

    match quote {
        Quote::None => {}
        Quote::Single => return Err("unterminated single quote".to_string()),
        Quote::Double => return Err("unterminated double quote".to_string()),
    }

    if arg_started {
        args.push(current);
    }

    Ok(args)
}

fn executable_candidates_for_path(path: PathBuf) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        if path.extension().is_some() {
            return vec![path];
        }
        let mut candidates = vec![path.clone()];
        for extension in ["exe", "cmd", "bat"] {
            let mut candidate = path.clone();
            candidate.set_extension(extension);
            candidates.push(candidate);
        }
        candidates
    }

    #[cfg(not(windows))]
    {
        vec![path]
    }
}

fn path_binary_names() -> Vec<&'static str> {
    #[cfg(windows)]
    {
        vec![
            "localcoder.exe",
            "localcoder.cmd",
            "localcoder.bat",
            PATH_BINARY_NAME,
        ]
    }

    #[cfg(not(windows))]
    {
        vec![PATH_BINARY_NAME]
    }
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

async fn is_executable_file_async(path: &Path) -> bool {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        async_fs::try_exists(path).await.unwrap_or(false)
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        let Ok(metadata) = async_fs::metadata(path).await else {
            return false;
        };
        if !metadata.is_file() {
            return false;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        }

        #[cfg(not(unix))]
        {
            true
        }
    }
}

fn canonical_or_original(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

async fn canonical_or_original_async(path: PathBuf) -> PathBuf {
    async_fs::canonicalize(&path).await.unwrap_or(path)
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn env_string(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_os_nonempty(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

fn io_error(message: &'static str) -> impl FnOnce(std::io::Error) -> LocalCoderError {
    move |err| LocalCoderError {
        kind: LocalCoderErrorKind::Io,
        message: message.to_string(),
        detail: Some(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_quoted_args() {
        assert_eq!(
            split_args("chat --prompt \"hello local coder\" '--mode=text'").unwrap(),
            vec!["chat", "--prompt", "hello local coder", "--mode=text"]
        );
    }

    #[test]
    fn rejects_unterminated_quotes() {
        let err = split_args("chat \"oops").unwrap_err();
        assert!(err.contains("unterminated double quote"));
    }
}
