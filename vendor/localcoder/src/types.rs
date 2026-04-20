/*!
 * Data Type Definitions
 *
 * Corresponds to: src/types/message.ts
 */

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Message structure
///
/// Corresponds to: src/types/message.ts:38-40
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String, // "user" or "assistant"
    pub content: String,
}

impl Message {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new("assistant", content)
    }
}

// ─── Tool calling types ────────────────────────────────────────────────────

/// A single tool call extracted from the Ollama response.
#[derive(Debug, Clone)]
pub struct ToolUseCall {
    pub name: String,
    pub arguments: Value,
}

/// Structured response from call_with_tools — returned by the agent loop.
#[derive(Debug)]
pub struct AgentResponse {
    /// Text content (may be empty when only tool_use blocks are present)
    pub text: String,
    /// "end_turn" or "tool_use"
    pub stop_reason: String,
    /// Tool calls that must be executed before the next API round-trip
    pub tool_uses: Vec<ToolUseCall>,
}

/// Native Ollama chat response.
#[derive(Debug, Deserialize)]
pub struct OllamaChatResponse {
    pub message: OllamaMessage,
}

#[derive(Debug, Deserialize)]
pub struct OllamaMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Debug, Deserialize)]
pub struct OllamaToolCall {
    pub function: OllamaFunctionCall,
}

#[derive(Debug, Deserialize)]
pub struct OllamaFunctionCall {
    pub name: String,
    pub arguments: Value,
}

/// Conversation history manager
pub struct ConversationHistory {
    messages: Vec<Message>,
}

impl ConversationHistory {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    pub fn add_user_message(&mut self, content: impl Into<String>) {
        self.messages.push(Message::user(content));
    }

    pub fn add_assistant_message(&mut self, content: impl Into<String>) {
        self.messages.push(Message::assistant(content));
    }

    pub fn get_messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.messages)
    }
}

impl Default for ConversationHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Message ---

    #[test]
    fn message_new_stores_role_and_content() {
        let msg = Message::new("user", "hello");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "hello");
    }

    #[test]
    fn message_user_sets_role() {
        let msg = Message::user("hi");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "hi");
    }

    #[test]
    fn message_assistant_sets_role() {
        let msg = Message::assistant("response");
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, "response");
    }

    // --- ConversationHistory ---

    #[test]
    fn new_history_is_empty() {
        let h = ConversationHistory::new();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert_eq!(h.get_messages().len(), 0);
    }

    #[test]
    fn default_history_is_empty() {
        let h = ConversationHistory::default();
        assert!(h.is_empty());
    }

    #[test]
    fn add_user_message_increments_len() {
        let mut h = ConversationHistory::new();
        h.add_user_message("hello");
        assert_eq!(h.len(), 1);
        assert!(!h.is_empty());
        assert_eq!(h.get_messages()[0].role, "user");
        assert_eq!(h.get_messages()[0].content, "hello");
    }

    #[test]
    fn add_assistant_message_increments_len() {
        let mut h = ConversationHistory::new();
        h.add_assistant_message("hi there");
        assert_eq!(h.len(), 1);
        assert_eq!(h.get_messages()[0].role, "assistant");
        assert_eq!(h.get_messages()[0].content, "hi there");
    }

    #[test]
    fn messages_stored_in_order() {
        let mut h = ConversationHistory::new();
        h.add_user_message("first");
        h.add_assistant_message("second");
        h.add_user_message("third");
        let msgs = h.get_messages();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[2].role, "user");
        assert_eq!(msgs[2].content, "third");
    }

    #[test]
    fn clear_removes_all_messages() {
        let mut h = ConversationHistory::new();
        h.add_user_message("a");
        h.add_assistant_message("b");
        h.clear();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn to_json_empty_history_returns_array() {
        let h = ConversationHistory::new();
        let json = h.to_json().unwrap();
        assert_eq!(json.trim(), "[]");
    }

    #[test]
    fn to_json_contains_role_and_content() {
        let mut h = ConversationHistory::new();
        h.add_user_message("ping");
        let json = h.to_json().unwrap();
        assert!(json.contains("\"role\""));
        assert!(json.contains("\"user\""));
        assert!(json.contains("\"content\""));
        assert!(json.contains("\"ping\""));
    }

    #[test]
    fn to_json_roundtrips() {
        let mut h = ConversationHistory::new();
        h.add_user_message("hello");
        h.add_assistant_message("world");
        let json = h.to_json().unwrap();
        let parsed: Vec<Message> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].role, "user");
        assert_eq!(parsed[1].role, "assistant");
    }
}
