/*!
 * Plan Mode Tools — S12
 *
 * EnterPlanMode / ExitPlanMode / TodoWrite
 */

use crate::plan::{PlanManager, TodoItem};
use crate::tools::Tool;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct EnterPlanModeTool {
    manager: PlanManager,
}

#[derive(Clone)]
pub struct ExitPlanModeTool {
    manager: PlanManager,
}

#[derive(Clone)]
pub struct TodoWriteTool {
    manager: PlanManager,
}

impl EnterPlanModeTool {
    pub fn new(manager: PlanManager) -> Self {
        Self { manager }
    }
}

impl ExitPlanModeTool {
    pub fn new(manager: PlanManager) -> Self {
        Self { manager }
    }
}

impl TodoWriteTool {
    pub fn new(manager: PlanManager) -> Self {
        Self { manager }
    }
}

impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        "EnterPlanMode"
    }

    fn description(&self) -> &str {
        "Enter plan mode before implementation. In plan mode only read-only tools plus planning tools are allowed."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "reason": {
                    "type": "string",
                    "description": "Optional short reason for entering plan mode"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        self.manager.enter_mode(input["reason"].as_str())
    }
}

impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "ExitPlanMode"
    }

    fn description(&self) -> &str {
        "Exit plan mode and restore normal tool access."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "reason": {
                    "type": "string",
                    "description": "Optional short reason for exiting plan mode"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        self.manager.exit_mode(input["reason"].as_str())
    }
}

impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "TodoWrite"
    }

    fn description(&self) -> &str {
        "Create or replace the current todo list for the task. Use to track pending, in_progress, and completed steps."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "Full todo list to persist in order",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "integer",
                                "description": "Optional stable todo id. If omitted, one is assigned automatically."
                            },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed"],
                                "description": "Current todo status"
                            },
                            "content": {
                                "type": "string",
                                "description": "Todo text"
                            },
                            "text": {
                                "type": "string",
                                "description": "Alias of content"
                            },
                            "step": {
                                "type": "string",
                                "description": "Alias of content"
                            }
                        },
                        "required": ["status"]
                    }
                }
            },
            "required": ["todos"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let todos_value = input
            .get("todos")
            .cloned()
            .ok_or_else(|| anyhow!("TodoWrite: missing required field 'todos'"))?;
        let todos: Vec<TodoItem> = serde_json::from_value(todos_value)
            .map_err(|err| anyhow!("TodoWrite: invalid todos payload: {}", err))?;
        self.manager.replace_todos(todos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn enter_plan_mode_updates_state() {
        let cwd = TempDir::new().unwrap();
        let tool = EnterPlanModeTool::new(PlanManager::new(cwd.path()).unwrap());
        let result = tool
            .execute(json!({"reason":"inspect first"}))
            .await
            .unwrap();
        assert!(result.contains("Entered plan mode"));
    }

    #[tokio::test]
    async fn todo_write_requires_todos() {
        let cwd = TempDir::new().unwrap();
        let tool = TodoWriteTool::new(PlanManager::new(cwd.path()).unwrap());
        let err = tool.execute(json!({})).await.unwrap_err();
        assert!(err.to_string().contains("todos"));
    }
}
