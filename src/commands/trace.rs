use clap::Args;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use homeboy::component::{Component, ScopedExtensionConfig};
use homeboy::engine::baseline::BaselineFlags;
use homeboy::engine::execution_context::{self, ResolveOptions};
use homeboy::engine::run_dir::RunDir;
use homeboy::extension::trace as extension_trace;
use homeboy::extension::trace::{
    TraceCommandOutput, TraceListWorkflowArgs, TraceRunWorkflowArgs, TraceSpanDefinition,
};
use homeboy::extension::ExtensionCapability;
use homeboy::observation::{
    NewRunRecord, NewTraceRunRecord, NewTraceSpanRecord, ObservationStore, RunStatus,
};
use homeboy::rig::{self, RigSpec};

use super::utils::args::{BaselineArgs, HiddenJsonArgs, PositionalComponentArgs, SettingArgs};
use super::{CmdResult, GlobalArgs};

#[derive(Args, Clone)]
pub struct TraceArgs {
    #[command(flatten)]
    comp: PositionalComponentArgs,

    /// Scenario ID to run, or `list` to discover available scenarios.
    pub scenario: String,

    /// Run trace against a rig-pinned component path after `rig check` passes.
    #[arg(long, value_name = "RIG_ID")]
    pub rig: Option<String>,

    #[command(flatten)]
    pub setting_args: SettingArgs,

    #[command(flatten)]
    pub _json: HiddenJsonArgs,

    /// Print compact machine-readable summary.
    #[arg(long)]
    pub json_summary: bool,

    /// Render a Markdown trace report instead of the JSON envelope.
    #[arg(long, value_parser = ["markdown"])]
    pub report: Option<String>,

    /// Run the same trace scenario multiple times.
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub repeat: usize,

    /// Aggregate repeated trace output.
    #[arg(long, value_parser = ["spans"])]
    pub aggregate: Option<String>,

    /// Add a span definition as `id:source.event:source.event`.
    #[arg(long = "span", value_name = "ID:FROM:TO", value_parser = extension_trace::spans::parse_span_definition)]
    pub spans: Vec<TraceSpanDefinition>,

    #[command(flatten)]
    pub baseline_args: BaselineArgs,

    /// Span regression tolerance as a percentage.
    #[arg(long, value_name = "PERCENT", default_value_t = extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT)]
    pub regression_threshold: f64,

    /// Minimum span slowdown in milliseconds before a regression can fail.
    #[arg(long, value_name = "MS", default_value_t = extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS)]
    pub regression_min_delta_ms: u64,

    /// Apply a patch file for this trace run, then reverse it afterward.
    #[arg(long = "overlay", value_name = "PATCH_FILE")]
    pub overlays: Vec<String>,

    /// Leave overlay changes in place after the trace run.
    #[arg(long)]
    pub keep_overlay: bool,
}

pub fn is_markdown_mode(args: &TraceArgs) -> bool {
    args.report.as_deref() == Some("markdown")
}

pub fn run_markdown(args: TraceArgs, global: &GlobalArgs) -> CmdResult<String> {
    let (output, exit_code) = run(args, global)?;
    match output {
        TraceCommandOutput::Run(run_output) => {
            let Some(results) = run_output.results else {
                return Ok(("# Trace\n\nNo trace results were produced.\n".to_string(), exit_code));
            };
            Ok((extension_trace::render_markdown(&results), exit_code))
        }
        TraceCommandOutput::Summary(summary) => Ok((
            format!(
                "# Trace Summary\n\n- **Component:** `{}`\n- **Status:** `{}`\n- **Exit code:** `{}`\n",
                summary.component, summary.status, summary.exit_code
            ),
            exit_code,
        )),
        TraceCommandOutput::Aggregate(aggregate) => {
            Ok((render_aggregate_markdown(&aggregate), exit_code))
        }
        TraceCommandOutput::List(list) => {
            let mut markdown = format!("# Trace Scenarios: `{}`\n\n", list.component_id);
            for scenario in list.scenarios {
                markdown.push_str(&format!("- `{}`", scenario.id));
                if let Some(summary) = scenario.summary {
                    markdown.push_str(&format!(": {}", summary));
                }
                markdown.push('\n');
            }
            Ok((markdown, exit_code))
        }
    }
}

pub fn run(args: TraceArgs, _global: &GlobalArgs) -> CmdResult<TraceCommandOutput> {
    if args.repeat == 0 {
        return Err(homeboy::Error::validation_invalid_argument(
            "--repeat",
            "repeat must be at least 1",
            None,
            None,
        ));
    }

    if args.scenario == "list" {
        return run_list(args);
    }

    if args.repeat > 1 || args.aggregate.as_deref() == Some("spans") {
        return run_repeat(args);
    }

    let summary_only = args.json_summary;
    let execution = execute_trace_run(args)?;

    Ok(extension_trace::from_main_workflow(
        execution.workflow,
        execution.rig_state,
        summary_only,
    ))
}

struct TraceRunExecution {
    workflow: extension_trace::TraceRunWorkflowResult,
    run_dir: RunDir,
    rig_state: Option<rig::RigStateSnapshot>,
}

