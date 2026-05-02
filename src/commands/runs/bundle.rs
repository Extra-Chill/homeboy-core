use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Args;
use serde::{Deserialize, Serialize};

use homeboy::observation::{ArtifactRecord, ObservationStore, RunRecord, TraceSpanRecord};
use homeboy::Error;

use super::{require_run, CmdResult, RunsOutput};

const BUNDLE_FORMAT: &str = "homeboy-observations";
const BUNDLE_VERSION: u32 = 1;

#[derive(Args, Clone)]
pub(super) struct RunsExportArgs {
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
pub(super) struct RunsImportArgs {
    /// Bundle directory produced by `homeboy runs export`
    pub input: PathBuf,
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
struct ObservationBundle {
    manifest: ObservationBundleManifest,
    runs: Vec<RunRecord>,
    artifacts: Vec<ArtifactRecord>,
    trace_spans: Vec<TraceSpanRecord>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ObservationBundleImportSummary {
    pub runs: usize,
    pub artifacts: usize,
    pub trace_spans: usize,
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

pub(super) fn export_runs(args: RunsExportArgs) -> CmdResult<RunsOutput> {
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

pub(super) fn import_runs(args: RunsImportArgs) -> CmdResult<RunsOutput> {
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
