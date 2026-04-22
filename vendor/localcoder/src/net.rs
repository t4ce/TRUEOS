use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
#[cfg(feature = "host-net")]
use std::time::Duration;

const DEFAULT_TIMEOUT_MS: u64 = 15_000;
const LOCALCODER_POST_TIMEOUT_MS: u64 = 60_000;

pub struct HttpResponse {
    status: u16,
    final_url: String,
    content_type: Option<String>,
    body: Vec<u8>,
}

impl HttpResponse {
    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn final_url(&self) -> &str {
        &self.final_url
    }

    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    pub fn text(self) -> Result<String> {
        String::from_utf8(self.body).context("response body is not valid UTF-8")
    }

    pub fn json<T: DeserializeOwned>(self) -> Result<T> {
        serde_json::from_slice(&self.body).context("failed to parse JSON response body")
    }
}

pub async fn get(url: &str) -> Result<HttpResponse> {
    backend::get(url).await
}

pub async fn post_json<T: Serialize>(url: &str, body: &T) -> Result<HttpResponse> {
    backend::post_json(url, body).await
}

pub async fn post_json_with_headers<T: Serialize>(
    url: &str,
    body: &T,
    headers: &[(&str, &str)],
) -> Result<HttpResponse> {
    backend::post_json_with_headers(url, body, headers).await
}

#[cfg(feature = "host-net")]
mod backend {
    use super::*;

