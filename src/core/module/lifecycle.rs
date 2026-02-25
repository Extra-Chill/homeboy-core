use crate::config::from_str;
use crate::error::{Error, Result};
use crate::git;
use crate::local_files::{self, FileSystem};
use crate::paths;
use crate::utils::slugify;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::execution::run_setup;
use super::manifest::ModuleManifest;
use super::{is_module_linked, load_module};

#[derive(Debug, Clone)]
pub struct InstallResult {
    pub module_id: String,
    pub url: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct UpdateResult {
    pub module_id: String,
    pub url: String,
    pub path: PathBuf,
}

pub fn slugify_id(value: &str) -> Result<String> {
    slugify::slugify_id(value, "module_id")
}

/// Derive a module ID from a git URL.
pub fn derive_id_from_url(url: &str) -> Result<String> {
    let trimmed = url.trim_end_matches('/');
    let segment = trimmed
        .split('/')
        .next_back()
        .unwrap_or(trimmed)
        .trim_end_matches(".git");

    slugify_id(segment)
}

/// Check if a string looks like a git URL (vs a local path).
pub fn is_git_url(source: &str) -> bool {
    source.starts_with("http://")
        || source.starts_with("https://")
        || source.starts_with("git@")
        || source.starts_with("ssh://")
        || source.ends_with(".git")
}

/// Check if a git working directory is clean (no uncommitted changes).
fn is_workdir_clean(path: &Path) -> bool {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output();

    match output {
        Ok(output) => output.status.success() && output.stdout.is_empty(),
        Err(_) => false,
    }
}

/// Returns the path to a module's manifest file: {module_dir}/{id}.json
fn manifest_path_for_module(module_dir: &Path, id: &str) -> PathBuf {
    module_dir.join(format!("{}.json", id))
}

/// Install a module from a git URL or link a local directory.
/// Automatically detects whether source is a URL (git clone) or local path (symlink).
pub fn install(source: &str, id_override: Option<&str>) -> Result<InstallResult> {
    if is_git_url(source) {
        install_from_url(source, id_override)
    } else {
        install_from_path(source, id_override)
    }
}

/// Install a module by cloning from a git repository URL.
///
/// Handles both single-module repos (manifest at repo root) and monorepos
/// (manifest in a subdirectory matching the module ID). For monorepos,
/// extracts just the target subdirectory.
fn install_from_url(url: &str, id_override: Option<&str>) -> Result<InstallResult> {
    let module_id = match id_override {
        Some(id) => slugify_id(id)?,
        None => derive_id_from_url(url)?,
    };

    let module_dir = paths::module(&module_id)?;
    if module_dir.exists() {
        return Err(Error::validation_invalid_argument(
            "module_id",
            format!("Module {} already exists", module_id),
            Some(module_id),
            None,
        ));
    }

    local_files::ensure_app_dirs()?;

    // Clone to a temp directory first so we can detect monorepos before
    // committing to the final module location.
    let modules_dir = paths::modules()?;
    let temp_dir = modules_dir.join(format!(".clone-tmp-{}", module_id));
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).map_err(|e| {
            Error::internal_io(e.to_string(), Some("clean stale temp dir".to_string()))
        })?;
    }

    git::clone_repo(url, &temp_dir)?;

    // Determine what was cloned and install accordingly.
    let result = resolve_cloned_module(&temp_dir, &module_id, &module_dir, url);

    // Always clean up the temp clone dir (may already be renamed on success).
    if temp_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    let module_id = result?;

    // Auto-run setup if module defines a setup_command
    // Setup is best-effort: install succeeds even if setup fails
    if let Ok(module) = load_module(&module_id) {
        if module.runtime().is_some_and(|r| r.setup_command.is_some()) {
            let _ = run_setup(&module_id);
        }
    }

    Ok(InstallResult {
        module_id,
        url: url.to_string(),
        path: module_dir,
    })
}

