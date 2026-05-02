//! Trace overlay application and locking.

use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::engine::run_dir::RunDir;
use crate::error::{Error, ErrorCode, Result};
use crate::paths;

use super::overlay_lock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceOverlayRequest {
    pub component_id: Option<String>,
    pub component_path: String,
    pub overlay_path: String,
}

#[derive(Debug, Clone)]
pub(super) struct AppliedTraceOverlay {
    pub component_id: Option<String>,
    pub component_path: PathBuf,
    pub patch_path: PathBuf,
    pub touched_files: Vec<String>,
    pub keep: bool,
}

#[derive(Debug)]
pub(super) struct TraceOverlayLock {
    pub(super) path: PathBuf,
}

#[derive(Debug, Serialize)]
struct TraceOverlayLockHolder {
    pid: u32,
    component_path: String,
    run_dir: String,
    acquired_at: String,
    command: String,
}

impl TraceOverlayLock {
    pub(super) fn acquire_all(
        requests: &[TraceOverlayRequest],
        run_dir: &RunDir,
    ) -> Result<Vec<Self>> {
        let mut component_paths = BTreeMap::new();
        for request in requests {
            let normalized = normalize_component_path(Path::new(&request.component_path));
            component_paths
                .entry(normalized.to_string_lossy().to_string())
                .or_insert(normalized);
        }

        let mut locks = Vec::new();
        for component_path in component_paths.values() {
            locks.push(Self::acquire(component_path, run_dir)?);
        }
        Ok(locks)
    }

