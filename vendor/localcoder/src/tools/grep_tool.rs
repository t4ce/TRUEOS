/*!
 * GrepTool — S03
 *
 * Corresponds to: src/tools/GrepTool/GrepTool.ts
 *
 * Content search tool built on the `ignore` crate (ripgrep's file walker)
 * and the `regex` crate. Supports three output modes:
 *   - files_with_matches (default): list filenames
 *   - content: show matching lines with line numbers and optional context
 *   - count: show match count per file
 *
 * Key features:
 *   - Respects .gitignore automatically (via `ignore` crate)
 *   - Context lines (-B, -A, -C)
 *   - Case-insensitive search (-i)
 *   - File type filter (type: "rust", "js", etc.)
 *   - Glob filter (e.g. "*.ts")
 *   - Head limit + offset for pagination
 *   - Multiline mode
 */

use crate::tools::Tool;
use anyhow::{Result, anyhow};
use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Default head_limit (matches Claude Code's default of 250).
const DEFAULT_HEAD_LIMIT: usize = 250;

/// VCS directories to always skip.
const SKIP_DIRS: &[&str] = &[".git", ".svn", ".hg", ".bzr"];

/// File type extensions mapping.
const TYPE_MAP: &[(&str, &[&str])] = &[
    ("rust", &["rs"]),
    ("js", &["js", "mjs", "cjs"]),
    ("ts", &["ts", "tsx"]),
    ("py", &["py"]),
    ("go", &["go"]),
    ("java", &["java"]),
    ("c", &["c", "h"]),
    ("cpp", &["cpp", "cc", "cxx", "hpp", "hh"]),
    ("ruby", &["rb"]),
    ("swift", &["swift"]),
];

pub struct GrepTool;

impl Tool for GrepTool {
    fn name(&self) -> &str {
        "Grep"
    }

    fn description(&self) -> &str {
        "Search file contents using regex patterns. Supports three output modes: \
         'files_with_matches' (list filenames), 'content' (show matching lines with context), \
         and 'count' (match counts per file). Respects .gitignore."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The regular expression pattern to search for in file contents"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory to search in. Defaults to current working directory."
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g. \"*.ts\", \"*.{rs,toml}\")"
                },
                "output_mode": {
                    "type": "string",
                    "enum": ["files_with_matches", "content", "count"],
                    "description": "Output mode (default: files_with_matches)"
                },
                "-i": {
                    "type": "boolean",
                    "description": "Case-insensitive search (default: false)"
                },
                "-C": {
                    "type": "integer",
                    "description": "Number of context lines before and after each match"
                },
                "-B": {
                    "type": "integer",
                    "description": "Number of lines before each match"
                },
                "-A": {
                    "type": "integer",
                    "description": "Number of lines after each match"
                },
                "type": {
                    "type": "string",
                    "description": "File type filter: rust, js, ts, py, go, java, c, cpp, ruby, swift"
                },
                "head_limit": {
                    "type": "integer",
                    "description": "Limit output to first N entries. Default 250. Pass 0 for unlimited."
                },
                "offset": {
                    "type": "integer",
                    "description": "Skip first N entries before applying head_limit."
                },
                "multiline": {
                    "type": "boolean",
                    "description": "Enable multiline mode where . matches newlines (default: false)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| anyhow!("Grep: missing required field 'pattern'"))?;

        let base_path = match input["path"].as_str() {
            Some(p) if !p.is_empty() => {
                let path = PathBuf::from(p);
                if !path.exists() {
                    return Err(anyhow!("Grep: path does not exist: {}", p));
                }
                path
            }
            _ => env::current_dir()?,
        };

        let output_mode = input["output_mode"]
            .as_str()
            .unwrap_or("files_with_matches");
        let case_insensitive = input["-i"].as_bool().unwrap_or(false);
        let context = input["-C"].as_u64().map(|n| n as usize);
        let before = input["-B"].as_u64().map(|n| n as usize);
        let after = input["-A"].as_u64().map(|n| n as usize);
        let file_type = input["type"].as_str();
        let glob_filter = input["glob"].as_str();
        let head_limit = match input["head_limit"].as_u64() {
            Some(0) => usize::MAX, // 0 means unlimited
            Some(n) => n as usize,
            None => DEFAULT_HEAD_LIMIT,
        };
        let offset = input["offset"].as_u64().unwrap_or(0) as usize;
        let multiline = input["multiline"].as_bool().unwrap_or(false);

        // Build regex
        let re = RegexBuilder::new(pattern)
            .case_insensitive(case_insensitive)
            .multi_line(!multiline)
            .dot_matches_new_line(multiline)
            .build()
            .map_err(|e| anyhow!("Grep: invalid regex pattern '{}': {}", pattern, e))?;

        // Determine file extensions for type filter
        let type_exts: Option<Vec<&str>> = file_type.and_then(|t| {
            TYPE_MAP
                .iter()
                .find(|(name, _)| *name == t)
                .map(|(_, exts)| exts.to_vec())
        });

        // Walk files
        let search_path = if base_path.is_file() {
            // Single file: search just that file
            vec![base_path.clone()]
        } else {
            walk_files(&base_path)
        };

        // Determine context sizes
        let ctx_before = context.or(before).unwrap_or(0);
        let ctx_after = context.or(after).unwrap_or(0);

        match output_mode {
            "content" => search_content(
                &search_path,
                &re,
                ctx_before,
                ctx_after,
                &type_exts,
                glob_filter,
                head_limit,
                offset,
            ),
            "count" => search_count(
                &search_path,
                &re,
                &type_exts,
                glob_filter,
                head_limit,
                offset,
            ),
            _ => search_files(
                &search_path,
                &re,
                &type_exts,
                glob_filter,
                head_limit,
                offset,
            ),
        }
    }
}

// ─── File walking ──────────────────────────────────────────────────────────

fn walk_files(base: &Path) -> Vec<PathBuf> {
    let mut builder = WalkBuilder::new(base);
    builder
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true);

    for dir in SKIP_DIRS {
        builder.add_ignore(dir);
    }

    builder
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map_or(false, |ft| ft.is_file()))
        .map(|e| e.into_path())
        .collect()
}