/// After cloning a repo to a temp dir, figure out whether it's a single-module
/// repo or a monorepo and move the right content to the final module directory.
///
/// Returns the installed module ID on success.
fn resolve_cloned_module(
    temp_dir: &Path,
    module_id: &str,
    module_dir: &Path,
    _url: &str,
) -> Result<String> {
    let manifest_at_root = temp_dir.join(format!("{}.json", module_id));

    // Case 1: Single-module repo — manifest at clone root.
    if manifest_at_root.exists() {
        std::fs::rename(temp_dir, module_dir).map_err(|e| {
            Error::internal_io(e.to_string(), Some("move cloned module".to_string()))
        })?;
        return Ok(module_id.to_string());
    }

    // Case 2: Monorepo — target module exists as a subdirectory.
    let subdir = temp_dir.join(module_id);
    let manifest_in_subdir = subdir.join(format!("{}.json", module_id));

    if subdir.is_dir() && manifest_in_subdir.exists() {
        // Validate the manifest is parseable before moving.
        let content = local_files::local().read(&manifest_in_subdir)?;
        let _manifest: ModuleManifest = from_str(&content)?;

        // Move just the subdirectory to the final module location.
        rename_dir(&subdir, module_dir)?;
        return Ok(module_id.to_string());
    }

    // Case 3: No matching module found. Scan for available modules to help the user.
    let available = scan_available_modules(temp_dir);

    if available.is_empty() {
        return Err(Error::validation_invalid_argument(
            "source",
            format!(
                "No module manifest '{}.json' found in cloned repository",
                module_id
            ),
            None,
            None,
        ));
    }

    let list = available.join(", ");
    Err(Error::validation_invalid_argument(
        "id",
        format!(
            "Module '{}' not found in repository. Available modules: {}",
            module_id, list
        ),
        Some(module_id.to_string()),
        None,
    )
    .with_hint(format!(
        "Install a specific module with: homeboy module install <url> --id <module>\nAvailable: {}",
        list
    )))
}

/// Scan a cloned repo for subdirectories that contain a matching manifest file.
/// Returns a sorted list of module IDs found.
fn scan_available_modules(repo_dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(repo_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                    // Skip hidden dirs (.git, .github, etc.)
                    if dir_name.starts_with('.') {
                        continue;
                    }
                    let manifest = path.join(format!("{}.json", dir_name));
                    if manifest.exists() {
                        found.push(dir_name.to_string());
                    }
                }
            }
        }
    }
    found.sort();
    found
}

/// Move a directory, falling back to recursive copy + delete if rename fails
/// (e.g., across filesystem boundaries).
fn rename_dir(from: &Path, to: &Path) -> Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }

    // Fallback: recursive copy then remove source.
    copy_dir_recursive(from, to)?;
    std::fs::remove_dir_all(from)
        .map_err(|e| Error::internal_io(e.to_string(), Some("remove source after copy".into())))?;
    Ok(())
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)
        .map_err(|e| Error::internal_io(e.to_string(), Some("create target dir".into())))?;

    for entry in std::fs::read_dir(src)
        .map_err(|e| Error::internal_io(e.to_string(), Some("read source dir".into())))?
    {
        let entry =
            entry.map_err(|e| Error::internal_io(e.to_string(), Some("read dir entry".into())))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| {
                Error::internal_io(e.to_string(), Some("copy file".into()))
            })?;
        }
    }
    Ok(())
}

/// Install a module by symlinking a local directory.
fn install_from_path(source_path: &str, id_override: Option<&str>) -> Result<InstallResult> {
    let source = Path::new(source_path);

    // Resolve to absolute path
    let source = if source.is_absolute() {
        source.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| Error::internal_io(e.to_string(), Some("get current dir".to_string())))?
            .join(source)
    };

    if !source.exists() {
        return Err(Error::validation_invalid_argument(
            "source",
            format!("Path does not exist: {}", source.display()),
            Some(source_path.to_string()),
            None,
        ));
    }

    // Derive module ID from directory name or override
    let dir_name = source.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        Error::validation_invalid_argument(
            "source",
            "Could not determine directory name",
            Some(source_path.to_string()),
            None,
        )
    })?;

    let module_id = match id_override {
        Some(id) => slugify_id(id)?,
        None => slugify_id(dir_name)?,
    };

    let manifest_path = manifest_path_for_module(&source, &module_id);
    if !manifest_path.exists() {
        return Err(Error::validation_invalid_argument(
            "source",
            format!("No {}.json found at {}", module_id, source.display()),
            Some(source_path.to_string()),
            None,
        ));
    }

    // Validate manifest is parseable
    let manifest_content = local_files::local().read(&manifest_path)?;
    let _manifest: ModuleManifest = from_str(&manifest_content)?;

    let module_dir = paths::module(&module_id)?;
    if module_dir.exists() {
        return Err(Error::validation_invalid_argument(
            "module_id",
            format!(
                "Module '{}' already exists at {}",
                module_id,
                module_dir.display()
            ),
            Some(module_id),
            None,
        ));
    }

    local_files::ensure_app_dirs()?;

    // Create symlink
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source, &module_dir)
        .map_err(|e| Error::internal_io(e.to_string(), Some("create symlink".to_string())))?;

    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&source, &module_dir)
        .map_err(|e| Error::internal_io(e.to_string(), Some("create symlink".to_string())))?;

    Ok(InstallResult {
        module_id,
        url: source.to_string_lossy().to_string(),
        path: module_dir,
    })
}

