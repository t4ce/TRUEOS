/*!
 * Agent Loop Engine — S01
 *
 * Corresponds to: src/query.ts — the while loop that runs until stop_reason != "tool_use"
 *
 * Core flow:
 *   loop {
 *     response = api.call_with_tools(messages, tool_schemas)
 *     append assistant message to messages
 *     if stop_reason == "tool_use" {
 *         for each tool_use call → execute tool → append tool_result
 *         continue
 *     } else {
 *         return final text
 *     }
 *   }
 */

use anyhow::Result;
use colored::*;
use serde_json::{Value, json};

use crate::api::LLMClient;
use crate::tools::ToolRegistry;
use crate::types::ToolUseCall;

/// Run the agent loop until the model reaches a terminal stop reason.
///
/// `messages` is mutated in-place: the function appends assistant messages and
/// tool_result messages as the conversation progresses, so the caller's history
/// stays up-to-date after the call returns.
///
pub async fn run_agent_loop(
    client: &LLMClient,
    registry: &ToolRegistry,
    messages: &mut Vec<Value>,
) -> Result<String> {
    run_agent_loop_with_system_prompt(client, registry, messages, None).await
}

pub async fn run_agent_loop_with_system_prompt(
    client: &LLMClient,
    registry: &ToolRegistry,
    messages: &mut Vec<Value>,
    system_prompt: Option<&str>,
) -> Result<String> {
    let result = async {
        loop {
            // ── 1. Call the model ─────────────────────────────────────────
            let request_messages = build_request_messages(registry, messages, system_prompt);
            let tools = registry.get_schemas();
            let response = client.call_with_tools(&request_messages, &tools).await?;

            // ── 2. Append assistant message to conversation history ───────
            messages.push(build_assistant_message(&response.text, &response.tool_uses));

            // ── 3. Check stop reason ──────────────────────────────────────
            if response.stop_reason != "tool_use" || response.tool_uses.is_empty() {
                break Ok(response.text);
            }

            // ── 4. Execute tool calls and collect results ─────────────────
            println!();
            let mut tool_results: Vec<Value> = Vec::new();

            for call in prioritize_tool_calls(&response.tool_uses) {
                println!("{}", format!("▶ Tool: {}", call.name).cyan());

                let (content, is_error) =
                    match registry.execute(&call.name, call.arguments.clone()).await {
                        Ok(result) => (result, false),
                        Err(e) => {
                            eprintln!("{} {}", "  ✗ Tool error:".red(), e);
                            (e.to_string(), true)
                        }
                    };

                tool_results.push(json!({
                    "role": "tool",
                    "tool_name": call.name,
                    "content": content,
                    "is_error": is_error
                }));
            }

            // ── 5. Append tool results and loop ───────────────────────────
            messages.extend(tool_results);
        }
    }
    .await;

    registry.clear_active_skill();
    result
}

fn prioritize_tool_calls(calls: &[ToolUseCall]) -> Vec<ToolUseCall> {
    let mut ordered = calls.to_vec();
    ordered.sort_by_key(|call| tool_priority(&call.name));
    ordered
}

fn tool_priority(name: &str) -> u8 {
    match name {
        "EnterPlanMode" => 0,
        "ExitPlanMode" => 0,
        "skill_tool" => 0,
        _ => 1,
    }
}

fn build_request_messages(
    registry: &ToolRegistry,
    messages: &[Value],
    system_prompt: Option<&str>,
) -> Vec<Value> {
    let mut request_messages = Vec::new();

    let mut parts = Vec::new();
    if let Some(prompt) = system_prompt.filter(|prompt| !prompt.trim().is_empty()) {
        parts.push(prompt.trim().to_string());
    }
    if let Some(active_skill) = registry.active_skill_prompt() {
        if !active_skill.trim().is_empty() {
            parts.push(active_skill);
        }
    }
    if let Some(active_plan) = registry.active_plan_prompt() {
        if !active_plan.trim().is_empty() {
            parts.push(active_plan);
        }
    }

    if !parts.is_empty() {
        request_messages.push(json!({
            "role": "system",
            "content": parts.join("\n\n")
        }));
    }
    request_messages.extend(messages.iter().cloned());
    request_messages
}

