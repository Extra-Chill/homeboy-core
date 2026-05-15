use clap::ValueEnum;
use homeboy::plan::{HomeboyPlan, PlanKind, PlanStep, PlanStepStatus, PlanSummary};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum TraceSchedule {
    Grouped,
    Interleaved,
}

impl TraceSchedule {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Grouped => "grouped",
            Self::Interleaved => "interleaved",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TraceRunPlanEntry {
    pub(super) plan: HomeboyPlan,
    pub(super) index: usize,
    pub(super) group: String,
    pub(super) iteration: usize,
}

pub(crate) fn plan_trace_run_order(
    repeat: usize,
    schedule: TraceSchedule,
    groups: &[&str],
) -> Vec<TraceRunPlanEntry> {
    let mut entries = Vec::new();
    let mut push_entry = |group: &str, iteration: usize| {
        entries.push(TraceRunPlanEntry {
            plan: trace_run_entry_plan(entries.len() + 1, group, iteration),
            index: entries.len() + 1,
            group: group.to_string(),
            iteration,
        });
    };
    match schedule {
        TraceSchedule::Grouped => {
            for group in groups {
                for iteration in 1..=repeat {
                    push_entry(group, iteration);
                }
            }
        }
        TraceSchedule::Interleaved => {
            for iteration in 1..=repeat {
                for group in groups {
                    push_entry(group, iteration);
                }
            }
        }
    }
    entries
}

fn trace_run_entry_plan(index: usize, group: &str, iteration: usize) -> HomeboyPlan {
    let mut plan = HomeboyPlan::for_description(PlanKind::Trace, format!("{group} {iteration}"));
    plan.mode = Some("run_order".to_string());
    plan.inputs.insert(
        "group".to_string(),
        serde_json::Value::String(group.to_string()),
    );
    plan.inputs.insert(
        "iteration".to_string(),
        serde_json::Value::Number(serde_json::Number::from(iteration)),
    );
    plan.steps = vec![PlanStep {
        id: format!("trace.run.{index}"),
        kind: "trace.run".to_string(),
        label: Some(format!("Run trace {group} iteration {iteration}")),
        blocking: true,
        scope: vec![group.to_string()],
        needs: Vec::new(),
        status: PlanStepStatus::Ready,
        inputs: plan.inputs.clone(),
        outputs: std::collections::HashMap::new(),
        skip_reason: None,
        policy: std::collections::HashMap::new(),
        missing: Vec::new(),
    }];
    plan.summary = Some(PlanSummary {
        total_steps: 1,
        ready: 1,
        blocked: 0,
        skipped: 0,
        next_actions: Vec::new(),
    });
    plan
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, ValueEnum)]
pub enum TraceVariantMatrixMode {
    #[default]
    None,
    Single,
    Cumulative,
}

impl TraceVariantMatrixMode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Single => "single",
            Self::Cumulative => "cumulative",
        }
    }
}
