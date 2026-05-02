mod bundle;
mod findings;
mod latest;
mod reconcile;

use std::collections::{BTreeMap, BTreeSet};

use clap::{Args, Subcommand};
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

#[derive(Args)]
pub struct RunsArgs {
    #[command(subcommand)]
    command: RunsCommand,
}

#[derive(Subcommand)]
enum RunsCommand {
    /// List persisted observation runs
    List(RunsListArgs),
    /// Show the latest persisted observation run matching filters
    LatestRun(RunsLatestRunArgs),
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

#[derive(Serialize)]
#[serde(untagged)]
pub enum RunsOutput {
    List(RunsListOutput),
    LatestRun(RunsLatestRunOutput),
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

#[derive(Serialize)]
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

pub fn list_runs(args: RunsListArgs, command: &'static str) -> CmdResult<RunsOutput> {
    let store = ObservationStore::open_initialized()?;
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
        NewFindingRecord, NewRunRecord, NewTraceSpanRecord, RunStatus, TraceSpanRecord,
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