/// Build the assistant JSON message from an Ollama response.
fn build_assistant_message(text: &str, tool_uses: &[ToolUseCall]) -> Value {
    let mut message = json!({
        "role": "assistant",
        "content": text
    });

    if !tool_uses.is_empty() {
        let tool_calls: Vec<Value> = tool_uses
            .iter()
            .map(|call| {
                json!({
                    "function": {
                        "name": call.name,
                        "arguments": call.arguments
                    }
                })
            })
            .collect();
        message["tool_calls"] = Value::Array(tool_calls);
    }

    message
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::PlanManager;
    use crate::skills::SkillManager;
    use crate::types::ToolUseCall;
    use tempfile::TempDir;

    #[test]
    fn build_assistant_message_text_only() {
        let msg = build_assistant_message("hello", &[]);
        assert_eq!(msg["role"], "assistant");
        assert_eq!(msg["content"], "hello");
        assert!(msg.get("tool_calls").is_none());
    }

    #[test]
    fn build_assistant_message_tool_only() {
        let calls = vec![ToolUseCall {
            name: "echo_tool".into(),
            arguments: json!({"text":"hi"}),
        }];
        let msg = build_assistant_message("", &calls);
        let tool_calls = msg["tool_calls"].as_array().unwrap();
        assert_eq!(msg["content"], "");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["function"]["name"], "echo_tool");
        assert_eq!(tool_calls[0]["function"]["arguments"]["text"], "hi");
    }

    #[test]
    fn build_assistant_message_text_and_tool() {
        let calls = vec![ToolUseCall {
            name: "echo_tool".into(),
            arguments: json!({"text":"world"}),
        }];
        let msg = build_assistant_message("I'll echo this:", &calls);
        let tool_calls = msg["tool_calls"].as_array().unwrap();
        assert_eq!(msg["content"], "I'll echo this:");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["function"]["name"], "echo_tool");
    }

    #[test]
    fn build_request_messages_prefixes_system_prompt() {
        let registry = ToolRegistry::new();
        let messages = vec![json!({"role":"user","content":"hello"})];
        let request = build_request_messages(&registry, &messages, Some("[持久记忆]\nfoo"));
        assert_eq!(request.len(), 2);
        assert_eq!(request[0]["role"], "system");
        assert_eq!(request[1]["role"], "user");
    }

    #[test]
    fn build_request_messages_appends_active_skill_prompt() {
        let cwd = TempDir::new().unwrap();
        let manager = SkillManager::new(cwd.path()).unwrap();
        manager.set_session_id(Some("s1"));
        manager
            .resolve_and_activate("simplify", "src/main.rs")
            .unwrap();

        let mut registry = ToolRegistry::new();
        registry.attach_skill_manager(manager);

        let messages = vec![json!({"role":"user","content":"hello"})];
        let request = build_request_messages(&registry, &messages, Some("[持久记忆]\nfoo"));
        let content = request[0]["content"].as_str().unwrap();
        assert!(content.contains("[持久记忆]"));
        assert!(content.contains("[技能 simplify]"));
    }

    #[test]
    fn build_request_messages_appends_plan_prompt() {
        let cwd = TempDir::new().unwrap();
        let manager = PlanManager::new(cwd.path()).unwrap();
        manager.enter_mode(None).unwrap();

        let mut registry = ToolRegistry::new();
        registry.attach_plan_manager(manager);

        let messages = vec![json!({"role":"user","content":"hello"})];
        let request = build_request_messages(&registry, &messages, None);
        let content = request[0]["content"].as_str().unwrap();
        assert!(content.contains("[计划状态]"));
        assert!(content.contains("mode: planning"));
    }

    #[test]
    fn prioritize_tool_calls_moves_control_tools_first() {
        let ordered = prioritize_tool_calls(&[
            ToolUseCall {
                name: "Edit".into(),
                arguments: json!({}),
            },
            ToolUseCall {
                name: "EnterPlanMode".into(),
                arguments: json!({}),
            },
            ToolUseCall {
                name: "skill_tool".into(),
                arguments: json!({}),
            },
        ]);

        assert_eq!(ordered[0].name, "EnterPlanMode");
        assert_eq!(ordered[1].name, "skill_tool");
        assert_eq!(ordered[2].name, "Edit");
    }
}
