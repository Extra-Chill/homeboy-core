use std::path::Path;

use serde::Serialize;

use homeboy::extension::trace as extension_trace;
use homeboy::extension::trace::TraceCommandOutput;
use homeboy::rig;

use super::output::{
    compare_trace_aggregates_with_focus, TraceAggregateInput, TraceAggregateRunInput,
    TraceAggregateSpanInput, TraceOverlayInput,
};
use super::{run_repeat, TraceArgs};
use crate::commands::CmdResult;

const TRACE_COMPARE_VARIANT_BASELINE_FILE: &str = "baseline.json";
const TRACE_COMPARE_VARIANT_VARIANT_FILE: &str = "variant.json";
const TRACE_COMPARE_VARIANT_COMPARE_FILE: &str = "compare.json";
const TRACE_COMPARE_VARIANT_SUMMARY_FILE: &str = "summary.md";

pub(super) fn run_compare_variant(mut args: TraceArgs) -> CmdResult<TraceCommandOutput> {
    let output_dir = args.output_dir.clone().ok_or_else(|| {
        homeboy::Error::validation_missing_argument(vec!["--output-dir".to_string()])
    })?;
    if args.overlays.is_empty() {
        return Err(homeboy::Error::validation_invalid_argument(
            "--overlay",
            "trace compare-variant requires at least one --overlay for the variant run",
            None,
            None,
        ));
    }
    if args.keep_overlay {
        return Err(homeboy::Error::validation_invalid_argument(
            "--keep-overlay",
            "trace compare-variant reuses the same component checkout and must revert overlays between runs",
            None,
            None,
        ));
    }

    args.comp.component = None;
    args.aggregate = Some("spans".to_string());
    args.json_summary = false;
    args.report = None;
    args.compare_after = None;
    let focus_spans = args.focus_spans.clone();
    let regression_threshold = args.regression_threshold;
    let regression_min_delta_ms = args.regression_min_delta_ms;

    let mut baseline_args = args.clone();
    baseline_args.overlays.clear();
    let baseline = run_repeat_output(baseline_args)?;

    let variant = run_repeat_output(args)?;

    create_trace_compare_variant_output_dir(&output_dir)?;
    let baseline_path = output_dir.join(TRACE_COMPARE_VARIANT_BASELINE_FILE);
    let variant_path = output_dir.join(TRACE_COMPARE_VARIANT_VARIANT_FILE);
    let compare_path = output_dir.join(TRACE_COMPARE_VARIANT_COMPARE_FILE);
    let summary_path = output_dir.join(TRACE_COMPARE_VARIANT_SUMMARY_FILE);

    write_trace_compare_variant_json(&baseline_path, &baseline)?;
    write_trace_compare_variant_json(&variant_path, &variant)?;

    let compare = compare_trace_aggregates_with_focus(
        &baseline_path,
        aggregate_to_compare_input(&baseline),
        &variant_path,
        aggregate_to_compare_input(&variant),
        &focus_spans,
        regression_threshold,
        regression_min_delta_ms,
    );
    write_trace_compare_variant_json(&compare_path, &compare)?;
    write_trace_compare_variant_summary(&summary_path, &output_dir, &baseline, &variant, &compare)?;

    let exit_code = if baseline.exit_code == 0
        && variant.exit_code == 0
        && compare.focus_status.as_deref() != Some("fail")
    {
        0
    } else {
        1
    };
    Ok((TraceCommandOutput::Compare(compare), exit_code))
}

fn run_repeat_output(args: TraceArgs) -> homeboy::Result<extension_trace::TraceAggregateOutput> {
    let (output, _exit_code) = run_repeat(args)?;
    match output {
        TraceCommandOutput::Aggregate(aggregate) => Ok(aggregate),
        _ => Err(homeboy::Error::internal_unexpected(
            "trace compare-variant expected aggregate output",
        )),
    }
}

fn create_trace_compare_variant_output_dir(output_dir: &Path) -> homeboy::Result<()> {
    std::fs::create_dir_all(output_dir).map_err(|err| {
        homeboy::Error::internal_io(
            format!(
                "Failed to create trace compare-variant output directory {}: {}",
                output_dir.display(),
                err
            ),
            Some("trace.compare_variant.output_dir".to_string()),
        )
    })
}

fn write_trace_compare_variant_json<T: Serialize>(path: &Path, value: &T) -> homeboy::Result<()> {
    let json = serde_json::to_string_pretty(value).map_err(|err| {
        homeboy::Error::internal_json(
            err.to_string(),
            Some(format!("serialize {}", path.display())),
        )
    })?;
    std::fs::write(path, format!("{}\n", json)).map_err(|err| {
        homeboy::Error::internal_io(
            format!("Failed to write {}: {}", path.display(), err),
            Some("trace.compare_variant.write".to_string()),
        )
    })
}

