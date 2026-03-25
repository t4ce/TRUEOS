use alloc::string::String;
use embassy_executor::Spawner;

use super::{ShellBackend2, print_shell_line};
use crate::tst_html_shack::{self, HtmlRoad, HtmlShackFileError};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfPromptPrefix {
    Http,
    Https,
    File,
    Html,
}

impl SurfPromptPrefix {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Https => Self::Http,
            Self::Http => Self::File,
            Self::File => Self::Html,
            Self::Html => Self::Https,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Http => "http://",
            Self::Https => "https://",
            Self::File => "file://",
            Self::Html => "html://",
        }
    }
}

pub(crate) enum SurfSubmit {
    Url(String),
    File(String),
    Html(String),
}

pub(crate) fn try_inline_html(line: &str) -> Option<String> {
    let candidate = strip_wrapping_quotes(line.trim());
    if !looks_like_inline_html(candidate) {
        return None;
    }
    Some(String::from(candidate))
}

pub(crate) fn try_parse_with_prefix(line: &str, prefix: SurfPromptPrefix) -> Option<SurfSubmit> {
    if let Some(html) = try_inline_html(line) {
        return Some(SurfSubmit::Html(html));
    }
    if let Some(file_ref) = try_file_reference(line) {
        return Some(SurfSubmit::File(file_ref));
    }

    let candidate = strip_wrapping_quotes(line.trim());
    if candidate.is_empty() {
        return None;
    }

    match prefix {
        SurfPromptPrefix::Html => Some(SurfSubmit::Html(String::from(candidate))),
        SurfPromptPrefix::File => Some(SurfSubmit::File(String::from(candidate))),
        SurfPromptPrefix::Http | SurfPromptPrefix::Https => {
            if candidate.split_whitespace().nth(1).is_some() || !is_url_token(candidate) {
                return None;
            }
            Some(SurfSubmit::Url(prepare_url_with_prefix(candidate, prefix)))
        }
    }
}

pub(crate) fn try_file_reference(line: &str) -> Option<String> {
    let candidate = strip_wrapping_quotes(line.trim());
    let path = candidate.strip_prefix("file://")?;
    if path.trim().is_empty() {
        return None;
    }
    Some(String::from(path))
}

pub(crate) fn load_inline_html(io: &'static dyn ShellBackend2, html: String) {
    let _ = tst_html_shack::get_ready_inline_html(html);
    print_shell_line(io, "shack enque");
}

pub(crate) fn load_file_reference(io: &'static dyn ShellBackend2, file_ref: &str) {
    match tst_html_shack::get_ready_file_html(file_ref) {
        Ok(_) => print_shell_line(io, "shack enque"),
        Err(HtmlShackFileError::NoRoot) => {
            print_shell_line(io, "surf: no TRUEOSFS root mounted");
        }
        Err(HtmlShackFileError::NotFound) => {
            print_shell_line(io, "surf: file not found");
        }
        Err(HtmlShackFileError::ReadFailed) => {
            print_shell_line(io, "surf: file read failed");
        }
    }
}

pub(crate) fn prepare_call_with_url(_spawner: &Spawner, io: &'static dyn ShellBackend2, url: &str) {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return;
    }

    if trimmed.len() > 256 {
        print_shell_line(io, "surf: url too long (max 256 chars)");
        return;
    }

    let road = if trimmed
        .get(..8)
        .map(|p| p.eq_ignore_ascii_case("https://"))
        .unwrap_or(false)
    {
        HtmlRoad::Https
    } else {
        HtmlRoad::Http
    };

    let _ = tst_html_shack::with_html_shack(|shack| shack.get_ready(trimmed, road, None));
    print_shell_line(io, "shack enque");
}

fn prepare_url_with_prefix(host: &str, prefix: SurfPromptPrefix) -> String {
    if has_known_scheme(host) {
        return String::from(host);
    }

    let mut url = String::from(match prefix {
        SurfPromptPrefix::Http => "http://",
        SurfPromptPrefix::Https => "https://",
        SurfPromptPrefix::File => "file://",
        SurfPromptPrefix::Html => "html://",
    });
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

fn has_known_scheme(s: &str) -> bool {
    has_http_scheme(s)
        || s.get(..7)
            .map(|p| p.eq_ignore_ascii_case("file://"))
            .unwrap_or(false)
        || s.get(..7)
            .map(|p| p.eq_ignore_ascii_case("html://"))
            .unwrap_or(false)
}

fn is_url_token(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(char::is_whitespace)
}

fn looks_like_inline_html(s: &str) -> bool {
    let lower = s.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }

    (lower.starts_with("<html") && lower.ends_with("</html>"))
        || lower.starts_with("<!doctype html")
        || (lower.starts_with('<') && lower.ends_with('>') && lower.contains("</"))
}
