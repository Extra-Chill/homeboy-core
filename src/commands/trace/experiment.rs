use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use homeboy::engine::run_dir::RunDir;
use homeboy::extension::trace as extension_trace;
use homeboy::rig;

use super::{TraceArgs, TraceRigContext};

pub(super) struct TraceExperimentRunPlan<'a> {
    name: String,
    spec: &'a rig::TraceExperimentSpec,
    context: &'a TraceRigContext,
}

pub(super) fn trace_experiment_plan_for_args<'a>(
    args: &TraceArgs,
    rig_context: Option<&'a TraceRigContext>,
) -> homeboy::Result<Option<TraceExperimentRunPlan<'a>>> {
    let Some(name) = args.experiment.as_deref() else {
        return Ok(None);
    };
    let context = rig_context.ok_or_else(|| {
        homeboy::Error::validation_invalid_argument(
            "--experiment",
            "trace experiment plans require --rig so Homeboy can read rig metadata",
            None,
            None,
        )
    })?;
    let experiment = context
        .rig_spec
        .trace_experiments
        .get(name)
        .ok_or_else(|| {
            let available = context
                .rig_spec
                .trace_experiments
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            homeboy::Error::validation_invalid_argument(
                "--experiment",
                format!(
                    "unknown trace experiment '{}' for rig '{}'",
                    name, context.rig_spec.id
                ),
                Some(format!(
                    "available experiments: {}",
                    if available.is_empty() {
                        "none".to_string()
                    } else {
                        available.join(", ")
                    }
                )),
                None,
            )
        })?;
    Ok(Some(TraceExperimentRunPlan {
        name: name.to_string(),
        spec: experiment,
        context,
    }))
}

pub(super) fn trace_experiment_settings(
    plan: Option<&TraceExperimentRunPlan>,
) -> homeboy::Result<Vec<(String, serde_json::Value)>> {
    let Some(plan) = plan else {
        return Ok(Vec::new());
    };
    plan.spec
        .settings
        .iter()
        .map(|(key, value)| {
            Ok((
                key.clone(),
                match value {
                    serde_json::Value::String(value) => serde_json::Value::String(
                        resolve_trace_experiment_string(plan.context, value),
                    ),
                    other => other.clone(),
                },
            ))
        })
        .collect()
}

pub(super) fn trace_experiment_env(
    plan: Option<&TraceExperimentRunPlan>,
) -> homeboy::Result<Vec<(String, String)>> {
    let Some(plan) = plan else {
        return Ok(Vec::new());
    };
    plan.spec
        .env
        .iter()
        .map(|(key, value)| {
            Ok((
                key.clone(),
                resolve_trace_experiment_string(plan.context, value),
            ))
        })
        .collect()
}

pub(super) fn run_trace_experiment_setup_for_plan(
    plan: Option<&TraceExperimentRunPlan>,
    run_dir: &RunDir,
) -> homeboy::Result<()> {
    let Some(plan) = plan else {
        return Ok(());
    };
    run_trace_experiment_commands(
        plan.context,
        &plan.name,
        "setup",
        &plan.spec.setup,
        &plan.spec.env,
        run_dir,
    )
}

pub(super) fn run_trace_experiment_teardown_for_plan(
    plan: Option<&TraceExperimentRunPlan>,
    run_dir: &RunDir,
) -> homeboy::Result<()> {
    let Some(plan) = plan else {
        return Ok(());
    };
    run_trace_experiment_commands(
        plan.context,
        &plan.name,
        "teardown",
        &plan.spec.teardown,
        &plan.spec.env,
        run_dir,
    )
}