    pub(super) fn acquire(component_path: &Path, run_dir: &RunDir) -> Result<Self> {
        let lock_dir = paths::homeboy_data()?.join("trace-overlay-locks");
        fs::create_dir_all(&lock_dir).map_err(|e| {
            Error::internal_io(
                format!(
                    "Failed to create trace overlay lock dir {}: {}",
                    lock_dir.display(),
                    e
                ),
                Some("trace.overlay.lock.create_dir".to_string()),
            )
        })?;

        let normalized_component_path = normalize_component_path(component_path);
        let path = lock_dir.join(format!(
            "{}.lock",
            overlay_lock::trace_overlay_lock_id(&normalized_component_path)
        ));

        match fs::create_dir(&path) {
            Ok(()) => {
                let holder = TraceOverlayLockHolder {
                    pid: std::process::id(),
                    component_path: normalized_component_path.to_string_lossy().to_string(),
                    run_dir: run_dir.path().to_string_lossy().to_string(),
                    acquired_at: chrono::Utc::now().to_rfc3339(),
                    command: std::env::args().collect::<Vec<_>>().join(" "),
                };
                let holder_path = path.join("holder.json");
                if let Err(error) = write_trace_overlay_lock_holder(&holder_path, &holder) {
                    let _ = fs::remove_dir_all(&path);
                    return Err(error);
                }
                Ok(Self { path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(trace_overlay_lock_error(
                    &normalized_component_path,
                    &path,
                    run_dir,
                    overlay_lock::read_trace_overlay_lock_holder(&path)
                        .and_then(|holder| serde_json::to_value(holder).ok()),
                ))
            }
            Err(e) => Err(Error::internal_io(
                format!(
                    "Failed to acquire trace overlay lock {}: {}",
                    path.display(),
                    e
                ),
                Some("trace.overlay.lock.acquire".to_string()),
            )),
        }
    }
}

impl Drop for TraceOverlayLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(super) fn normalize_component_path(component_path: &Path) -> PathBuf {
    fs::canonicalize(component_path).unwrap_or_else(|_| component_path.to_path_buf())
}

fn write_trace_overlay_lock_holder(path: &Path, holder: &TraceOverlayLockHolder) -> Result<()> {
    let content = serde_json::to_string_pretty(holder).map_err(|e| {
        Error::internal_json(e.to_string(), Some("trace.overlay.lock.holder".to_string()))
    })?;
    fs::write(path, content).map_err(|e| {
        Error::internal_io(
            format!(
                "Failed to write trace overlay lock holder {}: {}",
                path.display(),
                e
            ),
            Some("trace.overlay.lock.write_holder".to_string()),
        )
    })
}

fn trace_overlay_lock_error(
    component_path: &Path,
    lock_path: &Path,
    run_dir: &RunDir,
    holder: Option<serde_json::Value>,
) -> Error {
    let holder_summary = holder
        .as_ref()
        .and_then(trace_overlay_holder_summary)
        .unwrap_or_else(|| "unavailable".to_string());
    Error::new(
        ErrorCode::ValidationInvalidArgument,
        format!(
            "Trace overlay already active for component path {}. Lock path: {}. Active holder: {}. Current run directory: {}",
            component_path.display(),
            lock_path.display(),
            holder_summary,
            run_dir.path().display()
        ),
        serde_json::json!({
            "field": "--overlay",
            "component_path": component_path.to_string_lossy(),
            "lock_path": lock_path.to_string_lossy(),
            "run_dir": run_dir.path().to_string_lossy(),
            "active_holder": holder,
        }),
    )
}

fn trace_overlay_holder_summary(holder: &serde_json::Value) -> Option<String> {
    let pid = holder.get("pid").and_then(|value| value.as_u64())?;
    let run_dir = holder.get("run_dir").and_then(|value| value.as_str());
    let acquired_at = holder.get("acquired_at").and_then(|value| value.as_str());
    Some(match (run_dir, acquired_at) {
        (Some(run_dir), Some(acquired_at)) => {
            format!("pid {pid}, run directory {run_dir}, acquired at {acquired_at}")
        }
        (Some(run_dir), None) => format!("pid {pid}, run directory {run_dir}"),
        (None, Some(acquired_at)) => format!("pid {pid}, acquired at {acquired_at}"),
        (None, None) => format!("pid {pid}"),
    })
}

pub(super) fn apply_trace_overlays(
    requests: &[TraceOverlayRequest],
    keep: bool,
) -> Result<Vec<AppliedTraceOverlay>> {
    let mut applied = Vec::new();
    for request in requests {
        let component_path = PathBuf::from(&request.component_path);
        let patch_path = PathBuf::from(&request.overlay_path);
        let touched_files = match overlay_touched_files(&component_path, &patch_path) {
            Ok(files) => files,
            Err(error) => return cleanup_with_overlay_error(&applied, keep, error, request),
        };
        if let Err(error) =
            ensure_overlay_targets_clean(&component_path, &patch_path, &touched_files)
        {
            return cleanup_with_overlay_error(&applied, keep, error, request);
        }
        if let Err(error) = run_git_apply(&component_path, &patch_path, false) {
            return cleanup_with_overlay_error(&applied, keep, error, request);
        }
        print_trace_overlay("applied", &patch_path, &touched_files, keep);
        applied.push(AppliedTraceOverlay {
            component_id: request.component_id.clone(),
            component_path: component_path.clone(),
            patch_path,
            touched_files,
            keep,
        });
    }
    Ok(applied)
}

fn cleanup_with_overlay_error<T>(
    applied: &[AppliedTraceOverlay],
    keep: bool,
    error: Error,
    request: &TraceOverlayRequest,
) -> Result<T> {
    cleanup_after_overlay_error(applied, keep, trace_overlay_request_error(error, request))
}

fn trace_overlay_request_error(mut error: Error, request: &TraceOverlayRequest) -> Error {
    let component = request
        .component_id
        .as_deref()
        .unwrap_or("<unknown-component>");
    error.message = format!(
        "Trace overlay failed for component '{}' at {} using {}: {}",
        component, request.component_path, request.overlay_path, error.message
    );
    if let Some(details) = error.details.as_object_mut() {
        details.insert(
            "component_id".to_string(),
            serde_json::json!(request.component_id.clone()),
        );
        details.insert(
            "component_path".to_string(),
            serde_json::json!(request.component_path.clone()),
        );
        details.insert(
            "overlay_path".to_string(),
            serde_json::json!(request.overlay_path.clone()),
        );
    }
    error
}

pub(super) fn cleanup_after_overlay_error<T>(
    applied: &[AppliedTraceOverlay],
    keep: bool,
    error: Error,
) -> Result<T> {
    if !keep {
        let _ = cleanup_trace_overlays(applied);
    }
    Err(error)
}

pub(super) fn cleanup_trace_overlays(applied: &[AppliedTraceOverlay]) -> Result<()> {
    let mut first_error = None;
    for overlay in applied.iter().rev() {
        match run_git_apply(&overlay.component_path, &overlay.patch_path, true) {
            Ok(()) => print_trace_overlay(
                "reverted",
                &overlay.patch_path,
                &overlay.touched_files,
                overlay.keep,
            ),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn print_trace_overlay(action: &str, patch_path: &Path, touched_files: &[String], keep: bool) {
    eprintln!("trace overlay {action}: {}", patch_path.display());
    let retention = if action == "reverted" {
        "overlay changes reverted"
    } else if keep {
        "overlay changes will be kept"
    } else {
        "overlay changes will be reverted after the run"
    };
    eprintln!("  status: {retention}");
    if touched_files.is_empty() {
        eprintln!("  touched files: none reported by git apply --numstat");
        return;
    }
    eprintln!("  touched files:");
    for file in touched_files {
        eprintln!("    - {file}");
    }
}

pub(super) fn overlay_touched_files(
    component_path: &Path,
    patch_path: &Path,
) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["apply", "--numstat"])
        .arg(patch_path)
        .current_dir(component_path)
        .output()
        .map_err(|e| {
            Error::internal_io(
                format!(
                    "Failed to inspect trace overlay {}: {}",
                    patch_path.display(),
                    e
                ),
                Some("trace.overlay.inspect".to_string()),
            )
        })?;
    if !output.status.success() {
        return Err(Error::validation_invalid_argument(
            "--overlay",
            format!("trace overlay {} cannot be inspected", patch_path.display()),
            Some(String::from_utf8_lossy(&output.stderr).to_string()),
            None,
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split('\t').nth(2))
        .map(unquote_numstat_path)
        .filter(|path| !path.is_empty())
        .collect())
}

fn ensure_overlay_targets_clean(
    component_path: &Path,
    patch_path: &Path,
    touched_files: &[String],
) -> Result<()> {
    if touched_files.is_empty() {
        return Ok(());
    }
    let mut command = Command::new("git");
    command
        .args(["status", "--porcelain=v1", "--"])
        .args(touched_files)
        .current_dir(component_path);
    let output = command.output().map_err(|e| {
        Error::internal_io(
            format!(
                "Failed to check trace overlay targets for {}: {}",
                patch_path.display(),
                e
            ),
            Some("trace.overlay.status".to_string()),
        )
    })?;
    if !output.status.success() {
        return Err(Error::validation_invalid_argument(
            "--overlay",
            format!(
                "failed to check overlay target status for {}",
                patch_path.display()
            ),
            Some(String::from_utf8_lossy(&output.stderr).to_string()),
            None,
        ));
    }
    let dirty = String::from_utf8_lossy(&output.stdout);
    if !dirty.trim().is_empty() {
        return Err(Error::validation_invalid_argument(
            "--overlay",
            format!(
                "trace overlay {} touches files with pre-existing changes",
                patch_path.display()
            ),
            Some(dirty.to_string()),
            None,
        ));
    }
    Ok(())
}

fn run_git_apply(component_path: &Path, patch_path: &Path, reverse: bool) -> Result<()> {
    let mut command = Command::new("git");
    command.arg("apply");
    if reverse {
        command.arg("--reverse");
    }
    let output = command
        .arg(patch_path)
        .current_dir(component_path)
        .output()
        .map_err(|e| {
            Error::internal_io(
                format!(
                    "Failed to apply trace overlay {}: {}",
                    patch_path.display(),
                    e
                ),
                Some("trace.overlay.apply".to_string()),
            )
        })?;
    if output.status.success() {
        return Ok(());
    }
    let action = if reverse { "revert" } else { "apply" };
    Err(Error::validation_invalid_argument(
        "--overlay",
        format!(
            "failed to {} trace overlay {}",
            action,
            patch_path.display()
        ),
        Some(String::from_utf8_lossy(&output.stderr).to_string()),
        None,
    ))
}

fn unquote_numstat_path(path: &str) -> String {
    path.trim().trim_matches('"').to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use crate::engine::run_dir::RunDir;
    use crate::test_support::with_isolated_home;

    use super::*;

    #[test]
    fn test_acquire() {
        with_isolated_home(|_| {
            let component_dir = tempfile::tempdir().unwrap();
            let run_dir = RunDir::create().unwrap();
            let lock_path;

            {
                let lock = TraceOverlayLock::acquire(component_dir.path(), &run_dir).unwrap();
                lock_path = lock.path.clone();
                assert!(lock_path.exists());
                assert!(lock_path.join("holder.json").exists());
            }

            assert!(!lock_path.exists());
            run_dir.cleanup();
        });
    }

    #[test]
    fn test_acquire_all() {
        with_isolated_home(|_| {
            let first = tempfile::tempdir().unwrap();
            let second = tempfile::tempdir().unwrap();
            let run_dir = RunDir::create().unwrap();
            let requests = vec![
                TraceOverlayRequest {
                    component_id: Some("first".to_string()),
                    component_path: first.path().to_string_lossy().to_string(),
                    overlay_path: "/tmp/first.patch".to_string(),
                },
                TraceOverlayRequest {
                    component_id: Some("second".to_string()),
                    component_path: second.path().to_string_lossy().to_string(),
                    overlay_path: "/tmp/second.patch".to_string(),
                },
            ];

            let locks = TraceOverlayLock::acquire_all(&requests, &run_dir).unwrap();

            assert_eq!(locks.len(), 2);
            assert!(locks.iter().all(|lock| lock.path.exists()));
            run_dir.cleanup();
        });
    }

    #[test]
    fn test_apply_trace_overlays() {
        let fixture = overlay_fixture("overlay");
        let applied = apply_trace_overlays(&[fixture.request.clone()], true).unwrap();

        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].touched_files, vec!["scenario.txt"]);
        assert_eq!(
            fs::read_to_string(fixture.component_dir.join("scenario.txt")).unwrap(),
            "overlay\n"
        );
    }

    #[test]
    fn test_cleanup_trace_overlays() {
        let fixture = overlay_fixture("overlay");
        let applied = apply_trace_overlays(&[fixture.request.clone()], true).unwrap();

        cleanup_trace_overlays(&applied).unwrap();

        assert_eq!(
            fs::read_to_string(fixture.component_dir.join("scenario.txt")).unwrap(),
            "base\n"
        );
    }

    #[test]
    fn test_overlay_touched_files() {
        let fixture = overlay_fixture("overlay");

        let touched = overlay_touched_files(
            &fixture.component_dir,
            Path::new(&fixture.request.overlay_path),
        )
        .unwrap();

        assert_eq!(touched, vec!["scenario.txt"]);
    }

    #[test]
    fn test_normalize_component_path() {
        let dir = tempfile::tempdir().unwrap();
        let normalized = super::normalize_component_path(dir.path());

        assert!(normalized.is_absolute());
        assert_eq!(normalized, fs::canonicalize(dir.path()).unwrap());
    }

    #[test]
    fn test_trace_overlay_lock_id() {
        let first = overlay_lock::trace_overlay_lock_id(Path::new("/tmp/component-a"));
        let second = overlay_lock::trace_overlay_lock_id(Path::new("/tmp/component-b"));

        assert_eq!(first.len(), 24);
        assert_ne!(first, second);
    }

    struct OverlayFixture {
        _temp: tempfile::TempDir,
        component_dir: PathBuf,
        request: TraceOverlayRequest,
    }

    fn overlay_fixture(replacement: &str) -> OverlayFixture {
        let temp = tempfile::tempdir().unwrap();
        let component_dir = temp.path().join("component");
        fs::create_dir_all(&component_dir).unwrap();
        fs::write(component_dir.join("scenario.txt"), "base\n").unwrap();
        init_git_repo(&component_dir);
        let patch_path = temp.path().join("overlay.patch");
        fs::write(
            &patch_path,
            format!(
                r#"--- a/scenario.txt
+++ b/scenario.txt
@@ -1 +1 @@
-base
+{replacement}
"#
            ),
        )
        .unwrap();

        OverlayFixture {
            _temp: temp,
            component_dir: component_dir.clone(),
            request: TraceOverlayRequest {
                component_id: Some("component".to_string()),
                component_path: component_dir.to_string_lossy().to_string(),
                overlay_path: patch_path.to_string_lossy().to_string(),
            },
        }
    }

    fn init_git_repo(path: &Path) {
        git(path, &["init"]);
        git(path, &["add", "scenario.txt"]);
        git(
            path,
            &[
                "-c",
                "user.name=Homeboy Test",
                "-c",
                "user.email=homeboy@example.test",
                "commit",
                "-m",
                "init",
            ],
        );
    }

    fn git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