fn execute_trace_run(args: TraceArgs) -> homeboy::Result<TraceRunExecution> {
    let rig_context = load_rig_context(args.rig.as_deref())?;
    let effective_id = resolve_component_id(&args.comp, rig_context.as_ref().map(|c| &c.spec))?;
    let path_override = args.comp.path.clone().or_else(|| {
        rig_context
            .as_ref()
            .and_then(|context| rig_component_path(&context.spec, &effective_id))
    });
    let component_override = rig_context
        .as_ref()
        .and_then(|context| rig_component_for_trace(&context.spec, &effective_id));

    let ctx = execution_context::resolve_with_component(
        &ResolveOptions::with_capability_and_json(
            &effective_id,
            path_override.clone(),
            ExtensionCapability::Trace,
            args.setting_args.setting.clone(),
            args.setting_args.setting_json.clone(),
        ),
        component_override,
    )?;
    if let Some(context) = rig_context.as_ref() {
        run_rig_workload_preflight(
            &context.spec,
            ctx.extension_id.as_deref(),
            rig::RigWorkloadKind::Trace,
        )?;
    }

    let rig_state = rig_context
        .as_ref()
        .map(|context| rig::snapshot_state(&context.spec));
    let run_dir = RunDir::create()?;
    let scenario_id = args.scenario.clone();
    let rig_id = args.rig.clone();
    let requested_overlays = args.overlays.clone();
    let component_path_for_observation = path_override
        .clone()
        .unwrap_or_else(|| ctx.component.local_path.clone());
    let observation = ObservationStore::open_initialized().ok().and_then(|store| {
        store
            .start_run(NewRunRecord {
                kind: "trace".to_string(),
                component_id: Some(ctx.component_id.clone()),
                command: Some(std::env::args().collect::<Vec<_>>().join(" ")),
                cwd: std::env::current_dir()
                    .ok()
                    .map(|path| path.to_string_lossy().to_string()),
                homeboy_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                git_sha: homeboy::git::short_head_revision_at(Path::new(
                    &component_path_for_observation,
                )),
                rig_id: rig_id.clone(),
                metadata_json: serde_json::json!({
                    "scenario_id": scenario_id,
                    "component_path": component_path_for_observation,
                    "requested_overlays": requested_overlays,
                    "span_definitions": args.spans.clone(),
                    "baseline": {
                        "baseline": args.baseline_args.baseline,
                        "ignore_baseline": args.baseline_args.ignore_baseline,
                        "ratchet": args.baseline_args.ratchet,
                        "regression_threshold_percent": args.regression_threshold,
                        "regression_min_delta_ms": args.regression_min_delta_ms
                    }
                }),
            })
            .ok()
            .map(|run| ActiveTraceObservation {
                store,
                run_id: run.id,
                component_id: ctx.component_id.clone(),
                rig_id: rig_id.clone(),
                scenario_id: scenario_id.clone(),
            })
    });
    let extra_workloads = rig_context
        .as_ref()
        .and_then(|context| {
            ctx.extension_id.as_deref().map(|id| {
                rig::workloads_for_extension(
                    &context.spec,
                    rig::RigWorkloadKind::Trace,
                    context.package_root.as_deref(),
                    id,
                )
            })
        })
        .unwrap_or_default();
    let workflow = match extension_trace::run_trace_workflow(
        &ctx.component,
        TraceRunWorkflowArgs {
            component_label: effective_id.clone(),
            component_id: ctx.component_id.clone(),
            path_override,
            settings: settings_as_strings(&ctx.settings),
            settings_json: settings_as_json(&ctx.settings),
            scenario_id: args.scenario,
            json_summary: args.json_summary,
            rig_id: args.rig,
            overlays: args.overlays,
            keep_overlay: args.keep_overlay,
            extra_workloads,
            span_definitions: args.spans,
            baseline_flags: BaselineFlags {
                baseline: args.baseline_args.baseline,
                ignore_baseline: args.baseline_args.ignore_baseline,
                ratchet: args.baseline_args.ratchet,
            },
            regression_threshold_percent: args.regression_threshold,
            regression_min_delta_ms: args.regression_min_delta_ms,
        },
        &run_dir,
        rig_state.clone(),
    ) {
        Ok(workflow) => workflow,
        Err(error) => {
            if let Some(observation) = observation.as_ref() {
                persist_trace_workflow_error(observation, &run_dir, &error);
            }
            return Err(error);
        }
    };
    if let Some(observation) = observation.as_ref() {
        persist_trace_workflow_result(observation, &run_dir, &workflow, rig_state.as_ref());
    }

    Ok(TraceRunExecution {
        workflow,
        run_dir,
        rig_state,
    })
}

