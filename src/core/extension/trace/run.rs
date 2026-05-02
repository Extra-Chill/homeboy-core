//! Trace workflows: invoke extension runners, parse JSON, preserve artifacts.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::component::Component;
use crate::engine::baseline::BaselineFlags;
use crate::engine::run_dir::{self, RunDir};
use crate::error::{Error, ErrorCode, Result};
use crate::extension::trace::baseline::TraceBaselineComparison;
use crate::extension::{
    resolve_execution_context, stderr_tail, ExtensionCapability, ExtensionExecutionContext,
};
use crate::extension::{ExtensionRunner, RunnerOutput};
use crate::paths;
use crate::rig::RigStateSnapshot;

use super::parsing::{
    parse_trace_list_str, parse_trace_results_file, TraceList, TraceResults, TraceSpanDefinition,
};

#[derive(Debug, Clone)]
pub struct TraceRunWorkflowArgs {
    pub component_label: String,
    pub component_id: String,
    pub path_override: Option<String>,
    pub settings: Vec<(String, String)>,
    pub settings_json: Vec<(String, serde_json::Value)>,
    pub scenario_id: String,
    pub json_summary: bool,
    pub rig_id: Option<String>,
    pub overlays: Vec<String>,
    pub keep_overlay: bool,
    pub extra_workloads: Vec<PathBuf>,
    pub span_definitions: Vec<TraceSpanDefinition>,
    pub baseline_flags: BaselineFlags,
    pub regression_threshold_percent: f64,
    pub regression_min_delta_ms: u64,
}

