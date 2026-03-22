extern crate alloc;

use super::http::{self, HttpFetchError};
use crate::r::net::https;
use alloc::string::String;
use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::String as HString;

const SURF_TIMEOUT_MS: u32 = 35_000;
const SURF_MAX_BYTES: usize = 4 * 1024 * 1024;
const SURF_HTTPS_TIMEOUT_MS: u32 = SURF_TIMEOUT_MS * 4;

#[embassy_executor::task]
pub async fn html_fetch_service() {
    loop {}
}

fn build_best_effort_attempts(url: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return out;
    }
    if let Some(rest) = trimmed.strip_prefix("https://") {
        out.push(String::from(trimmed));
        out.push(alloc::format!("http://{}", rest));
        return out;
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        out.push(String::from(trimmed));
        out.push(alloc::format!("https://{}", rest));
        return out;
    }
    out.push(alloc::format!("https://{}", trimmed));
    out.push(alloc::format!("http://{}", trimmed));
    out
}

fn bytes_to_string_lossy(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(bytes.as_slice()).into_owned()
}

async fn fetch_html_attempt_with_redirects(url: &str) -> Result<Vec<u8>, &'static str> {
    const MAX_REDIRECTS: usize = 5;
    let mut current_url = String::from(url);
    let mut saw_timeout = false;

    for hop in 0..=MAX_REDIRECTS {
        if current_url.starts_with("https://") {
            match https::fetch_https_body_async(
                current_url.as_str(),
                SURF_HTTPS_TIMEOUT_MS,
                SURF_MAX_BYTES,
            )
            .await
            {
                Ok(body) => return Ok(body),
                Err(https::FetchError::Redirect { url, .. }) => {
                    if hop >= MAX_REDIRECTS {
                        return Err("too many redirects");
                    }
                    current_url = url;
                    continue;
                }
                Err(https::FetchError::ConnectTimeout)
                | Err(https::FetchError::DnsTimeout)
                | Err(https::FetchError::TlsTimeout)
                | Err(https::FetchError::BodyTimeout) => {
                    saw_timeout = true;
                    break;
                }
                Err(_) => break,
            }
        } else if current_url.starts_with("http://") {
            match http::fetch_http_body(current_url.as_str(), SURF_TIMEOUT_MS, SURF_MAX_BYTES).await
            {
                Ok(body) => return Ok(body),
                Err(HttpFetchError::Redirect(url)) => {
                    if hop >= MAX_REDIRECTS {
                        return Err("too many redirects");
                    }
                    current_url = url;
                    continue;
                }
                Err(HttpFetchError::TimedOut) => {
                    saw_timeout = true;
                    break;
                }
                Err(_) => break,
            }
        } else {
            break;
        }
    }

    if saw_timeout {
        return Err("timed out");
    }
    Err("all attempts failed")
}

pub async fn fetch_html_best_effort(url: HString<256>) -> Result<String, &'static str> {
    let attempts = build_best_effort_attempts(url.as_str());
    if attempts.is_empty() {
        return Err("bad url");
    }
    let mut saw_timeout = false;
    for attempt in attempts.iter() {
        match fetch_html_attempt_with_redirects(attempt.as_str()).await {
            Ok(body) => return Ok(bytes_to_string_lossy(body)),
            Err("timed out") => {
                saw_timeout = true;
            }
            Err(_) => {}
        }
    }
    if saw_timeout {
        return Err("timed out");
    }
    Err("all attempts failed")
}
