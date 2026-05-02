use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use homeboy::extension::trace as extension_trace;
use homeboy::extension::trace::TraceCommandOutput;

use super::TraceArgs;
use crate::commands::CmdResult;

#[derive(Deserialize)]
pub(super) struct TraceAggregateInput {
    pub(super) component: Option<String>,
    pub(super) scenario_id: Option<String>,
    #[serde(default)]
    pub(super) phase_preset: Option<String>,
    #[serde(default)]
    pub(super) repeat: Option<usize>,
    #[serde(default)]
    pub(super) rig_state: Option<Value>,
    #[serde(default)]
    pub(super) overlays: Vec<TraceOverlayInput>,
    #[serde(default)]
    pub(super) runs: Vec<TraceAggregateRunInput>,
    pub(super) spans: Vec<TraceAggregateSpanInput>,
}

#[derive(Deserialize)]
struct TraceAggregateEnvelopeInput {
    data: TraceAggregateInput,
}

#[derive(Deserialize)]
pub(super) struct TraceAggregateSpanInput {
    pub(super) id: String,
    pub(super) n: usize,
    pub(super) median_ms: Option<u64>,
    pub(super) avg_ms: Option<f64>,
    #[serde(default)]
    pub(super) max_ms: Option<u64>,
    #[serde(default)]
    pub(super) max_run_index: Option<usize>,
    #[serde(default)]
    pub(super) max_artifact_path: Option<String>,
    pub(super) failures: usize,
}

#[derive(Deserialize)]
pub(super) struct TraceOverlayInput {
    pub(super) path: String,
    pub(super) component_path: String,
    #[serde(default)]
    pub(super) touched_files: Vec<String>,
    pub(super) kept: bool,
}

#[derive(Deserialize)]
pub(super) struct TraceAggregateRunInput {
    pub(super) index: usize,
    pub(super) status: String,
    pub(super) exit_code: i32,
    pub(super) artifact_path: String,
    #[serde(default)]
    pub(super) failure: Option<String>,
}

pub(super) fn run_compare(args: TraceArgs) -> CmdResult<TraceCommandOutput> {
    let before_path = PathBuf::from(&args.scenario);
    let Some(after_path) = args.compare_after else {
        return Err(homeboy::Error::validation_invalid_argument(
            "AFTER_JSON",
            "trace compare requires before and after aggregate JSON files",
            None,
            None,
        ));
    };

    let before_json = read_trace_aggregate_json(&before_path)?;
    let after_json = read_trace_aggregate_json(&after_path)?;
    let before = parse_trace_aggregate_for_path(&before_json, &before_path)?;
    let after = parse_trace_aggregate_for_path(&after_json, &after_path)?;
    let output = compare_trace_aggregates_with_focus(
        &before_path,
        before,
        &after_path,
        after,
        &args.focus_spans,
        args.regression_threshold,
        args.regression_min_delta_ms,
    );
    let exit_code = if output.focus_status.as_deref() == Some("fail") {
        1
    } else {
        0
    };
    if let Some(experiment) = args.experiment.as_deref() {
        let before = parse_trace_aggregate_for_path(&before_json, &before_path)?;
        let after = parse_trace_aggregate_for_path(&after_json, &after_path)?;
        write_trace_experiment_bundle(TraceExperimentBundleRequest {
            name: experiment,
            bundle_root: None,
            command: std::env::args().collect::<Vec<_>>().join(" "),
            before_path: &before_path,
            before_json: &before_json,
            before: &before,
            after_path: &after_path,
            after_json: &after_json,
            after: &after,
            compare: &output,
        })?;
    }
    Ok((TraceCommandOutput::Compare(output), exit_code))
}

fn read_trace_aggregate_json(path: &Path) -> homeboy::Result<String> {
    fs::read_to_string(path).map_err(|err| {
        homeboy::Error::internal_io(
            format!("Failed to read trace aggregate {}: {}", path.display(), err),
            Some("trace.compare.read".to_string()),
        )
    })
}