// ─── Output modes ──────────────────────────────────────────────────────────

fn search_files(
    files: &[PathBuf],
    re: &regex::Regex,
    type_exts: &Option<Vec<&str>>,
    glob_filter: Option<&str>,
    head_limit: usize,
    offset: usize,
) -> Result<String> {
    let mut matches: Vec<String> = Vec::new();

    for path in files {
        if !passes_filters(path, type_exts, glob_filter) {
            continue;
        }
        if let Ok(content) = fs::read_to_string(path) {
            if re.is_match(&content) {
                let display = relativize(path);
                matches.push(display);
            }
        }
    }

    // Sort by modification time (newest first)
    matches.sort_by(|a, b| {
        let mtime_a = PathBuf::from(a).metadata().and_then(|m| m.modified()).ok();
        let mtime_b = PathBuf::from(b).metadata().and_then(|m| m.modified()).ok();
        mtime_b.cmp(&mtime_a)
    });

    if matches.is_empty() {
        return Ok("No files found".to_string());
    }

    let slice = apply_pagination(&matches, offset, head_limit);
    Ok(slice.join("\n"))
}

fn search_content(
    files: &[PathBuf],
    re: &regex::Regex,
    ctx_before: usize,
    ctx_after: usize,
    type_exts: &Option<Vec<&str>>,
    glob_filter: Option<&str>,
    head_limit: usize,
    offset: usize,
) -> Result<String> {
    let mut lines: Vec<String> = Vec::new();

    for path in files {
        if !passes_filters(path, type_exts, glob_filter) {
            continue;
        }
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let file_lines: Vec<&str> = content.lines().collect();
        let display = relativize(path);

        for (i, line) in file_lines.iter().enumerate() {
            if re.is_match(line) {
                // Show context before
                let start = i.saturating_sub(ctx_before);
                // Show match + context after
                let end = (i + ctx_after + 1).min(file_lines.len());

                for j in start..end {
                    let marker = if j == i { ">" } else { " " };
                    lines.push(format!(
                        "{}{}:{}\t{}",
                        marker,
                        display,
                        j + 1,
                        file_lines[j]
                    ));
                }
            }
        }
    }

    if lines.is_empty() {
        return Ok("No matches found".to_string());
    }

    let slice = apply_pagination(&lines, offset, head_limit);
    Ok(slice.join("\n"))
}

