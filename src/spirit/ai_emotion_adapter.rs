//! Model-facing adapter for Spirit's high-level emotion API.
//!
//! The model expresses one abstract emotional idea. This adapter validates
//! that idea, removes the control span from user-facing text, and hands the
//! canonical theme to [`super::lilly_protocol`]. Spirit continues to own clip
//! selection, variants, timing, color, and presentation.

extern crate alloc;

use alloc::string::String;

use super::lilly_protocol::LillyEmotion;

const TOOL_CALL_START: &str = "<|tool_call_start|>";
const TOOL_CALL_END: &str = "<|tool_call_end|>";
const TOOL_NAME: &str = "play_emotion";

/// Single compile-time gate for the model-facing emotion capability.
///
/// When false, Lumen receives no emotion tool schema and its replies never
/// enter this adapter.
pub(crate) const LUMEN_AI_EMOTION_ENABLED: bool = false;

/// Compact first-turn instruction for the pinned LFM2.5 tool-call format.
///
/// The JSON contract has one semantic field. Concrete Lilly presentation
/// details intentionally remain outside the model context.
pub(crate) const LUMEN_SYSTEM_PROMPT: &str = concat!(
    "You are Lilly, a concise helpful assistant. ",
    "List of tools: [{\"name\":\"play_emotion\",\"description\":\"Play one fitting emotional ",
    "idea through Lilly when it adds meaning.\",\"parameters\":{\"type\":\"object\",",
    "\"properties\":{\"idea\":{\"type\":\"string\",\"enum\":[\"anger\",\"disgust\",",
    "\"fear\",\"joy\",\"sadness\",\"surprise\"]}},\"required\":[\"idea\"],",
    "\"additionalProperties\":false}}]. ",
    "Use at most one tool call per reply and continue with the natural-language answer."
);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AiEmotionAdapterError {
    MalformedCall,
    UnknownIdea,
}

/// Consume an optional model-emitted emotion call and return display-safe text.
///
/// This is deliberately fire-and-forget. Playing an emotion does not need a
/// second inference pass or a verbose tool-result message in the context.
pub(crate) fn adapt_reply(raw: &str) -> String {
    let Some(start) = raw.find(TOOL_CALL_START) else {
        if let Some(end) = raw.find(TOOL_CALL_END) {
            return rejected_reply(
                raw,
                end,
                end + TOOL_CALL_END.len(),
                AiEmotionAdapterError::MalformedCall,
            );
        }
        return String::from(raw.trim());
    };
    let payload_start = start + TOOL_CALL_START.len();
    let Some(relative_end) = raw[payload_start..].find(TOOL_CALL_END) else {
        return rejected_reply(raw, start, raw.len(), AiEmotionAdapterError::MalformedCall);
    };
    let payload_end = payload_start + relative_end;
    let span_end = payload_end + TOOL_CALL_END.len();
    if raw[span_end..].contains(TOOL_CALL_START) {
        return rejected_reply(raw, start, raw.len(), AiEmotionAdapterError::MalformedCall);
    }

    let Some(emotion) = parse_call(&raw[payload_start..payload_end]) else {
        return rejected_reply(raw, start, span_end, AiEmotionAdapterError::UnknownIdea);
    };
    let text = remove_control_span(raw, start, span_end);
    match super::lilly_protocol::enqueue_emotion_words(&[emotion.as_word()]) {
        Ok(ring_len) => {
            crate::log_info!(
                target: "gfx";
                "trueos-spirit: ai emotion accepted idea={} ring_len={} contract=idea-only source=lumen\n",
                emotion.as_word(),
                ring_len,
            );
            text
        }
        Err(error) => {
            crate::log_warn!(
                target: "gfx";
                "trueos-spirit: ai emotion unavailable idea={} error={:?} action=keep-text\n",
                emotion.as_word(),
                error,
            );
            text
        }
    }
}