fn run_trace_experiment_commands(
    context: &TraceRigContext,
    experiment_name: &str,
    phase: &str,
    commands: &[rig::TraceExperimentCommandSpec],
    experiment_env: &BTreeMap<String, String>,
    run_dir: &RunDir,
) -> homeboy::Result<()> {
    for command_spec in commands {
        let command_text = resolve_trace_experiment_string(context, &command_spec.command);
        let mut command = Command::new(trace_experiment_shell());
        command.arg("-c").arg(&command_text);
        command.env("HOMEBOY_TRACE_EXPERIMENT", experiment_name);
        command.env("HOMEBOY_TRACE_EXPERIMENT_PHASE", phase);
        command.env("HOMEBOY_RUN_DIR", run_dir.path());
        command.env(
            "HOMEBOY_TRACE_ARTIFACT_DIR",
            run_dir.path().join("artifacts"),
        );
        for (key, value) in experiment_env {
            command.env(key, resolve_trace_experiment_string(context, value));
        }
        for (key, value) in &command_spec.env {
            command.env(key, resolve_trace_experiment_string(context, value));
        }
        if let Some(cwd) = &command_spec.cwd {
            command.current_dir(PathBuf::from(resolve_trace_experiment_string(context, cwd)));
        }
        let status = command.status().map_err(|err| {
            homeboy::Error::validation_invalid_argument(
                "--experiment",
                format!(
                    "trace experiment '{}' {} command failed to spawn: {}",
                    experiment_name, phase, err
                ),
                Some(command_text.clone()),
                None,
            )
        })?;
        if !status.success() {
            return Err(homeboy::Error::validation_invalid_argument(
                "--experiment",
                format!(
                    "trace experiment '{}' {} command exited {}",
                    experiment_name,
                    phase,
                    status.code().unwrap_or(-1)
                ),
                Some(command_text),
                None,
            ));
        }
    }
    Ok(())
}

pub(super) fn collect_trace_experiment_artifacts_for_plan(
    plan: Option<&TraceExperimentRunPlan>,
    run_dir: &RunDir,
    workflow: &mut extension_trace::TraceRunWorkflowResult,
) -> homeboy::Result<()> {
    let Some(plan) = plan else {
        return Ok(());
    };
    collect_trace_experiment_artifacts(plan.context, &plan.name, plan.spec, run_dir, workflow)
}

fn collect_trace_experiment_artifacts(
    context: &TraceRigContext,
    experiment_name: &str,
    experiment: &rig::TraceExperimentSpec,
    run_dir: &RunDir,
    workflow: &mut extension_trace::TraceRunWorkflowResult,
) -> homeboy::Result<()> {
    let Some(results) = workflow.results.as_mut() else {
        return Ok(());
    };
    for (index, artifact) in experiment.artifacts.iter().enumerate() {
        let (label, source) = match artifact {
            rig::TraceExperimentArtifactSpec::Path(path) => (
                Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("experiment artifact")
                    .to_string(),
                path.as_str(),
            ),
            rig::TraceExperimentArtifactSpec::Detailed { label, path } => {
                (label.clone(), path.as_str())
            }
        };
        let source_path = PathBuf::from(resolve_trace_experiment_string(context, source));
        if !source_path.is_file() {
            return Err(homeboy::Error::validation_invalid_argument(
                "--experiment",
                format!(
                    "trace experiment '{}' artifact '{}' does not exist or is not a file",
                    experiment_name,
                    source_path.display()
                ),
                None,
                None,
            ));
        }
        let file_name = source_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("artifact");
        let relative = PathBuf::from("artifacts")
            .join("experiments")
            .join(experiment_name)
            .join(format!("{:02}-{}", index + 1, file_name));
        let destination = run_dir.path().join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                homeboy::Error::internal_io(
                    format!(
                        "Failed to create trace experiment artifact dir {}: {}",
                        parent.display(),
                        err
                    ),
                    Some("trace.experiment.artifact.mkdir".to_string()),
                )
            })?;
        }
        fs::copy(&source_path, &destination).map_err(|err| {
            homeboy::Error::internal_io(
                format!(
                    "Failed to collect trace experiment artifact {} to {}: {}",
                    source_path.display(),
                    destination.display(),
                    err
                ),
                Some("trace.experiment.artifact.copy".to_string()),
            )
        })?;
        results.artifacts.push(extension_trace::TraceArtifact {
            label,
            path: relative.to_string_lossy().to_string(),
        });
    }
    Ok(())
}

#[cfg(unix)]
fn trace_experiment_shell() -> &'static str {
    "/bin/sh"
}

#[cfg(not(unix))]
fn trace_experiment_shell() -> &'static str {
    "sh"
}

fn resolve_trace_experiment_string(context: &TraceRigContext, value: &str) -> String {
    let expanded = rig::expand::expand_vars(&context.rig_spec, value);
    if let Some(root) = context.rig_package_root.as_ref() {
        expanded.replace("${package.root}", &root.to_string_lossy())
    } else {
        expanded
    }
}