fn search_count(
    files: &[PathBuf],
    re: &regex::Regex,
    type_exts: &Option<Vec<&str>>,
    glob_filter: Option<&str>,
    head_limit: usize,
    offset: usize,
) -> Result<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();

    for path in files {
        if !passes_filters(path, type_exts, glob_filter) {
            continue;
        }
        if let Ok(content) = fs::read_to_string(path) {
            let count = re.find_iter(&content).count();
            if count > 0 {
                counts.push((relativize(path), count));
            }
        }
    }

    if counts.is_empty() {
        return Ok("No matches found".to_string());
    }

    let total: usize = counts.iter().map(|(_, c)| c).sum();
    let slice = apply_pagination(
        &counts
            .iter()
            .map(|(f, c)| format!("{}:{}", f, c))
            .collect::<Vec<_>>(),
        offset,
        head_limit,
    );

    let mut output = slice.join("\n");
    output.push_str(&format!(
        "\nTotal: {} matches in {} files",
        total,
        counts.len()
    ));
    Ok(output)
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn passes_filters(path: &Path, type_exts: &Option<Vec<&str>>, glob_filter: Option<&str>) -> bool {
    // Type filter
    if let Some(exts) = type_exts {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !exts.contains(&ext) {
            return false;
        }
    }

    // Glob filter
    if let Some(glob_pat) = glob_filter {
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !glob_match(glob_pat, filename) {
            return false;
        }
    }

    true
}

/// Simple glob matching: supports comma/space-separated patterns and brace expansion.
fn glob_match(pattern: &str, name: &str) -> bool {
    // Split on spaces first, then on commas — but preserve brace patterns intact.
    let patterns: Vec<&str> = pattern
        .split_whitespace()
        .flat_map(|token| {
            if token.contains('{') {
                // Brace pattern: don't split on commas inside braces
                vec![token]
            } else {
                // Non-brace: split on commas
                token.split(',').filter(|s| !s.is_empty()).collect()
            }
        })
        .collect();

    for pat in patterns {
        if pat.contains('{') && pat.contains('}') {
            // Brace expansion: *.{rs,toml} → *.rs, *.toml
            if let Some(start) = pat.find('{') {
                if let Some(end) = pat.find('}') {
                    let prefix = &pat[..start];
                    let suffix = &pat[end + 1..];
                    let inner = &pat[start + 1..end];
                    for alt in inner.split(',') {
                        let expanded = format!("{}{}{}", prefix, alt, suffix);
                        if glob_match_single(&expanded, name) {
                            return true;
                        }
                    }
                }
            }
        } else if glob_match_single(pat, name) {
            return true;
        }
    }

    false
}

/// Match a single glob pattern against a name.
fn glob_match_single(pattern: &str, name: &str) -> bool {
    let pat = pattern.trim();
    if pat.starts_with("*.") {
        // Simple extension match: *.rs → name ends with .rs
        name.ends_with(&pat[1..])
    } else if pat == "*" {
        true
    } else {
        name == pat
    }
}

/// Convert absolute path to relative if possible.
fn relativize(path: &Path) -> String {
    let cwd = env::current_dir().unwrap_or_default();
    if let Ok(rel) = path.strip_prefix(&cwd) {
        rel.to_str()
            .unwrap_or(path.to_str().unwrap_or("?"))
            .to_string()
    } else {
        path.to_str().unwrap_or("?").to_string()
    }
}