    fn build_client() -> Result<reqwest::Client> {
        reqwest::Client::builder()
            .user_agent(format!("localcoder/{}", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_millis(DEFAULT_TIMEOUT_MS))
            .build()
            .context("failed to build HTTP client")
    }

    pub async fn get(url: &str) -> Result<HttpResponse> {
        let response = build_client()?
            .get(url)
            .send()
            .await
            .with_context(|| format!("request failed for {}", url))?;

        let status = response.status().as_u16();
        let final_url = response.url().to_string();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = response
            .bytes()
            .await
            .context("failed to read response body")?
            .to_vec();

        Ok(HttpResponse {
            status,
            final_url,
            content_type,
            body,
        })
    }

    pub async fn post_json<T: Serialize>(url: &str, body: &T) -> Result<HttpResponse> {
        post_json_with_headers(url, body, &[]).await
    }

    pub async fn post_json_with_headers<T: Serialize>(
        url: &str,
        body: &T,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse> {
        let client = build_client()?;
        let mut request = client
            .post(url)
            .header("content-type", "application/json");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request
            .json(body)
            .send()
            .await
            .with_context(|| format!("request failed for {}", url))?;

        let status = response.status().as_u16();
        let final_url = response.url().to_string();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = response
            .bytes()
            .await
            .context("failed to read response body")?
            .to_vec();

        Ok(HttpResponse {
            status,
            final_url,
            content_type,
            body,
        })
    }
}

#[cfg(all(not(feature = "host-net"), feature = "trueos-net"))]
mod backend {
    use super::*;
    use anyhow::{anyhow, bail};
    use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

    const FS_ERR_NOT_FOUND: i32 = -8;
    const FS_ERR_TIMEOUT: i32 = -14;
    const POLL_STEP_MS: u64 = 10;

    async fn wait_for_bytes_op(op_id: u32, timeout_ms: u64, what: &str) -> Result<()> {
        let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms);
        loop {
            let wait_rc = v::vnetfs::fetch_bytes_wait(op_id, 0);
            if wait_rc == 0 {
                return Ok(());
            }
            if wait_rc != FS_ERR_NOT_FOUND {
                bail!("TRUEOS {} wait failed rc={}", what, wait_rc);
            }
            if Instant::now() >= deadline {
                bail!("TRUEOS {} wait failed rc={}", what, FS_ERR_TIMEOUT);
            }
            Timer::after(EmbassyDuration::from_millis(POLL_STEP_MS)).await;
        }
    }

    async fn fetch_bytes_async(url: &str) -> Result<Vec<u8>> {
        let op_id = v::vnetfs::fetch_bytes(url.as_bytes())
            .map_err(|rc| anyhow!("TRUEOS fetch_bytes start failed rc={}", rc))?;
        if let Err(err) = wait_for_bytes_op(op_id, DEFAULT_TIMEOUT_MS, "fetch_bytes").await {
            let _ = v::vnetfs::fetch_bytes_discard(op_id);
            return Err(err);
        }

        let bytes = v::vnetfs::fetch_bytes_read(op_id)
            .map_err(|rc| anyhow!("TRUEOS fetch_bytes read failed rc={}", rc))?;
        let _ = v::vnetfs::fetch_bytes_discard(op_id);
        Ok(bytes)
    }

    async fn post_json_async<T: Serialize>(url: &str, body: &T) -> Result<Vec<u8>> {
        let body = serde_json::to_vec(body).context("failed to serialize request body")?;
        let op_id = v::vnetfs::fetch_post_json_bytes_with_timeout(
            url.as_bytes(),
            &body,
            None,
            LOCALCODER_POST_TIMEOUT_MS.min(u64::from(u32::MAX)) as u32,
        )
            .map_err(|rc| anyhow!("TRUEOS post_json start failed rc={}", rc))?;
        if let Err(err) = wait_for_bytes_op(op_id, LOCALCODER_POST_TIMEOUT_MS, "post_json").await {
            let _ = v::vnetfs::fetch_bytes_discard(op_id);
            return Err(err);
        }

        let bytes = v::vnetfs::fetch_bytes_read(op_id)
            .map_err(|rc| anyhow!("TRUEOS post_json read failed rc={}", rc))?;
        let _ = v::vnetfs::fetch_bytes_discard(op_id);
        Ok(bytes)
    }

    fn sniff_content_type(url: &str, body: &[u8]) -> Option<String> {
        let prefix = core::str::from_utf8(&body[..body.len().min(256)]).ok()?.trim_start();
        if prefix.starts_with("<!DOCTYPE html")
            || prefix.starts_with("<html")
            || prefix.contains("<html")
        {
            return Some("text/html".to_string());
        }
        if prefix.starts_with('{') || prefix.starts_with('[') {
            return Some("application/json".to_string());
        }
        if prefix.starts_with("<?xml") || prefix.contains("<rss") {
            return Some("application/xml".to_string());
        }
        if url.ends_with(".html") || url.ends_with(".htm") {
            return Some("text/html".to_string());
        }
        None
    }

    pub async fn get(url: &str) -> Result<HttpResponse> {
        let body = fetch_bytes_async(url).await?;
        Ok(HttpResponse {
            status: 200,
            final_url: url.to_string(),
            content_type: sniff_content_type(url, &body),
            body,
        })
    }

    pub async fn post_json<T: Serialize>(url: &str, body: &T) -> Result<HttpResponse> {
        post_json_with_headers(url, body, &[]).await
    }

    pub async fn post_json_with_headers<T: Serialize>(
        url: &str,
        body: &T,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse> {
        if headers
            .iter()
            .any(|(name, _)| !name.eq_ignore_ascii_case("content-type"))
        {
            bail!("TRUEOS net backend does not yet support arbitrary custom headers");
        }
        let body = post_json_async(url, body).await?;
        Ok(HttpResponse {
            status: 200,
            final_url: url.to_string(),
            content_type: Some("application/json".to_string()),
            body,
        })
    }
}

#[cfg(not(any(feature = "host-net", feature = "trueos-net")))]
mod backend {
    use super::*;
    use anyhow::bail;

    pub async fn get(_url: &str) -> Result<HttpResponse> {
        bail!("no network backend enabled; enable 'host-net' or 'trueos-net'")
    }

    pub async fn post_json<T: Serialize>(_url: &str, _body: &T) -> Result<HttpResponse> {
        bail!("no network backend enabled; enable 'host-net' or 'trueos-net'")
    }

    pub async fn post_json_with_headers<T: Serialize>(
        _url: &str,
        _body: &T,
        _headers: &[(&str, &str)],
    ) -> Result<HttpResponse> {
        bail!("no network backend enabled; enable 'host-net' or 'trueos-net'")
    }
}
