/*!
 * EditTool — S02
 *
 * Corresponds to: src/tools/FileEditTool/FileEditTool.ts
 *
 * Performs exact string replacement in a file.
 * Fails if the old_string is not found, or if there are multiple matches
 * and replace_all is false (forces the caller to be explicit).
 */

use crate::rt::fs as rt_fs;
use crate::tools::Tool;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::path::Path;

pub struct EditTool;

impl Tool for EditTool {
    fn name(&self) -> &str {
        "Edit"
    }

    fn description(&self) -> &str {
        "Performs exact string replacements in files. \
         Fails if old_string is not found, or if multiple matches exist and replace_all is false. \
         Use replace_all: true to replace every occurrence."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to modify"
                },
                "old_string": {
                    "type": "string",
                    "description": "The text to replace (must be unique in the file, or use replace_all)"
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement text"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences of old_string (default false)"
                }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let file_path = input["file_path"]
            .as_str()
            .ok_or_else(|| anyhow!("Edit: missing required field 'file_path'"))?;
        let old_string = input["old_string"]
            .as_str()
            .ok_or_else(|| anyhow!("Edit: missing required field 'old_string'"))?;
        let new_string = input["new_string"]
            .as_str()
            .ok_or_else(|| anyhow!("Edit: missing required field 'new_string'"))?;
        let replace_all = input["replace_all"].as_bool().unwrap_or(false);

        if old_string == new_string {
            return Err(anyhow!(
                "Edit: no changes to make: old_string and new_string are exactly the same."
            ));
        }

        let path = Path::new(file_path);
        if !rt_fs::exists(path) {
            return Err(anyhow!("Edit: file not found: {}", file_path));
        }

        let content = rt_fs::read_to_string(path)
            .map_err(|e| anyhow!("Edit: cannot read '{}': {}", file_path, e))?;

        let matches = content.matches(old_string).count();
        if matches == 0 {
            return Err(anyhow!(
                "Edit: string to replace not found in file.\nString: {}",
                old_string
            ));
        }
        if matches > 1 && !replace_all {
            return Err(anyhow!(
                "Edit: found {} matches of the string to replace, but replace_all is false. \
                 To replace all occurrences, set replace_all to true. \
                 To replace only one occurrence, provide more context to uniquely identify the instance.\nString: {}",
                matches,
                old_string
            ));
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        rt_fs::write(path, &new_content)
            .map_err(|e| anyhow!("Edit: cannot write '{}': {}", file_path, e))?;

        Ok(format!(
            "Successfully edited {} ({} replacement{})",
            file_path,
            matches.min(if replace_all { matches } else { 1 }),
            if matches == 1 { "" } else { "s" }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", content).unwrap();
        f
    }

    #[tokio::test]
    async fn edit_replaces_single_occurrence() {
        let f = write_temp("hello world\n");
        let path = f.path().to_str().unwrap().to_string();
        EditTool
            .execute(json!({
                "file_path": path,
                "old_string": "world",
                "new_string": "rust"
            }))
            .await
            .unwrap();
        let result = fs::read_to_string(f.path()).unwrap();
        assert_eq!(result, "hello rust\n");
    }

    #[tokio::test]
    async fn edit_errors_on_not_found() {
        let f = write_temp("hello world\n");
        let path = f.path().to_str().unwrap().to_string();
        let result = EditTool
            .execute(json!({
                "file_path": path,
                "old_string": "missing",
                "new_string": "x"
            }))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn edit_errors_on_multiple_matches_without_replace_all() {
        let f = write_temp("foo foo foo\n");
        let path = f.path().to_str().unwrap().to_string();
        let result = EditTool
            .execute(json!({
                "file_path": path,
                "old_string": "foo",
                "new_string": "bar"
            }))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("replace_all"));
    }

    #[tokio::test]
    async fn edit_replace_all() {
        let f = write_temp("foo foo foo\n");
        let path = f.path().to_str().unwrap().to_string();
        EditTool
            .execute(json!({
                "file_path": path,
                "old_string": "foo",
                "new_string": "bar",
                "replace_all": true
            }))
            .await
            .unwrap();
        let result = fs::read_to_string(f.path()).unwrap();
        assert_eq!(result, "bar bar bar\n");
    }

    #[tokio::test]
    async fn edit_errors_if_same_string() {
        let f = write_temp("hello\n");
        let path = f.path().to_str().unwrap().to_string();
        let result = EditTool
            .execute(json!({
                "file_path": path,
                "old_string": "hello",
                "new_string": "hello"
            }))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("same"));
    }

    #[tokio::test]
    async fn edit_missing_file_path_errors() {
        let result = EditTool.execute(json!({})).await;
        assert!(result.is_err());
    }
}
