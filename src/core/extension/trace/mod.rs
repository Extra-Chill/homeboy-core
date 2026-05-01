//! Extension trace capability — black-box evidence capture for lifecycle bugs.
//!
//! Trace is a sibling of `test` and `bench`: Homeboy resolves a component-owned
//! extension script, creates a run directory, passes an env-var contract, and
//! parses a JSON envelope written by the runner. Unlike bench, trace has no
//! baselines, ratchets, or metric gates; its job is to preserve causality and
//! evidence artifacts. Optional span baselines compare generic
//! `source.event` intervals without teaching core about product-specific
//! milestones.

pub mod baseline;
pub mod parsing;
pub mod report;
pub mod run;
pub mod spans;

use crate::component::Component;
use crate::extension::{ExtensionCapability, ExtensionExecutionContext};

pub use parsing::{parse_trace_list_str, parse_trace_results_file};
pub use parsing::{
    TraceArtifact, TraceAssertion, TraceEvent, TraceList, TraceScenario, TraceStatus,
};
pub use parsing::{TraceAssertionStatus, TraceResults, TraceSpanDefinition, TraceSpanResult};
pub use report::render_markdown;
pub use report::{
    from_list_workflow, from_main_workflow, from_main_workflow_outputs, TraceAggregateOutput,
    TraceAggregateRunOutput, TraceAggregateSpanOutput, TraceCommandOutput, TraceCompareOutput,
    TraceCompareSpanOutput,
};
pub use run::{run_trace_list_workflow, run_trace_workflow, TraceListWorkflowArgs};
pub use run::{TraceRunWorkflowArgs, TraceRunWorkflowResult};

pub fn resolve_trace_command(
    component: &Component,
) -> crate::error::Result<ExtensionExecutionContext> {
    crate::extension::resolve_execution_context(component, ExtensionCapability::Trace)
}
