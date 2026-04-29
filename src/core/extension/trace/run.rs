//! Trace workflows: invoke extension runners, parse JSON, preserve artifacts.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::component::Component;
use crate::engine::run_dir::{self, RunDir};
use crate::error::{Error, Result};
use crate::extension::{
    resolve_execution_context, stderr_tail, ExtensionCapability, ExtensionExecutionContext,
};
use crate::extension::{ExtensionRunner, RunnerOutput};
use crate::rig::RigStateSnapshot;

use super::parsing::{parse_trace_list_str, parse_trace_results_file, TraceList, TraceResults};

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
}

#[derive(Debug, Clone)]
pub struct TraceListWorkflowArgs {
    pub component_label: String,
    pub component_id: String,
    pub path_override: Option<String>,
    pub settings: Vec<(String, String)>,
    pub settings_json: Vec<(String, serde_json::Value)>,
    pub rig_id: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TraceOverlay {
    pub path: String,
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
    let applied_overlays = apply_trace_overlays(component_path, &args.overlays, args.keep_overlay)?;
    let runner = build_trace_runner(execution_context, component, &args, run_dir, false)?;
    let runner_output = runner.run();
    if !args.keep_overlay {
        if let Err(cleanup_error) = cleanup_trace_overlays(&applied_overlays) {
            return Err(cleanup_error);
        }
    }
    let runner_output = runner_output?;
    let results_path = run_dir.step_file(run_dir::files::TRACE_RESULTS);
    let results = if results_path.exists() {
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
                touched_files: overlay.touched_files,
                kept: overlay.keep,
            })
            .collect(),
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

    Ok(runner)
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
    }
    Ok(())
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

    use super::*;

    #[test]
    fn test_build_trace_runner() {
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
        assert!(env_dump.contains("results="));
        assert!(env_dump.contains("artifact="));
        assert!(env_dump.contains("run="));
        run_dir.cleanup();
    }

    #[test]
    fn test_run_trace_list_workflow() {
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
    fn trace_overlay_keep_overlay_leaves_changes_in_place() {
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
