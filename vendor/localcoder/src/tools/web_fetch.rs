/*!
 * WebFetchTool — S14
 *
 * Fetches a web page over HTTP(S), blocks local/private destinations, and
 * returns a markdown-like text extraction.
 */

use crate::net;
use crate::tools::Tool;
use anyhow::{Context, Result, anyhow};
use regex::Regex;
use serde_json::{Value, json};
use std::net::IpAddr;
use url::Url;

const DEFAULT_MAX_CHARS: usize = 12_000;
const MAX_FETCH_CHARS: usize = 50_000;

pub struct WebFetchTool;

impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "WebFetch"
    }

    fn description(&self) -> &str {
        "Fetch a public HTTP(S) URL and return a readable text extraction. Use for documentation pages, blog posts, API references, and articles."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Public HTTP or HTTPS URL to fetch"
                },
                "prompt": {
                    "type": "string",
                    "description": "Optional focus hint such as the part of the page you care about"
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Maximum characters to return (default 12000, max 50000)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow!("WebFetch: missing required field 'url'"))?;
        let prompt = input["prompt"].as_str();
        let max_chars = clamp_max_chars(input["max_chars"].as_u64());
        fetch_url(url, prompt, max_chars).await
    }
}

pub async fn fetch_url(url: &str, prompt: Option<&str>, max_chars: usize) -> Result<String> {
    let url = validate_public_web_url(url)?;
    let response = net::get(url.as_str())
        .await
        .with_context(|| format!("failed to fetch {}", url))?;

    let status = response.status();
    if !response.is_success() {
        return Err(anyhow!("WebFetch: request failed with HTTP {}", status));
    }

    let final_url = response.final_url().to_string();
    let content_type = response.content_type().unwrap_or("unknown").to_string();
    let raw = response.text().context("failed to read response body")?;

    let (title, body) = if is_html_content_type(&content_type) {
        let title = extract_html_title(&raw);
        let markdown = html_to_markdown_like(&raw);
        let focused = focus_content(&markdown, prompt);
        (title, truncate_chars(&focused, max_chars))
    } else {
        let focused = focus_content(&decode_html_entities(&raw), prompt);
        (None, truncate_chars(&focused, max_chars))
    };

    let mut out = format!("URL: {}\nContent-Type: {}", final_url, content_type);
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        out.push_str(&format!("\nTitle: {}", title));
    }
    if let Some(prompt) = prompt.map(str::trim).filter(|prompt| !prompt.is_empty()) {
        out.push_str(&format!("\nFocus: {}", prompt));
    }
    out.push_str("\n\n");
    out.push_str(&body);

    Ok(out)
}

fn clamp_max_chars(value: Option<u64>) -> usize {
    value
        .unwrap_or(DEFAULT_MAX_CHARS as u64)
        .clamp(500, MAX_FETCH_CHARS as u64) as usize
}

fn validate_public_web_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw).with_context(|| format!("invalid URL: {}", raw))?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => return Err(anyhow!("WebFetch: unsupported URL scheme '{}'", scheme)),
    }

    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("WebFetch: URL is missing a hostname"))?;
    if is_blocked_host(host) {
        return Err(anyhow!(
            "WebFetch: blocked non-public destination '{}'",
            host
        ));
    }

    Ok(url)
}

fn is_blocked_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".local") {
        return true;
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(ip) => {
                ip.is_private()
                    || ip.is_loopback()
                    || ip.is_link_local()
                    || ip.is_broadcast()
                    || ip.is_documentation()
                    || ip.is_unspecified()
            }
            IpAddr::V6(ip) => {
                ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
            }
        };
    }

    false
}

fn is_html_content_type(content_type: &str) -> bool {
    let value = content_type.to_ascii_lowercase();
    value.contains("text/html") || value.contains("application/xhtml")
}

fn extract_html_title(html: &str) -> Option<String> {
    let regex = Regex::new(r"(?is)<title[^>]*>(.*?)</title>").ok()?;
    let captures = regex.captures(html)?;
    let title = decode_html_entities(captures.get(1)?.as_str());
    let title = collapse_whitespace(&strip_tags(&title));
    if title.is_empty() { None } else { Some(title) }
}

