mod bundle;
mod findings;
mod latest;
mod reconcile;

use std::collections::{BTreeMap, BTreeSet};

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::Value;

use homeboy::observation::{ArtifactRecord, ObservationStore, RunListFilter, RunRecord};
use homeboy::Error;

use super::{CmdResult, GlobalArgs};
use bundle::{
    export_runs, import_runs, ObservationBundleImportSummary, ObservationBundleManifest,
    RunsExportArgs, RunsImportArgs,
};
use findings::{RunsFindingOutput, RunsFindingsOutput};
use latest::{RunsLatestFindingOutput, RunsLatestRunArgs, RunsLatestRunOutput};
use reconcile::{reconcile_runs, RunsReconcileArgs, RunsReconcileOutput};

const DEFAULT_LIMIT: i64 = 20;

#[derive(Args, Clone)]
pub struct RunsArgs {
    #[command(subcommand)]
    command: RunsCommand,
}

#[derive(Subcommand, Clone)]
enum RunsCommand {
    /// List persisted observation runs
    List(RunsListArgs),
    /// Show the latest persisted observation run matching filters
    LatestRun(RunsLatestRunArgs),
    /// Compare selected metrics across persisted run history
    Compare(RunsCompareArgs),
    /// Mark orphaned running observation records stale
    Reconcile(RunsReconcileArgs),
    /// Show one persisted observation run
    Show { run_id: String },
    /// List artifacts recorded for one run
    Artifacts { run_id: String },
    /// List findings recorded for one run
    Findings(findings::RunsFindingsArgs),
    /// Show one recorded finding
    Finding { finding_id: String },
    /// Show the latest finding from the latest run matching filters
    LatestFinding(findings::RunsLatestFindingArgs),
    /// Export observation records as an inspectable directory bundle
    Export(RunsExportArgs),
    /// Import an observation bundle into the local observation store
    Import(RunsImportArgs),
}

#[derive(Args, Clone, Default)]
pub struct RunsListArgs {
    /// Run kind: bench, rig, trace, etc.
    #[arg(long)]
    pub kind: Option<String>,
    /// Component ID
    #[arg(long = "component")]
    pub component_id: Option<String>,
    /// Rig ID
    #[arg(long)]
    pub rig: Option<String>,
    /// Run status
    #[arg(long)]
    pub status: Option<String>,
    /// Maximum runs to return
    #[arg(long, default_value_t = DEFAULT_LIMIT)]
    pub limit: i64,
}