fn run_repeat(args: TraceArgs) -> CmdResult<TraceCommandOutput> {
    let repeat = args.repeat;
    let scenario_id = args.scenario.clone();
    let mut runs = Vec::new();
    let mut span_samples: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    let mut span_failures: BTreeMap<String, usize> = BTreeMap::new();
    let mut all_span_ids: BTreeSet<String> =
        args.spans.iter().map(|span| span.id.clone()).collect();
    let mut rig_state = None;
    let mut component = None;
    let mut failure_count = 0;

    for index in 1..=repeat {
        let mut run_args = args.clone();
        run_args.repeat = 1;
        run_args.aggregate = None;
        match execute_trace_run(run_args) {
            Ok(execution) => {
                if rig_state.is_none() {
                    rig_state = execution.rig_state.clone();
                }
                if component.is_none() {
                    component = Some(execution.workflow.component.clone());
                }
                let passed =
                    execution.workflow.exit_code == 0 && execution.workflow.status == "pass";
                if !passed {
                    failure_count += 1;
                }
                let artifact_path = execution
                    .run_dir
                    .step_file(homeboy::engine::run_dir::files::TRACE_RESULTS)
                    .to_string_lossy()
                    .to_string();
                let mut seen_span_ids = BTreeSet::new();
                if let Some(results) = execution.workflow.results.as_ref() {
                    for span in &results.span_results {
                        all_span_ids.insert(span.id.clone());
                        seen_span_ids.insert(span.id.clone());
                        if span.status == extension_trace::parsing::TraceSpanStatus::Ok {
                            if let Some(duration) = span.duration_ms {
                                span_samples
                                    .entry(span.id.clone())
                                    .or_default()
                                    .push(duration);
                                continue;
                            }
                        }
                        *span_failures.entry(span.id.clone()).or_default() += 1;
                    }
                    for span_id in all_span_ids.difference(&seen_span_ids) {
                        *span_failures.entry(span_id.clone()).or_default() += 1;
                    }
                } else {
                    for span_id in &all_span_ids {
                        *span_failures.entry(span_id.clone()).or_default() += 1;
                    }
                }
                runs.push(extension_trace::TraceAggregateRunOutput {
                    index,
                    passed,
                    status: execution.workflow.status,
                    exit_code: execution.workflow.exit_code,
                    artifact_path,
                    scenario_id: execution
                        .workflow
                        .results
                        .as_ref()
                        .map(|r| r.scenario_id.clone()),
                    summary: execution
                        .workflow
                        .results
                        .as_ref()
                        .and_then(|r| r.summary.clone()),
                    failure: execution
                        .workflow
                        .failure
                        .as_ref()
                        .map(|failure| failure.stderr_excerpt.clone())
                        .or_else(|| {
                            execution
                                .workflow
                                .results
                                .as_ref()
                                .and_then(|r| r.failure.clone())
                        }),
                });
            }
            Err(error) => {
                failure_count += 1;
                for span_id in &all_span_ids {
                    *span_failures.entry(span_id.clone()).or_default() += 1;
                }
                runs.push(extension_trace::TraceAggregateRunOutput {
                    index,
                    passed: false,
                    status: "error".to_string(),
                    exit_code: 1,
                    artifact_path: String::new(),
                    scenario_id: Some(scenario_id.clone()),
                    summary: None,
                    failure: Some(error.message),
                });
            }
        }
    }

    let spans = all_span_ids
        .into_iter()
        .map(|id| {
            let samples = span_samples.remove(&id).unwrap_or_default();
            let failures = span_failures.remove(&id).unwrap_or(0);
            aggregate_span(id, samples, failures)
        })
        .collect::<Vec<_>>();
    let exit_code = if failure_count == 0 { 0 } else { 1 };
    let output = extension_trace::TraceAggregateOutput {
        command: "trace.aggregate.spans",
        passed: failure_count == 0,
        status: if failure_count == 0 { "pass" } else { "fail" }.to_string(),
        component: component.unwrap_or_else(|| args.comp.component.clone().unwrap_or_default()),
        scenario_id,
        repeat,
        run_count: runs.len(),
        failure_count,
        exit_code,
        rig_state,
        runs,
        spans,
    };

    Ok((TraceCommandOutput::Aggregate(output), exit_code))
}

fn aggregate_span(
    id: String,
    mut durations: Vec<u64>,
    failures: usize,
) -> extension_trace::TraceAggregateSpanOutput {
    durations.sort_unstable();
    let n = durations.len();
    let avg_ms = if n == 0 {
        None
    } else {
        Some(durations.iter().sum::<u64>() as f64 / n as f64)
    };
    extension_trace::TraceAggregateSpanOutput {
        id,
        n,
        min_ms: durations.first().copied(),
        median_ms: median(&durations),
        avg_ms,
        max_ms: durations.last().copied(),
        failures,
    }
}

fn median(values: &[u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let midpoint = values.len() / 2;
    if values.len() % 2 == 1 {
        Some(values[midpoint])
    } else {
        Some((values[midpoint - 1] + values[midpoint]) / 2)
    }
}

fn render_aggregate_markdown(aggregate: &extension_trace::TraceAggregateOutput) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Trace Aggregate: `{}`\n\n",
        aggregate.scenario_id
    ));
    out.push_str(&format!("- **Component:** `{}`\n", aggregate.component));
    out.push_str(&format!("- **Status:** `{}`\n", aggregate.status));
    out.push_str(&format!("- **Runs:** `{}`\n", aggregate.run_count));
    out.push_str(&format!("- **Failures:** `{}`\n", aggregate.failure_count));

    if !aggregate.spans.is_empty() {
        out.push_str("\n## Spans\n\n");
        out.push_str("| Span | n | min | median | avg | max | failures |\n");
        out.push_str("|---|---:|---:|---:|---:|---:|---:|\n");
        for span in &aggregate.spans {
            out.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {} | {} |\n",
                span.id,
                span.n,
                fmt_ms(span.min_ms),
                fmt_ms(span.median_ms),
                span.avg_ms
                    .map(|value| format!("{:.1}ms", value))
                    .unwrap_or_else(|| "-".to_string()),
                fmt_ms(span.max_ms),
                span.failures
            ));
        }
    }

    out.push_str("\n## Run Artifacts\n\n");
    for run in &aggregate.runs {
        out.push_str(&format!(
            "- Run {}: `{}` `{}`\n",
            run.index, run.status, run.artifact_path
        ));
    }
    out
}

fn fmt_ms(value: Option<u64>) -> String {
    value
        .map(|value| format!("{}ms", value))
        .unwrap_or_else(|| "-".to_string())
}

