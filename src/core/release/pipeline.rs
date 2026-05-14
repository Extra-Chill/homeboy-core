//! Release pipeline — planning, validation, and straight-line execution.
//!
//! `plan()` returns a serializable `ReleasePlan` for `--dry-run` / `--json`
//! consumers, and `run()` walks that same plan for real releases so the
//! previewed steps match execution.

use crate::component::{self, Component};
use crate::engine::validation::ValidationCollector;
use crate::error::{Error, Result};
use crate::extension::{self, ExtensionManifest};
use crate::git;
use crate::version;
use std::collections::HashSet;

use super::execution_plan::{
    build_initial_preflight_plan, execute_plan_steps, initial_executable_preflight_ids,
};
use super::pipeline_summary::{build_summary, derive_overall_status};
use super::plan_steps::{build_preflight_steps, build_release_steps};
use super::planning_changelog::{build_changelog_plan, generate_changelog_entries};
use super::planning_policy::release_skip_plan;
use super::planning_semver::{build_semver_recommendation, validate_release_version_floor};
use super::planning_worktree::validate_release_worktree;
use super::types::{ReleaseOptions, ReleasePlan, ReleaseRun, ReleaseRunResult, ReleaseStepResult};

/// Load a component with portable config fallback when path_override is set.
/// In CI environments, the component may not be registered — only homeboy.json exists.
pub(crate) fn load_component(component_id: &str, options: &ReleaseOptions) -> Result<Component> {
    component::resolve_effective(Some(component_id), options.path_override.as_deref(), None)
}

/// Resolve the component's declared extensions (for publish/package dispatch).
pub(super) fn resolve_extensions(component: &Component) -> Result<Vec<ExtensionManifest>> {
    let mut extensions = Vec::new();
    if let Some(configured) = component.extensions.as_ref() {
        let mut extension_ids: Vec<String> = configured.keys().cloned().collect();
        extension_ids.sort();
        let suggestions = extension::available_extension_ids();
        for extension_id in extension_ids {
            let manifest = extension::load_extension(&extension_id).map_err(|_| {
                Error::extension_not_found(extension_id.to_string(), suggestions.clone())
            })?;
            extensions.push(manifest);
        }
    }
    Ok(extensions)
}

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