#[derive(Args, Clone)]
pub struct RunsCompareArgs {
    /// Run kind: bench, rig, trace, etc.
    #[arg(long, default_value = "bench")]
    pub kind: String,
    /// Component ID
    #[arg(long = "component")]
    pub component_id: Option<String>,
    /// Rig ID
    #[arg(long)]
    pub rig: Option<String>,
    /// Scenario ID for scenario-scoped metrics
    #[arg(long = "scenario")]
    pub scenario_id: Option<String>,
    /// Run status
    #[arg(long)]
    pub status: Option<String>,
    /// Metric to include. Repeat to compare multiple metrics.
    #[arg(long = "metric", default_value = "total_elapsed_ms")]
    pub metrics: Vec<String>,
    /// Maximum runs to inspect
    #[arg(long, default_value_t = DEFAULT_LIMIT)]
    pub limit: i64,
    /// Output format
    #[arg(long, value_enum, default_value_t = RunsCompareFormat::Table)]
    pub format: RunsCompareFormat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum RunsCompareFormat {
    Table,
    Json,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum RunsOutput {
    List(RunsListOutput),
    LatestRun(RunsLatestRunOutput),
    Compare(RunsCompareOutput),
    Show(RunsShowOutput),
    Artifacts(RunsArtifactsOutput),
    Findings(RunsFindingsOutput),
    Finding(RunsFindingOutput),
    LatestFinding(RunsLatestFindingOutput),
    BenchHistory(BenchHistoryOutput),
    BenchCompare(BenchCompareOutput),
    Reconcile(RunsReconcileOutput),
    Export(RunsExportOutput),
    Import(RunsImportOutput),
}

#[derive(Serialize)]
pub struct RunsListOutput {
    pub command: &'static str,
    pub runs: Vec<RunSummary>,
}

#[derive(Serialize)]
pub struct RunsCompareOutput {
    pub command: &'static str,
    pub kind: String,
    pub component_id: Option<String>,
    pub rig_id: Option<String>,
    pub scenario_id: Option<String>,
    pub metrics: Vec<String>,
    pub rows: Vec<RunsCompareRow>,
}

#[derive(Serialize, Clone)]
pub struct RunsCompareRow {
    pub run: RunSummary,
    pub artifact_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario_id: Option<String>,
    pub metrics: BTreeMap<String, Option<f64>>,
}

#[derive(Serialize)]
pub struct RunsShowOutput {
    pub command: &'static str,
    pub run: RunDetail,
}

#[derive(Serialize)]
pub struct RunsArtifactsOutput {
    pub command: &'static str,
    pub run_id: String,
    pub artifacts: Vec<ArtifactRecord>,
}

#[derive(Serialize)]
pub struct BenchHistoryOutput {
    pub command: &'static str,
    pub component_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rig_id: Option<String>,
    pub runs: Vec<RunDetail>,
}

#[derive(Serialize)]
pub struct BenchCompareOutput {
    pub command: &'static str,
    pub from_run: RunSummary,
    pub to_run: RunSummary,
    pub comparisons: Vec<BenchMetricComparison>,
    pub missing: Vec<BenchMissingMetric>,
}

#[derive(Serialize)]
pub struct RunsExportOutput {
    pub command: &'static str,
    pub output: String,
    pub manifest: ObservationBundleManifest,
    pub run_count: usize,
    pub artifact_count: usize,
    pub trace_span_count: usize,
}

#[derive(Serialize)]
pub struct RunsImportOutput {
    pub command: &'static str,
    pub input: String,
    pub imported: ObservationBundleImportSummary,
}

#[derive(Serialize, Clone)]
pub struct RunSummary {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub component_id: Option<String>,
    pub rig_id: Option<String>,
    pub git_sha: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_note: Option<String>,
}

#[derive(Serialize)]
pub struct RunDetail {
    #[serde(flatten)]
    pub summary: RunSummary,
    pub homeboy_version: Option<String>,
    pub metadata: Value,
    pub artifacts: Vec<ArtifactRecord>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct BenchMetricComparison {
    pub scenario_id: String,
    pub metric: String,
    pub from_value: f64,
    pub to_value: f64,
    pub delta: f64,
    pub percent_change: Option<f64>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct BenchMissingMetric {
    pub scenario_id: String,
    pub metric: String,
    pub missing_from: String,
}

pub fn run(args: RunsArgs, _global: &GlobalArgs) -> CmdResult<RunsOutput> {
    match args.command {
        RunsCommand::List(args) => list_runs(args, "runs.list"),
        RunsCommand::LatestRun(args) => latest::latest_run(args),
        RunsCommand::Compare(args) => compare_runs(args),
        RunsCommand::Reconcile(args) => reconcile_runs(args),
        RunsCommand::Show { run_id } => show_run(&run_id),
        RunsCommand::Artifacts { run_id } => artifacts(&run_id),
        RunsCommand::Findings(args) => findings::findings(args),
        RunsCommand::Finding { finding_id } => findings::finding(&finding_id),
        RunsCommand::LatestFinding(args) => findings::latest_finding(args),
        RunsCommand::Export(args) => export_runs(args),
        RunsCommand::Import(args) => import_runs(args),
    }
}

pub fn is_markdown_mode(args: &RunsArgs) -> bool {
    matches!(args.command, RunsCommand::Compare(ref compare) if compare.format == RunsCompareFormat::Table)
}

pub fn run_markdown(args: RunsArgs, global: &GlobalArgs) -> CmdResult<String> {
    let (output, exit_code) = run(args, global)?;
    match output {
        RunsOutput::Compare(output) => Ok((render_compare_table(&output), exit_code)),
        _ => Err(Error::validation_invalid_argument(
            "output_mode",
            "Only `homeboy runs compare --format=table` supports table output",
            None,
            None,
        )),
    }
}

pub fn list_runs(args: RunsListArgs, command: &'static str) -> CmdResult<RunsOutput> {
    let store = ObservationStore::open_initialized()?;
    reconcile::reconcile_owned_stale_running_runs(&store, 1000)?;
    let runs = store
        .list_runs(RunListFilter {
            kind: args.kind,
            component_id: args.component_id,
            status: args.status,
            rig_id: args.rig,
            limit: Some(args.limit),
        })?
        .into_iter()
        .map(run_summary)
        .collect();

    Ok((RunsOutput::List(RunsListOutput { command, runs }), 0))
}

pub fn compare_runs(args: RunsCompareArgs) -> CmdResult<RunsOutput> {
    let store = ObservationStore::open_initialized()?;
    let limit = args.limit.clamp(1, 1000);
    let runs = store.list_runs(RunListFilter {
        kind: Some(args.kind.clone()),
        component_id: args.component_id.clone(),
        status: args.status.clone(),
        rig_id: args.rig.clone(),
        limit: Some(limit),
    })?;

    let mut rows = Vec::new();
    for run in runs {
        if args
            .scenario_id
            .as_deref()
            .is_some_and(|scenario| !run_contains_scenario(&run, scenario))
        {
            continue;
        }

        let artifact_count = store.list_artifacts(&run.id)?.len();
        let scenario_ids = compare_scenarios_for_run(
            &run.metadata_json,
            args.scenario_id.as_deref(),
            &args.metrics,
        );
        for scenario_id in scenario_ids {
            let metrics = args
                .metrics
                .iter()
                .map(|metric| {
                    (
                        metric.clone(),
                        run_metric_value(&run.metadata_json, scenario_id.as_deref(), metric),
                    )
                })
                .collect::<BTreeMap<_, _>>();

            rows.push(RunsCompareRow {
                run: run_summary(run.clone()),
                artifact_count,
                scenario_id,
                metrics,
            });
        }
    }

    Ok((
        RunsOutput::Compare(RunsCompareOutput {
            command: "runs.compare",
            kind: args.kind,
            component_id: args.component_id,
            rig_id: args.rig,
            scenario_id: args.scenario_id,
            metrics: args.metrics,
            rows: rows.into_iter().take(limit as usize).collect(),
        }),
        0,
    ))
}

fn show_run(run_id: &str) -> CmdResult<RunsOutput> {
    let store = ObservationStore::open_initialized()?;
    let run = require_run(&store, run_id)?;
    Ok((
        RunsOutput::Show(RunsShowOutput {
            command: "runs.show",
            run: run_detail(&store, run)?,
        }),
        0,
    ))
}

pub fn artifacts(run_id: &str) -> CmdResult<RunsOutput> {
    let store = ObservationStore::open_initialized()?;
    require_run(&store, run_id)?;
    Ok((
        RunsOutput::Artifacts(RunsArtifactsOutput {
            command: "runs.artifacts",
            run_id: run_id.to_string(),
            artifacts: store.list_artifacts(run_id)?,
        }),
        0,
    ))
}

pub fn bench_history(
    component_id: &str,
    scenario_id: Option<&str>,
    rig_id: Option<&str>,
    limit: i64,
) -> CmdResult<RunsOutput> {
    let store = ObservationStore::open_initialized()?;
    let runs = store
        .list_runs(RunListFilter {
            kind: Some("bench".to_string()),
            component_id: Some(component_id.to_string()),
            rig_id: rig_id.map(str::to_string),
            limit: Some(limit.clamp(1, 1000)),
            ..RunListFilter::default()
        })?
        .into_iter()
        .filter(|run| scenario_id.is_none_or(|scenario| run_contains_scenario(run, scenario)))
        .take(limit.max(1) as usize)
        .map(|run| run_detail(&store, run))
        .collect::<homeboy::Result<Vec<_>>>()?;

    Ok((
        RunsOutput::BenchHistory(BenchHistoryOutput {
            command: "bench.history",
            component_id: component_id.to_string(),
            scenario_id: scenario_id.map(str::to_string),
            rig_id: rig_id.map(str::to_string),
            runs,
        }),
        0,
    ))
}

pub fn bench_compare(from_run_id: &str, to_run_id: &str) -> CmdResult<RunsOutput> {
    let store = ObservationStore::open_initialized()?;
    let from_run = require_run(&store, from_run_id)?;
    let to_run = require_run(&store, to_run_id)?;
    require_kind(&from_run, "bench")?;
    require_kind(&to_run, "bench")?;

    let from_summary = run_summary(from_run.clone());
    let to_summary = run_summary(to_run.clone());
    let from_metrics = bench_numeric_metrics(&from_run.metadata_json);
    let to_metrics = bench_numeric_metrics(&to_run.metadata_json);
    let mut keys = BTreeSet::new();
    keys.extend(from_metrics.keys().cloned());
    keys.extend(to_metrics.keys().cloned());

    let mut comparisons = Vec::new();
    let mut missing = Vec::new();
    for key in keys {
        match (from_metrics.get(&key), to_metrics.get(&key)) {
            (Some(from), Some(to)) => comparisons.push(BenchMetricComparison {
                scenario_id: key.0,
                metric: key.1,
                from_value: *from,
                to_value: *to,
                delta: to - from,
                percent_change: if *from == 0.0 {
                    None
                } else {
                    Some(((to - from) / from) * 100.0)
                },
            }),
            (Some(_), None) => missing.push(BenchMissingMetric {
                scenario_id: key.0,
                metric: key.1,
                missing_from: "to_run".to_string(),
            }),
            (None, Some(_)) => missing.push(BenchMissingMetric {
                scenario_id: key.0,
                metric: key.1,
                missing_from: "from_run".to_string(),
            }),
            (None, None) => {}
        }
    }

    Ok((
        RunsOutput::BenchCompare(BenchCompareOutput {
            command: "bench.compare",
            from_run: from_summary,
            to_run: to_summary,
            comparisons,
            missing,
        }),
        0,
    ))
}

fn require_run(store: &ObservationStore, run_id: &str) -> homeboy::Result<RunRecord> {
    store.get_run(run_id)?.ok_or_else(|| {
        Error::validation_invalid_argument(
            "run_id",
            format!("run record not found: {run_id}"),
            Some(run_id.to_string()),
            None,
        )
    })
}

fn require_kind(run: &RunRecord, expected: &str) -> homeboy::Result<()> {
    if run.kind == expected {
        return Ok(());
    }
    Err(Error::validation_invalid_argument(
        "run_id",
        format!(
            "run {} is kind '{}', expected '{expected}'",
            run.id, run.kind
        ),
        Some(run.id.clone()),
        None,
    ))
}

fn run_detail(store: &ObservationStore, run: RunRecord) -> homeboy::Result<RunDetail> {
    let artifacts = store.list_artifacts(&run.id)?;
    Ok(RunDetail {
        summary: run_summary(run.clone()),
        homeboy_version: run.homeboy_version,
        metadata: run.metadata_json,
        artifacts,
    })
}

pub(crate) fn run_summary(run: RunRecord) -> RunSummary {
    let status_note = reconcile::running_status_note(&run);
    RunSummary {
        id: run.id,
        kind: run.kind,
        status: run.status,
        started_at: run.started_at,
        finished_at: run.finished_at,
        component_id: run.component_id,
        rig_id: run.rig_id,
        git_sha: run.git_sha,
        command: run.command,
        cwd: run.cwd,
        status_note,
    }
}

fn run_contains_scenario(run: &RunRecord, scenario_id: &str) -> bool {
    if run.metadata_json["selected_scenarios"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(scenario_id)))
    {
        return true;
    }
    bench_numeric_metrics(&run.metadata_json)
        .keys()
        .any(|(scenario, _)| scenario == scenario_id)
}

fn compare_scenarios_for_run(
    metadata: &Value,
    scenario_id: Option<&str>,
    metrics: &[String],
) -> Vec<Option<String>> {
    if let Some(scenario_id) = scenario_id {
        return vec![Some(scenario_id.to_string())];
    }

    let bench_metrics = bench_numeric_metrics(metadata);
    let mut scenario_ids = bench_metrics
        .keys()
        .filter(|(_, metric)| metrics.iter().any(|requested| requested == metric))
        .map(|(scenario, _)| scenario.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(Some)
        .collect::<Vec<_>>();

    if metrics
        .iter()
        .any(|metric| top_level_metric_value(metadata, metric).is_some())
    {
        scenario_ids.insert(0, None);
    }

    if scenario_ids.is_empty() {
        vec![None]
    } else {
        scenario_ids
    }
}

fn run_metric_value(metadata: &Value, scenario_id: Option<&str>, metric: &str) -> Option<f64> {
    if let Some(scenario_id) = scenario_id {
        let key = (scenario_id.to_string(), metric.to_string());
        if let Some(value) = bench_numeric_metrics(metadata).get(&key) {
            return Some(*value);
        }
    }

    top_level_metric_value(metadata, metric)
}

fn top_level_metric_value(metadata: &Value, metric: &str) -> Option<f64> {
    dotted_value(metadata, metric)
        .and_then(Value::as_f64)
        .or_else(|| dotted_value(&metadata["results"], metric).and_then(Value::as_f64))
}

fn dotted_value<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn render_compare_table(output: &RunsCompareOutput) -> String {
    let mut out = String::new();
    out.push_str("# Runs Compare\n\n");
    out.push_str(&format!("- **Kind:** `{}`\n", output.kind));
    if let Some(component_id) = &output.component_id {
        out.push_str(&format!("- **Component:** `{component_id}`\n"));
    }
    if let Some(rig_id) = &output.rig_id {
        out.push_str(&format!("- **Rig:** `{rig_id}`\n"));
    }
    if let Some(scenario_id) = &output.scenario_id {
        out.push_str(&format!("- **Scenario:** `{scenario_id}`\n"));
    }

    out.push_str("\n| Run | Status | Started | Git SHA | Rig | Artifacts | Scenario |");
    for metric in &output.metrics {
        out.push_str(&format!(" {} |", escape_table_cell(metric)));
    }
    out.push('\n');
    out.push_str("|---|---|---|---|---|---:|---|");
    for _ in &output.metrics {
        out.push_str("---:|");
    }
    out.push('\n');

    for row in &output.rows {
        let git_sha = row.run.git_sha.as_deref().map(short_sha).unwrap_or("-");
        out.push_str(&format!(
            "| `{}` | `{}` | {} | `{}` | `{}` | {} | `{}` |",
            short_sha(&row.run.id),
            row.run.status,
            row.run.started_at,
            git_sha,
            row.run.rig_id.as_deref().unwrap_or("-"),
            row.artifact_count,
            row.scenario_id.as_deref().unwrap_or("-")
        ));
        for metric in &output.metrics {
            out.push_str(&format!(
                " {} |",
                fmt_metric(row.metrics.get(metric).copied().flatten())
            ));
        }
        out.push('\n');
    }

    out
}

fn short_sha(value: &str) -> &str {
    value.get(..8).unwrap_or(value)
}

fn fmt_metric(value: Option<f64>) -> String {
    value
        .map(|value| {
            if value.fract().abs() < f64::EPSILON {
                format!("{value:.0}")
            } else {
                format!("{value:.3}")
            }
        })
        .unwrap_or_else(|| "-".to_string())
}

fn escape_table_cell(value: &str) -> String {
    value.replace('|', "\\|")
}

fn bench_numeric_metrics(metadata: &Value) -> BTreeMap<(String, String), f64> {
    let mut metrics = BTreeMap::new();
    if let Some(scenarios) = metadata["scenario_metrics"].as_array() {
        for scenario in scenarios {
            collect_scenario_metrics(scenario, &mut metrics);
        }
    }
    if metrics.is_empty() {
        if let Some(scenarios) = metadata["results"]["scenarios"].as_array() {
            for scenario in scenarios {
                collect_scenario_metrics(scenario, &mut metrics);
            }
        }
    }
    metrics
}

fn collect_scenario_metrics(scenario: &Value, metrics: &mut BTreeMap<(String, String), f64>) {
    let Some(scenario_id) = scenario["scenario_id"]
        .as_str()
        .or_else(|| scenario["id"].as_str())
    else {
        return;
    };

    collect_numeric_object(scenario_id, None, &scenario["metrics"], metrics);
    if let Some(groups) = scenario["metric_groups"].as_object() {
        for (group, values) in groups {
            collect_numeric_object(scenario_id, Some(group), values, metrics);
        }
    }
}

fn collect_numeric_object(
    scenario_id: &str,
    prefix: Option<&str>,
    value: &Value,
    metrics: &mut BTreeMap<(String, String), f64>,
) {
    let Some(object) = value.as_object() else {
        return;
    };
    for (name, value) in object {
        let Some(number) = value.as_f64() else {
            continue;
        };
        let metric = match prefix {
            Some(prefix) => format!("{prefix}.{name}"),
            None => name.clone(),
        };
        metrics.insert((scenario_id.to_string(), metric), number);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use homeboy::observation::{
        NewFindingRecord, NewRunRecord, NewTraceSpanRecord, RunRecord, RunStatus, TraceSpanRecord,
    };
    use homeboy::test_support::with_isolated_home;
    use serde::Deserialize;

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

    fn sample_run(kind: &str, component_id: &str, rig_id: &str, metadata: Value) -> NewRunRecord {
        NewRunRecord {
            kind: kind.to_string(),
            component_id: Some(component_id.to_string()),
            command: Some(format!("homeboy {kind} {component_id}")),
            cwd: Some("/tmp/homeboy-fixture".to_string()),
            homeboy_version: Some("test-version".to_string()),
            git_sha: Some("abc123".to_string()),
            rig_id: Some(rig_id.to_string()),
            metadata_json: metadata,
        }
    }

    #[test]
    fn run_list_filters_kind_component_rig_and_status() {
        with_isolated_home(|_home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let bench = store
                .start_run(sample_run("bench", "homeboy", "studio", Value::Null))
                .expect("bench");
            store
                .finish_run(&bench.id, RunStatus::Pass, None)
                .expect("finish bench");
            let trace = store
                .start_run(sample_run("trace", "homeboy", "studio", Value::Null))
                .expect("trace");
            store
                .finish_run(&trace.id, RunStatus::Fail, None)
                .expect("finish trace");

            let (output, _) = list_runs(
                RunsListArgs {
                    kind: Some("bench".to_string()),
                    component_id: Some("homeboy".to_string()),
                    rig: Some("studio".to_string()),
                    status: Some("pass".to_string()),
                    limit: 20,
                },
                "runs.list",
            )
            .expect("list");

            let RunsOutput::List(output) = output else {
                panic!("expected list output");
            };
            assert_eq!(output.runs.len(), 1);
            assert_eq!(output.runs[0].id, bench.id);
        });
    }

    #[test]
    fn run_list_reconciles_owned_dead_running_runs_before_listing() {
        with_isolated_home(|_home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            store
                .import_run(&RunRecord {
                    id: "dead-owned-run".to_string(),
                    kind: "bench".to_string(),
                    component_id: Some("homeboy".to_string()),
                    started_at: "2026-05-02T16:46:46Z".to_string(),
                    finished_at: None,
                    status: "running".to_string(),
                    command: Some("homeboy bench".to_string()),
                    cwd: Some("/tmp/homeboy-fixture".to_string()),
                    homeboy_version: Some("test-version".to_string()),
                    git_sha: Some("abc123".to_string()),
                    rig_id: Some("studio".to_string()),
                    metadata_json: serde_json::json!({
                        "homeboy_run_owner": { "pid": u32::MAX }
                    }),
                })
                .expect("import stale fixture");

            let (output, _) = list_runs(
                RunsListArgs {
                    kind: Some("bench".to_string()),
                    component_id: Some("homeboy".to_string()),
                    rig: Some("studio".to_string()),
                    status: None,
                    limit: 20,
                },
                "runs.list",
            )
            .expect("list");

            let RunsOutput::List(output) = output else {
                panic!("expected list output");
            };
            assert_eq!(output.runs.len(), 1);
            assert_eq!(output.runs[0].id, "dead-owned-run");
            assert_eq!(output.runs[0].status, "stale");
            assert!(output.runs[0].finished_at.is_some());
            assert_eq!(output.runs[0].status_note, None);

            let stored = store
                .get_run("dead-owned-run")
                .expect("get run")
                .expect("run exists");
            assert_eq!(stored.status, "stale");
            assert_eq!(
                stored.metadata_json["homeboy_reconciled"]["reason"],
                "owner_process_not_running"
            );
        });
    }

    #[test]
    fn run_show_includes_metadata_and_artifacts() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let run = store
                .start_run(sample_run(
                    "bench",
                    "homeboy",
                    "studio",
                    serde_json::json!({ "scenario_metrics": [] }),
                ))
                .expect("run");
            let artifact_path = home.path().join("bench-results.json");
            std::fs::write(&artifact_path, b"{}").expect("artifact");
            store
                .record_artifact(&run.id, "bench_results", &artifact_path)
                .expect("record artifact");

            let (output, _) = show_run(&run.id).expect("show");
            let RunsOutput::Show(output) = output else {
                panic!("expected show output");
            };
            assert_eq!(output.run.summary.id, run.id);
            assert_eq!(
                output.run.metadata["scenario_metrics"],
                serde_json::json!([])
            );
            assert_eq!(output.run.artifacts.len(), 1);
            assert_eq!(output.run.artifacts[0].kind, "bench_results");
        });
    }

