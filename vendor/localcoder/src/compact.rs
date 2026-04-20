/*!
 * Context Compaction — S08
 *
 * Reduces long conversations by summarizing older messages and keeping a
 * compact boundary plus the most recent turns.
 */

use anyhow::Result;
use serde_json::{Value, json};

use crate::api::LLMClient;

const TOKEN_THRESHOLD: usize = 12_000;
const CHARS_PER_TOKEN: usize = 4;
const KEEP_RECENT_MESSAGES: usize = 10;
const SUMMARY_MESSAGE_LIMIT: usize = 24;

pub async fn maybe_compact(client: &LLMClient, messages: &mut Vec<Value>) -> Result<bool> {
    if !should_compact(messages) {
        return Ok(false);
    }

    compact_messages(client, messages).await
}

pub async fn force_compact(client: &LLMClient, messages: &mut Vec<Value>) -> Result<bool> {
    if messages.len() <= KEEP_RECENT_MESSAGES {
        return Ok(false);
    }

    compact_messages(client, messages).await
}

pub fn estimate_tokens(messages: &[Value]) -> usize {
    messages
        .iter()
        .map(|m| m["content"].as_str().unwrap_or_default().chars().count() / CHARS_PER_TOKEN)
        .sum()
}

fn should_compact(messages: &[Value]) -> bool {
    messages.len() > KEEP_RECENT_MESSAGES && estimate_tokens(messages) >= TOKEN_THRESHOLD
}

async fn compact_messages(client: &LLMClient, messages: &mut Vec<Value>) -> Result<bool> {
    if messages.len() <= KEEP_RECENT_MESSAGES {
        return Ok(false);
    }

    let split_at = messages.len().saturating_sub(KEEP_RECENT_MESSAGES);
    let to_compress = messages[..split_at].to_vec();
    let recent = messages[split_at..].to_vec();

    let summary = client.summarize_messages(&to_compress).await?;

    let mut compacted = vec![
        json!({
            "role": "system",
            "content": format!("[对话摘要]\n{}", summary)
        }),
        json!({
            "role": "system",
            "content": "[compact_boundary]"
        }),
    ];
    compacted.extend(recent);
    *messages = compacted;
    Ok(true)
}

pub fn summarize_for_prompt(messages: &[Value]) -> String {
    messages
        .iter()
        .rev()
        .take(SUMMARY_MESSAGE_LIMIT)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(format_message)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn format_message(message: &Value) -> String {
    let role = message["role"].as_str().unwrap_or("unknown");
    let content = message["content"].as_str().unwrap_or_default();

    if role == "tool" {
        format!(
            "tool {}:\n{}",
            message["tool_name"].as_str().unwrap_or("unknown"),
            truncate(content, 800)
        )
    } else {
        format!("{}:\n{}", role, truncate(content, 1200))
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max_chars {
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
    fn estimate_tokens_uses_content_length() {
        let messages = vec![json!({"role":"user","content":"a".repeat(40)})];
        assert_eq!(estimate_tokens(&messages), 10);
    }

    #[test]
    fn summarize_for_prompt_formats_roles() {
        let messages = vec![
            json!({"role":"user","content":"hello"}),
            json!({"role":"assistant","content":"world"}),
        ];
        let prompt = summarize_for_prompt(&messages);
        assert!(prompt.contains("user:"));
        assert!(prompt.contains("assistant:"));
    }
}