fn html_to_markdown_like(html: &str) -> String {
    let mut text = html.to_string();

    for pattern in [
        r"(?is)<script[^>]*>.*?</script>",
        r"(?is)<style[^>]*>.*?</style>",
        r"(?is)<noscript[^>]*>.*?</noscript>",
        r"(?is)<!--.*?-->",
    ] {
        text = Regex::new(pattern)
            .expect("valid cleanup regex")
            .replace_all(&text, " ")
            .into_owned();
    }

    for pattern in [
        r"(?is)<br\s*/?>",
        r"(?is)</p>",
        r"(?is)</div>",
        r"(?is)</section>",
        r"(?is)</article>",
        r"(?is)</main>",
        r"(?is)</header>",
        r"(?is)</footer>",
        r"(?is)</aside>",
        r"(?is)</h[1-6]>",
        r"(?is)</li>",
        r"(?is)</ul>",
        r"(?is)</ol>",
        r"(?is)</table>",
        r"(?is)</tr>",
        r"(?is)</blockquote>",
        r"(?is)</pre>",
    ] {
        text = Regex::new(pattern)
            .expect("valid break regex")
            .replace_all(&text, "\n")
            .into_owned();
    }

    text = strip_tags(&text);
    text = decode_html_entities(&text);
    normalize_text_blocks(&text)
}

fn strip_tags(input: &str) -> String {
    Regex::new(r"(?is)<[^>]+>")
        .expect("valid tag regex")
        .replace_all(input, " ")
        .into_owned()
}

fn decode_html_entities(input: &str) -> String {
    let mut out = input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");

    let hex = Regex::new(r"&#x([0-9a-fA-F]+);").expect("valid hex entity regex");
    out = hex
        .replace_all(&out, |caps: &regex::Captures<'_>| {
            u32::from_str_radix(&caps[1], 16)
                .ok()
                .and_then(char::from_u32)
                .map(|ch| ch.to_string())
                .unwrap_or_default()
        })
        .into_owned();

    let dec = Regex::new(r"&#([0-9]+);").expect("valid dec entity regex");
    dec.replace_all(&out, |caps: &regex::Captures<'_>| {
        caps[1]
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map(|ch| ch.to_string())
            .unwrap_or_default()
    })
    .into_owned()
}

fn normalize_text_blocks(input: &str) -> String {
    let mut lines = Vec::new();
    let mut last_blank = false;

    for line in input.lines() {
        let collapsed = collapse_whitespace(line);
        if collapsed.is_empty() {
            if !last_blank {
                lines.push(String::new());
                last_blank = true;
            }
            continue;
        }
        lines.push(collapsed);
        last_blank = false;
    }

    lines.join("\n").trim().to_string()
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn focus_content(content: &str, prompt: Option<&str>) -> String {
    let prompt = prompt.unwrap_or("").trim();
    if prompt.is_empty() {
        return content.to_string();
    }

    let keywords = extract_keywords(prompt);
    if keywords.is_empty() {
        return content.to_string();
    }

    let blocks = content
        .split("\n\n")
        .map(str::trim)
        .filter(|block| !block.is_empty())
        .collect::<Vec<_>>();

    let mut scored = blocks
        .iter()
        .enumerate()
        .map(|(index, block)| (index, score_block(block, &keywords)))
        .filter(|(_, score)| *score > 0)
        .collect::<Vec<_>>();

    if scored.is_empty() {
        return content.to_string();
    }

    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut indices = scored
        .into_iter()
        .take(8)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    indices.sort_unstable();

    indices
        .into_iter()
        .map(|index| blocks[index])
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn extract_keywords(prompt: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    prompt
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|word| word.trim().to_ascii_lowercase())
        .filter(|word| word.len() >= 3)
        .filter(|word| {
            !matches!(
                word.as_str(),
                "the"
                    | "and"
                    | "for"
                    | "with"
                    | "that"
                    | "this"
                    | "from"
                    | "into"
                    | "what"
                    | "when"
                    | "where"
                    | "which"
                    | "please"
                    | "show"
                    | "tell"
            )
        })
        .filter(|word| seen.insert(word.clone()))
        .take(8)
        .collect()
}

fn score_block(block: &str, keywords: &[String]) -> usize {
    let lower = block.to_ascii_lowercase();
    keywords
        .iter()
        .map(|keyword| lower.match_indices(keyword).count())
        .sum()
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (index, ch) in input.chars().enumerate() {
        if index >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_public_web_url_blocks_localhost() {
        let err = validate_public_web_url("http://127.0.0.1:8080").unwrap_err();
        assert!(err.to_string().contains("blocked"));
    }

    #[test]
    fn html_to_markdown_like_strips_tags() {
        let input = "<html><head><title>Example</title><style>.x{}</style></head><body><h1>Hello</h1><p>world &amp; more</p></body></html>";
        let output = html_to_markdown_like(input);
        assert!(output.contains("Hello"));
        assert!(output.contains("world & more"));
        assert!(!output.contains("<h1>"));
    }

    #[test]
    fn focus_content_prefers_matching_blocks() {
        let content = "Alpha block\n\nRust async runtime details\n\nAnother block";
        let focused = focus_content(content, Some("rust async"));
        assert!(focused.contains("Rust async runtime details"));
        assert!(!focused.contains("Alpha block"));
    }
}