/// Plan a release: run all preflight validations, then return a description
/// of the steps the executor will run. Used by `--dry-run` to preview work
/// without side effects and by [`run`] to drive validation + auto-generated
/// changelog entries.
///
/// Requires a clean working tree (uncommitted changes cause an error).
///
/// Core steps (always generated):
/// 1. Version bump + changelog finalization
/// 2. Git commit
/// 3. Git tag
/// 4. Git push (commits AND tags)
///
/// Extension-derived steps, added when applicable:
/// - `release.prepare` — component has an extension with `release.prepare` action
/// - `package` — component has an extension with `release.package` action
/// - `publish.<target>` — one per extension with `release.publish` action
/// - `cleanup` — after publish (skipped with `--deploy`)
/// - `github.release` — component's remote resolves to a github.com URL
/// - `post_release` — component defines post-release hook commands
pub fn plan(component_id: &str, options: &ReleaseOptions) -> Result<ReleasePlan> {
    let component = load_component(component_id, options)?;
    let extensions = resolve_extensions(&component)?;

    let mut v = ValidationCollector::new();

    // Detect monorepo context for path-scoped commits and component-prefixed tags.
    let monorepo = git::MonorepoContext::detect(&component.local_path, component_id);

    // Build semver recommendation from commits early so both JSON output and
    // validation paths share a single source of truth.
    let semver_recommendation =
        build_semver_recommendation(&component, &options.bump_type, monorepo.as_ref())?;

    if let Some(skip_plan) = release_skip_plan(component_id, options, semver_recommendation.clone())
    {
        return Ok(skip_plan);
    }

    // === Stage 1: Generate changelog entries from conventional commits ===
    //
    // Homeboy owns the changelog end-to-end: entries come from commits, get
    // finalized into a `## [X.Y.Z]` section by `bump_component_version`, and
    // are written to disk in one shot. Users never hand-curate entries.
    //
    // An empty commit set is a clean gate — a zero-commit release makes no
    // sense in the automation model, so the generator errors out.
    let pending_entries = v
        .capture(
            generate_changelog_entries(&component, component_id, options, monorepo.as_ref()),
            "commits",
        )
        .unwrap_or_default();

    let version_info = v.capture(version::read_component_version(&component), "version");

    // === Stage 2: Version bump math ===
    let new_version = if let Some(ref info) = version_info {
        match version::increment_version(&info.version, &options.bump_type) {
            Some(ver) => Some(ver),
            None => {
                v.push(
                    "version",
                    &format!("Invalid version format: {}", info.version),
                    None,
                );
                None
            }
        }
    } else {
        None
    };

    if let (Some(ref info), Some(ref next_version)) = (&version_info, &new_version) {
        if let Some(message) = validate_release_version_floor(
            semver_recommendation
                .as_ref()
                .and_then(|rec| rec.latest_tag.as_deref()),
            &info.version,
            next_version,
        ) {
            v.push("version", &message, None);
        }
    }

    // === Stage 3: Working tree check ===
    if let Some(ref info) = version_info {
        if let Some(details) = validate_release_worktree(&component, options, info)? {
            v.push(
                "working_tree",
                "Uncommitted changes detected",
                Some(details),
            );
        }
    }

    // === Return aggregated errors or proceed ===
    v.finish()?;

    // All validations passed — these are guaranteed Some by the validator above
    let version_info = version_info.ok_or_else(|| {
        Error::internal_unexpected("version_info missing after validation".to_string())
    })?;
    let new_version = new_version.ok_or_else(|| {
        Error::internal_unexpected("new_version missing after validation".to_string())
    })?;

    let mut warnings = Vec::new();
    let mut hints = Vec::new();
    let changelog_plan = build_changelog_plan(&component, options, pending_entries)?;

    let mut steps = build_preflight_steps(options, semver_recommendation.as_ref());
    steps.extend(build_release_steps(
        &component,
        &extensions,
        &version_info.version,
        &new_version,
        &changelog_plan,
        options,
        monorepo.as_ref(),
        &mut warnings,
        &mut hints,
    )?);

    if options.dry_run {
        hints.push("Dry run: no changes will be made".to_string());
    }

    Ok(ReleasePlan {
        component_id: component_id.to_string(),
        enabled: true,
        steps,
        semver_recommendation,
        warnings,
        hints,
    })
}

#[cfg(test)]
mod tests {
    /// Regression for the homeboy-action release blocker:
    /// `validate_working_tree_fail_fast` builds an Error with a hint vec
    /// listing the dirty files. That error flows through ValidationCollector,
    /// which used to drop the hints on the single-error re-emit path —
    /// leaving CI consumers with a bare `Uncommitted changes detected`
    /// message and no way to see *which* files were dirty.
    ///
    /// This test pins down the round-trip: build the same shape of error
    /// that `validate_working_tree_fail_fast` would produce, push it through
    /// `ValidationCollector::finish_if_errors`, and assert the dirty file
    /// hints survive in the resulting JSON details.
    #[test]
    fn working_tree_fail_fast_error_preserves_file_hints_through_collector() {
        use crate::engine::validation::ValidationCollector;
        use crate::error::Error;

        let original = Error::validation_invalid_argument(
            "working_tree",
            "Uncommitted changes detected — refusing to release",
            None,
            Some(vec![
                "Commit, stash, or discard changes before releasing".to_string(),
                "Unexpected dirty files (2): src/lib.rs, Cargo.lock".to_string(),
            ]),
        );

        let mut collector = ValidationCollector::new();
        collector.capture::<()>(Err(original), "working_tree");
        let propagated = collector.finish_if_errors().unwrap_err();

        let details = &propagated.details;
        let tried = details
            .get("tried")
            .and_then(|v| v.as_array())
            .expect("tried hints must survive collector round-trip");
        assert_eq!(tried.len(), 2, "expected both hints to survive: {details}");
        let joined: String = tried
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            joined.contains("src/lib.rs"),
            "dirty file list must reach the JSON envelope, got: {joined}"
        );
        assert!(
            joined.contains("Cargo.lock"),
            "dirty file list must reach the JSON envelope, got: {joined}"
        );
    }

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
