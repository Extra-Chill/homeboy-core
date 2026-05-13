//! Release pipeline — planning, validation, and straight-line execution.
//!
//! `plan()` returns a serializable `ReleasePlan` for `--dry-run` / `--json`
//! consumers, and `run()` walks that same plan for real releases so the
//! previewed steps match execution.

use crate::component::{self, Component};
use crate::engine::command;
use crate::engine::local_files::FileSystem;
use crate::engine::run_dir::{self, RunDir};
use crate::engine::validation::ValidationCollector;
use crate::error::{Error, Result};
use crate::extension::{self, ExtensionManifest};
use crate::git;
use crate::release::changelog;
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

/// Fetch from remote and fast-forward if behind.
///
/// Ensures the release commit is created on top of the actual remote HEAD,
/// preventing detached release tags when PRs merge during a CI quality gate.
/// Returns Err if the branch has diverged and can't be fast-forwarded.
pub(crate) fn validate_remote_sync(component: &Component) -> Result<()> {
    let synced = git::fetch_and_fast_forward(&component.local_path)?;

    if let Some(n) = synced {
        log_status!(
            "release",
            "Fast-forwarded {} commit(s) from remote before release",
            n
        );
    }

    Ok(())
}

pub(crate) fn validate_default_branch(component: &Component) -> Result<()> {
    let current_branch = command::run_in_optional(
        &component.local_path,
        "git",
        &["symbolic-ref", "--short", "HEAD"],
    )
    .ok_or_else(|| {
        Error::validation_invalid_argument(
            "release",
            "Refusing to release from detached HEAD",
            None,
            Some(vec![
                "Check out the default branch before releasing".to_string()
            ]),
        )
    })?;

    let default_branch = command::run_in_optional(
        &component.local_path,
        "git",
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .map(|value| value.trim().trim_start_matches("origin/").to_string())
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| "main".to_string());

    if current_branch == default_branch {
        return Ok(());
    }

    Err(Error::validation_invalid_argument(
        "release",
        format!(
            "Refusing to release from non-default branch '{}' (default: '{}')",
            current_branch, default_branch
        ),
        None,
        Some(vec![
            format!("Check out '{}' before releasing", default_branch),
            "If you only want a preview, use --dry-run".to_string(),
        ]),
    ))
}

/// Run release lint via the component's extension.
///
/// Returns whether a lint command was available and executed. Missing lint
/// support is not a release blocker because not every extension provides it.
pub(crate) fn validate_lint_quality(component: &Component) -> Result<bool> {
    let lint_context = extension::lint::resolve_lint_command(component);

    let Ok(lint_context) = lint_context else {
        return Ok(false);
    };

    log_status!("release", "Running lint ({})...", lint_context.extension_id);

    let release_run_dir = RunDir::create()?;
    let lint_findings_file = release_run_dir.step_file(run_dir::files::LINT_FINDINGS);

    let output = extension::lint::build_lint_runner(
        component,
        None,
        &[],
        false,
        None,
        None,
        false,
        None,
        None,
        None,
        None,
        &release_run_dir,
    )
    .and_then(|runner| runner.run())
    .map_err(|e| quality_error("lint", format!("Lint runner error: {}", e)))?;

    let lint_passed = if output.success {
        true
    } else {
        let source_path = std::path::Path::new(&component.local_path);
        let findings = crate::extension::lint::baseline::parse_findings_file(&lint_findings_file)
            .unwrap_or_default();

        if let Some(baseline) = crate::extension::lint::baseline::load_baseline(source_path) {
            let comparison = crate::extension::lint::baseline::compare(&findings, &baseline);
            if comparison.drift_increased {
                log_status!(
                    "release",
                    "Lint baseline drift increased: {} new finding(s)",
                    comparison.new_items.len()
                );
                false
            } else {
                log_status!(
                    "release",
                    "Lint has known findings but no new drift (baseline honored)"
                );
                true
            }
        } else {
            false
        }
    };

    if lint_passed {
        log_status!("release", "Lint passed");
        Ok(true)
    } else {
        Err(quality_error(
            "lint",
            code_quality_failure_message("Lint", &output),
        ))
    }
}

/// Run release tests via the component's extension.
///
/// Returns whether a test command was available and executed. Missing test
/// support is not a release blocker because not every extension provides it.
pub(crate) fn validate_test_quality(component: &Component) -> Result<bool> {
    let test_context = extension::test::resolve_test_command(component);

    let Ok(test_context) = test_context else {
        return Ok(false);
    };

    log_status!(
        "release",
        "Running tests ({})...",
        test_context.extension_id
    );
    let test_run_dir = RunDir::create()?;
    let output = extension::test::build_test_runner(
        component,
        None,
        &[],
        false,
        false,
        None,
        None,
        &test_run_dir,
    )
    .and_then(|runner| runner.run())
    .map_err(|e| quality_error("test", format!("Test runner error: {}", e)))?;

    if output.success {
        log_status!("release", "Tests passed");
        Ok(true)
    } else {
        Err(quality_error(
            "test",
            code_quality_failure_message("Tests", &output),
        ))
    }
}