/// Update an installed module by pulling latest changes.
pub fn update(module_id: &str, force: bool) -> Result<UpdateResult> {
    let module_dir = paths::module(module_id)?;
    if !module_dir.exists() {
        return Err(Error::module_not_found(module_id.to_string(), vec![]));
    }

    // Linked modules are managed externally
    if is_module_linked(module_id) {
        return Err(Error::validation_invalid_argument(
            "module_id",
            format!(
                "Module '{}' is linked. Update the source directory directly.",
                module_id
            ),
            Some(module_id.to_string()),
            None,
        ));
    }

    if !force && !is_workdir_clean(&module_dir) {
        return Err(Error::validation_invalid_argument(
            "module_id",
            "Module has uncommitted changes; update may overwrite them. Use --force to proceed.",
            Some(module_id.to_string()),
            None,
        ));
    }

    let module = load_module(module_id)?;

    let source_url = module.source_url.ok_or_else(|| {
        Error::validation_invalid_argument(
            "module_id",
            format!(
                "Module '{}' has no sourceUrl. Reinstall with 'homeboy module install <url>'.",
                module_id
            ),
            Some(module_id.to_string()),
            None,
        )
    })?;

    git::pull_repo(&module_dir)?;

    // Auto-run setup if module defines a setup_command
    // Setup is best-effort: update succeeds even if setup fails
    if let Ok(module) = load_module(module_id) {
        if module.runtime().is_some_and(|r| r.setup_command.is_some()) {
            let _ = run_setup(module_id);
        }
    }

    Ok(UpdateResult {
        module_id: module_id.to_string(),
        url: source_url,
        path: module_dir,
    })
}

/// Uninstall a module. Automatically detects symlinks vs cloned directories.
/// - Symlinked modules: removes symlink only (source preserved)
/// - Cloned modules: removes directory entirely
pub fn uninstall(module_id: &str) -> Result<PathBuf> {
    let module_dir = paths::module(module_id)?;
    if !module_dir.exists() {
        return Err(Error::module_not_found(module_id.to_string(), vec![]));
    }

    if module_dir.is_symlink() {
        // Symlinked module: just remove the symlink, source directory is preserved
        std::fs::remove_file(&module_dir)
            .map_err(|e| Error::internal_io(e.to_string(), Some("remove symlink".to_string())))?;
    } else {
        // Cloned module: remove the directory
        std::fs::remove_dir_all(&module_dir).map_err(|e| {
            Error::internal_io(e.to_string(), Some("remove module directory".to_string()))
        })?;
    }

    Ok(module_dir)
}

/// Check if a git-cloned module has updates available.
/// Runs `git fetch` then checks if HEAD is behind the remote tracking branch.
/// Returns None for linked modules or if check fails.
pub fn check_update_available(module_id: &str) -> Option<UpdateAvailable> {
    let module_dir = paths::module(module_id).ok()?;
    if !module_dir.exists() || is_module_linked(module_id) {
        return None;
    }

    // Check it's a git repo
    if !module_dir.join(".git").exists() {
        return None;
    }

    // Fetch latest (best-effort, short timeout)
    Command::new("git")
        .args(["fetch", "--quiet"])
        .current_dir(&module_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    // Check how many commits we're behind
    let output = Command::new("git")
        .args(["rev-list", "HEAD..@{u}", "--count"])
        .current_dir(&module_dir)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;

    let count_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let behind_count: usize = count_str.parse().ok()?;

    if behind_count == 0 {
        return None;
    }

    // Get installed version
    let module = load_module(module_id).ok()?;
    let installed_version = module.version.clone();

    Some(UpdateAvailable {
        module_id: module_id.to_string(),
        installed_version,
        behind_count,
    })
}

#[derive(Debug, Clone)]
pub struct UpdateAvailable {
    pub module_id: String,
    pub installed_version: String,
    pub behind_count: usize,
}
