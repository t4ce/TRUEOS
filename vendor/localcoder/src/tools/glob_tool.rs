/*!
 * GlobTool — S03
 *
 * Corresponds to: src/tools/GlobTool/GlobTool.ts
 *
 * Fast file pattern matching tool. Returns matching file paths sorted by
 * modification time (newest first), capped at 100 results.
 */

use crate::tools::Tool;
use anyhow::{Result, anyhow};
use glob::glob;
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::PathBuf;

/// Maximum number of results before truncation.
const MAX_RESULTS: usize = 100;

pub struct GlobTool;

impl Tool for GlobTool {
    fn name(&self) -> &str {
        "Glob"
    }

    fn description(&self) -> &str {
        "Fast file pattern matching tool. Supports glob patterns like \"**/*.js\" or \"src/**/*.ts\". \
         Returns matching file paths sorted by modification time. Use this to find files by name patterns."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The glob pattern to match files against (e.g. \"**/*.ts\", \"src/**/*.rs\")"
                },
                "path": {
                    "type": "string",
                    "description": "The directory to search in. If not specified, the current working directory is used."
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| anyhow!("Glob: missing required field 'pattern'"))?;

        let base_dir = match input["path"].as_str() {
            Some(p) if !p.is_empty() => {
                let path = PathBuf::from(p);
                if !path.exists() {
                    return Err(anyhow!("Glob: directory does not exist: {}", p));
                }
                if !path.is_dir() {
                    return Err(anyhow!("Glob: path is not a directory: {}", p));
                }
                path
            }
            _ => PathBuf::from(env::current_dir()?),
        };

        // Build full glob pattern: base_dir/pattern
        let full_pattern = if pattern.starts_with('/') {
            pattern.to_string()
        } else {
            base_dir
                .join(pattern)
                .to_str()
                .ok_or_else(|| anyhow!("Glob: invalid path encoding"))?
                .to_string()
        };

        let entries: Vec<PathBuf> = glob(&full_pattern)?
            .filter_map(|e| e.ok())
            .filter(|p| p.is_file())
            .collect();

        if entries.is_empty() {
            return Ok("No files found".to_string());
        }

        // Sort by modification time (newest first)
        let mut entries = entries;
        entries.sort_by(|a, b| {
            let mtime_a = fs::metadata(a).and_then(|m| m.modified()).ok();
            let mtime_b = fs::metadata(b).and_then(|m| m.modified()).ok();
            mtime_b.cmp(&mtime_a) // newest first
        });

        let truncated = entries.len() > MAX_RESULTS;
        entries.truncate(MAX_RESULTS);

        // Convert to relative paths where possible
        let cwd = env::current_dir().unwrap_or_default();
        let filenames: Vec<String> = entries
            .iter()
            .map(|p| {
                if let Ok(rel) = p.strip_prefix(&cwd) {
                    rel.to_str()
                        .unwrap_or(p.to_str().unwrap_or("?"))
                        .to_string()
                } else {
                    p.to_str().unwrap_or("?").to_string()
                }
            })
            .collect();

        let mut output = filenames.join("\n");
        if truncated {
            output.push_str(
                "\n(Results are truncated. Consider using a more specific path or pattern.)",
            );
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let mut f = fs::File::create(&path).unwrap();
        write!(f, "{}", content).unwrap();
        path
    }

    #[tokio::test]
    async fn glob_finds_matching_files() {
        let dir = TempDir::new().unwrap();
        create_file(&dir, "a.rs", "fn a() {}");
        create_file(&dir, "b.ts", "const b = 1;");
        create_file(&dir, "c.rs", "fn c() {}");

        let path_str = dir.path().to_str().unwrap().to_string();
        let result = GlobTool
            .execute(json!({"pattern": "**/*.rs", "path": path_str}))
            .await
            .unwrap();

        assert!(result.contains("a.rs"));
        assert!(result.contains("c.rs"));
        assert!(!result.contains("b.ts"));
    }

    #[tokio::test]
    async fn glob_no_matches_returns_not_found() {
        let dir = TempDir::new().unwrap();
        create_file(&dir, "a.txt", "hello");

        let path_str = dir.path().to_str().unwrap().to_string();
        let result = GlobTool
            .execute(json!({"pattern": "**/*.py", "path": path_str}))
            .await
            .unwrap();

        assert_eq!(result, "No files found");
    }

    #[tokio::test]
    async fn glob_errors_on_nonexistent_directory() {
        let result = GlobTool
            .execute(json!({"pattern": "*.rs", "path": "/tmp/__nonexistent_localcoder_glob__"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn glob_missing_pattern_errors() {
        let result = GlobTool.execute(json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn glob_truncates_at_max_results() {
        let dir = TempDir::new().unwrap();
        for i in 0..110 {
            create_file(&dir, &format!("f{:03}.txt", i), "x");
        }

        let path_str = dir.path().to_str().unwrap().to_string();
        let result = GlobTool
            .execute(json!({"pattern": "*.txt", "path": path_str}))
            .await
            .unwrap();

        assert!(result.contains("truncated"));
        let count = result.lines().filter(|l| !l.contains("truncated")).count();
        assert_eq!(count, MAX_RESULTS);
    }
}