fn parse_trace_aggregate_for_path(
    content: &str,
    path: &Path,
) -> homeboy::Result<TraceAggregateInput> {
    parse_trace_aggregate_input(content).map_err(|err| {
        homeboy::Error::internal_json(
            err.to_string(),
            Some(format!("parse trace aggregate {}", path.display())),
        )
    })
}

pub(super) struct TraceExperimentBundleRequest<'a> {
    pub(super) name: &'a str,
    pub(super) bundle_root: Option<&'a Path>,
    pub(super) command: String,
    pub(super) before_path: &'a Path,
    pub(super) before_json: &'a str,
    pub(super) before: &'a TraceAggregateInput,
    pub(super) after_path: &'a Path,
    pub(super) after_json: &'a str,
    pub(super) after: &'a TraceAggregateInput,
    pub(super) compare: &'a extension_trace::TraceCompareOutput,
}

#[derive(Serialize)]
struct TraceExperimentManifest {
    command: String,
    timestamp: String,
    experiment: String,
    bundle_path: String,
    compare_path: String,
    report_path: String,
    variants: Vec<TraceExperimentVariantManifest>,
    overlays: Vec<TraceExperimentOverlayManifest>,
}

#[derive(Serialize)]
struct TraceExperimentVariantManifest {
    role: &'static str,
    source_path: String,
    bundle_path: String,
    component: Option<String>,
    scenario_id: Option<String>,
    phase_preset: Option<String>,
    repeat: Option<usize>,
    rig_id: Option<String>,
    components: Vec<TraceExperimentComponentManifest>,
    artifact_paths: Vec<String>,
}

#[derive(Serialize)]
struct TraceExperimentComponentManifest {
    id: String,
    path: Option<String>,
    branch: Option<String>,
    sha: Option<String>,
}

#[derive(Serialize)]
struct TraceExperimentOverlayManifest {
    role: &'static str,
    source_path: String,
    bundle_path: Option<String>,
    sha256: Option<String>,
    component_path: String,
    touched_files: Vec<String>,
    kept: bool,
}

pub(super) fn write_trace_experiment_bundle(
    request: TraceExperimentBundleRequest<'_>,
) -> homeboy::Result<PathBuf> {
    let experiment = sanitize_path_component(request.name);
    let bundle_root = request
        .bundle_root
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(".homeboy").join("experiments"));
    let bundle_dir = bundle_root.join(&experiment);
    fs::create_dir_all(&bundle_dir).map_err(|err| {
        homeboy::Error::internal_io(
            format!(
                "Failed to create trace experiment bundle {}: {}",
                bundle_dir.display(),
                err
            ),
            Some("trace.experiment.mkdir".to_string()),
        )
    })?;

    let baseline_path = bundle_dir.join("baseline.json");
    let variant_filename = format!("variant-{}.json", experiment);
    let variant_path = bundle_dir.join(&variant_filename);
    let compare_filename = format!("compare-{}.json", experiment);
    let compare_path = bundle_dir.join(&compare_filename);
    let report_path = bundle_dir.join("report.md");
    write_file(
        &baseline_path,
        request.before_json,
        "trace.experiment.baseline",
    )?;
    write_file(
        &variant_path,
        request.after_json,
        "trace.experiment.variant",
    )?;
    write_json_file(&compare_path, request.compare, "trace.experiment.compare")?;
    write_file(
        &report_path,
        &render_experiment_report(request.name, request.before, request.after, request.compare),
        "trace.experiment.report",
    )?;

    let overlay_dir = bundle_dir.join("overlays");
    let mut overlays = Vec::new();
    overlays.extend(copy_overlay_manifests(
        "baseline",
        &request.before.overlays,
        &overlay_dir,
    )?);
    overlays.extend(copy_overlay_manifests(
        "variant",
        &request.after.overlays,
        &overlay_dir,
    )?);

    let manifest = TraceExperimentManifest {
        command: request.command,
        timestamp: chrono::Utc::now().to_rfc3339(),
        experiment: request.name.to_string(),
        bundle_path: bundle_dir.display().to_string(),
        compare_path: compare_filename,
        report_path: "report.md".to_string(),
        variants: vec![
            variant_manifest(
                "baseline",
                request.before_path,
                "baseline.json".to_string(),
                request.before,
            ),
            variant_manifest(
                "variant",
                request.after_path,
                variant_filename,
                request.after,
            ),
        ],
        overlays,
    };
    write_json_file(
        &bundle_dir.join("manifest.json"),
        &manifest,
        "trace.experiment.manifest",
    )?;

    Ok(bundle_dir)
}

