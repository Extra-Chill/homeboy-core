//! Compatibility shim for historical `crate::core::refactor::planner` imports.
//!
//! Planner ownership now lives under `crate::refactor::plan::planner`.

pub use crate::refactor::plan::planner::{
    analyze_stage_overlaps, build_refactor_plan, lint_refactor_request, normalize_sources,
    run_lint_refactor, run_test_refactor, summarize_plan_totals, test_refactor_request,
    LintSourceOptions, PlanOverlap, PlanStageSummary, RefactorPlan, RefactorPlanRequest,
    TestSourceOptions, KNOWN_PLAN_SOURCES,
};
