//! Release pipeline — straight-line release execution.
//!
//! `planner::plan()` returns the serializable release plan, and `run()` walks
//! that same plan for real releases so the previewed steps match execution.

use crate::error::Result;
use crate::git;
use std::collections::HashSet;

use super::context::load_component;
use super::execution_plan::{
    build_initial_preflight_plan, execute_plan_steps, initial_executable_preflight_ids,
};
use super::pipeline_summary::{build_summary, derive_overall_status};
use super::planner::plan;
use super::types::{ReleaseOptions, ReleasePlan, ReleaseRun, ReleaseRunResult, ReleaseStepResult};

/// Execute a release end-to-end.
///
/// Runs the preflight validations (via [`plan`]), then walks the release
/// steps in order, threading [`ReleaseState`] between them. Steps that fail
/// cause subsequent steps to be marked `Skipped` but execution continues so
/// the caller gets a full per-step result list; post-release hooks still
/// run so any failure can be observed.
///
/// What you preview with `--dry-run` is what executes.
pub fn run(component_id: &str, options: &ReleaseOptions) -> Result<ReleaseRun> {
    run_with_plan(component_id, options).map(|(_plan, run)| run)
}

/// Execute a release and return the plan that drove it alongside the run.
pub(crate) fn run_with_plan(
    component_id: &str,
    options: &ReleaseOptions,
) -> Result<(ReleasePlan, ReleaseRun)> {
    let mut results: Vec<ReleaseStepResult> = Vec::new();

    let initial_plan = build_initial_preflight_plan(component_id, options);
    let initial_stop = execute_plan_steps(
        &initial_plan.steps,
        component_id,
        options,
        &mut results,
        &HashSet::new(),
    )?;

    if initial_stop {
        let component = load_component(component_id, options)?;
        let monorepo = git::MonorepoContext::detect(&component.local_path, component_id);
        return Ok((
            initial_plan,
            finalize(component_id, results, monorepo.as_ref()),
        ));
    }

    // Rebuild the full plan after executable preflights. `preflight.remote_sync`
    // may fast-forward HEAD and `preflight.changelog_bootstrap` may create the
    // first changelog file; changelog/version planning must observe those
    // changes instead of stale checkout state.
    let release_plan = plan(component_id, options)?;
    let completed_preflights: HashSet<&'static str> =
        initial_executable_preflight_ids().iter().copied().collect();

    let full_stop = execute_plan_steps(
        &release_plan.steps,
        component_id,
        options,
        &mut results,
        &completed_preflights,
    )?;

    let component = load_component(component_id, options)?;
    let monorepo = git::MonorepoContext::detect(&component.local_path, component_id);

    if full_stop {
        return Ok((
            release_plan,
            finalize(component_id, results, monorepo.as_ref()),
        ));
    }

    Ok((
        release_plan,
        finalize(component_id, results, monorepo.as_ref()),
    ))
}

/// Wrap the accumulated step results into a `ReleaseRun` with an overall
/// status and a human-friendly summary.
fn finalize(
    component_id: &str,
    results: Vec<ReleaseStepResult>,
    _monorepo: Option<&git::MonorepoContext>,
) -> ReleaseRun {
    let status = derive_overall_status(&results);
    let summary = build_summary(&results, &status);

    ReleaseRun {
        component_id: component_id.to_string(),
        enabled: true,
        result: ReleaseRunResult {
            steps: results,
            status,
            warnings: Vec::new(),
            summary: Some(summary),
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn release_runtime_core_stays_ecosystem_agnostic() {
        let files = [
            ("executor.rs", include_str!("executor.rs")),
            ("pipeline.rs", include_str!("pipeline.rs")),
            ("version.rs", include_str!("version.rs")),
        ];
        let forbidden_terms = ["Cargo", "cargo", "Rust", "rust"];

        for (file, source) in files {
            let runtime_source = source.split("#[cfg(test)]").next().unwrap_or(source);
            for term in forbidden_terms {
                assert!(
                    !runtime_source.contains(term),
                    "release runtime core must not branch on ecosystem-specific term {term:?} in {file}"
                );
            }
        }
    }
}