fn variant_manifest(
    role: &'static str,
    source_path: &Path,
    bundle_path: String,
    input: &TraceAggregateInput,
) -> TraceExperimentVariantManifest {
    TraceExperimentVariantManifest {
        role,
        source_path: source_path.display().to_string(),
        bundle_path,
        component: input.component.clone(),
        scenario_id: input.scenario_id.clone(),
        phase_preset: input.phase_preset.clone(),
        repeat: input.repeat,
        rig_id: rig_id(input),
        components: rig_components(input),
        artifact_paths: input
            .runs
            .iter()
            .filter(|run| !run.artifact_path.is_empty())
            .map(|run| run.artifact_path.clone())
            .collect(),
    }
}

fn copy_overlay_manifests(
    role: &'static str,
    overlays: &[TraceOverlayInput],
    overlay_dir: &Path,
) -> homeboy::Result<Vec<TraceExperimentOverlayManifest>> {
    let mut manifests = Vec::new();
    for (index, overlay) in overlays.iter().enumerate() {
        let source = Path::new(&overlay.path);
        let (bundle_path, sha256) = if source.is_file() {
            fs::create_dir_all(overlay_dir).map_err(|err| {
                homeboy::Error::internal_io(
                    format!(
                        "Failed to create overlay bundle dir {}: {}",
                        overlay_dir.display(),
                        err
                    ),
                    Some("trace.experiment.overlay.mkdir".to_string()),
                )
            })?;
            let filename = source
                .file_name()
                .and_then(|name| name.to_str())
                .map(sanitize_path_component)
                .unwrap_or_else(|| format!("overlay-{}.patch", index + 1));
            let target = overlay_dir.join(format!("{}-{}", role, filename));
            fs::copy(source, &target).map_err(|err| {
                homeboy::Error::internal_io(
                    format!(
                        "Failed to copy trace overlay {} to {}: {}",
                        source.display(),
                        target.display(),
                        err
                    ),
                    Some("trace.experiment.overlay.copy".to_string()),
                )
            })?;
            (
                Some(target.display().to_string()),
                Some(sha256_file(source)?),
            )
        } else {
            (None, None)
        };
        manifests.push(TraceExperimentOverlayManifest {
            role,
            source_path: overlay.path.clone(),
            bundle_path,
            sha256,
            component_path: overlay.component_path.clone(),
            touched_files: overlay.touched_files.clone(),
            kept: overlay.kept,
        });
    }
    Ok(manifests)
}

fn render_experiment_report(
    name: &str,
    before: &TraceAggregateInput,
    after: &TraceAggregateInput,
    compare: &extension_trace::TraceCompareOutput,
) -> String {
    let mut out = render_compare_markdown(compare);
    out.push_str(&format!(
        "\n## Experiment Bundle\n\n- **Name:** `{}`\n",
        name
    ));
    push_span_deltas(&mut out, "Top Median Improvements", compare, true, true);
    push_span_deltas(&mut out, "Top Median Regressions", compare, false, true);
    push_span_deltas(&mut out, "Top Average Improvements", compare, true, false);
    push_span_deltas(&mut out, "Top Average Regressions", compare, false, false);
    push_failures_and_outliers(&mut out, "Baseline", before);
    push_failures_and_outliers(&mut out, "Variant", after);
    push_run_artifacts(&mut out, "Baseline", before);
    push_run_artifacts(&mut out, "Variant", after);
    out
}

