/*!
 * Git Integration — S09
 */

use anyhow::{Context, Result, anyhow};
use std::path::Path;
use std::process::Command;

pub fn ensure_git_repo(cwd: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .output()
        .context("failed to run git rev-parse")?;

    if !status.status.success() {
        return Err(anyhow!("not inside a git repository"));
    }

    Ok(())
}

pub fn get_staged_diff(cwd: &Path) -> Result<String> {
    run_git(cwd, &["diff", "--staged"])
}

pub fn get_working_diff(cwd: &Path) -> Result<String> {
    if has_head(cwd)? {
        return run_git(cwd, &["diff", "HEAD"]);
    }

    let diff = run_git(cwd, &["diff"])?;
    let status = run_git(cwd, &["status", "--short"])?;
    if status.trim().is_empty() {
        Ok(diff)
    } else if diff.trim().is_empty() {
        Ok(status)
    } else {
        Ok(format!("{}\n\n{}", diff, status))
    }
}

pub fn get_combined_diff(cwd: &Path) -> Result<String> {
    let staged = get_staged_diff(cwd)?;
    if !staged.trim().is_empty() {
        return Ok(staged);
    }
    get_working_diff(cwd)
}

pub fn has_staged_changes(cwd: &Path) -> Result<bool> {
    Ok(!get_staged_diff(cwd)?.trim().is_empty())
}

pub fn stage_all(cwd: &Path) -> Result<()> {
    run_git_status(cwd, &["add", "-A"])
}

pub fn commit(cwd: &Path, message: &str) -> Result<()> {
    run_git_status(cwd, &["commit", "-m", message])
}

fn has_head(cwd: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(cwd)
        .output()
        .context("failed to run git rev-parse --verify HEAD")?;
    Ok(output.status.success())
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("git {} failed: {}", args.join(" "), stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_git_status(cwd: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("git {} failed: {}", args.join(" "), stderr.trim()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn init_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        dir
    }

    #[test]
    fn combined_diff_returns_working_tree_changes() {
        let dir = init_repo();
        fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        stage_all(dir.path()).unwrap();
        commit(dir.path(), "feat: initial").unwrap();
        fs::write(dir.path().join("a.txt"), "hello\nworld\n").unwrap();
        let diff = get_combined_diff(dir.path()).unwrap();
        assert!(diff.contains("a.txt"));
    }

    #[test]
    fn stage_all_makes_changes_staged() {
        let dir = init_repo();
        fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        stage_all(dir.path()).unwrap();
        assert!(has_staged_changes(dir.path()).unwrap());
    }
}