fn aggregate_to_compare_input(
    aggregate: &extension_trace::TraceAggregateOutput,
) -> TraceAggregateInput {
    TraceAggregateInput {
        component: Some(aggregate.component.clone()),
        scenario_id: Some(aggregate.scenario_id.clone()),
        phase_preset: aggregate.phase_preset.clone(),
        repeat: Some(aggregate.repeat),
        rig_state: aggregate
            .rig_state
            .as_ref()
            .and_then(|rig_state| serde_json::to_value(rig_state).ok()),
        overlays: aggregate
            .overlays
            .iter()
            .map(|overlay| TraceOverlayInput {
                path: overlay.path.clone(),
                component_path: overlay.component_path.clone(),
                touched_files: overlay.touched_files.clone(),
                kept: overlay.kept,
            })
            .collect(),
        runs: aggregate
            .runs
            .iter()
            .map(|run| TraceAggregateRunInput {
                index: run.index,
                status: run.status.clone(),
                exit_code: run.exit_code,
                artifact_path: run.artifact_path.clone(),
                failure: run.failure.clone(),
            })
            .collect(),
        spans: aggregate
            .spans
            .iter()
            .map(|span| TraceAggregateSpanInput {
                id: span.id.clone(),
                n: span.n,
                median_ms: span.median_ms,
                avg_ms: span.avg_ms,
                max_ms: span.max_ms,
                max_run_index: span.max_run_index,
                max_artifact_path: span.max_artifact_path.clone(),
                failures: span.failures,
            })
            .collect(),
    }
}

fn write_trace_compare_variant_summary(
    path: &Path,
    output_dir: &Path,
    baseline: &extension_trace::TraceAggregateOutput,
    variant: &extension_trace::TraceAggregateOutput,
    compare: &extension_trace::TraceCompareOutput,
) -> homeboy::Result<()> {
    let mut out = String::new();
    out.push_str(&format!(
        "# Trace Compare Variant: `{}`\n\n",
        baseline.scenario_id
    ));
    out.push_str(&format!("- **Component:** `{}`\n", baseline.component));
    out.push_str(&format!("- **Output dir:** `{}`\n", output_dir.display()));
    out.push_str(&format!("- **Baseline runs:** `{}`\n", baseline.run_count));
    out.push_str(&format!("- **Variant runs:** `{}`\n", variant.run_count));
    out.push_str(&format!("- **Baseline status:** `{}`\n", baseline.status));
    out.push_str(&format!("- **Variant status:** `{}`\n", variant.status));
    out.push_str(&format!(
        "- **Files:** `{}`, `{}`, `{}`\n",
        TRACE_COMPARE_VARIANT_BASELINE_FILE,
        TRACE_COMPARE_VARIANT_VARIANT_FILE,
        TRACE_COMPARE_VARIANT_COMPARE_FILE
    ));

    push_component_sha_summary(&mut out, "Baseline", baseline.rig_state.as_ref());
    push_component_sha_summary(&mut out, "Variant", variant.rig_state.as_ref());
    push_overlay_summary(&mut out, &variant.overlays);
    push_compare_variant_span_summary(&mut out, compare);

    std::fs::write(path, out).map_err(|err| {
        homeboy::Error::internal_io(
            format!("Failed to write {}: {}", path.display(), err),
            Some("trace.compare_variant.summary".to_string()),
        )
    })
}

fn push_component_sha_summary(
    out: &mut String,
    label: &str,
    rig_state: Option<&rig::RigStateSnapshot>,
) {
    out.push_str(&format!("\n## {label} Component SHAs\n\n"));
    let Some(rig_state) = rig_state else {
        out.push_str("- No rig state captured.\n");
        return;
    };
    for (component_id, component) in &rig_state.components {
        out.push_str(&format!(
            "- `{}`: `{}` ({})\n",
            component_id,
            component.sha.as_deref().unwrap_or("unknown"),
            component.path
        ));
    }
}

fn push_overlay_summary(out: &mut String, overlays: &[extension_trace::run::TraceOverlay]) {
    out.push_str("\n## Variant Overlays\n\n");
    if overlays.is_empty() {
        out.push_str("- No overlays applied.\n");
        return;
    }
    for overlay in overlays {
        out.push_str(&format!("- `{}`\n", overlay.path));
        for file in &overlay.touched_files {
            out.push_str(&format!("- touched: `{}`\n", file));
        }
        if overlay.touched_files.is_empty() {
            out.push_str("- touched: none reported\n");
        }
    }
}

fn push_compare_variant_span_summary(
    out: &mut String,
    compare: &extension_trace::TraceCompareOutput,
) {
    out.push_str("\n## Largest Span Deltas\n\n");
    if compare.spans.is_empty() {
        out.push_str("No comparable spans were produced.\n");
        return;
    }
    out.push_str("| Span | baseline median | variant median | delta | delta % |\n");
    out.push_str("|---|---:|---:|---:|---:|\n");
    for span in compare.spans.iter().take(10) {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            span.id,
            fmt_summary_ms(span.before_median_ms),
            fmt_summary_ms(span.after_median_ms),
            span.median_delta_ms
                .map(|value| format!("{:+}ms", value))
                .unwrap_or_else(|| "-".to_string()),
            span.median_delta_percent
                .map(|value| format!("{:+.1}%", value))
                .unwrap_or_else(|| "-".to_string())
        ));
    }
}

fn fmt_summary_ms(value: Option<u64>) -> String {
    value
        .map(|value| format!("{}ms", value))
        .unwrap_or_else(|| "-".to_string())
}
