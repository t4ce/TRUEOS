/*!
 * WebSearchTool — S14
 *
 * Runs a public web search without API keys and returns compact result cards.
 */

use crate::net;
use crate::tools::Tool;
use anyhow::{Context, Result, anyhow};
use regex::Regex;
use serde_json::{Value, json};
use url::Url;

const DEFAULT_RESULTS: usize = 5;
const MAX_RESULTS: usize = 10;

pub struct WebSearchTool;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "WebSearch"
    }

    fn description(&self) -> &str {
        "Search the public web and return compact result cards with titles, URLs, and snippets. Use for recent docs, articles, and general web discovery."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of search results to return (default 5, max 10)"
                },
                "domain": {
                    "type": "string",
                    "description": "Optional site/domain filter, for example docs.rs"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| anyhow!("WebSearch: missing required field 'query'"))?;
        let limit = clamp_limit(input["limit"].as_u64());
        let domain = input["domain"].as_str();
        search_web(query, domain, limit).await
    }
}

pub async fn search_web(query: &str, domain: Option<&str>, limit: usize) -> Result<String> {
    let query = query.trim();
    if query.is_empty() {
        return Err(anyhow!("WebSearch: query must not be empty"));
    }

    let effective_query = match domain.map(str::trim).filter(|domain| !domain.is_empty()) {
        Some(domain) => format!("site:{} {}", sanitize_domain(domain)?, query),
        None => query.to_string(),
    };

    let mut url = Url::parse("https://www.bing.com/search").expect("static search url");
    url.query_pairs_mut()
        .append_pair("format", "rss")
        .append_pair("q", &effective_query);

    let response = net::get(url.as_str())
        .await
        .context("failed to execute web search request")?;

    let status = response.status();
    if !response.is_success() {
        return Err(anyhow!("WebSearch: request failed with HTTP {}", status));
    }

    let rss = response.text().context("failed to read web search body")?;
    let results = parse_bing_rss_results(&rss)
        .into_iter()
        .take(limit)
        .collect::<Vec<_>>();

    if results.is_empty() {
        return Err(anyhow!("WebSearch: no results found"));
    }

    Ok(format_search_results(query, domain, &results))
}

fn clamp_limit(value: Option<u64>) -> usize {
    value
        .unwrap_or(DEFAULT_RESULTS as u64)
        .clamp(1, MAX_RESULTS as u64) as usize
}

fn sanitize_domain(domain: &str) -> Result<String> {
    let domain = domain
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_matches('/');
    if domain.is_empty() {
        return Err(anyhow!("WebSearch: domain must not be empty"));
    }
    if !domain
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '*'))
    {
        return Err(anyhow!("WebSearch: invalid domain filter '{}'", domain));
    }
    Ok(domain.to_string())
}

fn parse_bing_rss_results(rss: &str) -> Vec<SearchResult> {
    let item_re = Regex::new(r"(?is)<item>(.*?)</item>").expect("valid item regex");
    let title_re = Regex::new(r"(?is)<title>(.*?)</title>").expect("valid title regex");
    let link_re = Regex::new(r"(?is)<link>(.*?)</link>").expect("valid link regex");
    let desc_re =
        Regex::new(r"(?is)<description>(.*?)</description>").expect("valid description regex");

    item_re
        .captures_iter(rss)
        .filter_map(|item_caps| {
            let item = item_caps.get(1)?.as_str();
            let title = capture_xml_text(&title_re, item)?;
            let url = capture_xml_text(&link_re, item)?;
            let snippet = capture_xml_text(&desc_re, item).unwrap_or_default();

            Some(SearchResult {
                title,
                url,
                snippet,
            })
        })
        .collect()
}

fn capture_xml_text(regex: &Regex, item: &str) -> Option<String> {
    let raw = regex.captures(item)?.get(1)?.as_str();
    let raw = raw
        .trim()
        .trim_start_matches("<![CDATA[")
        .trim_end_matches("]]>")
        .trim();
    let decoded = decode_html_entities(raw);
    let clean = collapse_whitespace(&strip_tags(&decoded));
    if clean.is_empty() { None } else { Some(clean) }
}

fn strip_tags(input: &str) -> String {
    Regex::new(r"(?is)<[^>]+>")
        .expect("valid tag regex")
        .replace_all(input, " ")
        .into_owned()
}

fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_search_results(query: &str, domain: Option<&str>, results: &[SearchResult]) -> String {
    let mut out = format!("Query: {}", query);
    if let Some(domain) = domain.map(str::trim).filter(|domain| !domain.is_empty()) {
        out.push_str(&format!("\nDomain filter: {}", domain));
    }

    for (index, result) in results.iter().enumerate() {
        out.push_str(&format!(
            "\n\n{}. {}\nURL: {}\nSnippet: {}",
            index + 1,
            result.title,
            result.url,
            result.snippet
        ));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bing_rss_results_reads_items() {
        let rss = r#"
        <rss><channel>
          <item>
            <title><![CDATA[Rust Async Book]]></title>
            <link>https://rust-lang.github.io/async-book/</link>
            <description><![CDATA[Learn async programming in Rust]]></description>
          </item>
          <item>
            <title>Tokio Tutorial</title>
            <link>https://tokio.rs/tokio/tutorial</link>
            <description>Practical Tokio guide</description>
          </item>
        </channel></rss>
        "#;

        let results = parse_bing_rss_results(rss);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Async Book");
        assert_eq!(results[1].url, "https://tokio.rs/tokio/tutorial");
    }

    #[test]
    fn sanitize_domain_rejects_invalid_chars() {
        let err = sanitize_domain("docs.rs/path").unwrap_err();
        assert!(err.to_string().contains("invalid"));
    }

    #[test]
    fn format_search_results_renders_cards() {
        let output = format_search_results(
            "rust async runtime",
            Some("docs.rs"),
            &[SearchResult {
                title: "Tokio".to_string(),
                url: "https://docs.rs/tokio".to_string(),
                snippet: "Async runtime docs".to_string(),
            }],
        );

        assert!(output.contains("Query: rust async runtime"));
        assert!(output.contains("1. Tokio"));
    }
}
