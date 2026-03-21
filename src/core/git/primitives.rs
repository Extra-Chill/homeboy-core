use std::path::Path;
use std::process::Command;

use crate::engine::command;
use crate::error::{Error, Result};

/// Clone a git repository to a target directory.
pub fn clone_repo(url: &str, target_dir: &Path) -> Result<()> {
    command::run(
        "git",
        &["clone", url, &target_dir.to_string_lossy()],
        "git clone",
    )
    .map_err(|e| Error::git_command_failed(e.to_string()))?;
    Ok(())
}

/// Pull latest changes in a git repository.
pub fn pull_repo(repo_dir: &Path) -> Result<()> {
    command::run_in(&repo_dir.to_string_lossy(), "git", &["pull"], "git pull")
        .map_err(|e| Error::git_command_failed(e.to_string()))?;
    Ok(())
}

/// Check if a git working directory has no uncommitted changes.
///
/// Uses direct Command execution to properly handle empty output (clean repo).
/// `run_in_optional` returns None for empty stdout, which would incorrectly
/// indicate a dirty repo when used with `.unwrap_or(false)`.
pub fn is_workdir_clean(path: &Path) -> bool {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output();

    match output {
        Ok(o) if o.status.success() => o.stdout.is_empty(),
        _ => false, // Command failed = assume not clean (conservative)
    }
}

/// List all git-tracked markdown files in a directory.
/// Uses `git ls-files` to respect .gitignore and only include tracked/staged files.
/// Returns relative paths from the repository root.
pub(crate) fn list_tracked_markdown_files(path: &Path) -> Result<Vec<String>> {
    let stdout = command::run_in(
        &path.to_string_lossy(),
        "git",
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "*.md",
        ],
        "git ls-files",
    )
    .map_err(|e| Error::git_command_failed(e.to_string()))?;

    Ok(stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

pub(crate) fn is_git_repo(path: &str) -> bool {
    command::succeeded_in(path, "git", &["rev-parse", "--git-dir"])
}

/// Get the git repository root directory from any path within the repo.
pub fn get_git_root(path: &str) -> Result<String> {
    command::run_in(path, "git", &["rev-parse", "--show-toplevel"], "git root")
        .map(|s| s.trim().to_string())
        .map_err(|e| Error::git_command_failed(e.to_string()))
}

/// Compute the relative path prefix of a component within a monorepo.
///
/// If `local_path` is a subdirectory of the git root, returns the relative path
/// (e.g. "wordpress" for `/repo/wordpress`). Returns None if local_path IS the
/// git root (not a monorepo component).
pub fn get_component_path_prefix(local_path: &str) -> Option<String> {
    let git_root = get_git_root(local_path).ok()?;
    let root = std::path::Path::new(&git_root).canonicalize().ok()?;
    let component = std::path::Path::new(local_path).canonicalize().ok()?;

    if root == component {
        return None; // Not a monorepo — component IS the repo root
    }

    component
        .strip_prefix(&root)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}
