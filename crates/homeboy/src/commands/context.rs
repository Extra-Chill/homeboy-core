use clap::Args;
use serde::Serialize;
use std::path::PathBuf;

use super::CmdResult;
use homeboy_core::config::ConfigManager;

#[derive(Args)]
pub struct ContextArgs {}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextOutput {
    pub command: String,
    pub cwd: String,
    pub git_root: Option<String>,
    pub managed: bool,
    pub matched_components: Vec<String>,
    pub suggestion: Option<String>,
}

pub fn run(_args: ContextArgs, _global: &crate::commands::GlobalArgs) -> CmdResult<ContextOutput> {
    let cwd = std::env::current_dir()
        .map_err(|e| homeboy_core::Error::internal_io(e.to_string(), None))?;

    let cwd_str = cwd.to_string_lossy().to_string();

    let git_root = detect_git_root(&cwd);

    let components = ConfigManager::list_components().unwrap_or_default();

    let matched: Vec<String> = components
        .iter()
        .filter(|c| path_matches(&cwd, &c.local_path))
        .map(|c| c.id.clone())
        .collect();

    let managed = !matched.is_empty();

    let suggestion = if managed {
        None
    } else {
        Some(
            "This directory is not managed by Homeboy. To initialize, create a project or component."
                .to_string(),
        )
    };

    Ok((
        ContextOutput {
            command: "context.show".to_string(),
            cwd: cwd_str,
            git_root,
            managed,
            matched_components: matched,
            suggestion,
        },
        0,
    ))
}

fn detect_git_root(cwd: &PathBuf) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(path);
        }
    }
    None
}

fn path_matches(cwd: &PathBuf, local_path: &str) -> bool {
    let local = PathBuf::from(local_path);

    let cwd_canonical = cwd.canonicalize().ok();
    let local_canonical = local.canonicalize().ok();

    match (cwd_canonical, local_canonical) {
        (Some(cwd_path), Some(local_path)) => {
            cwd_path == local_path || cwd_path.starts_with(&local_path)
        }
        _ => false,
    }
}