/// Apply offset + head_limit pagination.
fn apply_pagination(items: &[String], offset: usize, limit: usize) -> Vec<String> {
    items.iter().skip(offset).take(limit).cloned().collect()
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
    async fn grep_files_with_matches() {
        let dir = TempDir::new().unwrap();
        create_file(&dir, "a.rs", "fn main() {}");
        create_file(&dir, "b.rs", "fn helper() {}");
        create_file(&dir, "c.txt", "no code here");

        let path_str = dir.path().to_str().unwrap().to_string();
        let result = GrepTool
            .execute(json!({
                "pattern": "fn ",
                "path": path_str,
                "output_mode": "files_with_matches"
            }))
            .await
            .unwrap();

        assert!(result.contains("a.rs"));
        assert!(result.contains("b.rs"));
        assert!(!result.contains("c.txt"));
    }

    #[tokio::test]
    async fn grep_content_mode() {
        let dir = TempDir::new().unwrap();
        create_file(&dir, "a.rs", "fn main() {\n    println!(\"hi\");\n}");

        let path_str = dir.path().to_str().unwrap().to_string();
        let result = GrepTool
            .execute(json!({
                "pattern": "fn main",
                "path": path_str,
                "output_mode": "content"
            }))
            .await
            .unwrap();

        assert!(result.contains("fn main"));
        assert!(result.contains(">")); // match marker
    }

    #[tokio::test]
    async fn grep_count_mode() {
        let dir = TempDir::new().unwrap();
        create_file(&dir, "a.rs", "fn a() {}\nfn b() {}");
        create_file(&dir, "b.rs", "fn c() {}");

        let path_str = dir.path().to_str().unwrap().to_string();
        let result = GrepTool
            .execute(json!({
                "pattern": "fn ",
                "path": path_str,
                "output_mode": "count"
            }))
            .await
            .unwrap();

        assert!(result.contains("Total: 3 matches"));
    }

    #[tokio::test]
    async fn grep_case_insensitive() {
        let dir = TempDir::new().unwrap();
        create_file(&dir, "a.txt", "Hello World");

        let path_str = dir.path().to_str().unwrap().to_string();
        let result = GrepTool
            .execute(json!({
                "pattern": "hello",
                "path": path_str,
                "-i": true,
                "output_mode": "files_with_matches"
            }))
            .await
            .unwrap();

        assert!(result.contains("a.txt"));
    }

    #[tokio::test]
    async fn grep_type_filter() {
        let dir = TempDir::new().unwrap();
        create_file(&dir, "a.rs", "fn main() {}");
        create_file(&dir, "b.ts", "function main() {}");

        let path_str = dir.path().to_str().unwrap().to_string();
        let result = GrepTool
            .execute(json!({
                "pattern": "main",
                "path": path_str,
                "type": "rust",
                "output_mode": "files_with_matches"
            }))
            .await
            .unwrap();

        assert!(result.contains("a.rs"));
        assert!(!result.contains("b.ts"));
    }

    #[tokio::test]
    async fn grep_no_matches() {
        let dir = TempDir::new().unwrap();
        create_file(&dir, "a.txt", "hello world");

        let path_str = dir.path().to_str().unwrap().to_string();
        let result = GrepTool
            .execute(json!({
                "pattern": "xyz_nonexistent",
                "path": path_str,
                "output_mode": "files_with_matches"
            }))
            .await
            .unwrap();

        assert_eq!(result, "No files found");
    }

    #[tokio::test]
    async fn grep_invalid_regex_errors() {
        let result = GrepTool.execute(json!({"pattern": "[invalid"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn grep_missing_pattern_errors() {
        let result = GrepTool.execute(json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn grep_context_lines() {
        let dir = TempDir::new().unwrap();
        create_file(&dir, "a.txt", "line1\nline2\nline3\nline4\nline5");

        let path_str = dir.path().to_str().unwrap().to_string();
        let result = GrepTool
            .execute(json!({
                "pattern": "line3",
                "path": path_str,
                "output_mode": "content",
                "-C": 1
            }))
            .await
            .unwrap();

        // Should show line2, line3, line4
        assert!(result.contains("line2"));
        assert!(result.contains("line3"));
        assert!(result.contains("line4"));
        assert!(!result.contains("line1"));
        assert!(!result.contains("line5"));
    }

    #[test]
    fn glob_match_extension() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", "main.ts"));
    }

    #[test]
    fn glob_match_brace() {
        assert!(glob_match("*.{rs,toml}", "Cargo.toml"));
        assert!(glob_match("*.{rs,toml}", "main.rs"));
        assert!(!glob_match("*.{rs,toml}", "main.ts"));
    }

    #[test]
    fn glob_match_comma_separated() {
        assert!(glob_match("*.rs, *.ts", "main.rs"));
        assert!(glob_match("*.rs, *.ts", "main.ts"));
        assert!(!glob_match("*.rs, *.ts", "main.py"));
    }
}
