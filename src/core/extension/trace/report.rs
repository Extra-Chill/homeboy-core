//! Trace command output envelopes.

use serde::Serialize;

use super::baseline::TraceBaselineComparison;
use super::parsing::{
    TraceArtifact, TraceAssertionStatus, TraceList, TraceResults, TraceSpanStatus,
};
use super::run::{TraceOverlay, TraceRunWorkflowResult};
use crate::rig::RigStateSnapshot;

#[derive(Serialize)]
#[serde(untagged)]
pub enum TraceCommandOutput {
    Run(Box<TraceRunOutput>),
    Summary(TraceRunSummaryOutput),
    Aggregate(TraceAggregateOutput),
    List(TraceListOutput),
}

#[derive(Serialize)]
pub struct TraceRunOutput {
    pub passed: bool,
    pub status: String,
    pub component: String,
    pub exit_code: i32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<TraceArtifact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<TraceResults>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rig_state: Option<RigStateSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<super::run::TraceRunFailure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overlays: Vec<TraceOverlay>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_comparison: Option<TraceBaselineComparison>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct TraceRunSummaryOutput {
    pub summary_only: bool,
    pub passed: bool,
    pub status: String,
    pub component: String,
    pub exit_code: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub assertion_count: usize,
    pub artifact_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rig_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overlays: Vec<TraceOverlay>,
    pub span_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct TraceListOutput {
    pub command: &'static str,
    pub component: String,
    pub component_id: String,
    pub count: usize,
    pub scenarios: Vec<super::parsing::TraceScenario>,
}

#[derive(Serialize)]
pub struct TraceAggregateOutput {
    pub command: &'static str,
    pub passed: bool,
    pub status: String,
    pub component: String,
    pub scenario_id: String,
    pub repeat: usize,
    pub run_count: usize,
    pub failure_count: usize,
    pub exit_code: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rig_state: Option<RigStateSnapshot>,
    pub runs: Vec<TraceAggregateRunOutput>,
    pub spans: Vec<TraceAggregateSpanOutput>,
}

#[derive(Serialize)]
pub struct TraceAggregateRunOutput {
    pub index: usize,
    pub passed: bool,
    pub status: String,
    pub exit_code: i32,
    pub artifact_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

#[derive(Serialize)]
pub struct TraceAggregateSpanOutput {
    pub id: String,
    pub n: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_ms: Option<u64>,
    pub failures: usize,
}

pub fn from_main_workflow(
    result: TraceRunWorkflowResult,
    rig_state: Option<RigStateSnapshot>,
    summary_only: bool,
) -> (TraceCommandOutput, i32) {
    let exit_code = result.exit_code;
    if summary_only {
        let output = TraceRunSummaryOutput {
            summary_only: true,
            passed: exit_code == 0 && result.status == "pass",
            status: result.status,
            component: result.component,
            exit_code,
            scenario_id: result.results.as_ref().map(|r| r.scenario_id.clone()),
            summary: result.results.as_ref().and_then(|r| r.summary.clone()),
            assertion_count: result
                .results
                .as_ref()
                .map(|r| r.assertions.len())
                .unwrap_or(0),
            artifact_count: result
                .results
                .as_ref()
                .map(|r| r.artifacts.len())
                .unwrap_or(0),
            rig_id: rig_state.as_ref().map(|r| r.rig_id.clone()),
            overlays: result.overlays,
            span_count: result
                .results
                .as_ref()
                .map(|r| r.span_results.len())
                .unwrap_or(0),
            hints: result.hints,
        };
        return (TraceCommandOutput::Summary(output), exit_code);
    }

    let artifacts = result
        .results
        .as_ref()
        .map(|r| r.artifacts.clone())
        .unwrap_or_default();
    (
        TraceCommandOutput::Run(Box::new(TraceRunOutput {
            passed: exit_code == 0 && result.status == "pass",
            status: result.status,
            component: result.component,
            exit_code,
            artifacts,
            results: result.results,
            rig_state,
            failure: result.failure,
            overlays: result.overlays,
            baseline_comparison: result.baseline_comparison,
            hints: result.hints,
        })),
        exit_code,
    )
}

pub fn render_markdown(results: &TraceResults) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Trace: `{}`\n\n", results.scenario_id));
    out.push_str(&format!("- **Component:** `{}`\n", results.component_id));
    out.push_str(&format!("- **Status:** `{}`\n", results.status.as_str()));
    if let Some(summary) = &results.summary {
        out.push_str(&format!("- **Summary:** {}\n", summary));
    }
    if let Some(failure) = &results.failure {
        out.push_str(&format!("- **Failure:** {}\n", failure));
    }

    if !results.span_results.is_empty() {
        out.push_str("\n## Spans\n\n");
        out.push_str("| Span | From | To | Duration | Status |\n");
        out.push_str("|---|---|---|---:|---|\n");
        for span in &results.span_results {
            let duration = span
                .duration_ms
                .map(|ms| format!("{}ms", ms))
                .unwrap_or_else(|| "-".to_string());
            let status = match span.status {
                TraceSpanStatus::Ok => "ok".to_string(),
                TraceSpanStatus::Skipped => span
                    .message
                    .clone()
                    .unwrap_or_else(|| "skipped".to_string()),
            };
            out.push_str(&format!(
                "| `{}` | `{}` | `{}` | {} | {} |\n",
                span.id, span.from, span.to, duration, status
            ));
        }
    }

    if !results.assertions.is_empty() {
        out.push_str("\n## Assertions\n\n");
        for assertion in &results.assertions {
            let status = match assertion.status {
                TraceAssertionStatus::Pass => "pass",
                TraceAssertionStatus::Fail => "fail",
                TraceAssertionStatus::Error => "error",
            };
            match &assertion.message {
                Some(message) => out.push_str(&format!(
                    "- `{}`: **{}** - {}\n",
                    assertion.id, status, message
                )),
                None => out.push_str(&format!("- `{}`: **{}**\n", assertion.id, status)),
            }
        }
    }

    if !results.artifacts.is_empty() {
        out.push_str("\n## Artifacts\n\n");
        for artifact in &results.artifacts {
            out.push_str(&format!("- **{}:** `{}`\n", artifact.label, artifact.path));
        }
    }

    if !results.timeline.is_empty() {
        out.push_str("\n## Timeline\n\n");
        for event in &results.timeline {
            out.push_str(&format!(
                "- `{}ms` `{}.{}`\n",
                event.t_ms, event.source, event.event
            ));
        }
    }

    out
}

pub fn from_list_workflow(component: String, list: TraceList) -> (TraceCommandOutput, i32) {
    let count = list.scenarios.len();
    (
        TraceCommandOutput::List(TraceListOutput {
            command: "trace.list",
            component,
            component_id: list.component_id,
            count,
            scenarios: list.scenarios,
        }),
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::trace::parsing::{TraceScenario, TraceStatus};

    #[test]
    fn test_from_list_workflow() {
        let list = TraceList {
            component_id: "studio".to_string(),
            scenario_id: None,
            status: None,
            scenarios: vec![TraceScenario {
                id: "close-window-running-site".to_string(),
                source: Some("fixtures/close-window.trace.js".to_string()),
                summary: Some("Close window while a site is running".to_string()),
            }],
            timeline: Vec::new(),
            assertions: Vec::new(),
            artifacts: Vec::new(),
        };

        let (output, exit_code) = from_list_workflow("Studio".to_string(), list);
        let value = serde_json::to_value(output).expect("list output should serialize");

        assert_eq!(exit_code, 0);
        assert_eq!(value["command"], "trace.list");
        assert_eq!(value["component"], "Studio");
        assert_eq!(value["component_id"], "studio");
        assert_eq!(value["count"], 1);
        assert_eq!(value["scenarios"][0]["id"], "close-window-running-site");
    }

    #[test]
    fn test_from_main_workflow() {
        let result = TraceRunWorkflowResult {
            status: "pass".to_string(),
            component: "Studio".to_string(),
            exit_code: 0,
            results: Some(TraceResults {
                component_id: "studio".to_string(),
                scenario_id: "close-window-running-site".to_string(),
                status: TraceStatus::Pass,
                summary: Some("No window reopened".to_string()),
                failure: None,
                rig: None,
                timeline: Vec::new(),
                span_definitions: Vec::new(),
                span_results: Vec::new(),
                assertions: Vec::new(),
                artifacts: vec![TraceArtifact {
                    label: "main log".to_string(),
                    path: "artifacts/main.log".to_string(),
                }],
            }),
            failure: None,
            overlays: vec![TraceOverlay {
                path: "/tmp/overlay.patch".to_string(),
                touched_files: vec!["scenario.txt".to_string()],
                kept: false,
            }],
            baseline_comparison: None,
            hints: None,
        };

        let (output, exit_code) = from_main_workflow(result, None, true);
        let value = serde_json::to_value(output).expect("summary output should serialize");

        assert_eq!(exit_code, 0);
        assert_eq!(value["summary_only"], true);
        assert_eq!(value["passed"], true);
        assert_eq!(value["scenario_id"], "close-window-running-site");
        assert_eq!(value["artifact_count"], 1);
        assert_eq!(value["span_count"], 0);
        assert_eq!(value["overlays"][0]["path"], "/tmp/overlay.patch");
        assert_eq!(value["overlays"][0]["touched_files"][0], "scenario.txt");
        assert_eq!(value["overlays"][0]["kept"], false);
    }

    #[test]
    fn test_render_markdown() {
        let results = TraceResults {
            component_id: "studio".to_string(),
            scenario_id: "create-site".to_string(),
            status: TraceStatus::Pass,
            summary: Some("Created a site".to_string()),
            failure: None,
            rig: None,
            timeline: Vec::new(),
            span_definitions: Vec::new(),
            span_results: vec![crate::extension::trace::parsing::TraceSpanResult {
                id: "submit_to_cli".to_string(),
                from: "ui.submit".to_string(),
                to: "cli.start".to_string(),
                status: crate::extension::trace::parsing::TraceSpanStatus::Ok,
                duration_ms: Some(42),
                from_t_ms: Some(10),
                to_t_ms: Some(52),
                missing: Vec::new(),
                message: None,
            }],
            assertions: Vec::new(),
            artifacts: Vec::new(),
        };

        let markdown = render_markdown(&results);

        assert!(markdown.contains("# Trace: `create-site`"));
        assert!(markdown.contains("| `submit_to_cli` | `ui.submit` | `cli.start` | 42ms | ok |"));
    }
}
