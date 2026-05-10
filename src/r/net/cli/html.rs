extern crate alloc;

use crate::t::net::http::{self, HttpFetchError};
use crate::t::net::https;
use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;
use heapless::String as HString;

const SURF_TIMEOUT_MS: u32 = 35_000;
const SURF_MAX_BYTES: usize = 4 * 1024 * 1024;
const SURF_HTTPS_TIMEOUT_MS: u32 = SURF_TIMEOUT_MS;
const HTML_PREVIEW_FRONT_LINES: usize = 5;
const HTML_PREVIEW_LINE_CHARS: usize = 160;

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

fn preview_line(line: &str) -> &str {
    if line.len() <= HTML_PREVIEW_LINE_CHARS {
        return line;
    }

    let mut end = 0usize;
    for (idx, ch) in line.char_indices() {
        let next = idx + ch.len_utf8();
        if next > HTML_PREVIEW_LINE_CHARS {
            break;
        }
        end = next;
    }

    if end == 0 { "" } else { &line[..end] }
}

fn log_html_preview(url: &str, html: &str) {
    let line_count = html.lines().count();
    crate::log_trace!(
        "html: received url={} bytes={} lines={} front={}\n",
        url,
        html.len(),
        line_count,
        HTML_PREVIEW_FRONT_LINES
    );

    for (idx, line) in html.lines().take(HTML_PREVIEW_FRONT_LINES).enumerate() {
        let front = preview_line(line);
        if front.len() == line.len() {
            crate::log_trace!("html: [{}] {}\n", idx + 1, front);
        } else {
            crate::log_trace!("html: [{}] {}...\n", idx + 1, front);
        }
    }
}

async fn fetch_html_attempt_with_redirects(url: &str) -> Result<Vec<u8>, &'static str> {
    // Legit sites need at most 2 hops (http→https, www→bare). Cap at 3 to give
    // one extra for edge cases and hard-stop tracker/ad redirect chains.
    const MAX_REDIRECTS: usize = 3;
    let mut current_url = String::from(url);
    let mut saw_timeout = false;
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for hop in 0..=MAX_REDIRECTS {
        if !seen.insert(current_url.clone()) {
            crate::log_trace!("html: redirect loop detected at hop={} url={}\n", hop, current_url);
            return Err("redirect loop");
        }

        if current_url.starts_with("https://") {
            match https::fetch_https_body_hyper_async(
                current_url.as_str(),
                SURF_HTTPS_TIMEOUT_MS,
                SURF_MAX_BYTES,
            )
            .await
            {
                Ok(body) => return Ok(body),
                Err(https::FetchError::Redirect { url: next, status }) => {
                    if hop >= MAX_REDIRECTS {
                        crate::log_trace!(
                            "html: too many redirects ({}), last={} next={}\n",
                            hop,
                            current_url,
                            next
                        );
                        return Err("too many redirects");
                    }
                    crate::log_trace!(
                        "html: redirect hop={} status={} {} -> {}\n",
                        hop + 1,
                        status,
                        current_url,
                        next
                    );
                    current_url = next;
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
            match http::fetch_http_body_hyper(current_url.as_str(), SURF_TIMEOUT_MS, SURF_MAX_BYTES)
                .await
            {
                Ok(body) => return Ok(body),
                Err(HttpFetchError::Redirect(next)) => {
                    if hop >= MAX_REDIRECTS {
                        crate::log_trace!(
                            "html: too many redirects ({}), last={} next={}\n",
                            hop,
                            current_url,
                            next
                        );
                        return Err("too many redirects");
                    }
                    crate::log_trace!(
                        "html: redirect hop={} {} -> {}\n",
                        hop + 1,
                        current_url,
                        next
                    );
                    current_url = next;
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
            Ok(body) => {
                let html = bytes_to_string_lossy(body);
                log_html_preview(attempt.as_str(), html.as_str());
                return Ok(html);
            }
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
