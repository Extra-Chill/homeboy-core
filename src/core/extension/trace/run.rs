//! Trace workflows: invoke extension runners, parse JSON, preserve artifacts.

use serde::Serialize;

use crate::component::Component;
use crate::engine::run_dir::{self, RunDir};
use crate::error::{Error, Result};
use crate::extension::{resolve_execution_context, ExtensionCapability, ExtensionExecutionContext};
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
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceRunFailure {
    pub component_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_path: Option<String>,
    pub scenario_id: String,
    pub exit_code: i32,
    pub stderr_tail: String,
}

pub fn run_trace_workflow(
    component: &Component,
    args: TraceRunWorkflowArgs,
    run_dir: &RunDir,
    rig_state: Option<RigStateSnapshot>,
) -> Result<TraceRunWorkflowResult> {
    let execution_context = resolve_execution_context(component, ExtensionCapability::Trace)?;
    let runner_output =
        build_trace_runner(&execution_context, component, &args, run_dir, false)?.run()?;
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
        component_path: args.path_override.clone(),
        scenario_id: args.scenario_id.clone(),
        exit_code: output.exit_code,
        stderr_tail: stderr_tail(&output.stderr),
    }
}

fn stderr_tail(stderr: &str) -> String {
    const MAX_LINES: usize = 20;
    let lines: Vec<&str> = stderr.lines().collect();
    let start = lines.len().saturating_sub(MAX_LINES);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;

    use crate::component::{Component, ScopedExtensionConfig};
    use crate::extension::{ExtensionCapability, ExtensionExecutionContext};

    use super::*;

    #[test]
    fn trace_runner_invokes_extension_script_with_expected_env_vars() {
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
    fn trace_list_mode_sets_list_env_var() {
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
