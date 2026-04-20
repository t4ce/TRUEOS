use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
#[cfg(feature = "host-net")]
use std::time::Duration;

const DEFAULT_TIMEOUT_MS: u64 = 15_000;

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

    fn fetch_bytes_blocking(url: &str) -> Result<Vec<u8>> {
        let op_id = v::vnetfs::fetch_bytes(url.as_bytes())
            .map_err(|rc| anyhow!("TRUEOS fetch_bytes start failed rc={}", rc))?;
        let wait_rc = v::vnetfs::fetch_bytes_wait(op_id, DEFAULT_TIMEOUT_MS);
        if wait_rc != 0 {
            let _ = v::vnetfs::fetch_bytes_discard(op_id);
            bail!("TRUEOS fetch_bytes wait failed rc={}", wait_rc);
        }

        let bytes = v::vnetfs::fetch_bytes_read(op_id)
            .map_err(|rc| anyhow!("TRUEOS fetch_bytes read failed rc={}", rc))?;
        let _ = v::vnetfs::fetch_bytes_discard(op_id);
        Ok(bytes)
    }

    fn post_json_blocking<T: Serialize>(url: &str, body: &T) -> Result<Vec<u8>> {
        let body = serde_json::to_vec(body).context("failed to serialize request body")?;
        let op_id = v::vnetfs::fetch_post_json_bytes(url.as_bytes(), &body, None)
            .map_err(|rc| anyhow!("TRUEOS post_json start failed rc={}", rc))?;
        let wait_rc = v::vnetfs::fetch_bytes_wait(op_id, DEFAULT_TIMEOUT_MS);
        if wait_rc != 0 {
            let _ = v::vnetfs::fetch_bytes_discard(op_id);
            bail!("TRUEOS post_json wait failed rc={}", wait_rc);
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
        let body = fetch_bytes_blocking(url)?;
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
        let body = post_json_blocking(url, body)?;
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