fn run_list(args: TraceArgs) -> CmdResult<TraceCommandOutput> {
    let rig_context = load_rig_context(args.rig.as_deref())?;
    let effective_id = resolve_component_id(&args.comp, rig_context.as_ref().map(|c| &c.spec))?;
    let path_override = args.comp.path.clone().or_else(|| {
        rig_context
            .as_ref()
            .and_then(|context| rig_component_path(&context.spec, &effective_id))
    });
    let component_override = rig_context
        .as_ref()
        .and_then(|context| rig_component_for_trace(&context.spec, &effective_id));

    let ctx = execution_context::resolve_with_component(
        &ResolveOptions::with_capability_and_json(
            &effective_id,
            path_override.clone(),
            ExtensionCapability::Trace,
            args.setting_args.setting.clone(),
            args.setting_args.setting_json.clone(),
        ),
        component_override,
    )?;
    if let Some(context) = rig_context.as_ref() {
        run_rig_workload_preflight(
            &context.spec,
            ctx.extension_id.as_deref(),
            rig::RigWorkloadKind::Trace,
        )?;
    }

    let run_dir = RunDir::create()?;
    let extra_workloads = rig_context
        .as_ref()
        .and_then(|context| {
            ctx.extension_id.as_deref().map(|id| {
                rig::workloads_for_extension(
                    &context.spec,
                    rig::RigWorkloadKind::Trace,
                    context.package_root.as_deref(),
                    id,
                )
            })
        })
        .unwrap_or_default();
    let list = extension_trace::run_trace_list_workflow(
        &ctx.component,
        TraceListWorkflowArgs {
            component_label: effective_id.clone(),
            component_id: ctx.component_id.clone(),
            path_override,
            settings: settings_as_strings(&ctx.settings),
            settings_json: settings_as_json(&ctx.settings),
            rig_id: args.rig,
            extra_workloads,
        },
        &run_dir,
    )?;

    Ok(extension_trace::from_list_workflow(effective_id, list))
}

struct TraceRigContext {
    spec: RigSpec,
    package_root: Option<PathBuf>,
}

fn load_rig_context(rig_id: Option<&str>) -> homeboy::Result<Option<TraceRigContext>> {
    let Some(rig_id) = rig_id else {
        return Ok(None);
    };
    let spec = rig::load(rig_id)?;
    let package_root =
        rig::read_source_metadata(&spec.id).map(|metadata| PathBuf::from(metadata.package_path));
    Ok(Some(TraceRigContext { spec, package_root }))
}

fn run_rig_workload_preflight(
    spec: &RigSpec,
    extension_id: Option<&str>,
    kind: rig::RigWorkloadKind,
) -> homeboy::Result<()> {
    let groups =
        extension_id.and_then(|id| rig::check_groups_for_extension_workloads(spec, kind, id));
    let check = match groups {
        Some(groups) => rig::run_check_groups(spec, &groups)?,
        None => rig::run_check(spec)?,
    };
    if !check.success {
        return Err(homeboy::Error::validation_invalid_argument(
            "--rig",
            format!(
                "rig '{}' check failed; fix the rig before running trace",
                spec.id
            ),
            None,
            None,
        ));
    }
    Ok(())
}

fn resolve_component_id(
    comp: &PositionalComponentArgs,
    rig_spec: Option<&RigSpec>,
) -> homeboy::Result<String> {
    if let Some(id) = comp.id() {
        return Ok(id.to_string());
    }
    if let Some(spec) = rig_spec {
        if spec.components.len() == 1 {
            return Ok(spec.components.keys().next().unwrap().clone());
        }
        return Err(homeboy::Error::validation_invalid_argument(
            "component",
            format!(
                "rig '{}' has multiple components; pass the component id to trace",
                spec.id
            ),
            None,
            None,
        ));
    }
    comp.resolve_id()
}

fn rig_component_path(spec: &RigSpec, component_id: &str) -> Option<String> {
    let component = spec.components.get(component_id)?;
    Some(homeboy::rig::expand::expand_vars(spec, &component.path))
}

fn rig_component_for_trace(spec: &RigSpec, component_id: &str) -> Option<Component> {
    let component = spec.components.get(component_id)?;
    let mut extensions = component.extensions.clone().unwrap_or_default();
    for extension_id in rig::extension_ids_for_workloads(spec, rig::RigWorkloadKind::Trace) {
        extensions
            .entry(extension_id)
            .or_insert_with(ScopedExtensionConfig::default);
    }
    Some(Component {
        id: component_id.to_string(),
        local_path: rig_component_path(spec, component_id)
            .unwrap_or_else(|| component.path.clone()),
        remote_url: component.remote_url.clone(),
        triage_remote_url: component.triage_remote_url.clone(),
        extensions: if extensions.is_empty() {
            None
        } else {
            Some(extensions)
        },
        ..Default::default()
    })
}

fn settings_as_strings(settings: &[(String, serde_json::Value)]) -> Vec<(String, String)> {
    settings
        .iter()
        .filter_map(|(key, value)| match value {
            serde_json::Value::String(s) => Some((key.clone(), s.clone())),
            _ => None,
        })
        .collect()
}

fn settings_as_json(settings: &[(String, serde_json::Value)]) -> Vec<(String, serde_json::Value)> {
    settings
        .iter()
        .filter_map(|(key, value)| match value {
            serde_json::Value::String(_) => None,
            other => Some((key.clone(), other.clone())),
        })
        .collect()
}

struct ActiveTraceObservation {
    store: ObservationStore,
    run_id: String,
    component_id: String,
    rig_id: Option<String>,
    scenario_id: String,
}