#[derive(Debug, Clone)]
pub struct TraceListWorkflowArgs {
    pub component_label: String,
    pub component_id: String,
    pub path_override: Option<String>,
    pub settings: Vec<(String, String)>,
    pub settings_json: Vec<(String, serde_json::Value)>,
    pub rig_id: Option<String>,
    pub extra_workloads: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceRunWorkflowResult {
    pub status: String,
    pub component: String,
    pub exit_code: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<TraceResults>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<TraceRunFailure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overlays: Vec<TraceOverlay>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_comparison: Option<TraceBaselineComparison>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TraceOverlay {
    pub path: String,
    pub component_path: String,
    pub touched_files: Vec<String>,
    pub kept: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceRunFailure {
    pub component_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_override: Option<String>,
    pub scenario_id: String,
    pub exit_code: i32,
    pub stderr_excerpt: String,
}

pub fn run_trace_workflow(
    component: &Component,
    args: TraceRunWorkflowArgs,
    run_dir: &RunDir,
    rig_state: Option<RigStateSnapshot>,
) -> Result<TraceRunWorkflowResult> {
    let execution_context = resolve_execution_context(component, ExtensionCapability::Trace)?;
    run_trace_workflow_with_context(&execution_context, component, args, run_dir, rig_state)
}

fn run_trace_workflow_with_context(
    execution_context: &ExtensionExecutionContext,
    component: &Component,
    args: TraceRunWorkflowArgs,
    run_dir: &RunDir,
    rig_state: Option<RigStateSnapshot>,
) -> Result<TraceRunWorkflowResult> {
    let component_path = args
        .path_override
        .as_deref()
        .unwrap_or(component.local_path.as_str());
    let _overlay_lock = if args.overlays.is_empty() {
        None
    } else {
        Some(TraceOverlayLock::acquire(
            Path::new(component_path),
            &args.overlays,
            run_dir,
        )?)
    };
    let applied_overlays = apply_trace_overlays(component_path, &args.overlays, args.keep_overlay)?;
    let runner = match build_trace_runner(execution_context, component, &args, run_dir, false) {
        Ok(runner) => runner,
        Err(error) => {
            return cleanup_after_overlay_error(&applied_overlays, args.keep_overlay, error)
        }
    };
    let runner_output = runner.run();
    if !args.keep_overlay {
        cleanup_trace_overlays(&applied_overlays)?
    }
    let runner_output = runner_output?;
    let results_path = run_dir.step_file(run_dir::files::TRACE_RESULTS);
    let mut results = if results_path.exists() {
        let mut parsed = parse_trace_results_file(&results_path)?;
        if parsed.rig.is_none() {
            parsed.rig = rig_state;
        }
        Some(parsed)
    } else {
        None
    };
    let status = results
        .as_ref()
        .map(|r| r.status.as_str().to_string())
        .unwrap_or_else(|| {
            if runner_output.success {
                "pass"
            } else {
                "error"
            }
            .to_string()
        });
    let exit_code = if runner_output.success {
        if status == "pass" {
            0
        } else {
            1
        }
    } else {
        runner_output.exit_code
    };
    let failure = (!runner_output.success).then(|| failure_from_output(&args, &runner_output));
    if let Some(parsed) = results.as_mut() {
        super::spans::apply_span_definitions(parsed, &args.span_definitions);
    }

    let rig_id = args.rig_id.as_deref();
    let source_path = Path::new(component_path);
    let mut baseline_comparison = None;
    let mut baseline_exit_override = None;
    let mut hints = Vec::new();
    let has_span_results = results
        .as_ref()
        .is_some_and(|parsed| !parsed.span_results.is_empty());

    if args.baseline_flags.baseline && status == "pass" && has_span_results {
        if let Some(ref parsed) = results {
            let _ =
                super::baseline::save_baseline(source_path, &args.component_id, parsed, rig_id)?;
        }
    }
    if has_span_results && !args.baseline_flags.baseline && !args.baseline_flags.ignore_baseline {
        if let Some(ref parsed) = results {
            if let Some(existing) = super::baseline::load_baseline(source_path, rig_id) {
                let comparison = super::baseline::compare(
                    parsed,
                    &existing,
                    args.regression_threshold_percent,
                    args.regression_min_delta_ms,
                );
                if comparison.regression {
                    baseline_exit_override = Some(1);
                } else if comparison.has_improvements && args.baseline_flags.ratchet {
                    let _ = super::baseline::save_baseline(
                        source_path,
                        &args.component_id,
                        parsed,
                        rig_id,
                    );
                }
                baseline_comparison = Some(comparison);
            }
        }
    }

    let trace_invocation = match rig_id {
        Some(id) => format!(
            "homeboy trace {} {} --rig {}",
            args.component_id, args.scenario_id, id
        ),
        None => format!("homeboy trace {} {}", args.component_id, args.scenario_id),
    };
    if has_span_results && !args.baseline_flags.baseline && baseline_comparison.is_none() {
        hints.push(format!(
            "Save trace span baseline: {} --baseline",
            trace_invocation
        ));
    }
    if baseline_comparison.is_some() && !args.baseline_flags.ratchet {
        hints.push(format!(
            "Auto-update trace span baseline on improvement: {} --ratchet",
            trace_invocation
        ));
    }
    if let Some(ref cmp) = baseline_comparison {
        if cmp.regression {
            hints.push(format!(
                "Trace span regression threshold: {}% and {}ms. Raise them with --regression-threshold=<PCT> or --regression-min-delta-ms=<MS> if expected.",
                cmp.threshold_percent, cmp.min_delta_ms
            ));
        }
    }

    let exit_code = baseline_exit_override.unwrap_or(exit_code);

    Ok(TraceRunWorkflowResult {
        status,
        component: args.component_label,
        exit_code,
        results,
        failure,
        overlays: applied_overlays
            .into_iter()
            .map(|overlay| TraceOverlay {
                path: overlay.patch_path.to_string_lossy().to_string(),
                component_path: overlay.component_path.to_string_lossy().to_string(),
                touched_files: overlay.touched_files,
                kept: overlay.keep,
            })
            .collect(),
        baseline_comparison,
        hints: if hints.is_empty() { None } else { Some(hints) },
    })
}

pub fn run_trace_list_workflow(
    component: &Component,
    args: TraceListWorkflowArgs,
    run_dir: &RunDir,
) -> Result<TraceList> {
    let execution_context = resolve_execution_context(component, ExtensionCapability::Trace)?;
    let runner_args = TraceRunWorkflowArgs {
        component_label: args.component_label.clone(),
        component_id: args.component_id,
        path_override: args.path_override,
        settings: args.settings,
        settings_json: args.settings_json,
        scenario_id: String::new(),
        json_summary: false,
        rig_id: args.rig_id,
        overlays: Vec::new(),
        keep_overlay: false,
        extra_workloads: args.extra_workloads,
        span_definitions: Vec::new(),
        baseline_flags: BaselineFlags {
            baseline: false,
            ignore_baseline: true,
            ratchet: false,
        },
        regression_threshold_percent: super::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
        regression_min_delta_ms: super::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
    };
    let output =
        build_trace_runner(&execution_context, component, &runner_args, run_dir, true)?.run()?;
    if !output.success {
        return Err(Error::validation_invalid_argument(
            "trace_list",
            format!(
                "trace scenario discovery failed with exit code {}",
                output.exit_code
            ),
            Some(format!(
                "stdout:\n{}\n\nstderr:\n{}",
                output.stdout, output.stderr
            )),
            None,
        ));
    }

    let results_path = run_dir.step_file(run_dir::files::TRACE_RESULTS);
    if results_path.exists() {
        let content = std::fs::read_to_string(&results_path).map_err(|e| {
            Error::internal_io(
                format!(
                    "Failed to read trace list file {}: {}",
                    results_path.display(),
                    e
                ),
                Some("trace.list.read".to_string()),
            )
        })?;
        return parse_trace_list_str(&content);
    }

    parse_trace_list_str(&output.stdout)
}

pub(crate) fn build_trace_runner(
    execution_context: &ExtensionExecutionContext,
    component: &Component,
    args: &TraceRunWorkflowArgs,
    run_dir: &RunDir,
    list_only: bool,
) -> Result<ExtensionRunner> {
    let artifact_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifact_dir).map_err(|e| {
        Error::internal_io(
            format!(
                "Failed to create trace artifact dir {}: {}",
                artifact_dir.display(),
                e
            ),
            Some("trace.artifacts.create".to_string()),
        )
    })?;

    let mut runner = ExtensionRunner::for_context(execution_context.clone())
        .component(component.clone())
        .path_override(args.path_override.clone())
        .settings(&args.settings)
        .settings_json(&args.settings_json)
        .with_run_dir(run_dir)
        .cleanup_process_group(true)
        .env(
            "HOMEBOY_TRACE_RESULTS_FILE",
            &run_dir
                .step_file(run_dir::files::TRACE_RESULTS)
                .to_string_lossy(),
        )
        .env("HOMEBOY_TRACE_SCENARIO", &args.scenario_id)
        .env(
            "HOMEBOY_TRACE_ARTIFACT_DIR",
            &artifact_dir.to_string_lossy(),
        )
        .env("HOMEBOY_TRACE_LIST_ONLY", if list_only { "1" } else { "0" });

    if let Some(rig_id) = &args.rig_id {
        runner = runner.env("HOMEBOY_TRACE_RIG_ID", rig_id);
    }
    if let Some(path) = &args.path_override {
        runner = runner.env("HOMEBOY_TRACE_COMPONENT_PATH", path);
    }
    if !args.extra_workloads.is_empty() {
        runner = runner.env(
            "HOMEBOY_TRACE_EXTRA_WORKLOADS",
            &extra_workloads_env_value(&args.extra_workloads)?,
        );
    }

    Ok(runner)
}

fn extra_workloads_env_value(paths: &[PathBuf]) -> Result<String> {
    std::env::join_paths(paths)
        .map_err(|e| {
            Error::validation_invalid_argument(
                "trace_workloads",
                format!("trace workload path cannot be exported: {}", e),
                None,
                None,
            )
        })
        .map(|joined| joined.to_string_lossy().to_string())
}

fn failure_from_output(args: &TraceRunWorkflowArgs, output: &RunnerOutput) -> TraceRunFailure {
    TraceRunFailure {
        component_id: args.component_id.clone(),
        path_override: args.path_override.clone(),
        scenario_id: args.scenario_id.clone(),
        exit_code: output.exit_code,
        stderr_excerpt: stderr_tail(&output.stderr),
    }
}

#[derive(Debug)]
struct TraceOverlayLock {
    path: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct TraceOverlayLockHolder {
    pid: u32,
    component_path: String,
    run_dir: String,
    acquired_at: String,
    command: String,
    #[serde(default)]
    overlay_paths: Vec<String>,
    #[serde(default)]
    touched_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TraceOverlayLockRecord {
    pub lock_path: String,
    pub status: TraceOverlayLockStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holder: Option<TraceOverlayLockHolder>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TraceOverlayLockStatus {
    Active,
    Stale,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceOverlayLockCleanupResult {
    pub removed: Vec<TraceOverlayLockRecord>,
    pub kept: Vec<TraceOverlayLockRecord>,
}

impl TraceOverlayLock {
    fn acquire(component_path: &Path, overlay_paths: &[String], run_dir: &RunDir) -> Result<Self> {
        let lock_dir = trace_overlay_lock_dir()?;

        let normalized_component_path = normalize_component_path(component_path);
        let path = lock_dir.join(format!(
            "{}.lock",
            trace_overlay_lock_id(&normalized_component_path)
        ));

        match fs::create_dir(&path) {
            Ok(()) => {
                let touched_files = trace_overlay_touched_files_for_paths(
                    &normalized_component_path,
                    overlay_paths,
                )?;
                let holder = TraceOverlayLockHolder {
                    pid: std::process::id(),
                    component_path: normalized_component_path.to_string_lossy().to_string(),
                    run_dir: run_dir.path().to_string_lossy().to_string(),
                    acquired_at: chrono::Utc::now().to_rfc3339(),
                    command: std::env::args().collect::<Vec<_>>().join(" "),
                    overlay_paths: overlay_paths.to_vec(),
                    touched_files,
                };
                let holder_path = path.join("holder.json");
                if let Err(error) = write_trace_overlay_lock_holder(&holder_path, &holder) {
                    let _ = fs::remove_dir_all(&path);
                    return Err(error);
                }
                Ok(Self { path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let holder = read_trace_overlay_lock_holder(&path);
                Err(trace_overlay_lock_error(
                    &normalized_component_path,
                    &path,
                    run_dir,
                    holder,
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

pub fn list_trace_overlay_locks() -> Result<Vec<TraceOverlayLockRecord>> {
    let lock_dir = trace_overlay_lock_dir()?;
    let entries = fs::read_dir(&lock_dir).map_err(|e| {
        Error::internal_io(
            format!(
                "Failed to read trace overlay lock dir {}: {}",
                lock_dir.display(),
                e
            ),
            Some("trace.overlay.lock.read_dir".to_string()),
        )
    })?;
    let mut records = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| {
            Error::internal_io(
                format!("Failed to read trace overlay lock entry: {e}"),
                Some("trace.overlay.lock.read_entry".to_string()),
            )
        })?;
        let path = entry.path();
        if !path.is_dir() || path.extension().and_then(|ext| ext.to_str()) != Some("lock") {
            continue;
        }
        records.push(read_trace_overlay_lock_record(&path));
    }
    records.sort_by(|a, b| a.lock_path.cmp(&b.lock_path));
    Ok(records)
}

pub fn cleanup_stale_trace_overlay_locks(force: bool) -> Result<TraceOverlayLockCleanupResult> {
    let locks = list_trace_overlay_locks()?;
    let mut removed = Vec::new();
    let mut kept = Vec::new();
    for lock in locks {
        if lock.status != TraceOverlayLockStatus::Stale {
            kept.push(lock);
            continue;
        }
        if !force {
            ensure_trace_overlay_lock_touched_files_clean(&lock)?;
        }
        fs::remove_dir_all(&lock.lock_path).map_err(|e| {
            Error::internal_io(
                format!(
                    "Failed to remove stale trace overlay lock {}: {}",
                    lock.lock_path, e
                ),
                Some("trace.overlay.lock.cleanup".to_string()),
            )
        })?;
        removed.push(lock);
    }
    Ok(TraceOverlayLockCleanupResult { removed, kept })
}

fn trace_overlay_lock_dir() -> Result<PathBuf> {
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
    Ok(lock_dir)
}

fn read_trace_overlay_lock_record(lock_path: &Path) -> TraceOverlayLockRecord {
    let holder = read_trace_overlay_lock_holder(lock_path);
    let status = holder
        .as_ref()
        .map(trace_overlay_lock_status)
        .unwrap_or(TraceOverlayLockStatus::Unknown);
    TraceOverlayLockRecord {
        lock_path: lock_path.to_string_lossy().to_string(),
        status,
        holder,
    }
}

fn trace_overlay_lock_status(holder: &TraceOverlayLockHolder) -> TraceOverlayLockStatus {
    if process_is_alive(holder.pid) {
        TraceOverlayLockStatus::Active
    } else {
        TraceOverlayLockStatus::Stale
    }
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe {
        if libc::kill(pid as libc::pid_t, 0) == 0 {
            return true;
        }
        last_errno() == libc::EPERM
    }
}

#[cfg(all(unix, any(target_os = "linux", target_os = "android")))]
unsafe fn last_errno() -> i32 {
    unsafe { *libc::__errno_location() }
}

#[cfg(all(
    unix,
    any(target_os = "macos", target_os = "ios", target_os = "freebsd")
))]
unsafe fn last_errno() -> i32 {
    unsafe { *libc::__error() }
}

#[cfg(not(unix))]
fn process_is_alive(_pid: u32) -> bool {
    true
}

impl Drop for TraceOverlayLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn normalize_component_path(component_path: &Path) -> PathBuf {
    fs::canonicalize(component_path).unwrap_or_else(|_| component_path.to_path_buf())
}

fn trace_overlay_lock_id(component_path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(component_path.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    hex[..24].to_string()
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

fn read_trace_overlay_lock_holder(lock_path: &Path) -> Option<TraceOverlayLockHolder> {
    let holder_path = lock_path.join("holder.json");
    let content = fs::read_to_string(holder_path).ok()?;
    serde_json::from_str(&content).ok()
}

fn trace_overlay_lock_error(
    component_path: &Path,
    lock_path: &Path,
    run_dir: &RunDir,
    holder: Option<TraceOverlayLockHolder>,
) -> Error {
    let holder_summary = holder
        .as_ref()
        .and_then(trace_overlay_holder_summary)
        .unwrap_or_else(|| "unavailable".to_string());
    let status = holder
        .as_ref()
        .map(trace_overlay_lock_status)
        .unwrap_or(TraceOverlayLockStatus::Unknown);
    let status_label = match status {
        TraceOverlayLockStatus::Active => "active",
        TraceOverlayLockStatus::Stale => "stale",
        TraceOverlayLockStatus::Unknown => "unknown",
    };
    let message = match status {
        TraceOverlayLockStatus::Stale => format!(
            "Trace overlay lock is stale for component path {}. Lock path: {}. Dead holder: {}. Current run directory: {}",
            component_path.display(),
            lock_path.display(),
            holder_summary,
            run_dir.path().display()
        ),
        _ => format!(
            "Trace overlay already active for component path {}. Lock path: {}. Active holder: {}. Current run directory: {}",
            component_path.display(),
            lock_path.display(),
            holder_summary,
            run_dir.path().display()
        ),
    };
    Error::new(
        ErrorCode::ValidationInvalidArgument,
        message,
        serde_json::json!({
            "field": "--overlay",
            "component_path": component_path.to_string_lossy(),
            "lock_path": lock_path.to_string_lossy(),
            "run_dir": run_dir.path().to_string_lossy(),
            "lock_status": status_label,
            "holder": holder,
        }),
    )
    .with_hint("Inspect locks: homeboy trace overlay-locks list")
    .with_hint(
        "Remove stale locks after safety checks: homeboy trace overlay-locks cleanup --stale",
    )
}

fn trace_overlay_holder_summary(holder: &TraceOverlayLockHolder) -> Option<String> {
    let pid = holder.pid;
    Some(
        match (
            Some(holder.run_dir.as_str()),
            Some(holder.acquired_at.as_str()),
        ) {
            (Some(run_dir), Some(acquired_at)) => {
                format!("pid {pid}, run directory {run_dir}, acquired at {acquired_at}")
            }
            (Some(run_dir), None) => format!("pid {pid}, run directory {run_dir}"),
            (None, Some(acquired_at)) => format!("pid {pid}, acquired at {acquired_at}"),
            (None, None) => format!("pid {pid}"),
        },
    )
}

fn trace_overlay_touched_files_for_paths(
    component_path: &Path,
    overlay_paths: &[String],
) -> Result<Vec<String>> {
    let mut touched_files = Vec::new();
    for overlay_path in overlay_paths {
        for touched_file in overlay_touched_files(component_path, Path::new(overlay_path))? {
            if !touched_files.contains(&touched_file) {
                touched_files.push(touched_file);
            }
        }
    }
    Ok(touched_files)
}

fn ensure_trace_overlay_lock_touched_files_clean(lock: &TraceOverlayLockRecord) -> Result<()> {
    let Some(holder) = &lock.holder else {
        return Err(Error::validation_invalid_argument(
            "--stale",
            format!(
                "trace overlay lock {} has no holder metadata; pass --force to remove it",
                lock.lock_path
            ),
            None,
            None,
        ));
    };
    if holder.touched_files.is_empty() {
        return Ok(());
    }
    let component_path = Path::new(&holder.component_path);
    let dirty = trace_overlay_dirty_files(component_path, &holder.touched_files, &lock.lock_path)?;
    if dirty.trim().is_empty() {
        return Ok(());
    }
    Err(Error::validation_invalid_argument(
        "--stale",
        format!(
            "stale trace overlay lock {} touches dirty files; pass --force to remove the lock anyway",
            lock.lock_path
        ),
        Some(dirty),
        None,
    ))
}

#[derive(Debug, Clone)]
struct AppliedTraceOverlay {
    component_path: PathBuf,
    patch_path: PathBuf,
    touched_files: Vec<String>,
    keep: bool,
}

fn apply_trace_overlays(
    component_path: &str,
    overlay_paths: &[String],
    keep: bool,
) -> Result<Vec<AppliedTraceOverlay>> {
    let component_path = PathBuf::from(component_path);
    let mut applied = Vec::new();
    for overlay_path in overlay_paths {
        let patch_path = PathBuf::from(overlay_path);
        let touched_files = match overlay_touched_files(&component_path, &patch_path) {
            Ok(files) => files,
            Err(error) => return cleanup_after_overlay_error(&applied, keep, error),
        };
        if let Err(error) =
            ensure_overlay_targets_clean(&component_path, &patch_path, &touched_files)
        {
            return cleanup_after_overlay_error(&applied, keep, error);
        }
        if let Err(error) = run_git_apply(&component_path, &patch_path, false) {
            return cleanup_after_overlay_error(&applied, keep, error);
        }
        print_trace_overlay("applied", &patch_path, &touched_files, keep);
        applied.push(AppliedTraceOverlay {
            component_path: component_path.clone(),
            patch_path,
            touched_files,
            keep,
        });
    }
    Ok(applied)
}

fn cleanup_after_overlay_error<T>(
    applied: &[AppliedTraceOverlay],
    keep: bool,
    error: Error,
) -> Result<T> {
    if !keep {
        let _ = cleanup_trace_overlays(applied);
    }
    Err(error)
}

fn cleanup_trace_overlays(applied: &[AppliedTraceOverlay]) -> Result<()> {
    for overlay in applied.iter().rev() {
        run_git_apply(&overlay.component_path, &overlay.patch_path, true)?;
        print_trace_overlay(
            "reverted",
            &overlay.patch_path,
            &overlay.touched_files,
            overlay.keep,
        );
    }
    Ok(())
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

fn overlay_touched_files(component_path: &Path, patch_path: &Path) -> Result<Vec<String>> {
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

fn trace_overlay_dirty_files(
    component_path: &Path,
    touched_files: &[String],
    context_path: &str,
) -> Result<String> {
    let mut command = Command::new("git");
    command
        .args(["status", "--porcelain=v1", "--"])
        .args(touched_files)
        .current_dir(component_path);
    let output = command.output().map_err(|e| {
        Error::internal_io(
            format!(
                "Failed to check trace overlay target status for {}: {}",
                context_path, e
            ),
            Some("trace.overlay.status".to_string()),
        )
    })?;
    if !output.status.success() {
        return Err(Error::validation_invalid_argument(
            "--overlay",
            format!("failed to check overlay target status for {}", context_path),
            Some(String::from_utf8_lossy(&output.stderr).to_string()),
            None,
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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
    use std::collections::HashMap;
    use std::fs;
    use std::process::Command;

    use crate::component::{Component, ScopedExtensionConfig};
    use crate::extension::{ExtensionCapability, ExtensionExecutionContext};
    use crate::test_support::with_isolated_home;

    use super::*;

    #[test]
    fn test_build_trace_runner() {
        let _home_env_guard = crate::test_support::home_env_guard();
        let temp = tempfile::tempdir().unwrap();
        let extension_dir = temp.path().join("extension");
        let component_dir = temp.path().join("component");
        fs::create_dir_all(&extension_dir).unwrap();
        fs::create_dir_all(&component_dir).unwrap();
        write_extension_manifest(&extension_dir);
        let script = extension_dir.join("trace-runner.sh");
        fs::write(
            &script,
            r#"#!/usr/bin/env bash
set -euo pipefail
{
  printf 'results=%s\n' "$HOMEBOY_TRACE_RESULTS_FILE"
  printf 'scenario=%s\n' "$HOMEBOY_TRACE_SCENARIO"
  printf 'list=%s\n' "$HOMEBOY_TRACE_LIST_ONLY"
  printf 'artifact=%s\n' "$HOMEBOY_TRACE_ARTIFACT_DIR"
  printf 'run=%s\n' "$HOMEBOY_RUN_DIR"
  printf 'rig=%s\n' "${HOMEBOY_TRACE_RIG_ID:-}"
  printf 'component_path=%s\n' "${HOMEBOY_TRACE_COMPONENT_PATH:-}"
  printf 'extra_workloads=%s\n' "${HOMEBOY_TRACE_EXTRA_WORKLOADS:-}"
} > "$HOMEBOY_TRACE_ARTIFACT_DIR/env.txt"
cat > "$HOMEBOY_TRACE_RESULTS_FILE" <<JSON
{"component_id":"example","scenario_id":"close-window","status":"pass","timeline":[],"assertions":[],"artifacts":[{"label":"env","path":"artifacts/env.txt"}]}
JSON
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).unwrap();
        }

        let component = component_with_extension("example", &component_dir);
        let context = trace_context(&component, &extension_dir);
        let run_dir = RunDir::create().unwrap();
        let args = TraceRunWorkflowArgs {
            component_label: "example".to_string(),
            component_id: "example".to_string(),
            path_override: Some(component_dir.to_string_lossy().to_string()),
            settings: Vec::new(),
            settings_json: Vec::new(),
            scenario_id: "close-window".to_string(),
            json_summary: false,
            rig_id: Some("studio".to_string()),
            overlays: Vec::new(),
            keep_overlay: false,
            extra_workloads: vec![component_dir.join("trace-fixture.trace.mjs")],
            span_definitions: Vec::new(),
            baseline_flags: BaselineFlags {
                baseline: false,
                ignore_baseline: true,
                ratchet: false,
            },
            regression_threshold_percent:
                crate::extension::trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
            regression_min_delta_ms:
                crate::extension::trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
        };

        let output = build_trace_runner(&context, &component, &args, &run_dir, false)
            .unwrap()
            .run()
            .unwrap();
        assert!(output.success);

        let env_dump = fs::read_to_string(run_dir.path().join("artifacts/env.txt")).unwrap();
        assert!(env_dump.contains("scenario=close-window"));
        assert!(env_dump.contains("list=0"));
        assert!(env_dump.contains("rig=studio"));
        assert!(env_dump.contains(&format!("component_path={}", component_dir.display())));
        assert!(env_dump.contains("trace-fixture.trace.mjs"));
        assert!(env_dump.contains("results="));
        assert!(env_dump.contains("artifact="));
        assert!(env_dump.contains("run="));
        run_dir.cleanup();
    }

    #[test]
    fn test_run_trace_list_workflow() {
        let _home_env_guard = crate::test_support::home_env_guard();
        let temp = tempfile::tempdir().unwrap();
        let extension_dir = temp.path().join("extension");
        let component_dir = temp.path().join("component");
        fs::create_dir_all(&extension_dir).unwrap();
        fs::create_dir_all(&component_dir).unwrap();
        write_extension_manifest(&extension_dir);
        let script = extension_dir.join("trace-runner.sh");
        fs::write(
            &script,
            r#"#!/usr/bin/env bash
set -euo pipefail
printf '%s' "$HOMEBOY_TRACE_LIST_ONLY" > "$HOMEBOY_TRACE_ARTIFACT_DIR/list.txt"
cat > "$HOMEBOY_TRACE_RESULTS_FILE" <<JSON
{"component_id":"example","scenarios":[{"id":"close-window"}]}
JSON
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).unwrap();
        }

        let component = component_with_extension("example", &component_dir);
        let context = trace_context(&component, &extension_dir);
        let run_dir = RunDir::create().unwrap();
        let args = TraceRunWorkflowArgs {
            component_label: "example".to_string(),
            component_id: "example".to_string(),
            path_override: Some(component_dir.to_string_lossy().to_string()),
            settings: Vec::new(),
            settings_json: Vec::new(),
            scenario_id: String::new(),
            json_summary: false,
            rig_id: None,
            overlays: Vec::new(),
            keep_overlay: false,
            extra_workloads: Vec::new(),
            span_definitions: Vec::new(),
            baseline_flags: BaselineFlags {
                baseline: false,
                ignore_baseline: true,
                ratchet: false,
            },
            regression_threshold_percent:
                crate::extension::trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
            regression_min_delta_ms:
                crate::extension::trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
        };

        let output = build_trace_runner(&context, &component, &args, &run_dir, true)
            .unwrap()
            .run()
            .unwrap();
        assert!(output.success);
        assert_eq!(
            fs::read_to_string(run_dir.path().join("artifacts/list.txt")).unwrap(),
            "1"
        );
        run_dir.cleanup();
    }

    #[test]
    fn test_run_trace_workflow() {
        let args = TraceRunWorkflowArgs {
            component_label: "example".to_string(),
            component_id: "example".to_string(),
            path_override: Some("/tmp/example".to_string()),
            settings: Vec::new(),
            settings_json: Vec::new(),
            scenario_id: "close-window".to_string(),
            json_summary: false,
            rig_id: None,
            overlays: Vec::new(),
            keep_overlay: false,
            extra_workloads: Vec::new(),
            span_definitions: Vec::new(),
            baseline_flags: BaselineFlags {
                baseline: false,
                ignore_baseline: true,
                ratchet: false,
            },
            regression_threshold_percent:
                crate::extension::trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
            regression_min_delta_ms:
                crate::extension::trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
        };
        let output = RunnerOutput {
            success: false,
            exit_code: 2,
            stdout: String::new(),
            stderr: (0..25)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        };

        let failure = failure_from_output(&args, &output);

        assert_eq!(failure.component_id, "example");
        assert_eq!(failure.scenario_id, "close-window");
        assert_eq!(failure.exit_code, 2);
        assert!(failure.stderr_excerpt.contains("line 24"));
        assert!(!failure.stderr_excerpt.contains("line 0"));
    }

    #[test]
    fn trace_overlay_applies_for_run_and_reverts_afterward() {
        with_isolated_home(|_| {
            let fixture = overlay_fixture(false);
            let run_dir = RunDir::create().unwrap();
            let context = trace_context(&fixture.component, &fixture.extension_dir);

            let result = run_trace_workflow_with_context(
                &context,
                &fixture.component,
                fixture.args,
                &run_dir,
                None,
            )
            .unwrap();

            assert_eq!(result.exit_code, 0);
            assert_eq!(result.overlays.len(), 1);
            assert_eq!(result.overlays[0].touched_files, vec!["scenario.txt"]);
            assert!(!result.overlays[0].kept);
            assert_eq!(
                fs::read_to_string(fixture.component_dir.join("scenario.txt")).unwrap(),
                "base\n"
            );
            run_dir.cleanup();
        });
    }

    #[test]
    fn trace_overlay_dirty_target_file_fails_before_patching() {
        let fixture = overlay_fixture(false);
        fs::write(fixture.component_dir.join("scenario.txt"), "dirty\n").unwrap();

        let err = apply_trace_overlays(
            fixture.component_dir.to_str().unwrap(),
            &[fixture.patch_path.to_string_lossy().to_string()],
            false,
        )
        .unwrap_err();

        assert!(err.message.contains("pre-existing changes"));
        assert_eq!(
            fs::read_to_string(fixture.component_dir.join("scenario.txt")).unwrap(),
            "dirty\n"
        );
    }

    #[test]
    fn trace_overlay_lock_acquisition_releases_on_drop() {
        with_isolated_home(|_| {
            let component_dir = tempfile::tempdir().unwrap();
            let run_dir = RunDir::create().unwrap();
            let lock_path;

            {
                let lock = TraceOverlayLock::acquire(component_dir.path(), &[], &run_dir).unwrap();
                lock_path = lock.path.clone();
                assert!(lock_path.exists());
                assert!(lock_path.join("holder.json").exists());
            }

            assert!(!lock_path.exists());
            run_dir.cleanup();
        });
    }

    #[test]
    fn trace_overlay_lock_contention_fails_fast_with_context() {
        with_isolated_home(|_| {
            let component_dir = tempfile::tempdir().unwrap();
            let first_run_dir = RunDir::create().unwrap();
            let second_run_dir = RunDir::create().unwrap();
            let lock =
                TraceOverlayLock::acquire(component_dir.path(), &[], &first_run_dir).unwrap();

            let err =
                TraceOverlayLock::acquire(component_dir.path(), &[], &second_run_dir).unwrap_err();

            assert!(err.message.contains("Trace overlay already active"));
            assert!(err
                .message
                .contains(&component_dir.path().display().to_string()));
            assert!(err.message.contains(&lock.path.display().to_string()));
            assert!(err
                .message
                .contains(&first_run_dir.path().display().to_string()));
            assert!(err
                .message
                .contains(&second_run_dir.path().display().to_string()));
            assert_eq!(
                err.details["component_path"].as_str(),
                Some(
                    normalize_component_path(component_dir.path())
                        .to_str()
                        .unwrap()
                )
            );
            assert_eq!(
                err.details["lock_path"].as_str(),
                Some(lock.path.to_str().unwrap())
            );
            assert_eq!(err.details["lock_status"].as_str(), Some("active"));

            drop(lock);
            first_run_dir.cleanup();
            second_run_dir.cleanup();
        });
    }

    #[test]
    fn trace_overlay_keep_overlay_leaves_changes_in_place() {
        with_isolated_home(|_| {
            let fixture = overlay_fixture(true);
            let run_dir = RunDir::create().unwrap();
            let context = trace_context(&fixture.component, &fixture.extension_dir);

            let result = run_trace_workflow_with_context(
                &context,
                &fixture.component,
                fixture.args,
                &run_dir,
                None,
            )
            .unwrap();

            assert_eq!(result.exit_code, 0);
            assert_eq!(result.overlays.len(), 1);
            assert!(result.overlays[0].kept);
            assert_eq!(
                fs::read_to_string(fixture.component_dir.join("scenario.txt")).unwrap(),
                "overlay\n"
            );
            run_dir.cleanup();
        });
    }

    #[test]
    fn trace_overlay_run_failure_reverts_patch_and_releases_lock() {
        with_isolated_home(|_| {
            let fixture = overlay_fixture(false);
            write_failing_overlay_runner(&fixture.extension_dir.join("trace-runner.sh"));
            let run_dir = RunDir::create().unwrap();
            let context = trace_context(&fixture.component, &fixture.extension_dir);
            let lock_path = paths::homeboy_data()
                .unwrap()
                .join("trace-overlay-locks")
                .join(format!(
                    "{}.lock",
                    trace_overlay_lock_id(&normalize_component_path(&fixture.component_dir))
                ));

            let result = run_trace_workflow_with_context(
                &context,
                &fixture.component,
                fixture.args,
                &run_dir,
                None,
            )
            .unwrap();

            assert_eq!(result.status, "error");
            assert_eq!(result.exit_code, 7);
            assert_eq!(
                fs::read_to_string(fixture.component_dir.join("scenario.txt")).unwrap(),
                "base\n"
            );
            assert!(!lock_path.exists());
            run_dir.cleanup();
        });
    }

    #[test]
    fn trace_overlay_locks_list_reports_active_holder() {
        with_isolated_home(|_| {
            let component_dir = tempfile::tempdir().unwrap();
            let run_dir = RunDir::create().unwrap();
            let lock = TraceOverlayLock::acquire(component_dir.path(), &[], &run_dir).unwrap();

            let locks = list_trace_overlay_locks().unwrap();

            assert_eq!(locks.len(), 1);
            assert_eq!(locks[0].status, TraceOverlayLockStatus::Active);
            assert_eq!(locks[0].lock_path, lock.path.to_string_lossy());
            assert_eq!(locks[0].holder.as_ref().unwrap().pid, std::process::id());

            drop(lock);
            run_dir.cleanup();
        });
    }

    #[test]
    fn trace_overlay_locks_cleanup_removes_dead_holder_with_clean_checkout() {
        with_isolated_home(|_| {
            let fixture = overlay_fixture(false);
            let lock_path = write_test_overlay_lock(&fixture.component_dir, dead_test_pid());

            let result = cleanup_stale_trace_overlay_locks(false).unwrap();

            assert_eq!(result.removed.len(), 1);
            assert_eq!(result.removed[0].status, TraceOverlayLockStatus::Stale);
            assert!(!lock_path.exists());
        });
    }

    #[test]
    fn trace_overlay_locks_cleanup_refuses_dead_holder_with_dirty_checkout() {
        with_isolated_home(|_| {
            let fixture = overlay_fixture(false);
            let lock_path = write_test_overlay_lock(&fixture.component_dir, dead_test_pid());
            fs::write(fixture.component_dir.join("scenario.txt"), "dirty\n").unwrap();

            let err = cleanup_stale_trace_overlay_locks(false).unwrap_err();

            assert!(err.message.contains("touches dirty files"));
            assert!(lock_path.exists());

            let forced = cleanup_stale_trace_overlay_locks(true).unwrap();
            assert_eq!(forced.removed.len(), 1);
            assert!(!lock_path.exists());
        });
    }

    struct OverlayFixture {
        _temp: tempfile::TempDir,
        component: Component,
        component_dir: std::path::PathBuf,
        extension_dir: std::path::PathBuf,
        patch_path: std::path::PathBuf,
        args: TraceRunWorkflowArgs,
    }

    fn overlay_fixture(keep_overlay: bool) -> OverlayFixture {
        let temp = tempfile::tempdir().unwrap();
        let extension_dir = temp.path().join("extension");
        let component_dir = temp.path().join("component");
        fs::create_dir_all(&extension_dir).unwrap();
        fs::create_dir_all(&component_dir).unwrap();
        write_extension_manifest(&extension_dir);
        write_overlay_runner(&extension_dir.join("trace-runner.sh"));
        fs::write(component_dir.join("scenario.txt"), "base\n").unwrap();
        init_git_repo(&component_dir);
        let patch_path = temp.path().join("overlay.patch");
        fs::write(
            &patch_path,
            r#"--- a/scenario.txt
+++ b/scenario.txt
@@ -1 +1 @@
-base
+overlay
"#,
        )
        .unwrap();
        let component = component_with_extension("example", &component_dir);
        let args = TraceRunWorkflowArgs {
            component_label: "example".to_string(),
            component_id: "example".to_string(),
            path_override: Some(component_dir.to_string_lossy().to_string()),
            settings: Vec::new(),
            settings_json: Vec::new(),
            scenario_id: "overlay".to_string(),
            json_summary: false,
            rig_id: None,
            overlays: vec![patch_path.to_string_lossy().to_string()],
            keep_overlay,
            extra_workloads: Vec::new(),
            span_definitions: Vec::new(),
            baseline_flags: BaselineFlags {
                baseline: false,
                ignore_baseline: true,
                ratchet: false,
            },
            regression_threshold_percent:
                crate::extension::trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
            regression_min_delta_ms:
                crate::extension::trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
        };
        OverlayFixture {
            _temp: temp,
            component,
            component_dir,
            extension_dir,
            patch_path,
            args,
        }
    }

    fn write_overlay_runner(script: &std::path::Path) {
        fs::write(
            script,
            r#"#!/usr/bin/env bash
set -euo pipefail
grep -q '^overlay$' "$HOMEBOY_TRACE_COMPONENT_PATH/scenario.txt"
cat > "$HOMEBOY_TRACE_RESULTS_FILE" <<JSON
{"component_id":"example","scenario_id":"overlay","status":"pass","timeline":[],"assertions":[],"artifacts":[]}
JSON
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(script).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(script, perms).unwrap();
        }
    }

    fn write_failing_overlay_runner(script: &std::path::Path) {
        fs::write(
            script,
            r#"#!/usr/bin/env bash
set -euo pipefail
grep -q '^overlay$' "$HOMEBOY_TRACE_COMPONENT_PATH/scenario.txt"
printf 'intentional trace failure\n' >&2
exit 7
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(script).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(script, perms).unwrap();
        }
    }

    fn init_git_repo(path: &std::path::Path) {
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

    fn git(path: &std::path::Path, args: &[&str]) {
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

    fn write_test_overlay_lock(component_dir: &std::path::Path, pid: u32) -> std::path::PathBuf {
        let component_path = normalize_component_path(component_dir);
        let lock_dir = trace_overlay_lock_dir().unwrap();
        let lock_path = lock_dir.join(format!("{}.lock", trace_overlay_lock_id(&component_path)));
        fs::create_dir_all(&lock_path).unwrap();
        let holder = TraceOverlayLockHolder {
            pid,
            component_path: component_path.to_string_lossy().to_string(),
            run_dir: component_dir.join("run").to_string_lossy().to_string(),
            acquired_at: "2026-05-02T00:00:00Z".to_string(),
            command: "homeboy trace example overlay --overlay overlay.patch".to_string(),
            overlay_paths: vec!["overlay.patch".to_string()],
            touched_files: vec!["scenario.txt".to_string()],
        };
        write_trace_overlay_lock_holder(&lock_path.join("holder.json"), &holder).unwrap();
        lock_path
    }

    fn dead_test_pid() -> u32 {
        999_999
    }

    fn component_with_extension(id: &str, path: &std::path::Path) -> Component {
        let mut extensions = HashMap::new();
        extensions.insert(
            "trace-extension".to_string(),
            ScopedExtensionConfig::default(),
        );
        Component {
            id: id.to_string(),
            local_path: path.to_string_lossy().to_string(),
            extensions: Some(extensions),
            ..Default::default()
        }
    }

    fn trace_context(
        component: &Component,
        extension_dir: &std::path::Path,
    ) -> ExtensionExecutionContext {
        ExtensionExecutionContext {
            component: component.clone(),
            capability: ExtensionCapability::Trace,
            extension_id: "trace-extension".to_string(),
            extension_path: extension_dir.to_path_buf(),
            script_path: "trace-runner.sh".to_string(),
            settings: Vec::new(),
        }
    }

    fn write_extension_manifest(extension_dir: &std::path::Path) {
        fs::write(
            extension_dir.join("extension.json"),
            r#"{
                "name":"Trace Extension",
                "version":"0.0.0",
                "trace":{"extension_script":"trace-runner.sh"}
            }"#,
        )
        .unwrap();
    }
}
