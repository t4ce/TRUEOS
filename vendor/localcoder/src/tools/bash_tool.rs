/*!
 * BashTool — S04
 *
 * Runs shell commands via bash with basic safety checks.
 *
 * Features:
 *   - Dangerous command pattern blocking
 *   - Foreground execution with timeout (default 120s)
 *   - Optional background execution
 */

use crate::tools::Tool;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 600_000;

const BLOCKED_PATTERNS: &[(&str, &str)] = &[
    ("rm -rf /", "dangerous recursive delete of root"),
    ("rm -rf ~", "dangerous recursive delete of home"),
    (":(){:|:&};:", "fork bomb"),
    ("dd if=/dev/zero of=/dev/", "disk/device overwrite"),
    ("mkfs", "filesystem format command"),
    ("shutdown", "system shutdown"),
    ("reboot", "system reboot"),
    ("> /dev/sda", "raw disk write redirection"),
];

pub struct BashTool;

impl Tool for BashTool {
    fn name(&self) -> &str {
        "Bash"
    }

    fn description(&self) -> &str {
        "Execute a bash command in the current working directory. Supports timeout control and optional background execution. Dangerous command patterns are blocked."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Bash command to run"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default 120000, max 600000)"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "Run command in background and return immediately"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let command = input["command"]
            .as_str()
            .ok_or_else(|| anyhow!("Bash: missing required field 'command'"))?
            .trim();

        if command.is_empty() {
            return Err(anyhow!("Bash: command cannot be empty"));
        }

        if let Some((pattern, reason)) = is_dangerous(command) {
            return Err(anyhow!(
                "Bash: blocked dangerous command pattern '{}': {}",
                pattern,
                reason
            ));
        }

        let timeout_ms = input["timeout"]
            .as_u64()
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .clamp(1, MAX_TIMEOUT_MS);

        let run_in_background = input["run_in_background"].as_bool().unwrap_or(false);

        if run_in_background {
            let mut child = Command::new("bash")
                .arg("-lc")
                .arg(command)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| anyhow!("Bash: failed to spawn background command: {}", e))?;

            let pid = child.id().unwrap_or(0);

            tokio::spawn(async move {
                let _ = child.wait().await;
            });

            return Ok(format!(
                "Started background command (pid {}): {}",
                pid, command
            ));
        }

        let output_future = Command::new("bash").arg("-lc").arg(command).output();

        let output = tokio::time::timeout(Duration::from_millis(timeout_ms), output_future)
            .await
            .map_err(|_| anyhow!("Bash: command timed out after {} ms", timeout_ms))?
            .map_err(|e| anyhow!("Bash: failed to execute command: {}", e))?;

        Ok(format_output(output))
    }
}

fn is_dangerous(cmd: &str) -> Option<(&'static str, &'static str)> {
    let normalized = cmd.to_ascii_lowercase();

    BLOCKED_PATTERNS
        .iter()
        .find(|(pattern, _)| normalized.contains(&pattern.to_ascii_lowercase()))
        .copied()
}

fn format_output(output: std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    let exit_code = output
        .status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "terminated by signal".to_string());

    let mut result = format!("exit_code: {}", exit_code);

    if !stdout.is_empty() {
        result.push_str("\nstdout:\n");
        result.push_str(&stdout);
    }

    if !stderr.is_empty() {
        result.push_str("\nstderr:\n");
        result.push_str(&stderr);
    }

    if stdout.is_empty() && stderr.is_empty() {
        result.push_str("\n(no output)");
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bash_executes_simple_command() {
        let result = BashTool
            .execute(json!({"command": "echo hello"}))
            .await
            .unwrap();

        assert!(result.contains("exit_code: 0"));
        assert!(result.contains("hello"));
    }

    #[tokio::test]
    async fn bash_blocks_dangerous_command() {
        let result = BashTool
            .execute(json!({"command": "rm -rf /tmp/foo && rm -rf /"}))
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("blocked dangerous command")
        );
    }

    #[tokio::test]
    async fn bash_respects_timeout() {
        let result = BashTool
            .execute(json!({"command": "sleep 2", "timeout": 20}))
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn bash_background_returns_pid_message() {
        let result = BashTool
            .execute(json!({"command": "sleep 1", "run_in_background": true}))
            .await
            .unwrap();

        assert!(result.contains("Started background command"));
    }

    #[tokio::test]
    async fn bash_missing_command_errors() {
        let result = BashTool.execute(json!({})).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("command"));
    }
}
