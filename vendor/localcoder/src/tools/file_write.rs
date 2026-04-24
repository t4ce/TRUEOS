/*!
 * WriteTool — S02
 *
 * Corresponds to: src/tools/FileWriteTool/FileWriteTool.ts
 *
 * Creates or completely overwrites a file with new content.
 * Creates parent directories if they don't exist.
 */

use crate::rt::fs as rt_fs;
use crate::tools::Tool;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::path::Path;

pub struct WriteTool;

impl Tool for WriteTool {
    fn name(&self) -> &str {
        "Write"
    }

    fn description(&self) -> &str {
        "Writes content to a file, creating it if it doesn't exist or overwriting if it does. \
         Creates parent directories as needed. \
         Prefer Edit for modifying existing files — use Write only to create new files or for complete rewrites."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to write (must be absolute, not relative)"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file"
                }
            },
            "required": ["file_path", "content"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let file_path = input["file_path"]
            .as_str()
            .ok_or_else(|| anyhow!("Write: missing required field 'file_path'"))?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| anyhow!("Write: missing required field 'content'"))?;

        let path = Path::new(file_path);

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            if !rt_fs::exists(parent) {
                rt_fs::create_dir_all(parent).map_err(|e| {
                    anyhow!("Write: cannot create directories for '{}': {}", file_path, e)
                })?;
            }
        }

        let existed = rt_fs::exists(path);

        rt_fs::write(path, content)
            .map_err(|e| anyhow!("Write: cannot write '{}': {}", file_path, e))?;

        if existed {
            Ok(format!("Successfully overwrote {}", file_path))
        } else {
            Ok(format!("Successfully created {}", file_path))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn write_creates_new_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("new_file.txt");
        let path_str = path.to_str().unwrap().to_string();

        let result = WriteTool
            .execute(json!({"file_path": path_str, "content": "hello\n"}))
            .await
            .unwrap();

        assert!(result.contains("created"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello\n");
    }

    #[tokio::test]
    async fn write_overwrites_existing_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("existing.txt");
        fs::write(&path, "old content").unwrap();
        let path_str = path.to_str().unwrap().to_string();

        let result = WriteTool
            .execute(json!({"file_path": path_str, "content": "new content"}))
            .await
            .unwrap();

        assert!(result.contains("overwrote"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "new content");
    }

    #[tokio::test]
    async fn write_creates_parent_directories() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a/b/c/file.txt");
        let path_str = path.to_str().unwrap().to_string();

        WriteTool
            .execute(json!({"file_path": path_str, "content": "nested"}))
            .await
            .unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "nested");
    }

    #[tokio::test]
    async fn write_missing_file_path_errors() {
        let result = WriteTool.execute(json!({"content": "x"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn write_missing_content_errors() {
        let result = WriteTool.execute(json!({"file_path": "/tmp/x"})).await;
        assert!(result.is_err());
    }
}