fn rejected_reply(
    raw: &str,
    span_start: usize,
    span_end: usize,
    error: AiEmotionAdapterError,
) -> String {
    crate::log_warn!(
        target: "gfx";
        "trueos-spirit: ai emotion rejected error={:?} action=strip-control+keep-text\n",
        error,
    );
    remove_control_span(raw, span_start, span_end)
}

fn parse_call(payload: &str) -> Option<LillyEmotion> {
    parse_pythonic_call(payload).or_else(|| parse_json_call(payload))
}

fn parse_pythonic_call(payload: &str) -> Option<LillyEmotion> {
    let payload = payload.trim();
    let payload = payload
        .strip_prefix('[')
        .and_then(|payload| payload.strip_suffix(']'))
        .unwrap_or(payload)
        .trim();
    let arguments = payload
        .strip_prefix(TOOL_NAME)?
        .trim()
        .strip_prefix('(')?
        .strip_suffix(')')?
        .trim();
    let value = arguments
        .strip_prefix("idea")?
        .trim()
        .strip_prefix('=')?
        .trim();
    parse_quoted_idea(value)
}

fn parse_json_call(payload: &str) -> Option<LillyEmotion> {
    let value: serde_json::Value = serde_json::from_str(payload.trim()).ok()?;
    let object = value.as_object()?;
    if object.len() == 1 {
        return LillyEmotion::from_word(object.get("idea")?.as_str()?);
    }
    if object.len() != 2 || object.get("name")?.as_str()? != TOOL_NAME {
        return None;
    }
    let arguments = object
        .get("arguments")
        .or_else(|| object.get("parameters"))?
        .as_object()?;
    if arguments.len() != 1 {
        return None;
    }
    LillyEmotion::from_word(arguments.get("idea")?.as_str()?)
}

fn parse_quoted_idea(value: &str) -> Option<LillyEmotion> {
    let quote = value.as_bytes().first().copied()?;
    if !matches!(quote, b'\'' | b'"') || value.as_bytes().last().copied() != Some(quote) {
        return None;
    }
    let idea = value.get(1..value.len().checked_sub(1)?)?;
    if idea.chars().any(|ch| matches!(ch, '\\' | '\'' | '"')) {
        return None;
    }
    LillyEmotion::from_word(idea)
}

fn remove_control_span(raw: &str, start: usize, end: usize) -> String {
    let before = raw.get(..start).unwrap_or_default().trim_end();
    let after = raw.get(end..).unwrap_or_default().trim_start();
    let mut text =
        String::with_capacity(before.len().saturating_add(after.len()).saturating_add(1));
    text.push_str(before);
    if !before.is_empty() && !after.is_empty() {
        text.push(' ');
    }
    text.push_str(after);
    text
}

#[cfg(test)]
mod tests {
    use super::{LillyEmotion, parse_call, remove_control_span};

    #[test]
    fn parses_liquid_pythonic_and_minimal_json_calls() {
        assert_eq!(parse_call("[play_emotion(idea=\"joy\")]"), Some(LillyEmotion::Joy));
        assert_eq!(parse_call("{\"idea\":\"surprise\"}"), Some(LillyEmotion::Surprise));
        assert_eq!(
            parse_call("{\"name\":\"play_emotion\",\"arguments\":{\"idea\":\"sadness\"}}"),
            Some(LillyEmotion::Sadness)
        );
    }

    #[test]
    fn rejects_extra_or_unknown_control() {
        assert_eq!(parse_call("play_emotion(idea=\"pride\")"), None);
        assert_eq!(parse_call("{\"idea\":\"joy\",\"duration\":5}"), None);
        assert_eq!(parse_call("other_tool(idea=\"joy\")"), None);
    }

    #[test]
    fn removes_control_without_joining_visible_words() {
        let raw = "Hello <control> there.";
        assert_eq!(remove_control_span(raw, 6, 15), "Hello there.");
    }
}
