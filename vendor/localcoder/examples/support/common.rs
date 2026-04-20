use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

#[path = "../../src/net.rs"]
mod net;

const ANTHROPIC_MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";

pub async fn anthropic_post(body: &Value, api_key: &str) -> Result<Value> {
    let response = net::post_json_with_headers(
        ANTHROPIC_MESSAGES_URL,
        body,
        &[
            ("x-api-key", api_key),
            ("anthropic-version", "2023-06-01"),
            ("content-type", "application/json"),
        ],
    )
    .await
    .context("Anthropic request failed")?;

    if !response.is_success() {
        let status = response.status();
        let error_text = response.text().unwrap_or_default();
        bail!("Anthropic returned error {}: {}", status, error_text);
    }

    response
        .json()
        .context("failed to parse Anthropic JSON response")
}

pub async fn anthropic_post_allow_error(body: &Value, api_key: &str) -> Result<Value> {
    let response = net::post_json_with_headers(
        ANTHROPIC_MESSAGES_URL,
        body,
        &[
            ("x-api-key", api_key),
            ("anthropic-version", "2023-06-01"),
            ("content-type", "application/json"),
        ],
    )
    .await
    .context("Anthropic request failed")?;

    if !response.is_success() {
        let status = response.status();
        let error_text = response.text().unwrap_or_default();
        return Err(anyhow!("Anthropic returned error {}: {}", status, error_text));
    }

    response
        .json()
        .context("failed to parse Anthropic JSON response")
}