fn persist_trace_workflow_result(
    observation: &ActiveTraceObservation,
    run_dir: &RunDir,
    workflow: &extension_trace::TraceRunWorkflowResult,
    rig_state: Option<&rig::RigStateSnapshot>,
) {
    let run_status = trace_run_status(workflow);
    let baseline_status = baseline_status(workflow);
    let results = workflow.results.as_ref();
    let _ = observation.store.record_trace_run(NewTraceRunRecord {
        run_id: observation.run_id.clone(),
        component_id: observation.component_id.clone(),
        rig_id: observation.rig_id.clone(),
        scenario_id: results
            .map(|results| results.scenario_id.clone())
            .unwrap_or_else(|| observation.scenario_id.clone()),
        status: run_status.as_str().to_string(),
        baseline_status: baseline_status.clone(),
        metadata_json: serde_json::json!({
            "status": &workflow.status,
            "exit_code": workflow.exit_code,
            "summary": results.and_then(|results| results.summary.clone()),
            "failure": &workflow.failure,
            "overlays": &workflow.overlays,
            "baseline_comparison": &workflow.baseline_comparison,
            "baseline_status": baseline_status,
            "hints": &workflow.hints,
            "rig_state": rig_state,
            "assertion_count": results.map(|results| results.assertions.len()).unwrap_or(0),
            "artifact_count": results.map(|results| results.artifacts.len()).unwrap_or(0),
            "span_count": results.map(|results| results.span_results.len()).unwrap_or(0),
        }),
    });

    if let Some(results) = results {
        for span in &results.span_results {
            let _ = observation.store.record_trace_span(NewTraceSpanRecord {
                run_id: observation.run_id.clone(),
                span_id: span.id.clone(),
                status: format!("{:?}", span.status).to_ascii_lowercase(),
                duration_ms: span.duration_ms.map(|value| value as f64),
                from_event: Some(span.from.clone()),
                to_event: Some(span.to.clone()),
                metadata_json: serde_json::json!({
                    "from_t_ms": span.from_t_ms,
                    "to_t_ms": span.to_t_ms,
                    "missing": span.missing,
                    "message": &span.message,
                }),
            });
        }
    }

    record_trace_artifacts(&observation.store, &observation.run_id, run_dir, results);
    let _ = observation.store.finish_run(
        &observation.run_id,
        run_status,
        Some(trace_run_finish_metadata(workflow)),
    );
}

fn persist_trace_workflow_error(
    observation: &ActiveTraceObservation,
    run_dir: &RunDir,
    error: &homeboy::Error,
) {
    let error_metadata = serde_json::json!({
        "error": {
            "code": error.code.as_str(),
            "message": &error.message,
            "details": &error.details,
        }
    });
    let _ = observation.store.record_trace_run(NewTraceRunRecord {
        run_id: observation.run_id.clone(),
        component_id: observation.component_id.clone(),
        rig_id: observation.rig_id.clone(),
        scenario_id: observation.scenario_id.clone(),
        status: RunStatus::Error.as_str().to_string(),
        baseline_status: None,
        metadata_json: error_metadata.clone(),
    });
    record_trace_artifacts(&observation.store, &observation.run_id, run_dir, None);
    let _ =
        observation
            .store
            .finish_run(&observation.run_id, RunStatus::Error, Some(error_metadata));
}

fn trace_run_status(workflow: &extension_trace::TraceRunWorkflowResult) -> RunStatus {
    if workflow.failure.is_some() || workflow.status == "error" {
        RunStatus::Error
    } else if workflow.exit_code == 0 && workflow.status == "pass" {
        RunStatus::Pass
    } else {
        RunStatus::Fail
    }
}

fn baseline_status(workflow: &extension_trace::TraceRunWorkflowResult) -> Option<String> {
    workflow.baseline_comparison.as_ref().map(|comparison| {
        if comparison.regression {
            "regression"
        } else if comparison.has_improvements {
            "improvement"
        } else {
            "pass"
        }
        .to_string()
    })
}

fn trace_run_finish_metadata(
    workflow: &extension_trace::TraceRunWorkflowResult,
) -> serde_json::Value {
    serde_json::json!({
        "status": &workflow.status,
        "exit_code": workflow.exit_code,
        "failure": &workflow.failure,
        "overlays": &workflow.overlays,
        "baseline_comparison": &workflow.baseline_comparison,
        "hints": &workflow.hints,
        "results": &workflow.results,
    })
}

fn record_trace_artifacts(
    store: &ObservationStore,
    run_id: &str,
    run_dir: &RunDir,
    results: Option<&extension_trace::TraceResults>,
) {
    let trace_results_path = run_dir.step_file(homeboy::engine::run_dir::files::TRACE_RESULTS);
    record_artifact_if_file(store, run_id, "trace-results", &trace_results_path);
    if let Some(results) = results {
        for artifact in &results.artifacts {
            let path = PathBuf::from(&artifact.path);
            let resolved = if path.is_absolute() {
                path
            } else {
                run_dir.path().join(path)
            };
            record_artifact_if_file(store, run_id, "trace-artifact", &resolved);
        }
    }
}