fn quality_error(field: &str, message: String) -> Error {
    log_status!("release", "Code quality check failed: {}", message);

    Error::validation_invalid_argument(
        field,
        message,
        None,
        Some(vec![
            "Fix the issue above before releasing".to_string(),
            "To bypass: homeboy release <component> --skip-checks".to_string(),
        ]),
    )
}

fn code_quality_failure_message(check: &str, output: &extension::RunnerOutput) -> String {
    if is_runner_infrastructure_failure(output) {
        format!(
            "{} runner infrastructure failure (exit code {})",
            check, output.exit_code
        )
    } else {
        format!("{} failed (exit code {})", check, output.exit_code)
    }
}

fn is_runner_infrastructure_failure(output: &extension::RunnerOutput) -> bool {
    if output.exit_code >= 2 || output.exit_code < 0 {
        return true;
    }

    let combined = format!("{}\n{}", output.stdout, output.stderr).to_lowercase();
    [
        "playground bootstrap helper not found",
        "playground php crash",
        "bootstrap failure:",
        "test harness infrastructure failure",
        "lint runner infrastructure failure",
        "failed opening required '/homeboy-extension/scripts/lib/playground-bootstrap.php'",
    ]
    .iter()
    .any(|needle| combined.contains(needle))
}

/// First-release bootstrap: if the component's configured `changelog_target`
/// doesn't exist on disk (and no fallback candidate exists), create a minimal
/// changelog scaffold so `resolve_changelog_path` + `finalize_with_generated_entries`
/// downstream have a file to work with.
///
/// Writes the standard first-run seed — no `## Unreleased` section. The downstream
/// `finalize_with_generated_entries` handles inserting the new `## [x.y.z] - YYYY-MM-DD`
/// section directly. See #1172 + #1205.
///
/// No-op when:
/// - `changelog_target` is unset (resolver emits a teaching error downstream),
/// - the configured path exists,
/// - a fallback candidate exists (resolver's discovery covers this case).
pub(crate) fn ensure_changelog_initialized(component: &Component) -> Result<()> {
    let Some(ref target) = component.changelog_target else {
        return Ok(());
    };

    let configured_path = crate::paths::resolve_path(&component.local_path, target);
    if configured_path.exists() {
        return Ok(());
    }

    let repo_root = std::path::Path::new(&component.local_path);
    if changelog::discover_changelog_relative_path(repo_root).is_some() {
        return Ok(());
    }

    if let Some(parent) = configured_path.parent() {
        crate::engine::local_files::local().ensure_dir(parent)?;
    }

    // Seed only. `finalize_with_generated_entries` will create the
    // `## [X.Y.Z]` section directly on top of this.
    crate::engine::local_files::local()
        .write(&configured_path, changelog::INITIAL_CHANGELOG_CONTENT)?;

    log_status!(
        "release",
        "Initialized changelog at {} (first release for {})",
        configured_path.display(),
        component.id
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        code_quality_failure_message, ensure_changelog_initialized,
        is_runner_infrastructure_failure, validate_default_branch,
    };
    use crate::component::Component;
    use crate::extension::RunnerOutput;

    fn run_git(dir: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {:?} failed: stdout={} stderr={}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_component(dir: &std::path::Path) -> Component {
        Component {
            id: "fixture".to_string(),
            local_path: dir.to_string_lossy().to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_validate_default_branch_allows_default_branch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path();
        run_git(dir, &["init", "-q"]);
        run_git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);

        validate_default_branch(&git_component(dir)).expect("main should be allowed");
    }

    #[test]
    fn test_validate_default_branch_blocks_non_default_branch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path();
        run_git(dir, &["init", "-q"]);
        run_git(dir, &["symbolic-ref", "HEAD", "refs/heads/feature"]);

        let err = validate_default_branch(&git_component(dir)).expect_err("feature should fail");

        assert_eq!(err.code.as_str(), "validation.invalid_argument");
        assert!(err.message.contains("non-default branch 'feature'"));
    }

    fn runner_output(exit_code: i32, stdout: &str, stderr: &str) -> RunnerOutput {
        RunnerOutput {
            exit_code,
            success: exit_code == 0,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }

    #[test]
    fn code_quality_failure_message_separates_test_findings_from_runner_infra() {
        let findings = runner_output(1, "FAILURES!\nTests: 3, Assertions: 4, Failures: 1", "");
        let infra = runner_output(
            2,
            "Error: Playground bootstrap helper not found at /tmp/missing",
            "",
        );

        assert!(!is_runner_infrastructure_failure(&findings));
        assert!(is_runner_infrastructure_failure(&infra));
        assert_eq!(
            code_quality_failure_message("Tests", &findings),
            "Tests failed (exit code 1)"
        );
        assert_eq!(
            code_quality_failure_message("Tests", &infra),
            "Tests runner infrastructure failure (exit code 2)"
        );
    }

    #[test]
    fn code_quality_failure_message_detects_pre_runner_playground_fatal_output() {
        let output = runner_output(
            1,
            "Fatal error: Uncaught Error: Failed opening required '/homeboy-extension/scripts/lib/playground-bootstrap.php'",
            "",
        );

        assert!(is_runner_infrastructure_failure(&output));
        assert_eq!(
            code_quality_failure_message("Tests", &output),
            "Tests runner infrastructure failure (exit code 1)"
        );
    }

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

    fn component_with_changelog_target(
        temp_dir: &tempfile::TempDir,
        target: Option<&str>,
    ) -> Component {
        Component {
            id: "test-component".to_string(),
            local_path: temp_dir.path().to_string_lossy().to_string(),
            remote_path: String::new(),
            changelog_target: target.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    /// Regression for #1172: first release with `changelog_target` configured
    /// but no file on disk. The preflight must create the file so downstream
    /// stages don't all fail with "File not found".
    ///
    /// Post-#1205: the scaffold only contains the `# Changelog` title, not a
    /// `## Unreleased` section. `finalize_with_generated_entries` inserts the
    /// `## [X.Y.Z]` section directly.
    #[test]
    fn ensure_changelog_initialized_creates_missing_file() {
        let temp = tempfile::tempdir().unwrap();
        let component = component_with_changelog_target(&temp, Some("CHANGELOG.md"));

        let changelog_path = temp.path().join("CHANGELOG.md");
        assert!(!changelog_path.exists(), "precondition: no changelog yet");

        ensure_changelog_initialized(&component).expect("preflight should bootstrap");

        let content = std::fs::read_to_string(&changelog_path).expect("file created");
        assert_eq!(content, super::changelog::INITIAL_CHANGELOG_CONTENT);
        assert!(
            !content.contains("## Unreleased"),
            "should NOT pre-create Unreleased section (legacy): {}",
            content
        );
    }

    /// Nested targets like `docs/CHANGELOG.md` must have the parent directory
    /// created before the file is written.
    #[test]
    fn ensure_changelog_initialized_creates_parent_dir_for_nested_target() {
        let temp = tempfile::tempdir().unwrap();
        let component = component_with_changelog_target(&temp, Some("docs/CHANGELOG.md"));

        let docs_dir = temp.path().join("docs");
        assert!(!docs_dir.exists(), "precondition: no docs/ yet");

        ensure_changelog_initialized(&component).expect("preflight should bootstrap");

        assert!(docs_dir.is_dir(), "docs/ parent should be created");
        assert!(
            temp.path().join("docs/CHANGELOG.md").exists(),
            "changelog should land at docs/CHANGELOG.md"
        );
    }

    /// Idempotent: if the configured path already exists, leave it alone.
    /// A second release run must not overwrite an existing changelog.
    #[test]
    fn ensure_changelog_initialized_leaves_existing_file_untouched() {
        let temp = tempfile::tempdir().unwrap();
        let component = component_with_changelog_target(&temp, Some("CHANGELOG.md"));
        let changelog_path = temp.path().join("CHANGELOG.md");
        let original = "# Changelog\n\n## [1.0.0] - 2026-01-01\n\n### Added\n- real release\n";
        std::fs::write(&changelog_path, original).unwrap();

        ensure_changelog_initialized(&component).expect("no-op on existing file");

        let after = std::fs::read_to_string(&changelog_path).unwrap();
        assert_eq!(after, original, "existing changelog must not be rewritten");
    }

    /// If a fallback candidate (e.g. `docs/CHANGELOG.md`) exists but the
    /// configured target points elsewhere, prefer the fallback (the resolver
    /// already does this) instead of creating a second changelog alongside.
    #[test]
    fn ensure_changelog_initialized_defers_to_existing_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let component = component_with_changelog_target(&temp, Some("CHANGELOG.md"));

        // Create a fallback candidate at a different location.
        std::fs::create_dir_all(temp.path().join("docs")).unwrap();
        let fallback = temp.path().join("docs/CHANGELOG.md");
        std::fs::write(&fallback, "# Changelog\n\n## [0.1.0] - 2026-01-01\n").unwrap();

        ensure_changelog_initialized(&component).expect("defer to fallback");

        // The configured target should NOT have been created — the resolver
        // will pick up the fallback instead.
        assert!(
            !temp.path().join("CHANGELOG.md").exists(),
            "should not create duplicate when fallback exists"
        );
    }

    /// If no `changelog_target` is configured at all, the preflight is a
    /// no-op — `resolve_changelog_path()` downstream emits its existing
    /// "No changelog configured" teaching error.
    #[test]
    fn ensure_changelog_initialized_is_noop_without_configured_target() {
        let temp = tempfile::tempdir().unwrap();
        let component = component_with_changelog_target(&temp, None);

        ensure_changelog_initialized(&component).expect("no-op without target");

        // No file created.
        for entry in std::fs::read_dir(temp.path()).unwrap() {
            let path = entry.unwrap().path();
            panic!("should have created nothing, but found: {}", path.display());
        }
    }
}
