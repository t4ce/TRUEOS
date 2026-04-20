use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::Path;
use std::process::Stdio;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

pub struct LspClient {
    server_name: String,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    request_id: u64,
}

impl LspClient {
    pub async fn start(
        server_name: &str,
        command: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<Self> {
        let mut child = Command::new(command)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| {
                format!(
                    "failed to start language server '{}' via command '{}'",
                    server_name, command
                )
            })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            anyhow!(
                "failed to capture stdin for language server '{}'",
                server_name
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            anyhow!(
                "failed to capture stdout for language server '{}'",
                server_name
            )
        })?;

        Ok(Self {
            server_name: server_name.to_string(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            request_id: 0,
        })
    }

    pub async fn initialize(&mut self, params: Value) -> Result<()> {
        self.send_request("initialize", params).await?;
        self.send_notification("initialized", json!({})).await
    }

    pub async fn send_request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id();
        write_message(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }),
        )
        .await
        .with_context(|| format!("failed to send LSP request '{}'", method))?;

        self.read_response(id).await
    }

    pub async fn send_notification(&mut self, method: &str, params: Value) -> Result<()> {
        write_message(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
            }),
        )
        .await
        .with_context(|| format!("failed to send LSP notification '{}'", method))
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = self.send_request("shutdown", json!(null)).await;
        let _ = self.send_notification("exit", json!(null)).await;
        let _ = self.child.kill().await;
        Ok(())
    }

    fn next_id(&mut self) -> u64 {
        self.request_id += 1;
        self.request_id
    }

    async fn read_response(&mut self, target_id: u64) -> Result<Value> {
        loop {
            let message = read_message(&mut self.stdout).await.with_context(|| {
                format!(
                    "failed to read message from language server '{}'",
                    self.server_name
                )
            })?;

            if is_notification(&message) {
                continue;
            }

            if is_server_request(&message) {
                self.respond_to_server_request(&message).await?;
                continue;
            }

            let Some(id) = message.get("id").and_then(Value::as_u64) else {
                continue;
            };
            if id != target_id {
                continue;
            }

            if let Some(error) = message.get("error") {
                let code = error
                    .get("code")
                    .and_then(Value::as_i64)
                    .unwrap_or_default();
                let msg = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown LSP error");
                return Err(anyhow!("LSP error {}: {}", code, msg));
            }

            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    async fn respond_to_server_request(&mut self, request: &Value) -> Result<()> {
        let id = request
            .get("id")
            .cloned()
            .ok_or_else(|| anyhow!("server request missing id"))?;
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();

        let result = match method {
            "workspace/configuration" => Value::Array(Vec::new()),
            "window/workDoneProgress/create" => Value::Null,
            "client/registerCapability" => Value::Null,
            "client/unregisterCapability" => Value::Null,
            _ => Value::Null,
        };

        write_message(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }),
        )
        .await
        .with_context(|| format!("failed to respond to LSP server request '{}'", method))
    }
}

fn is_notification(message: &Value) -> bool {
    message.get("method").is_some() && message.get("id").is_none()
}

fn is_server_request(message: &Value) -> bool {
    message.get("method").is_some() && message.get("id").is_some()
}

async fn write_message<W>(writer: &mut W, payload: &Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let body = serde_json::to_vec(payload).context("failed to serialize LSP payload")?;
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await
        .context("failed to write LSP content length")?;
    writer
        .write_all(&body)
        .await
        .context("failed to write LSP payload body")?;
    writer
        .flush()
        .await
        .context("failed to flush LSP payload")?;
    Ok(())
}

async fn read_message<R>(reader: &mut R) -> Result<Value>
where
    R: AsyncBufRead + Unpin,
{
    let mut content_length = None;

    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .await
            .context("failed to read LSP header line")?;
        if read == 0 {
            return Err(anyhow!("language server closed stdout"));
        }

        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }

        if let Some(value) = line.strip_prefix("Content-Length:") {
            let length = value
                .trim()
                .parse::<usize>()
                .with_context(|| format!("invalid Content-Length header: {}", line))?;
            content_length = Some(length);
        }
    }

    let length =
        content_length.ok_or_else(|| anyhow!("LSP message missing Content-Length header"))?;
    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .await
        .context("failed to read LSP message body")?;

    serde_json::from_slice(&body).context("failed to decode LSP JSON payload")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{BufReader, duplex};

    #[tokio::test]
    async fn write_and_read_message_roundtrip() {
        let (client, server) = duplex(1024);
        let payload = json!({"jsonrpc":"2.0","id":1,"result":{"ok":true}});

        let writer = tokio::spawn(async move {
            let mut client = client;
            write_message(&mut client, &payload).await.unwrap();
        });

        let mut reader = BufReader::new(server);
        let message = read_message(&mut reader).await.unwrap();
        writer.await.unwrap();
        assert_eq!(message["id"], 1);
        assert_eq!(message["result"]["ok"], true);
    }

    #[tokio::test]
    async fn read_message_rejects_missing_content_length() {
        let (_, server) = duplex(256);
        let mut reader = BufReader::new(server);
        let err = read_message(&mut reader).await.unwrap_err();
        assert!(err.to_string().contains("closed stdout"));
    }
}
