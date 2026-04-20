/*!
 * SkillTool — S13
 *
 * Loads a reusable skill from SKILL.md and injects its prompt into the current
 * agent run.
 */

use crate::skills::SkillManager;
use crate::tools::Tool;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct SkillTool {
    manager: SkillManager,
}

impl SkillTool {
    pub fn new(manager: SkillManager) -> Self {
        Self { manager }
    }
}

impl Tool for SkillTool {
    fn name(&self) -> &str {
        "skill_tool"
    }

    fn description(&self) -> &str {
        "Load a reusable skill from SKILL.md and apply it to the current task. Use when the user references a named skill or when a specialized workflow clearly matches the request."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "Skill name, for example `simplify` or `review-diff`"
                },
                "args": {
                    "type": "string",
                    "description": "Optional free-form arguments passed to the skill body"
                }
            },
            "required": ["skill"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let skill_name = input["skill"]
            .as_str()
            .ok_or_else(|| anyhow!("skill_tool: missing required field 'skill'"))?;
        let args = input["args"].as_str().unwrap_or_default();

        let resolved = self.manager.resolve_and_activate(skill_name, args)?;
        Ok(resolved.prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn skill_tool_requires_skill_name() {
        let cwd = TempDir::new().unwrap();
        let tool = SkillTool::new(SkillManager::new(cwd.path()).unwrap());
        let err = tool.execute(json!({})).await.unwrap_err();
        assert!(err.to_string().contains("skill"));
    }
}
