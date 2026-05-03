//! Trace workflows: invoke extension runners, parse JSON, preserve artifacts.

use serde::Serialize;
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
use crate::rig::RigStateSnapshot;

use super::overlay::{
    acquire_trace_overlay_locks, apply_trace_overlays, cleanup_after_overlay_error,
    cleanup_trace_overlays, TraceOverlayRequest,
};

use super::parsing::{
    parse_trace_list_str, parse_trace_results_file, TraceList, TraceResults, TraceSpanDefinition,
};

#[derive(Debug, Clone)]
pub struct TraceRunWorkflowArgs {
    pub component_label: String,
    pub component_id: String,
    pub path_override: Option<String>,
    pub settings: Vec<(String, String)>,
    pub runner_inputs: TraceRunnerInputs,
    pub scenario_id: String,
    pub json_summary: bool,
    pub rig_id: Option<String>,
    pub overlays: Vec<TraceOverlayRequest>,
    pub keep_overlay: bool,
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
    pub runner_inputs: TraceRunnerInputs,
    pub rig_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TraceRunnerInputs {
    pub json_settings: Vec<(String, serde_json::Value)>,
    pub env: Vec<(String, String)>,
    pub workload_paths: Vec<PathBuf>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
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
    let execution_context = match resolve_execution_context(component, ExtensionCapability::Trace) {
        Ok(execution_context) => Some(execution_context),
        Err(error) if trace_is_unclaimed(&error) => None,
        Err(error) => return Err(error),
    };
    run_trace_workflow_with_context(
        execution_context.as_ref(),
        component,
        args,
        run_dir,
        rig_state,
    )
}

fn run_trace_workflow_with_context(
    execution_context: Option<&ExtensionExecutionContext>,
    component: &Component,
    args: TraceRunWorkflowArgs,
    run_dir: &RunDir,
    rig_state: Option<RigStateSnapshot>,
) -> Result<TraceRunWorkflowResult> {
    let component_path = args
        .path_override
        .as_deref()
        .unwrap_or(component.local_path.as_str());
    let _overlay_locks = if args.overlays.is_empty() {
        None
    } else {
        Some(acquire_trace_overlay_locks(&args.overlays, run_dir)?)
    };
    let applied_overlays = apply_trace_overlays(&args.overlays, args.keep_overlay)?;
    let runner_output =
        match build_trace_runner(execution_context, component, &args, run_dir, false) {
            Ok(runner) => runner,
            Err(error) => {
                return cleanup_after_overlay_error(&applied_overlays, args.keep_overlay, error)
            }
        };
    if !args.keep_overlay {
        cleanup_trace_overlays(&applied_overlays)?
    }
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
                variant: overlay.variant,
                component_id: overlay.component_id,
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
    let execution_context = match resolve_execution_context(component, ExtensionCapability::Trace) {
        Ok(execution_context) => Some(execution_context),
        Err(error) if trace_is_unclaimed(&error) => None,
        Err(error) => return Err(error),
    };
    let runner_args = TraceRunWorkflowArgs {
        component_label: args.component_label.clone(),
        component_id: args.component_id,
        path_override: args.path_override,
        settings: args.settings,
        runner_inputs: args.runner_inputs,
        scenario_id: String::new(),
        json_summary: false,
        rig_id: args.rig_id,
        overlays: Vec::new(),
        keep_overlay: false,
        span_definitions: Vec::new(),
        baseline_flags: BaselineFlags {
            baseline: false,
            ignore_baseline: true,
            ratchet: false,
        },
        regression_threshold_percent: super::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
        regression_min_delta_ms: super::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
    };
    let output = build_trace_runner(
        execution_context.as_ref(),
        component,
        &runner_args,
        run_dir,
        true,
    )?;
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
    execution_context: Option<&ExtensionExecutionContext>,
    component: &Component,
    args: &TraceRunWorkflowArgs,
    run_dir: &RunDir,
    list_only: bool,
) -> Result<RunnerOutput> {
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

    let Some(execution_context) = execution_context else {
        return run_generic_trace_runner(component, args, run_dir, &artifact_dir, list_only);
    };

    let mut runner = ExtensionRunner::for_context(execution_context.clone())
        .component(component.clone())
        .path_override(args.path_override.clone())
        .settings(&args.settings)
        .settings_json(&args.runner_inputs.json_settings)
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
    if !args.runner_inputs.workload_paths.is_empty() {
        runner = runner.env(
            "HOMEBOY_TRACE_EXTRA_WORKLOADS",
            &extra_workloads_env_value(&args.runner_inputs.workload_paths)?,
        );
    }
    for (key, value) in &args.runner_inputs.env {
        runner = runner.env(key, value);
    }

    runner.run()
}

pub fn trace_is_unclaimed(error: &Error) -> bool {
    error.code == ErrorCode::ExtensionUnsupported
        || (error.code == ErrorCode::ValidationInvalidArgument
            && error
                .message
                .contains("has no linked extensions that provide trace support"))
}

fn run_generic_trace_runner(
    component: &Component,
    args: &TraceRunWorkflowArgs,
    run_dir: &RunDir,
    artifact_dir: &Path,
    list_only: bool,
) -> Result<RunnerOutput> {
    let component_path = args
        .path_override
        .as_deref()
        .unwrap_or(component.local_path.as_str());
    let workloads =
        discover_generic_trace_workloads(Path::new(component_path), &args.runner_inputs)?;

    if list_only {
        let scenarios = workloads
            .iter()
            .map(|path| {
                serde_json::json!({
                    "id": trace_workload_scenario_id(path),
                    "source": path.to_string_lossy()
                })
            })
            .collect::<Vec<_>>();
        let stdout = serde_json::json!({
            "component_id": args.component_id,
            "scenarios": scenarios
        })
        .to_string();
        return Ok(RunnerOutput {
            exit_code: 0,
            success: true,
            stdout,
            stderr: String::new(),
        });
    }

    let Some(workload) = workloads
        .iter()
        .find(|path| trace_workload_scenario_id(path) == args.scenario_id)
    else {
        return Ok(RunnerOutput {
            exit_code: 3,
            success: false,
            stdout: String::new(),
            stderr: format!("unknown trace scenario {}", args.scenario_id),
        });
    };

    let mut command = generic_trace_workload_command(workload);
    command.current_dir(component_path);
    command.envs(generic_trace_env(
        component,
        args,
        run_dir,
        artifact_dir,
        &workloads,
        list_only,
    )?);
    let output = command.output().map_err(|e| {
        Error::internal_io(
            format!(
                "Failed to run generic trace workload {}: {}",
                workload.display(),
                e
            ),
            Some("trace.generic.run".to_string()),
        )
    })?;

    Ok(RunnerOutput {
        exit_code: output.status.code().unwrap_or(1),
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn discover_generic_trace_workloads(
    component_path: &Path,
    runner_inputs: &TraceRunnerInputs,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for dir in [
        component_path.join("traces"),
        component_path.join("scripts/trace"),
    ] {
        if !dir.is_dir() {
            continue;
        }
        let entries = std::fs::read_dir(&dir).map_err(|e| {
            Error::internal_io(
                format!("Failed to read trace workload dir {}: {}", dir.display(), e),
                Some("trace.generic.discover".to_string()),
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                Error::internal_io(
                    format!(
                        "Failed to read trace workload entry in {}: {}",
                        dir.display(),
                        e
                    ),
                    Some("trace.generic.discover".to_string()),
                )
            })?;
            let path = entry.path();
            if is_generic_trace_workload(&path) {
                paths.push(path);
            }
        }
    }

    paths.extend(runner_inputs.workload_paths.iter().cloned());
    if let Some(extra) = std::env::var_os("HOMEBOY_TRACE_EXTRA_WORKLOADS") {
        paths.extend(std::env::split_paths(&extra));
    }

    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn is_generic_trace_workload(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    (name.ends_with(".trace.mjs") || name.ends_with(".trace.sh") || name.ends_with(".trace.py"))
        || matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("mjs" | "sh" | "py")
        ) && path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            == Some("trace")
}

fn trace_workload_scenario_id(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    for suffix in [".trace.mjs", ".trace.sh", ".trace.py", ".mjs", ".sh", ".py"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            return stripped.to_string();
        }
    }
    name.to_string()
}

fn generic_trace_workload_command(workload: &Path) -> Command {
    match workload.extension().and_then(|ext| ext.to_str()) {
        Some("mjs") => {
            let mut command = Command::new("node");
            command.arg(workload);
            command
        }
        Some("py") => {
            let mut command = Command::new("python3");
            command.arg(workload);
            command
        }
        Some("sh") => {
            let mut command = Command::new("sh");
            command.arg(workload);
            command
        }
        _ => Command::new(workload),
    }
}

fn generic_trace_env(
    component: &Component,
    args: &TraceRunWorkflowArgs,
    run_dir: &RunDir,
    artifact_dir: &Path,
    workloads: &[PathBuf],
    list_only: bool,
) -> Result<Vec<(String, String)>> {
    let component_path = args
        .path_override
        .as_deref()
        .unwrap_or(component.local_path.as_str());
    let mut env = run_dir.legacy_env_vars();
    env.extend([
        (
            "HOMEBOY_EXTENSION_ID".to_string(),
            "generic-shell".to_string(),
        ),
        (
            "HOMEBOY_COMPONENT_ID".to_string(),
            args.component_id.clone(),
        ),
        (
            "HOMEBOY_COMPONENT_PATH".to_string(),
            component_path.to_string(),
        ),
        (
            "HOMEBOY_TRACE_RESULTS_FILE".to_string(),
            run_dir
                .step_file(run_dir::files::TRACE_RESULTS)
                .to_string_lossy()
                .to_string(),
        ),
        (
            "HOMEBOY_TRACE_SCENARIO".to_string(),
            args.scenario_id.clone(),
        ),
        (
            "HOMEBOY_TRACE_ARTIFACT_DIR".to_string(),
            artifact_dir.to_string_lossy().to_string(),
        ),
        (
            "HOMEBOY_TRACE_LIST_ONLY".to_string(),
            if list_only { "1" } else { "0" }.to_string(),
        ),
        (
            "HOMEBOY_TRACE_EXTRA_WORKLOADS".to_string(),
            extra_workloads_env_value(workloads)?,
        ),
    ]);
    if let Some(rig_id) = &args.rig_id {
        env.push(("HOMEBOY_TRACE_RIG_ID".to_string(), rig_id.clone()));
    }
    for (key, value) in &args.runner_inputs.env {
        env.push((key.clone(), value.clone()));
    }
    Ok(env)
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::process::Command;
    use std::sync::{Mutex, OnceLock};

    use crate::component::{Component, ScopedExtensionConfig};
    use crate::extension::{ExtensionCapability, ExtensionExecutionContext};
    use crate::test_support::with_isolated_home;

    use super::*;

    static TRACE_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn with_extra_workloads<T>(value: Option<String>, f: impl FnOnce() -> T) -> T {
        let _guard = TRACE_ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("trace env lock");
        let prior = std::env::var_os("HOMEBOY_TRACE_EXTRA_WORKLOADS");
        match value {
            Some(value) => std::env::set_var("HOMEBOY_TRACE_EXTRA_WORKLOADS", value),
            None => std::env::remove_var("HOMEBOY_TRACE_EXTRA_WORKLOADS"),
        }
        let result = f();
        match prior {
            Some(value) => std::env::set_var("HOMEBOY_TRACE_EXTRA_WORKLOADS", value),
            None => std::env::remove_var("HOMEBOY_TRACE_EXTRA_WORKLOADS"),
        }
        result
    }

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
            runner_inputs: TraceRunnerInputs {
                json_settings: Vec::new(),
                env: Vec::new(),
                workload_paths: vec![component_dir.join("trace-fixture.trace.mjs")],
            },
            scenario_id: "close-window".to_string(),
            json_summary: false,
            rig_id: Some("studio".to_string()),
            overlays: Vec::new(),
            keep_overlay: false,
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

        let output =
            build_trace_runner(Some(&context), &component, &args, &run_dir, false).unwrap();
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
            runner_inputs: TraceRunnerInputs::default(),
            scenario_id: String::new(),
            json_summary: false,
            rig_id: None,
            overlays: Vec::new(),
            keep_overlay: false,
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

        let output = build_trace_runner(Some(&context), &component, &args, &run_dir, true).unwrap();
        assert!(output.success);
        assert_eq!(
            fs::read_to_string(run_dir.path().join("artifacts/list.txt")).unwrap(),
            "1"
        );
        run_dir.cleanup();
    }

    #[test]
    fn generic_trace_discovery_includes_conventions_and_extra_workloads() {
        let temp = tempfile::tempdir().unwrap();
        let component_dir = temp.path().join("component");
        let traces_dir = component_dir.join("traces");
        let scripts_trace_dir = component_dir.join("scripts/trace");
        fs::create_dir_all(&traces_dir).unwrap();
        fs::create_dir_all(&scripts_trace_dir).unwrap();
        fs::write(traces_dir.join("startup.trace.mjs"), "").unwrap();
        fs::write(scripts_trace_dir.join("smoke.py"), "").unwrap();
        let extra = temp.path().join("external.trace.sh");
        fs::write(&extra, "").unwrap();
        let extra_env = std::env::join_paths([extra.as_path()])
            .unwrap()
            .to_string_lossy()
            .to_string();

        with_extra_workloads(Some(extra_env), || {
            let workloads = discover_generic_trace_workloads(
                &component_dir,
                &TraceRunnerInputs {
                    workload_paths: vec![temp.path().join("rig.trace.mjs")],
                    ..TraceRunnerInputs::default()
                },
            )
            .unwrap();
            let scenario_ids = workloads
                .iter()
                .map(|path| trace_workload_scenario_id(path))
                .collect::<Vec<_>>();

            assert!(scenario_ids.contains(&"startup".to_string()));
            assert!(scenario_ids.contains(&"smoke".to_string()));
            assert!(scenario_ids.contains(&"external".to_string()));
            assert!(scenario_ids.contains(&"rig".to_string()));
        });
    }

    #[test]
    fn test_trace_is_unclaimed() {
        let unsupported = Error::new(
            ErrorCode::ExtensionUnsupported,
            "No extension provider configured for component 'example'",
            serde_json::json!({}),
        );
        assert!(trace_is_unclaimed(&unsupported));

        let missing_trace = Error::validation_invalid_argument(
            "extension",
            "Component 'example' has no linked extensions that provide trace support",
            None,
            None,
        );
        assert!(trace_is_unclaimed(&missing_trace));

        let other =
            Error::validation_invalid_argument("extension", "different problem", None, None);
        assert!(!trace_is_unclaimed(&other));
    }

    #[test]
    fn test_run_trace_workflow() {
        let args = TraceRunWorkflowArgs {
            component_label: "example".to_string(),
            component_id: "example".to_string(),
            path_override: Some("/tmp/example".to_string()),
            settings: Vec::new(),
            runner_inputs: TraceRunnerInputs::default(),
            scenario_id: "close-window".to_string(),
            json_summary: false,
            rig_id: None,
            overlays: Vec::new(),
            keep_overlay: false,
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
                Some(&context),
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
            &[TraceOverlayRequest {
                variant: None,
                component_id: Some("example".to_string()),
                component_path: fixture.component_dir.to_string_lossy().to_string(),
                overlay_path: fixture.patch_path.to_string_lossy().to_string(),
            }],
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
    fn trace_overlay_keep_overlay_leaves_changes_in_place() {
        with_isolated_home(|_| {
            let fixture = overlay_fixture(true);
            let run_dir = RunDir::create().unwrap();
            let context = trace_context(&fixture.component, &fixture.extension_dir);

            let result = run_trace_workflow_with_context(
                Some(&context),
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
            let result = run_trace_workflow_with_context(
                Some(&context),
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
            assert!(crate::extension::trace::list_trace_overlay_locks()
                .unwrap()
                .is_empty());
            run_dir.cleanup();
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
            runner_inputs: TraceRunnerInputs::default(),
            scenario_id: "overlay".to_string(),
            json_summary: false,
            rig_id: None,
            overlays: vec![TraceOverlayRequest {
                variant: None,
                component_id: Some("example".to_string()),
                component_path: component_dir.to_string_lossy().to_string(),
                overlay_path: patch_path.to_string_lossy().to_string(),
            }],
            keep_overlay,
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
