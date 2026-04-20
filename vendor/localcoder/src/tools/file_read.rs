/*!
 * ReadTool — S02
 *
 * Corresponds to: src/tools/FileReadTool/FileReadTool.ts
 *
 * Reads a file and returns its contents with line numbers (cat -n format).
 * Supports optional offset (1-based start line) and limit (max lines).
 */

use crate::rt::fs as rt_fs;
use crate::tools::Tool;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::path::Path;

pub struct ReadTool;

impl Tool for ReadTool {
    fn name(&self) -> &str {
        "Read"
    }

    fn description(&self) -> &str {
        "Reads a file from the local filesystem. Returns content with line numbers in cat -n format. \
         Use offset and limit to read specific portions of large files."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "The line number to start reading from (1-based). Only provide if the file is too large to read at once."
                },
                "limit": {
                    "type": "integer",
                    "description": "The number of lines to read. Only provide if the file is too large to read at once."
                }
            },
            "required": ["file_path"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let file_path = input["file_path"]
            .as_str()
            .ok_or_else(|| anyhow!("Read: missing required field 'file_path'"))?;

        let path = Path::new(file_path);

        if !rt_fs::exists(path) {
            return Err(anyhow!("Read: file not found: {}", file_path));
        }

        if !rt_fs::is_file(path) {
            return Err(anyhow!("Read: path is not a file: {}", file_path));
        }

        let content = rt_fs::read_to_string(path)
            .map_err(|e| anyhow!("Read: cannot read '{}': {}", file_path, e))?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        // offset is 1-based; default to line 1
        let offset = match input["offset"].as_u64() {
            Some(o) if o >= 1 => (o as usize).saturating_sub(1), // convert to 0-based index
            Some(_) => 0,
            None => 0,
        };

        let limit = input["limit"].as_u64().map(|l| l as usize);

        if offset >= total_lines && total_lines > 0 {
            return Err(anyhow!(
                "Read: offset {} exceeds file length ({} lines)",
                offset + 1,
                total_lines
            ));
        }

        let slice = &lines[offset..];
        let slice = match limit {
            Some(n) => &slice[..n.min(slice.len())],
            None => slice,
        };

        // Format with line numbers (cat -n style, 1-based)
        let output = slice
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>6}\t{}", offset + i + 1, line))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(output)
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
    async fn read_full_file() {
        let f = write_temp("hello\nworld\n");
        let path = f.path().to_str().unwrap().to_string();
        let result = ReadTool.execute(json!({"file_path": path})).await.unwrap();
        assert!(result.contains("hello"));
        assert!(result.contains("world"));
        assert!(result.contains("     1\t"));
        assert!(result.contains("     2\t"));
    }

    #[tokio::test]
    async fn read_with_offset_and_limit() {
        let f = write_temp("a\nb\nc\nd\ne\n");
        let path = f.path().to_str().unwrap().to_string();
        // offset=2, limit=2 → lines 2 and 3
        let result = ReadTool
            .execute(json!({"file_path": path, "offset": 2, "limit": 2}))
            .await
            .unwrap();
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("b"));
        assert!(lines[1].contains("c"));
        // line numbers should be 2 and 3
        assert!(lines[0].starts_with("     2\t"));
        assert!(lines[1].starts_with("     3\t"));
    }

    #[tokio::test]
    async fn read_missing_file_errors() {
        let result = ReadTool
            .execute(json!({"file_path": "/tmp/__nonexistent_localcoder_test__.txt"}))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn read_missing_file_path_errors() {
        let result = ReadTool.execute(json!({})).await;
        assert!(result.is_err());
    }
}
