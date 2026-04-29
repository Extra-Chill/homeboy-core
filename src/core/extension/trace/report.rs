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
