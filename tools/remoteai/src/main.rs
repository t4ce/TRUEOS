use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use remoteai::{ChatBackend, CopilotBackend, ServerConfig, app};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("remoteai=info")),
        )
        .with_target(false)
        .init();

    serve().await
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let bind: SocketAddr = "192.168.178.111:3042".parse()?;
    let bearer_token = static_bearer_token();
    let model = "auto".to_string();
    let timeout_seconds = 24;
    if !(5..=25).contains(&timeout_seconds) {
        return Err("request_timeout_seconds must be between 5 and 25".into());
    }
    let copilot_home = default_copilot_home();

    let backend = Arc::new(
        CopilotBackend::start(
            model.clone(),
            Duration::from_secs(timeout_seconds),
            copilot_home,
            None,
        )
        .await?,
    );
    let router = app(
        ServerConfig {
            bearer_token,
            advertised_model: model.clone(),
        },
        backend.clone(),
    )?;
    let listener = TcpListener::bind(bind).await?;
    let local_addr = listener.local_addr()?;
    tracing::info!(bind = %local_addr, model = %model, "remoteAI OpenAI-compatible facade ready");
    if !local_addr.ip().is_loopback() {
        tracing::warn!(
            "serving plaintext HTTP beyond loopback; use only on a trusted LAN with the bearer token"
        );
    }

    let serve_result = axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    if let Err(error) = backend.shutdown().await {
        tracing::warn!(%error, "Copilot runtime did not shut down cleanly");
    }
    serve_result?;
    Ok(())
}

fn static_bearer_token() -> String {
    let mut seed = String::new();
    if let Ok(machine_id) = std::fs::read_to_string("/etc/machine-id") {
        seed.push_str(machine_id.trim());
    }
    if let Ok(hostname) = std::fs::read_to_string("/etc/hostname") {
        seed.push_str(hostname.trim());
    }
    if let Ok(executable) = env::current_exe() {
        seed.push_str(&executable.to_string_lossy());
    }
    if seed.is_empty() {
        seed.push_str("remoteai-static-host-token");
    }

    let digest = Sha256::digest(seed.as_bytes());
    hex(&digest)
}

fn default_copilot_home() -> PathBuf {
    if let Some(state_home) = env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(state_home).join("remoteai/copilot");
    }
    if let Some(user_home) = env::var_os("HOME") {
        return PathBuf::from(user_home).join(".local/state/remoteai/copilot");
    }
    env::temp_dir().join("remoteai-copilot")
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    tracing::info!("shutdown requested");
}