fn record_artifact_if_file(store: &ObservationStore, run_id: &str, kind: &str, path: &Path) {
    if path.is_file() {
        let _ = store.record_artifact(run_id, kind, path);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;

    use crate::test_support::with_isolated_home;

    use homeboy::component::ScopedExtensionConfig;
    use homeboy::rig::ComponentSpec;

    use super::*;

    #[test]
    fn rig_component_path_and_trace_env_are_threaded() {
        let component_dir = tempfile::TempDir::new().expect("component dir");
        let mut components = HashMap::new();
        let mut extensions = HashMap::new();
        extensions.insert(
            "trace-extension".to_string(),
            ScopedExtensionConfig::default(),
        );
        components.insert(
            "studio".to_string(),
            ComponentSpec {
                path: component_dir.path().to_string_lossy().to_string(),
                remote_url: Some("https://github.com/Automattic/studio".to_string()),
                triage_remote_url: None,
                stack: None,
                branch: None,
                extensions: Some(extensions),
            },
        );
        let spec = RigSpec {
            id: "studio-rig".to_string(),
            components,
            ..serde_json::from_str(r#"{"id":"studio-rig"}"#).unwrap()
        };

        let path = rig_component_path(&spec, "studio").expect("path resolves");
        assert_eq!(path, component_dir.path().to_string_lossy());
        let component = rig_component_for_trace(&spec, "studio").expect("component resolves");
        assert_eq!(component.id, "studio");
        assert_eq!(component.local_path, path);
        assert!(component.extensions.is_some());
    }

    #[test]
    fn rig_component_for_trace_synthesizes_trace_workload_extensions() {
        let rig_spec: RigSpec = serde_json::from_str(
            r#"{
                "id": "studio",
                "components": {
                    "studio": { "path": "/tmp/studio" }
                },
                "trace_workloads": {
                    "nodejs": ["/tmp/create-site.trace.mjs"]
                }
            }"#,
        )
        .expect("parse rig spec");

        let component = rig_component_for_trace(&rig_spec, "studio").expect("component");

        assert!(component
            .extensions
            .as_ref()
            .expect("extensions")
            .contains_key("nodejs"));
    }

    #[test]
    fn rig_trace_list_uses_rig_default_component_and_workloads() {
        with_isolated_home(|home| {
            write_trace_extension(home);
            let component_dir = tempfile::TempDir::new().expect("component dir");
            write_trace_rig(home, "studio-rig", "studio", component_dir.path());

            let (output, exit_code) = run_list(TraceArgs {
                comp: PositionalComponentArgs {
                    component: None,
                    path: None,
                },
                scenario: "list".to_string(),
                rig: Some("studio-rig".to_string()),
                setting_args: SettingArgs::default(),
                _json: HiddenJsonArgs::default(),
                json_summary: false,
                report: None,
                repeat: 1,
                aggregate: None,
                spans: Vec::new(),
                baseline_args: BaselineArgs::default(),
                regression_threshold:
                    extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                regression_min_delta_ms: extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
                overlays: Vec::new(),
                keep_overlay: false,
            })
            .expect("rig trace list should run");

            assert_eq!(exit_code, 0);
            match output {
                TraceCommandOutput::List(result) => {
                    assert_eq!(result.component, "studio");
                    assert_eq!(result.component_id, "studio");
                    assert_eq!(result.count, 2);
                    assert_eq!(result.scenarios[0].id, "studio-app-create-site");
                    let expected_source = format!(
                        "{}/studio-app-create-site.trace.mjs",
                        component_dir.path().display()
                    );
                    assert_eq!(
                        result.scenarios[0].source.as_deref(),
                        Some(expected_source.as_str())
                    );
                }
                _ => panic!("expected list output"),
            }
        });
    }

    #[test]
    fn rig_trace_list_uses_scoped_workload_preflight() {
        with_isolated_home(|home| {
            write_trace_extension(home);
            let component_dir = tempfile::TempDir::new().expect("component dir");
            let rig_dir = home.path().join(".config").join("homeboy").join("rigs");
            fs::create_dir_all(&rig_dir).expect("mkdir rigs");
            fs::write(
                rig_dir.join("studio-rig.json"),
                format!(
                    r#"{{
                        "components": {{
                            "studio": {{ "path": "{}" }}
                        }},
                        "pipeline": {{
                            "check": [
                                {{
                                    "kind": "check",
                                    "label": "desktop app packaged",
                                    "groups": ["desktop-app"],
                                    "command": "true"
                                }},
                                {{
                                    "kind": "check",
                                    "label": "unrelated cli symlink",
                                    "groups": ["cli-dev-copy"],
                                    "command": "false"
                                }}
                            ]
                        }},
                        "trace_workloads": {{ "nodejs": [
                            {{
                                "path": "${{components.studio.path}}/studio-app-create-site.trace.mjs",
                                "check_groups": ["desktop-app"]
                            }}
                        ] }}
                    }}"#,
                    component_dir.path().display()
                ),
            )
            .expect("write rig");

            let (output, exit_code) = run_list(TraceArgs {
                comp: PositionalComponentArgs {
                    component: None,
                    path: None,
                },
                scenario: "list".to_string(),
                rig: Some("studio-rig".to_string()),
                setting_args: SettingArgs::default(),
                _json: HiddenJsonArgs::default(),
                json_summary: false,
                report: None,
                repeat: 1,
                aggregate: None,
                spans: Vec::new(),
                baseline_args: BaselineArgs::default(),
                regression_threshold:
                    extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                regression_min_delta_ms: extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
                overlays: Vec::new(),
                keep_overlay: false,
            })
            .expect("scoped rig trace list should bypass unrelated failed check");

            assert_eq!(exit_code, 0);
            match output {
                TraceCommandOutput::List(result) => {
                    assert_eq!(result.count, 1);
                    assert_eq!(result.scenarios[0].id, "studio-app-create-site");
                }
                _ => panic!("expected list output"),
            }
        });
    }

    #[test]
    fn rig_trace_run_uses_rig_owned_workload_extension_without_component_link() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            write_trace_extension(home);
            let component_dir = tempfile::TempDir::new().expect("component dir");
            write_trace_rig(home, "studio-rig", "studio", component_dir.path());

            let (output, exit_code) = run(
                TraceArgs {
                    comp: PositionalComponentArgs {
                        component: Some("studio".to_string()),
                        path: None,
                    },
                    scenario: "studio-app-create-site".to_string(),
                    rig: Some("studio-rig".to_string()),
                    setting_args: SettingArgs::default(),
                    _json: HiddenJsonArgs::default(),
                    json_summary: false,
                    report: None,
                    repeat: 1,
                    aggregate: None,
                    spans: Vec::new(),
                    baseline_args: BaselineArgs::default(),
                    regression_threshold:
                        extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                    regression_min_delta_ms:
                        extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
                    overlays: Vec::new(),
                    keep_overlay: false,
                },
                &GlobalArgs {},
            )
            .expect("rig trace run should run");

            assert_eq!(exit_code, 0);
            match output {
                TraceCommandOutput::Run(result) => {
                    assert!(result.passed);
                    assert_eq!(result.component, "studio");
                    assert_eq!(
                        result.results.expect("results").scenario_id,
                        "studio-app-create-site"
                    );
                }
                _ => panic!("expected run output"),
            }
        });
    }

    #[test]
    fn trace_run_persists_observation_history() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            write_trace_extension(home);
            let component_dir = tempfile::TempDir::new().expect("component dir");
            write_trace_rig(home, "studio-rig", "studio", component_dir.path());

            let (_output, exit_code) = run(
                TraceArgs {
                    comp: PositionalComponentArgs {
                        component: Some("studio".to_string()),
                        path: None,
                    },
                    scenario: "studio-app-create-site".to_string(),
                    rig: Some("studio-rig".to_string()),
                    setting_args: SettingArgs::default(),
                    _json: HiddenJsonArgs::default(),
                    json_summary: false,
                    report: None,
                    repeat: 1,
                    aggregate: None,
                    spans: Vec::new(),
                    baseline_args: BaselineArgs::default(),
                    regression_threshold:
                        extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                    regression_min_delta_ms:
                        extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
                    overlays: Vec::new(),
                    keep_overlay: false,
                },
                &GlobalArgs {},
            )
            .expect("trace should run");

            assert_eq!(exit_code, 0);
            let store = ObservationStore::open_initialized().expect("store");
            let runs = store
                .list_runs(homeboy::observation::RunListFilter {
                    kind: Some("trace".to_string()),
                    ..Default::default()
                })
                .expect("runs");
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].status, "pass");
            assert_eq!(runs[0].component_id.as_deref(), Some("studio"));
            assert_eq!(runs[0].rig_id.as_deref(), Some("studio-rig"));

            let trace_run = store
                .get_trace_run(&runs[0].id)
                .expect("trace run")
                .expect("trace run row");
            assert_eq!(trace_run.component_id, "studio");
            assert_eq!(trace_run.scenario_id, "studio-app-create-site");
            assert_eq!(trace_run.status, "pass");
            assert_eq!(trace_run.metadata_json["span_count"], 1);

            let spans = store.list_trace_spans(&runs[0].id).expect("spans");
            assert_eq!(spans.len(), 1);
            assert_eq!(spans[0].span_id, "boot_to_ready");
            assert_eq!(spans[0].duration_ms, Some(125.0));

            let artifacts = store.list_artifacts(&runs[0].id).expect("artifacts");
            assert_eq!(artifacts.len(), 2);
            assert!(artifacts
                .iter()
                .any(|artifact| artifact.kind == "trace-results"));
            assert!(artifacts
                .iter()
                .any(|artifact| artifact.kind == "trace-artifact"));
        });
    }

    #[test]
    fn trace_repeat_aggregates_span_timings_and_preserves_artifacts() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            write_trace_extension(home);
            let component_dir = tempfile::TempDir::new().expect("component dir");
            write_trace_rig(home, "studio-rig", "studio", component_dir.path());

            let (output, exit_code) = run(
                TraceArgs {
                    comp: PositionalComponentArgs {
                        component: Some("studio".to_string()),
                        path: None,
                    },
                    scenario: "studio-app-create-site".to_string(),
                    rig: Some("studio-rig".to_string()),
                    setting_args: SettingArgs::default(),
                    _json: HiddenJsonArgs::default(),
                    json_summary: false,
                    report: None,
                    repeat: 3,
                    aggregate: Some("spans".to_string()),
                    spans: Vec::new(),
                    baseline_args: BaselineArgs::default(),
                    regression_threshold:
                        extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                    overlays: Vec::new(),
                    keep_overlay: false,
                },
                &GlobalArgs {},
            )
            .expect("repeat trace should run");

            assert_eq!(exit_code, 0);
            match output {
                TraceCommandOutput::Aggregate(aggregate) => {
                    assert_eq!(aggregate.repeat, 3);
                    assert_eq!(aggregate.run_count, 3);
                    assert_eq!(aggregate.failure_count, 0);
                    assert_eq!(aggregate.spans.len(), 1);
                    let span = &aggregate.spans[0];
                    assert_eq!(span.id, "boot_to_ready");
                    assert_eq!(span.n, 3);
                    assert_eq!(span.min_ms, Some(125));
                    assert_eq!(span.median_ms, Some(125));
                    assert_eq!(span.avg_ms, Some(125.0));
                    assert_eq!(span.max_ms, Some(125));
                    assert_eq!(span.failures, 0);
                    assert!(aggregate
                        .runs
                        .iter()
                        .all(|run| std::path::Path::new(&run.artifact_path).is_file()));
                }
                _ => panic!("expected aggregate output"),
            }
        });
    }

    #[test]
    fn failed_trace_run_persists_observation_history() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            write_trace_extension(home);
            let component_dir = tempfile::TempDir::new().expect("component dir");
            write_trace_rig(home, "studio-rig", "studio", component_dir.path());

            let (_output, exit_code) = run(
                TraceArgs {
                    comp: PositionalComponentArgs {
                        component: Some("studio".to_string()),
                        path: None,
                    },
                    scenario: "missing-scenario".to_string(),
                    rig: Some("studio-rig".to_string()),
                    setting_args: SettingArgs::default(),
                    _json: HiddenJsonArgs::default(),
                    json_summary: false,
                    report: None,
                    repeat: 1,
                    aggregate: None,
                    spans: Vec::new(),
                    baseline_args: BaselineArgs::default(),
                    regression_threshold:
                        extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                    regression_min_delta_ms:
                        extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
                    overlays: Vec::new(),
                    keep_overlay: false,
                },
                &GlobalArgs {},
            )
            .expect("trace command should return structured failure output");

            assert_eq!(exit_code, 3);
            let store = ObservationStore::open_initialized().expect("store");
            let runs = store
                .list_runs(homeboy::observation::RunListFilter {
                    kind: Some("trace".to_string()),
                    ..Default::default()
                })
                .expect("runs");
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].status, "error");

            let trace_run = store
                .get_trace_run(&runs[0].id)
                .expect("trace run")
                .expect("trace run row");
            assert_eq!(trace_run.status, "error");
            assert!(trace_run.metadata_json["failure"]["stderr_excerpt"]
                .as_str()
                .expect("stderr excerpt")
                .contains("unknown scenario missing-scenario"));
        });
    }

    struct XdgGuard(Option<String>);

    impl XdgGuard {
        fn unset() -> Self {
            let prior = std::env::var("XDG_DATA_HOME").ok();
            std::env::remove_var("XDG_DATA_HOME");
            Self(prior)
        }
    }

    impl Drop for XdgGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("XDG_DATA_HOME", value),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }

    fn write_trace_extension(home: &tempfile::TempDir) {
        let extension_dir = home
            .path()
            .join(".config")
            .join("homeboy")
            .join("extensions")
            .join("nodejs");
        fs::create_dir_all(&extension_dir).expect("mkdir extension");
        fs::write(
            extension_dir.join("nodejs.json"),
            r#"{
                "name": "Node.js",
                "version": "0.0.0",
                "trace": { "extension_script": "trace-runner.sh" }
            }"#,
        )
        .expect("write extension manifest");

        let script_path = extension_dir.join("trace-runner.sh");
        fs::write(
            &script_path,
            r#"#!/bin/sh
set -eu
scenario_ids=""
old_ifs="$IFS"
IFS=":"
for workload in ${HOMEBOY_TRACE_EXTRA_WORKLOADS:-}; do
  name="$(basename "$workload")"
  name="${name%%.trace.*}"
  name="${name%.*}"
  if [ -n "$scenario_ids" ]; then
    scenario_ids="$scenario_ids $name"
  else
    scenario_ids="$name"
  fi
done
IFS="$old_ifs"

if [ "$HOMEBOY_TRACE_LIST_ONLY" = "1" ]; then
  cat > "$HOMEBOY_TRACE_RESULTS_FILE" <<JSON
{"component_id":"$HOMEBOY_COMPONENT_ID","scenarios":[
JSON
  comma=""
  old_ifs="$IFS"
  IFS=":"
  for workload in ${HOMEBOY_TRACE_EXTRA_WORKLOADS:-}; do
    name="$(basename "$workload")"
    name="${name%%.trace.*}"
    name="${name%.*}"
    printf '%s{"id":"%s","source":"%s"}' "$comma" "$name" "$workload" >> "$HOMEBOY_TRACE_RESULTS_FILE"
    comma=","
  done
  IFS="$old_ifs"
  printf ']}' >> "$HOMEBOY_TRACE_RESULTS_FILE"
  exit 0
fi

case " $scenario_ids " in
  *" $HOMEBOY_TRACE_SCENARIO "*) ;;
  *) printf 'unknown scenario %s\n' "$HOMEBOY_TRACE_SCENARIO" >&2; exit 3 ;;
esac

cat > "$HOMEBOY_TRACE_RESULTS_FILE" <<JSON
{"component_id":"$HOMEBOY_COMPONENT_ID","scenario_id":"$HOMEBOY_TRACE_SCENARIO","status":"pass","timeline":[{"t_ms":0,"source":"runner","event":"boot"},{"t_ms":125,"source":"runner","event":"ready"}],"span_results":[{"id":"boot_to_ready","from":"runner.boot","to":"runner.ready","status":"ok","duration_ms":125,"from_t_ms":0,"to_t_ms":125}],"assertions":[],"artifacts":[{"label":"trace log","path":"artifacts/trace-log.txt"}]}
JSON
mkdir -p "$HOMEBOY_TRACE_ARTIFACT_DIR"
printf 'trace log\n' > "$HOMEBOY_TRACE_ARTIFACT_DIR/trace-log.txt"
"#,
        )
        .expect("write trace script");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&script_path)
                .expect("script metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script_path, permissions).expect("chmod script");
        }
    }

    fn write_trace_rig(
        home: &tempfile::TempDir,
        rig_id: &str,
        component_id: &str,
        path: &std::path::Path,
    ) {
        let rig_dir = home.path().join(".config").join("homeboy").join("rigs");
        fs::create_dir_all(&rig_dir).expect("mkdir rigs");
        fs::write(
            rig_dir.join(format!("{}.json", rig_id)),
            format!(
                r#"{{
                    "components": {{
                        "{component_id}": {{ "path": "{}" }}
                    }},
                    "trace_workloads": {{ "nodejs": [
                        "${{components.{component_id}.path}}/studio-app-create-site.trace.mjs",
                        "${{components.{component_id}.path}}/studio-list-sites.trace.mjs"
                    ] }}
                }}"#,
                path.display()
            ),
        )
        .expect("write rig");
    }
}
