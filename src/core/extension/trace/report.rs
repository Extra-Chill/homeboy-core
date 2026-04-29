//! Trace command output envelopes.

use serde::Serialize;

use super::parsing::{TraceArtifact, TraceList, TraceResults};
use super::run::TraceRunWorkflowResult;
use crate::rig::RigStateSnapshot;

#[derive(Serialize)]
#[serde(untagged)]
pub enum TraceCommandOutput {
    Run(Box<TraceRunOutput>),
    Summary(TraceRunSummaryOutput),
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
}

#[derive(Serialize)]
pub struct TraceListOutput {
    pub command: &'static str,
    pub component: String,
    pub component_id: String,
    pub count: usize,
    pub scenarios: Vec<super::parsing::TraceScenario>,
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
        })),
        exit_code,
    )
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
                assertions: Vec::new(),
                artifacts: vec![TraceArtifact {
                    label: "main log".to_string(),
                    path: "artifacts/main.log".to_string(),
                }],
            }),
            failure: None,
        };

        let (output, exit_code) = from_main_workflow(result, None, true);
        let value = serde_json::to_value(output).expect("summary output should serialize");

        assert_eq!(exit_code, 0);
        assert_eq!(value["summary_only"], true);
        assert_eq!(value["passed"], true);
        assert_eq!(value["scenario_id"], "close-window-running-site");
        assert_eq!(value["artifact_count"], 1);
    }
}
