use alloc::string::String;
use embassy_executor::{SpawnError, Spawner};
use heapless::String as HString;

use super::{ShellBackend2, print_shell_line};

const ALLOWED_SUFFIXES: [&str; 8] = [".de", ".eu", ".com", ".fr", ".co.uk", ".io", ".net", ".it"];

pub(crate) fn try_parse(line: &str) -> Option<String> {
    let candidate = line.trim();
    if candidate.is_empty() || candidate.split_whitespace().nth(1).is_some() {
        return None;
    }

    if !is_domain_chars_only(candidate) {
        return None;
    }

    let lowered = candidate.to_ascii_lowercase();
    if !ALLOWED_SUFFIXES.iter().any(|suffix| lowered.ends_with(suffix)) {
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
    let mut url = String::from("https://");
    url.push_str(host);
    url
}

fn is_domain_chars_only(s: &str) -> bool {
    let mut saw_dot = false;
    let mut prev_dot = false;

    for ch in s.chars() {
        let ok = ch.is_ascii_alphanumeric() || ch == '-' || ch == '.';
        if !ok {
            return false;
        }

        if ch == '.' {
            if prev_dot {
                return false;
            }
            saw_dot = true;
            prev_dot = true;
        } else {
            prev_dot = false;
        }
    }

    saw_dot && !s.starts_with('.') && !s.ends_with('.')
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