fn push_span_deltas(
    out: &mut String,
    heading: &str,
    compare: &extension_trace::TraceCompareOutput,
    improvement: bool,
    median: bool,
) {
    let mut spans = compare
        .spans
        .iter()
        .filter(|span| {
            let delta = if median {
                span.median_delta_ms.map(|value| value as f64)
            } else {
                span.avg_delta_ms
            };
            delta.is_some_and(|value| {
                if improvement {
                    value < 0.0
                } else {
                    value > 0.0
                }
            })
        })
        .collect::<Vec<_>>();
    spans.sort_by(|left, right| {
        let left_delta = if median {
            left.median_delta_ms.map(|value| value as f64)
        } else {
            left.avg_delta_ms
        }
        .unwrap_or(0.0)
        .abs();
        let right_delta = if median {
            right.median_delta_ms.map(|value| value as f64)
        } else {
            right.avg_delta_ms
        }
        .unwrap_or(0.0)
        .abs();
        right_delta
            .partial_cmp(&left_delta)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.id.cmp(&right.id))
    });
    if spans.is_empty() {
        return;
    }
    out.push_str(&format!("\n## {}\n\n", heading));
    for span in spans.into_iter().take(5) {
        let delta = if median {
            fmt_delta_ms(span.median_delta_ms)
        } else {
            fmt_delta_avg_ms(span.avg_delta_ms)
        };
        out.push_str(&format!("- `{}`: {}\n", span.id, delta));
    }
}

