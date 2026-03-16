use alloc::string::String;
use embassy_executor::{SpawnError, Spawner};
use heapless::String as HString;

use super::{ShellBackend2, print_shell_line};

pub(crate) fn try_parse(line: &str) -> Option<String> {
    let candidate = strip_wrapping_quotes(line.trim());
    if candidate.is_empty() || candidate.split_whitespace().nth(1).is_some() {
        return None;
    }

    if !is_url_token(candidate) {
        return None;
    }

    Some(prepare_url(candidate))
}

pub(crate) fn prepare_call_with_url(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    url: &str,
) -> Result<(), SpawnError> {
    let mut job_url: HString<256> = HString::new();
    for ch in url.trim().chars() {
        if job_url.push(ch).is_err() {
            print_shell_line(io, "surf: url too long (max 256 chars)");
            return Ok(());
        }
    }

    if job_url.is_empty() {
        return Ok(());
    }

    let rc = spawner.spawn(surf_job(io, job_url));
    if rc.is_ok() {
        print_shell_line(io, "surf: started");
    }
    rc
}

fn prepare_url(host: &str) -> String {
    if has_http_scheme(host) {
        return String::from(host);
    }

    let mut url = String::from("http://");
    url.push_str(host);
    url
}

fn strip_wrapping_quotes(s: &str) -> &str {
    if s.len() >= 2 {
        let b = s.as_bytes();
        let first = b[0];
        let last = b[b.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return s[1..s.len() - 1].trim();
        }
    }
    s
}

fn has_http_scheme(s: &str) -> bool {
    s.get(..7)
        .map(|p| p.eq_ignore_ascii_case("http://"))
        .unwrap_or(false)
        || s.get(..8)
            .map(|p| p.eq_ignore_ascii_case("https://"))
            .unwrap_or(false)
}

fn is_url_token(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(char::is_whitespace)
}

#[embassy_executor::task]
async fn surf_job(io: &'static dyn ShellBackend2, url: HString<256>) {
    let source_url = String::from(url.as_str().trim());
    match crate::tst_html::fetch_html_best_effort(url).await {
        Ok(html) => {
            if !trueos_qjs::browser_task::queue_set_html_with_url(
                String::from(html.as_str()),
                Some(source_url),
            ) {
                print_shell_line(io, "surf: browser not running");
            }
        }
        Err(e) => {
            if e == "timed out" {
                print_shell_line(io, "surf: download timed out");
            } else {
                let msg = alloc::format!("surf: fetch failed: {}", e);
                print_shell_line(io, msg.as_str());
            }
        }
    }
}