    #[test]
    fn artifacts_command_reports_paths() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let run = store
                .start_run(sample_run("trace", "homeboy", "studio", Value::Null))
                .expect("run");
            let artifact_path = home.path().join("trace-results.json");
            std::fs::write(&artifact_path, b"{}").expect("artifact");
            store
                .record_artifact(&run.id, "trace_results", &artifact_path)
                .expect("record artifact");

            let (output, _) = artifacts(&run.id).expect("artifacts");
            let RunsOutput::Artifacts(output) = output else {
                panic!("expected artifacts output");
            };
            assert_eq!(output.artifacts.len(), 1);
            assert_eq!(output.artifacts[0].path, artifact_path.to_string_lossy());
        });
    }

    #[test]
    fn artifacts_command_reports_url_artifacts() {
        with_isolated_home(|_home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let run = store
                .start_run(sample_run("bench", "homeboy", "studio", Value::Null))
                .expect("run");
            store
                .record_url_artifact(&run.id, "frontend_url", "https://example.test/")
                .expect("record URL artifact");

            let (output, _) = artifacts(&run.id).expect("artifacts");
            let RunsOutput::Artifacts(output) = output else {
                panic!("expected artifacts output");
            };
            assert_eq!(output.artifacts.len(), 1);
            assert_eq!(output.artifacts[0].kind, "frontend_url");
            assert_eq!(output.artifacts[0].artifact_type, "url");
            assert_eq!(
                output.artifacts[0].url.as_deref(),
                Some("https://example.test/")
            );
        });
    }

    #[test]
    fn findings_commands_list_and_show_records() {
        with_isolated_home(|_home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let run = store
                .start_run(sample_run("lint", "homeboy", "studio", Value::Null))
                .expect("run");
            let recorded = store
                .record_finding(&NewFindingRecord {
                    run_id: run.id.clone(),
                    tool: "lint".to_string(),
                    rule: Some("security".to_string()),
                    file: Some("src/foo.php".to_string()),
                    line: Some(12),
                    severity: Some("error".to_string()),
                    fingerprint: Some("src/foo.php::security".to_string()),
                    message: "Missing escaping".to_string(),
                    fixable: Some(true),
                    metadata_json: serde_json::json!({ "category": "security" }),
                })
                .expect("finding");

            let (output, _) = findings::findings(findings::RunsFindingsArgs {
                run_id: run.id,
                tool: Some("lint".to_string()),
                file: Some("src/foo.php".to_string()),
                fingerprint: None,
                limit: 20,
            })
            .expect("list findings");
            let RunsOutput::Findings(output) = output else {
                panic!("expected findings output");
            };
            assert_eq!(output.findings.len(), 1);
            assert_eq!(output.findings[0].id, recorded.id);
            assert_eq!(output.findings[0].message, "Missing escaping");

            let (output, _) = findings::finding(&recorded.id).expect("show finding");
            let RunsOutput::Finding(output) = output else {
                panic!("expected finding output");
            };
            assert_eq!(output.finding.metadata_json["category"], "security");
            assert_eq!(output.finding.fixable, Some(true));
        });
    }

    #[test]
    fn latest_run_command_returns_newest_matching_run() {
        with_isolated_home(|_home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let old = store
                .start_run(sample_run("lint", "homeboy", "studio", Value::Null))
                .expect("old");
            store
                .finish_run(&old.id, RunStatus::Pass, None)
                .expect("finish old");
            let latest = store
                .start_run(sample_run("lint", "homeboy", "studio", Value::Null))
                .expect("latest");
            store
                .finish_run(&latest.id, RunStatus::Fail, None)
                .expect("finish latest");

            let (output, _) = latest::latest_run(latest::RunsLatestRunArgs {
                kind: Some("lint".to_string()),
                component_id: Some("homeboy".to_string()),
                rig: Some("studio".to_string()),
                status: None,
            })
            .expect("latest run");

            let RunsOutput::LatestRun(output) = output else {
                panic!("expected latest run output");
            };
            assert_eq!(output.command, "runs.latest-run");
            assert_eq!(output.run.id, latest.id);
        });
    }

    #[test]
    fn latest_finding_command_uses_latest_matching_run() {
        with_isolated_home(|_home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let old_run = store
                .start_run(sample_run("lint", "homeboy", "studio", Value::Null))
                .expect("old run");
            store
                .record_finding(&NewFindingRecord {
                    run_id: old_run.id.clone(),
                    tool: "lint".to_string(),
                    rule: Some("security".to_string()),
                    file: Some("src/foo.php".to_string()),
                    line: Some(12),
                    severity: Some("error".to_string()),
                    fingerprint: Some("old".to_string()),
                    message: "Old finding".to_string(),
                    fixable: Some(true),
                    metadata_json: serde_json::json!({}),
                })
                .expect("old finding");
            let latest_run = store
                .start_run(sample_run("lint", "homeboy", "studio", Value::Null))
                .expect("latest run");
            let latest_finding = store
                .record_finding(&NewFindingRecord {
                    run_id: latest_run.id.clone(),
                    tool: "lint".to_string(),
                    rule: Some("security".to_string()),
                    file: Some("src/foo.php".to_string()),
                    line: Some(12),
                    severity: Some("error".to_string()),
                    fingerprint: Some("latest".to_string()),
                    message: "Latest finding".to_string(),
                    fixable: Some(true),
                    metadata_json: serde_json::json!({}),
                })
                .expect("latest finding");

            let (output, _) = findings::latest_finding(findings::RunsLatestFindingArgs {
                kind: Some("lint".to_string()),
                component_id: Some("homeboy".to_string()),
                rig: Some("studio".to_string()),
                status: None,
                tool: Some("lint".to_string()),
                file: Some("src/foo.php".to_string()),
            })
            .expect("latest finding command");

            let RunsOutput::LatestFinding(output) = output else {
                panic!("expected latest finding output");
            };
            assert_eq!(output.command, "runs.latest-finding");
            assert_eq!(output.run.id, latest_run.id);
            assert_eq!(output.finding.id, latest_finding.id);
            assert_eq!(output.finding.message, "Latest finding");
        });
    }

    #[test]
    fn bench_history_orders_and_filters_by_scenario() {
        with_isolated_home(|_home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let old = store
                .start_run(sample_run(
                    "bench",
                    "homeboy",
                    "studio",
                    serde_json::json!({
                        "scenario_metrics": [{
                            "scenario_id": "cold",
                            "metrics": { "p95_ms": 10.0 }
                        }]
                    }),
                ))
                .expect("old");
            store
                .finish_run(&old.id, RunStatus::Pass, None)
                .expect("finish old");
            let new = store
                .start_run(sample_run(
                    "bench",
                    "homeboy",
                    "studio",
                    serde_json::json!({
                        "scenario_metrics": [{
                            "scenario_id": "cold",
                            "metrics": { "p95_ms": 12.0 }
                        }]
                    }),
                ))
                .expect("new");
            store
                .finish_run(&new.id, RunStatus::Pass, None)
                .expect("finish new");

            let (output, _) =
                bench_history("homeboy", Some("cold"), Some("studio"), 20).expect("history");
            let RunsOutput::BenchHistory(output) = output else {
                panic!("expected history output");
            };
            assert_eq!(output.runs.len(), 2);
            assert_eq!(output.runs[0].summary.id, new.id);
            assert_eq!(output.runs[1].summary.id, old.id);
        });
    }

    #[test]
    fn bench_compare_reports_deltas_and_missing_metrics() {
        with_isolated_home(|_home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let from = store
                .start_run(sample_run(
                    "bench",
                    "homeboy",
                    "studio",
                    serde_json::json!({
                        "scenario_metrics": [{
                            "scenario_id": "cold",
                            "metrics": { "p95_ms": 100.0, "only_from": 1.0 },
                            "metric_groups": { "warm": { "mean_ms": 50.0 } }
                        }]
                    }),
                ))
                .expect("from");
            let to = store
                .start_run(sample_run(
                    "bench",
                    "homeboy",
                    "studio",
                    serde_json::json!({
                        "scenario_metrics": [{
                            "scenario_id": "cold",
                            "metrics": { "p95_ms": 125.0, "only_to": 2.0 },
                            "metric_groups": { "warm": { "mean_ms": 40.0 } }
                        }]
                    }),
                ))
                .expect("to");

            let (output, _) = bench_compare(&from.id, &to.id).expect("compare");
            let RunsOutput::BenchCompare(output) = output else {
                panic!("expected compare output");
            };
            let p95 = output
                .comparisons
                .iter()
                .find(|row| row.metric == "p95_ms")
                .expect("p95 row");
            assert_eq!(p95.delta, 25.0);
            assert_eq!(p95.percent_change, Some(25.0));
            assert!(output
                .comparisons
                .iter()
                .any(|row| row.metric == "warm.mean_ms" && row.delta == -10.0));
            assert!(output
                .missing
                .iter()
                .any(|row| row.metric == "only_from" && row.missing_from == "to_run"));
            assert!(output
                .missing
                .iter()
                .any(|row| row.metric == "only_to" && row.missing_from == "from_run"));
        });
    }

    #[test]
    fn runs_compare_filters_history_and_reports_selected_metrics() {
        with_isolated_home(|_home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let old = store
                .start_run(sample_run(
                    "bench",
                    "studio",
                    "studio-bfb",
                    serde_json::json!({
                        "results": { "total_elapsed_ms": 177754.0 },
                        "scenario_metrics": [{
                            "scenario_id": "site-build",
                            "metrics": { "p95_ms": 90.0 }
                        }]
                    }),
                ))
                .expect("old");
            store
                .finish_run(&old.id, RunStatus::Pass, None)
                .expect("finish old");
            let new = store
                .start_run(sample_run(
                    "bench",
                    "studio",
                    "studio-bfb",
                    serde_json::json!({
                        "results": { "total_elapsed_ms": 213151.0 },
                        "scenario_metrics": [{
                            "scenario_id": "site-build",
                            "metrics": { "p95_ms": 120.0 }
                        }]
                    }),
                ))
                .expect("new");
            store
                .finish_run(&new.id, RunStatus::Fail, None)
                .expect("finish new");
            let other = store
                .start_run(sample_run("trace", "studio", "studio-bfb", Value::Null))
                .expect("trace");
            store
                .finish_run(&other.id, RunStatus::Pass, None)
                .expect("finish trace");

            let (output, _) = compare_runs(RunsCompareArgs {
                kind: "bench".to_string(),
                component_id: Some("studio".to_string()),
                rig: Some("studio-bfb".to_string()),
                scenario_id: Some("site-build".to_string()),
                status: None,
                metrics: vec!["total_elapsed_ms".to_string(), "p95_ms".to_string()],
                limit: 20,
                format: RunsCompareFormat::Json,
            })
            .expect("compare");

            let RunsOutput::Compare(output) = output else {
                panic!("expected compare output");
            };
            assert_eq!(output.rows.len(), 2);
            assert_eq!(output.rows[0].run.id, new.id);
            assert_eq!(output.rows[0].metrics["total_elapsed_ms"], Some(213151.0));
            assert_eq!(output.rows[0].metrics["p95_ms"], Some(120.0));
            assert_eq!(output.rows[1].run.id, old.id);
        });
    }

    #[test]
    fn runs_compare_table_renders_metric_columns() {
        let output = RunsCompareOutput {
            command: "runs.compare",
            kind: "bench".to_string(),
            component_id: Some("studio".to_string()),
            rig_id: Some("studio-bfb".to_string()),
            scenario_id: None,
            metrics: vec!["total_elapsed_ms".to_string()],
            rows: vec![RunsCompareRow {
                run: RunSummary {
                    id: "38f271b9-0000".to_string(),
                    kind: "bench".to_string(),
                    status: "pass".to_string(),
                    started_at: "2026-05-02T00:00:00Z".to_string(),
                    finished_at: None,
                    component_id: Some("studio".to_string()),
                    rig_id: Some("studio-bfb".to_string()),
                    git_sha: Some("abcdef123456".to_string()),
                    command: None,
                    cwd: None,
                    status_note: None,
                },
                artifact_count: 3,
                scenario_id: None,
                metrics: BTreeMap::from([("total_elapsed_ms".to_string(), Some(213151.0))]),
            }],
        };

        let table = render_compare_table(&output);
        assert!(table.contains(
            "| Run | Status | Started | Git SHA | Rig | Artifacts | Scenario | total_elapsed_ms |"
        ));
        assert!(table.contains("| `38f271b9` | `pass`"));
        assert!(table.contains("| 213151 |"));
    }

    #[test]
    fn missing_and_mismatched_run_ids_return_clear_errors() {
        with_isolated_home(|_home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let trace = store
                .start_run(sample_run("trace", "homeboy", "studio", Value::Null))
                .expect("trace");

            let missing = show_run("missing-run").err().expect("missing should fail");
            assert_eq!(missing.code.as_str(), "validation.invalid_argument");
            assert!(missing.message.contains("run record not found"));

            let mismatch = bench_compare(&trace.id, &trace.id)
                .err()
                .expect("kind mismatch should fail");
            assert_eq!(mismatch.code.as_str(), "validation.invalid_argument");
            assert!(mismatch.message.contains("expected 'bench'"));
        });
    }

    #[test]
    fn export_one_run_writes_directory_bundle() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let run = store
                .start_run(sample_run("bench", "homeboy", "studio", Value::Null))
                .expect("run");
            store
                .finish_run(&run.id, RunStatus::Pass, None)
                .expect("finish");
            let output = home.path().join("bundle");

            let (result, _) = export_runs(RunsExportArgs {
                run: Some(run.id.clone()),
                since: None,
                output: output.clone(),
            })
            .expect("export");

            let RunsOutput::Export(result) = result else {
                panic!("expected export output");
            };
            assert_eq!(result.run_count, 1);
            assert!(output.join("manifest.json").exists());
            assert!(output.join("runs.json").exists());
            assert!(output.join("artifacts.json").exists());
            assert!(output.join("trace_spans.json").exists());
            let runs: Vec<RunRecord> = read_bundle_test_json(&output.join("runs.json"));
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].id, run.id);
        });
    }

    #[test]
    fn export_since_writes_multiple_runs() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let first = store
                .start_run(sample_run("bench", "homeboy", "studio", Value::Null))
                .expect("first");
            let second = store
                .start_run(sample_run("trace", "homeboy", "studio", Value::Null))
                .expect("second");
            let output = home.path().join("recent-bundle");

            export_runs(RunsExportArgs {
                run: None,
                since: Some("1d".to_string()),
                output: output.clone(),
            })
            .expect("export recent");

            let runs: Vec<RunRecord> = read_bundle_test_json(&output.join("runs.json"));
            let ids = runs
                .iter()
                .map(|run| run.id.clone())
                .collect::<BTreeSet<_>>();
            assert_eq!(ids, BTreeSet::from([first.id, second.id]));
        });
    }

    #[test]
    fn export_artifacts_is_metadata_only() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let run = store
                .start_run(sample_run("bench", "homeboy", "studio", Value::Null))
                .expect("run");
            let artifact_path = home.path().join("bench-results.json");
            std::fs::write(&artifact_path, br#"{"ok":true}"#).expect("artifact");
            let artifact = store
                .record_artifact(&run.id, "bench_results", &artifact_path)
                .expect("record artifact");
            let output = home.path().join("artifact-bundle");

            export_runs(RunsExportArgs {
                run: Some(run.id),
                since: None,
                output: output.clone(),
            })
            .expect("export");

            let artifacts: Vec<ArtifactRecord> =
                read_bundle_test_json(&output.join("artifacts.json"));
            assert_eq!(artifacts, vec![artifact]);
            assert!(!output.join("files").exists());
        });
    }

    #[test]
    fn export_trace_spans_when_present() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let run = store
                .start_run(sample_run("trace", "homeboy", "studio", Value::Null))
                .expect("run");
            let span = store
                .record_trace_span(NewTraceSpanRecord {
                    run_id: run.id.clone(),
                    span_id: "boot".to_string(),
                    status: "ok".to_string(),
                    duration_ms: Some(12.5),
                    from_event: Some("start".to_string()),
                    to_event: Some("ready".to_string()),
                    metadata_json: serde_json::json!({ "phase": "cold" }),
                })
                .expect("span");
            let output = home.path().join("trace-bundle");

            export_runs(RunsExportArgs {
                run: Some(run.id),
                since: None,
                output: output.clone(),
            })
            .expect("export");

            let spans: Vec<TraceSpanRecord> =
                read_bundle_test_json(&output.join("trace_spans.json"));
            assert_eq!(spans, vec![span]);
        });
    }

    #[test]
    fn import_into_empty_db_and_reimport_is_idempotent() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            let bundle = home.path().join("portable-bundle");
            let run_id = {
                let store = ObservationStore::open_initialized().expect("store");
                let run = store
                    .start_run(sample_run("trace", "homeboy", "studio", Value::Null))
                    .expect("run");
                let artifact_path = home.path().join("trace.json");
                std::fs::write(&artifact_path, b"{}").expect("artifact");
                store
                    .record_artifact(&run.id, "trace_results", &artifact_path)
                    .expect("artifact record");
                store
                    .record_trace_span(NewTraceSpanRecord {
                        run_id: run.id.clone(),
                        span_id: "first".to_string(),
                        status: "ok".to_string(),
                        duration_ms: Some(1.0),
                        from_event: None,
                        to_event: Some("done".to_string()),
                        metadata_json: serde_json::json!({}),
                    })
                    .expect("span");
                export_runs(RunsExportArgs {
                    run: Some(run.id.clone()),
                    since: None,
                    output: bundle.clone(),
                })
                .expect("export");
                run.id
            };
            std::fs::remove_file(home.path().join(".local/share/homeboy/homeboy.sqlite"))
                .expect("remove db");

            import_runs(RunsImportArgs {
                input: bundle.clone(),
            })
            .expect("import");
            import_runs(RunsImportArgs {
                input: bundle.clone(),
            })
            .expect("second import is idempotent");

            let store = ObservationStore::open_initialized().expect("store");
            assert!(store.get_run(&run_id).expect("get").is_some());
            assert_eq!(store.list_artifacts(&run_id).expect("artifacts").len(), 1);
            assert_eq!(store.list_trace_spans(&run_id).expect("spans").len(), 1);
        });
    }

    #[test]
    fn malformed_bundle_validation_fails_clearly() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            let bundle = home.path().join("bad-bundle");
            std::fs::create_dir_all(&bundle).expect("bundle dir");
            std::fs::write(bundle.join("manifest.json"), "not json").expect("manifest");

            let err = match import_runs(RunsImportArgs { input: bundle }) {
                Ok(_) => panic!("malformed bundle should fail"),
                Err(err) => err,
            };

            assert_eq!(err.code.as_str(), "validation.invalid_json");
        });
    }

    #[test]
    fn conflicting_existing_rows_fail_clearly() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            let store = ObservationStore::open_initialized().expect("store");
            let run = store
                .start_run(sample_run("bench", "homeboy", "studio", Value::Null))
                .expect("run");
            let bundle = home.path().join("conflict-bundle");
            export_runs(RunsExportArgs {
                run: Some(run.id.clone()),
                since: None,
                output: bundle.clone(),
            })
            .expect("export");
            let mut runs: Vec<RunRecord> = read_bundle_test_json(&bundle.join("runs.json"));
            runs[0].status = "pass".to_string();
            std::fs::write(
                bundle.join("runs.json"),
                serde_json::to_string_pretty(&runs).expect("json"),
            )
            .expect("rewrite runs");

            let err = match import_runs(RunsImportArgs { input: bundle }) {
                Ok(_) => panic!("conflicting import should fail"),
                Err(err) => err,
            };

            assert_eq!(err.code.as_str(), "validation.invalid_argument");
            assert!(err
                .message
                .contains("conflicts with imported bundle record"));
        });
    }

    fn read_bundle_test_json<T: for<'de> Deserialize<'de>>(path: &Path) -> T {
        serde_json::from_str(&std::fs::read_to_string(path).expect("read json")).expect("json")
    }
}