fn push_failures_and_outliers(out: &mut String, label: &str, input: &TraceAggregateInput) {
    let mut lines = Vec::new();
    for run in &input.runs {
        if run.exit_code != 0 || run.failure.is_some() {
            lines.push(format!(
                "- Run {}: `{}` exit={} artifact=`{}`{}",
                run.index,
                run.status,
                run.exit_code,
                run.artifact_path,
                run.failure
                    .as_ref()
                    .map(|failure| format!(" failure={}", failure))
                    .unwrap_or_default()
            ));
        }
    }
    for span in &input.spans {
        if span.failures > 0 || span.max_artifact_path.is_some() {
            lines.push(format!(
                "- `{}`: failures={} max={} run={} artifact=`{}`",
                span.id,
                span.failures,
                fmt_ms(span.max_ms),
                span.max_run_index
                    .map(|index| index.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                span.max_artifact_path.as_deref().unwrap_or("-")
            ));
        }
    }
    if lines.is_empty() {
        return;
    }
    out.push_str(&format!("\n## {} Failures and Outliers\n\n", label));
    for line in lines {
        out.push_str(&line);
        out.push('\n');
    }
}

fn push_run_artifacts(out: &mut String, label: &str, input: &TraceAggregateInput) {
    let artifact_paths = input
        .runs
        .iter()
        .filter(|run| !run.artifact_path.is_empty())
        .collect::<Vec<_>>();
    if artifact_paths.is_empty() {
        return;
    }
    out.push_str(&format!("\n## {} Artifact Paths\n\n", label));
    for run in artifact_paths {
        out.push_str(&format!("- Run {}: `{}`\n", run.index, run.artifact_path));
    }
}

fn rig_id(input: &TraceAggregateInput) -> Option<String> {
    input
        .rig_state
        .as_ref()
        .and_then(|value| value.get("rig_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn rig_components(input: &TraceAggregateInput) -> Vec<TraceExperimentComponentManifest> {
    input
        .rig_state
        .as_ref()
        .and_then(|value| value.get("components"))
        .and_then(Value::as_object)
        .map(|components| {
            components
                .iter()
                .map(|(id, component)| TraceExperimentComponentManifest {
                    id: id.clone(),
                    path: component
                        .get("path")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    branch: component
                        .get("branch")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    sha: component
                        .get("sha")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn write_file(path: &Path, content: &str, context: &str) -> homeboy::Result<()> {
    fs::write(path, content).map_err(|err| {
        homeboy::Error::internal_io(
            format!("Failed to write {}: {}", path.display(), err),
            Some(context.to_string()),
        )
    })
}

fn write_json_file<T: Serialize>(path: &Path, value: &T, context: &str) -> homeboy::Result<()> {
    let content = serde_json::to_string_pretty(value)
        .map_err(|err| homeboy::Error::internal_json(err.to_string(), Some(context.to_string())))?;
    write_file(path, &(content + "\n"), context)
}

fn sha256_file(path: &Path) -> homeboy::Result<String> {
    let bytes = fs::read(path).map_err(|err| {
        homeboy::Error::internal_io(
            format!("Failed to read {} for checksum: {}", path.display(), err),
            Some("trace.experiment.overlay.sha256".to_string()),
        )
    })?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn sanitize_path_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '-',
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "experiment".to_string()
    } else {
        sanitized
    }
}

pub(super) fn parse_trace_aggregate_input(
    content: &str,
) -> serde_json::Result<TraceAggregateInput> {
    match serde_json::from_str::<TraceAggregateInput>(content) {
        Ok(input) => Ok(input),
        Err(direct_error) => serde_json::from_str::<TraceAggregateEnvelopeInput>(content)
            .map(|envelope| envelope.data)
            .map_err(|_| direct_error),
    }
}

#[cfg(test)]
pub(super) fn compare_trace_aggregates(
    before_path: &Path,
    before: TraceAggregateInput,
    after_path: &Path,
    after: TraceAggregateInput,
) -> extension_trace::TraceCompareOutput {
    compare_trace_aggregates_with_focus(
        before_path,
        before,
        after_path,
        after,
        &[],
        extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
        extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
    )
}

pub(super) fn compare_trace_aggregates_with_focus(
    before_path: &Path,
    before: TraceAggregateInput,
    after_path: &Path,
    after: TraceAggregateInput,
    focus_span_ids: &[String],
    regression_threshold_percent: f64,
    regression_min_delta_ms: u64,
) -> extension_trace::TraceCompareOutput {
    let before_spans = before
        .spans
        .into_iter()
        .map(|span| (span.id.clone(), span))
        .collect::<BTreeMap<_, _>>();
    let after_spans = after
        .spans
        .into_iter()
        .map(|span| (span.id.clone(), span))
        .collect::<BTreeMap<_, _>>();
    let span_ids = before_spans
        .keys()
        .chain(after_spans.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut spans = span_ids
        .into_iter()
        .map(|id| {
            let before_span = before_spans.get(&id);
            let after_span = after_spans.get(&id);
            let before_median = before_span.and_then(|span| span.median_ms);
            let after_median = after_span.and_then(|span| span.median_ms);
            let before_avg = before_span.and_then(|span| span.avg_ms);
            let after_avg = after_span.and_then(|span| span.avg_ms);

            extension_trace::TraceCompareSpanOutput {
                id,
                before_n: before_span.map(|span| span.n),
                after_n: after_span.map(|span| span.n),
                before_median_ms: before_median,
                after_median_ms: after_median,
                median_delta_ms: option_delta_i64(before_median, after_median),
                median_delta_percent: option_percent_delta(
                    before_median.map(|value| value as f64),
                    after_median.map(|value| value as f64),
                ),
                before_avg_ms: before_avg,
                after_avg_ms: after_avg,
                avg_delta_ms: option_delta_f64(before_avg, after_avg),
                avg_delta_percent: option_percent_delta(before_avg, after_avg),
                before_failures: before_span.map(|span| span.failures),
                after_failures: after_span.map(|span| span.failures),
            }
        })
        .collect::<Vec<_>>();
    spans.sort_by(compare_trace_span_impact);

    let focus_spans = focus_compare_spans(&spans, focus_span_ids);
    let focus_regression_count = focus_spans
        .iter()
        .filter(|span| {
            is_focused_span_regression(span, regression_threshold_percent, regression_min_delta_ms)
        })
        .count();
    let focus_failure_count = focus_spans
        .iter()
        .filter(|span| span.after_failures.unwrap_or(0) > span.before_failures.unwrap_or(0))
        .count();
    let focus_status = if focus_span_ids.is_empty() {
        None
    } else if focus_regression_count > 0 || focus_failure_count > 0 {
        Some("fail".to_string())
    } else {
        Some("pass".to_string())
    };

    extension_trace::TraceCompareOutput {
        command: "trace.compare.spans",
        before_path: before_path.display().to_string(),
        after_path: after_path.display().to_string(),
        before_component: before.component,
        after_component: after.component,
        before_scenario_id: before.scenario_id,
        after_scenario_id: after.scenario_id,
        span_count: spans.len(),
        spans,
        focus_span_ids: focus_span_ids.to_vec(),
        focus_spans,
        focus_regression_count,
        focus_failure_count,
        focus_status,
    }
}

fn focus_compare_spans(
    spans: &[extension_trace::TraceCompareSpanOutput],
    focus_span_ids: &[String],
) -> Vec<extension_trace::TraceCompareSpanOutput> {
    if focus_span_ids.is_empty() {
        return Vec::new();
    }
    let focus = focus_span_ids.iter().collect::<BTreeSet<_>>();
    spans
        .iter()
        .filter(|span| focus.contains(&span.id))
        .cloned()
        .collect()
}

fn is_focused_span_regression(
    span: &extension_trace::TraceCompareSpanOutput,
    regression_threshold_percent: f64,
    regression_min_delta_ms: u64,
) -> bool {
    let Some(delta_ms) = span.median_delta_ms else {
        return false;
    };
    if delta_ms <= 0 || delta_ms < regression_min_delta_ms as i64 {
        return false;
    }
    span.median_delta_percent
        .is_some_and(|percent| percent >= regression_threshold_percent)
}

fn compare_trace_span_impact(
    left: &extension_trace::TraceCompareSpanOutput,
    right: &extension_trace::TraceCompareSpanOutput,
) -> std::cmp::Ordering {
    right
        .median_delta_ms
        .map(i64::abs)
        .cmp(&left.median_delta_ms.map(i64::abs))
        .then_with(|| {
            right
                .avg_delta_ms
                .map(f64::abs)
                .partial_cmp(&left.avg_delta_ms.map(f64::abs))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| left.id.cmp(&right.id))
}

fn option_delta_i64(before: Option<u64>, after: Option<u64>) -> Option<i64> {
    Some(after? as i64 - before? as i64)
}

fn option_delta_f64(before: Option<f64>, after: Option<f64>) -> Option<f64> {
    Some(after? - before?)
}

fn option_percent_delta(before: Option<f64>, after: Option<f64>) -> Option<f64> {
    let before = before?;
    let after = after?;
    if before.abs() < f64::EPSILON {
        if after.abs() < f64::EPSILON {
            Some(0.0)
        } else {
            None
        }
    } else {
        Some(((after - before) / before) * 100.0)
    }
}

pub(super) fn aggregate_span(
    id: String,
    samples: Vec<TraceAggregateSpanSample>,
    failures: usize,
) -> extension_trace::TraceAggregateSpanOutput {
    let max_sample: Option<TraceAggregateSpanSample> =
        samples.iter().fold(None, |max, sample| match max {
            Some(current) if current.duration_ms >= sample.duration_ms => Some(current),
            _ => Some(sample.clone()),
        });
    let mut durations = samples
        .into_iter()
        .map(|sample| sample.duration_ms)
        .collect::<Vec<_>>();
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
        p75_ms: percentile(&durations, 75, 4),
        p90_ms: percentile(&durations, 90, 10),
        p95_ms: percentile(&durations, 95, 20),
        max_ms: durations.last().copied(),
        max_run_index: max_sample.as_ref().map(|sample| sample.run_index),
        max_artifact_path: max_sample.map(|sample| sample.artifact_path),
        failures,
    }
}

#[derive(Clone)]
pub(super) struct TraceAggregateSpanSample {
    pub(super) duration_ms: u64,
    pub(super) run_index: usize,
    pub(super) artifact_path: String,
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

fn percentile(values: &[u64], percentile: usize, min_samples: usize) -> Option<u64> {
    if values.len() < min_samples {
        return None;
    }
    let index = (values.len() * percentile).div_ceil(100).saturating_sub(1);
    values.get(index).copied()
}

pub(super) fn render_aggregate_markdown(
    aggregate: &extension_trace::TraceAggregateOutput,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Trace Aggregate: `{}`\n\n",
        aggregate.scenario_id
    ));
    out.push_str(&format!("- **Component:** `{}`\n", aggregate.component));
    out.push_str(&format!("- **Status:** `{}`\n", aggregate.status));
    out.push_str(&format!("- **Runs:** `{}`\n", aggregate.run_count));
    out.push_str(&format!("- **Failures:** `{}`\n", aggregate.failure_count));
    if let Some(schedule) = aggregate.schedule.as_deref() {
        out.push_str(&format!("- **Schedule:** `{}`\n", schedule));
    }
    extension_trace::push_overlay_markdown(&mut out, &aggregate.overlays);

    if !aggregate.focus_span_ids.is_empty() {
        out.push_str("\n## Focus Spans\n\n");
        if aggregate.focus_spans.is_empty() {
            out.push_str("No focused spans matched the aggregate output.\n");
        } else {
            out.push_str("| Span | n | min | median | avg | p75 | p90 | p95 | max | failures |\n");
            out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
            for span in &aggregate.focus_spans {
                push_aggregate_span_row(&mut out, span);
            }
        }
    }

    if !aggregate.spans.is_empty() {
        out.push_str("\n## Spans\n\n");
        out.push_str("| Span | n | min | median | avg | p75 | p90 | p95 | max | failures |\n");
        out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
        for span in &aggregate.spans {
            push_aggregate_span_row(&mut out, span);
        }

        let outliers = aggregate
            .spans
            .iter()
            .filter(|span| span.max_ms.is_some() && span.max_run_index.is_some())
            .collect::<Vec<_>>();
        if !outliers.is_empty() {
            out.push_str("\n## Outliers\n\n");
            for span in outliers {
                out.push_str(&format!(
                    "- `{}`: run {}, max={}, artifact=`{}`\n",
                    span.id,
                    span.max_run_index.unwrap_or_default(),
                    fmt_ms(span.max_ms),
                    span.max_artifact_path.as_deref().unwrap_or("")
                ));
            }
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

fn push_aggregate_span_row(out: &mut String, span: &extension_trace::TraceAggregateSpanOutput) {
    out.push_str(&format!(
        "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
        span.id,
        span.n,
        fmt_ms(span.min_ms),
        fmt_ms(span.median_ms),
        span.avg_ms
            .map(|value| format!("{:.1}ms", value))
            .unwrap_or_else(|| "-".to_string()),
        fmt_ms(span.p75_ms),
        fmt_ms(span.p90_ms),
        fmt_ms(span.p95_ms),
        fmt_ms(span.max_ms),
        span.failures
    ));
}

pub(super) fn render_compare_markdown(compare: &extension_trace::TraceCompareOutput) -> String {
    let mut out = String::new();
    out.push_str("# Trace Compare\n\n");
    out.push_str(&format!("- **Before:** `{}`\n", compare.before_path));
    out.push_str(&format!("- **After:** `{}`\n", compare.after_path));
    if let (Some(before), Some(after)) = (&compare.before_scenario_id, &compare.after_scenario_id) {
        out.push_str(&format!("- **Scenario:** `{}` -> `{}`\n", before, after));
    }

    if !compare.spans.is_empty() {
        out.push_str("\n## Spans\n\n");
        out.push_str("| Span | before median | after median | median delta | median % | before avg | after avg | avg delta | avg % |\n");
        out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|\n");
        for span in &compare.spans {
            out.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                span.id,
                fmt_ms(span.before_median_ms),
                fmt_ms(span.after_median_ms),
                fmt_signal_delta_ms(span.median_delta_ms),
                fmt_percent(span.median_delta_percent),
                fmt_avg_ms(span.before_avg_ms),
                fmt_avg_ms(span.after_avg_ms),
                fmt_signal_delta_avg_ms(span.avg_delta_ms),
                fmt_percent(span.avg_delta_percent),
            ));
        }
    }

    if !compare.focus_span_ids.is_empty() {
        out.push_str("\n## Focus Spans\n\n");
        out.push_str(&format!(
            "- **Status:** `{}`\n",
            compare.focus_status.as_deref().unwrap_or("pass")
        ));
        out.push_str(&format!(
            "- **Regressions:** `{}`\n",
            compare.focus_regression_count
        ));
        out.push_str(&format!(
            "- **Failures:** `{}`\n",
            compare.focus_failure_count
        ));
        if compare.focus_spans.is_empty() {
            out.push_str("\nNo focused spans matched the compared aggregates.\n");
        } else {
            out.push_str("\n| Span | before median | after median | median delta | median % | before avg | after avg | avg delta | avg % | before failures | after failures |\n");
            out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
            for span in &compare.focus_spans {
                out.push_str(&format!(
                    "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                    span.id,
                    fmt_ms(span.before_median_ms),
                    fmt_ms(span.after_median_ms),
                    fmt_signal_delta_ms(span.median_delta_ms),
                    fmt_percent(span.median_delta_percent),
                    fmt_avg_ms(span.before_avg_ms),
                    fmt_avg_ms(span.after_avg_ms),
                    fmt_signal_delta_avg_ms(span.avg_delta_ms),
                    fmt_percent(span.avg_delta_percent),
                    fmt_count(span.before_failures),
                    fmt_count(span.after_failures),
                ));
            }
        }
    }

    out
}

fn fmt_count(value: Option<usize>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn fmt_ms(value: Option<u64>) -> String {
    value
        .map(|value| format!("{}ms", value))
        .unwrap_or_else(|| "-".to_string())
}

fn fmt_avg_ms(value: Option<f64>) -> String {
    value
        .map(|value| format!("{:.1}ms", value))
        .unwrap_or_else(|| "-".to_string())
}

fn fmt_delta_ms(value: Option<i64>) -> String {
    value
        .map(|value| format!("{:+}ms", value))
        .unwrap_or_else(|| "-".to_string())
}

fn fmt_signal_delta_ms(value: Option<i64>) -> String {
    let formatted = fmt_delta_ms(value);
    if value.is_some_and(|value| value != 0) {
        format!("**{}**", formatted)
    } else {
        formatted
    }
}

fn fmt_delta_avg_ms(value: Option<f64>) -> String {
    value
        .map(|value| format!("{:+.1}ms", value))
        .unwrap_or_else(|| "-".to_string())
}

fn fmt_signal_delta_avg_ms(value: Option<f64>) -> String {
    let formatted = fmt_delta_avg_ms(value);
    if value.is_some_and(|value| value.abs() >= f64::EPSILON) {
        format!("**{}**", formatted)
    } else {
        formatted
    }
}

fn fmt_percent(value: Option<f64>) -> String {
    value
        .map(|value| format!("{:+.1}%", value))
        .unwrap_or_else(|| "-".to_string())
}
