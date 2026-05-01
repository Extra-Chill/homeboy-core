use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use homeboy::observation::{
    ArtifactRecord, ObservationStore, RunListFilter, RunRecord, TraceSpanRecord,
};
use homeboy::Error;

use super::{CmdResult, GlobalArgs};

const DEFAULT_LIMIT: i64 = 20;
const BUNDLE_FORMAT: &str = "homeboy-observations";
const BUNDLE_VERSION: u32 = 1;

#[derive(Args)]
pub struct RunsArgs {
    #[command(subcommand)]
    command: RunsCommand,
}

#[derive(Subcommand)]
enum RunsCommand {
    /// List persisted observation runs
    List(RunsListArgs),
    /// Show one persisted observation run
    Show { run_id: String },
    /// List artifacts recorded for one run
    Artifacts { run_id: String },
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
pub struct RunsExportArgs {
    /// Export one run by id
    #[arg(long, conflicts_with = "since")]
    pub run: Option<String>,
    /// Export runs started within a duration, e.g. 24h, 7d, 30m
    #[arg(long, conflicts_with = "run")]
    pub since: Option<String>,
    /// Output bundle directory. Zip output is intentionally out of scope for v1.
    #[arg(long, value_name = "DIR")]
    pub output: PathBuf,
}

#[derive(Args, Clone)]
pub struct RunsImportArgs {
    /// Bundle directory produced by `homeboy runs export`
    pub input: PathBuf,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum RunsOutput {
    List(RunsListOutput),
    Show(RunsShowOutput),
    Artifacts(RunsArtifactsOutput),
    BenchHistory(BenchHistoryOutput),
    BenchCompare(BenchCompareOutput),
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservationBundleManifest {
    pub format: String,
    pub version: u32,
    pub created_at: String,
    pub homeboy_version: String,
    pub run_count: usize,
    pub artifact_count: usize,
    pub trace_span_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObservationBundle {
    pub manifest: ObservationBundleManifest,
    pub runs: Vec<RunRecord>,
    pub artifacts: Vec<ArtifactRecord>,
    pub trace_spans: Vec<TraceSpanRecord>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ObservationBundleImportSummary {
    pub runs: usize,
    pub artifacts: usize,
    pub trace_spans: usize,
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
        RunsCommand::Show { run_id } => show_run(&run_id),
        RunsCommand::Artifacts { run_id } => artifacts(&run_id),
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

pub fn export_runs(args: RunsExportArgs) -> CmdResult<RunsOutput> {
    if args
        .output
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        return Err(Error::validation_invalid_argument(
            "output",
            "zip output is out of scope for observation bundle v1; pass a directory path",
            Some(args.output.to_string_lossy().to_string()),
            None,
        ));
    }

    let store = ObservationStore::open_initialized()?;
    let runs = if let Some(run_id) = args.run.as_deref() {
        vec![require_run(&store, run_id)?]
    } else if let Some(since) = args.since.as_deref() {
        let threshold = since_threshold(since)?;
        store.list_runs_started_since(&threshold)?
    } else {
        return Err(Error::validation_missing_argument(vec![
            "--run <run-id> or --since <duration>".to_string(),
        ]));
    };

    let bundle = build_bundle(&store, runs)?;
    write_bundle_dir(&args.output, &bundle)?;

    Ok((
        RunsOutput::Export(RunsExportOutput {
            command: "runs.export",
            output: args.output.to_string_lossy().to_string(),
            run_count: bundle.runs.len(),
            artifact_count: bundle.artifacts.len(),
            trace_span_count: bundle.trace_spans.len(),
            manifest: bundle.manifest,
        }),
        0,
    ))
}

pub fn import_runs(args: RunsImportArgs) -> CmdResult<RunsOutput> {
    let bundle = read_bundle_dir(&args.input)?;
    let store = ObservationStore::open_initialized()?;
    for run in &bundle.runs {
        store.import_run(run)?;
    }
    for artifact in &bundle.artifacts {
        store.import_artifact(artifact)?;
    }
    for span in &bundle.trace_spans {
        store.import_trace_span(span)?;
    }

    Ok((
        RunsOutput::Import(RunsImportOutput {
            command: "runs.import",
            input: args.input.to_string_lossy().to_string(),
            imported: ObservationBundleImportSummary {
                runs: bundle.runs.len(),
                artifacts: bundle.artifacts.len(),
                trace_spans: bundle.trace_spans.len(),
            },
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

fn build_bundle(
    store: &ObservationStore,
    runs: Vec<RunRecord>,
) -> homeboy::Result<ObservationBundle> {
    let mut artifacts = Vec::new();
    let mut trace_spans = Vec::new();
    for run in &runs {
        artifacts.extend(store.list_artifacts(&run.id)?);
        trace_spans.extend(store.list_trace_spans(&run.id)?);
    }
    let manifest = ObservationBundleManifest {
        format: BUNDLE_FORMAT.to_string(),
        version: BUNDLE_VERSION,
        created_at: chrono::Utc::now().to_rfc3339(),
        homeboy_version: env!("CARGO_PKG_VERSION").to_string(),
        run_count: runs.len(),
        artifact_count: artifacts.len(),
        trace_span_count: trace_spans.len(),
    };
    Ok(ObservationBundle {
        manifest,
        runs,
        artifacts,
        trace_spans,
    })
}

fn write_bundle_dir(path: &Path, bundle: &ObservationBundle) -> homeboy::Result<()> {
    if path.exists() && !path.is_dir() {
        return Err(Error::validation_invalid_argument(
            "output",
            "observation bundle output must be a directory",
            Some(path.to_string_lossy().to_string()),
            None,
        ));
    }
    fs::create_dir_all(path).map_err(|e| {
        Error::internal_io(
            e.to_string(),
            Some(format!("create observation bundle dir {}", path.display())),
        )
    })?;
    write_json(path.join("manifest.json"), &bundle.manifest)?;
    write_json(path.join("runs.json"), &bundle.runs)?;
    write_json(path.join("artifacts.json"), &bundle.artifacts)?;
    write_json(path.join("trace_spans.json"), &bundle.trace_spans)?;
    Ok(())
}

fn read_bundle_dir(path: &Path) -> homeboy::Result<ObservationBundle> {
    if !path.is_dir() {
        return Err(Error::validation_invalid_argument(
            "input",
            "observation bundle input must be a directory",
            Some(path.to_string_lossy().to_string()),
            None,
        ));
    }
    let manifest: ObservationBundleManifest = read_json(path.join("manifest.json"))?;
    if manifest.format != BUNDLE_FORMAT {
        return Err(Error::validation_invalid_argument(
            "manifest.format",
            format!("expected {BUNDLE_FORMAT}, got {}", manifest.format),
            Some(manifest.format),
            None,
        ));
    }
    if manifest.version != BUNDLE_VERSION {
        return Err(Error::validation_invalid_argument(
            "manifest.version",
            format!(
                "expected version {BUNDLE_VERSION}, got {}",
                manifest.version
            ),
            Some(manifest.version.to_string()),
            None,
        ));
    }

    let runs: Vec<RunRecord> = read_json(path.join("runs.json"))?;
    let artifacts: Vec<ArtifactRecord> = read_json(path.join("artifacts.json"))?;
    let trace_spans: Vec<TraceSpanRecord> = read_json(path.join("trace_spans.json"))?;
    if manifest.run_count != runs.len()
        || manifest.artifact_count != artifacts.len()
        || manifest.trace_span_count != trace_spans.len()
    {
        return Err(Error::validation_invalid_argument(
            "manifest",
            "bundle manifest counts do not match record files",
            Some(path.to_string_lossy().to_string()),
            None,
        ));
    }
    Ok(ObservationBundle {
        manifest,
        runs,
        artifacts,
        trace_spans,
    })
}

fn write_json(path: PathBuf, value: &impl Serialize) -> homeboy::Result<()> {
    let json = serde_json::to_string_pretty(value).map_err(|e| {
        Error::internal_json(e.to_string(), Some(format!("serialize {}", path.display())))
    })?;
    fs::write(&path, json).map_err(|e| {
        Error::internal_io(
            e.to_string(),
            Some(format!("write observation bundle file {}", path.display())),
        )
    })
}

fn read_json<T: for<'de> Deserialize<'de>>(path: PathBuf) -> homeboy::Result<T> {
    let raw = fs::read_to_string(&path).map_err(|e| {
        Error::internal_io(
            e.to_string(),
            Some(format!("read observation bundle file {}", path.display())),
        )
    })?;
    serde_json::from_str(&raw).map_err(|e| {
        Error::validation_invalid_json(
            e,
            Some(format!("parse observation bundle file {}", path.display())),
            Some(raw),
        )
    })
}

fn since_threshold(raw: &str) -> homeboy::Result<String> {
    let duration = parse_duration(raw)?;
    let chrono_duration = chrono::Duration::from_std(duration).map_err(|e| {
        Error::validation_invalid_argument("since", e.to_string(), Some(raw.to_string()), None)
    })?;
    Ok((chrono::Utc::now() - chrono_duration).to_rfc3339())
}

fn parse_duration(raw: &str) -> homeboy::Result<Duration> {
    let trimmed = raw.trim();
    let split = trimmed
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (amount, unit) = trimmed.split_at(split);
    if amount.is_empty() || unit.is_empty() || !unit.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return Err(Error::validation_invalid_argument(
            "since",
            "expected duration like 30m, 24h, or 7d",
            Some(raw.to_string()),
            None,
        ));
    }
    let amount = amount.parse::<u64>().map_err(|_| {
        Error::validation_invalid_argument(
            "since",
            "duration amount must be a positive integer",
            Some(raw.to_string()),
            None,
        )
    })?;
    if amount == 0 {
        return Err(Error::validation_invalid_argument(
            "since",
            "duration amount must be greater than zero",
            Some(raw.to_string()),
            None,
        ));
    }
    let seconds = match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => amount,
        "m" | "min" | "mins" | "minute" | "minutes" => amount * 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => amount * 60 * 60,
        "d" | "day" | "days" => amount * 60 * 60 * 24,
        _ => {
            return Err(Error::validation_invalid_argument(
                "since",
                "duration unit must be one of s, m, h, or d",
                Some(raw.to_string()),
                None,
            ))
        }
    };
    Ok(Duration::from_secs(seconds))
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

fn run_summary(run: RunRecord) -> RunSummary {
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
    use homeboy::observation::{NewRunRecord, NewTraceSpanRecord, RunStatus};
    use homeboy::test_support::with_isolated_home;

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
