use crate::engine::command;
use crate::error::{Error, Result};
use crate::git;
use std::io::{self, BufRead, IsTerminal, Write};

use super::pipeline::load_component;
use super::types::{
    BatchReleaseComponentResult, BatchReleaseResult, BatchReleaseSummary, ReleaseCommandInput,
    ReleaseCommandResult, ReleaseOptions, ReleasePlan, ReleasePlanStatus, ReleasePlanStep,
    ReleaseRun,
};

pub fn run_command(input: ReleaseCommandInput) -> Result<(ReleaseCommandResult, i32)> {
    if input.recover {
        return run_recover(&input);
    }

    let component = load_component(
        &input.component_id,
        &ReleaseOptions {
            path_override: input.path_override.clone(),
            ..Default::default()
        },
    )?;

    if !input.dry_run {
        ensure_release_on_default_branch(&component.local_path)?;

        // Configure git identity for release commits/tags
        if let Some(ref identity_str) = input.git_identity {
            let identity = git::parse_git_identity(Some(identity_str));
            git::configure_identity(&component.local_path, &identity)?;
            log_status!(
                "release",
                "Git identity: {} <{}>",
                identity.name,
                identity.email
            );
        }
    }

    let monorepo = git::MonorepoContext::detect(&component.local_path, &input.component_id);
    let (auto_bump_type, releasable_count) =
        match resolve_bump(&component.local_path, monorepo.as_ref())? {
            Some(result) => result,
            None => {
                // No releasable commits, but --bump can still force a release
                if input.bump_override.is_some() {
                    ("none".to_string(), 0)
                } else {
                    log_status!(
                        "release",
                        "No releasable commits since last tag — nothing to release"
                    );
                    return Ok((
                        ReleaseCommandResult {
                            component_id: input.component_id.clone(),
                            bump_type: "none".to_string(),
                            dry_run: input.dry_run,
                            releasable_commits: 0,
                            new_version: None,
                            tag: None,
                            skipped_reason: Some("no-releasable-commits".to_string()),
                            plan: Some(skipped_release_plan(
                                &input.component_id,
                                "no-releasable-commits",
                                "No releasable commits since last tag",
                                "Use --bump to force a release when this is intentional",
                            )),
                            run: None,
                            deployment: None,
                        },
                        0,
                    ));
                }
            }
        };

    let has_breaking_commits = auto_bump_type == "major";

    // Resolve the effective bump type: --bump overrides auto-detection.
    let bump_type = if let Some(ref override_value) = input.bump_override {
        // Check if it's an explicit version string (e.g. "2.0.0")
        let is_explicit_version = override_value.contains('.');

        if is_explicit_version {
            // Explicit version — pass through as-is, skip all semver logic
            if has_breaking_commits {
                log_status!(
                    "release",
                    "Breaking changes detected in commits — releasing as explicit version {}",
                    override_value
                );
            }
            override_value.clone()
        } else {
            // Semver keyword: major, minor, patch
            let bump = override_value.to_lowercase();
            if !["major", "minor", "patch"].contains(&bump.as_str()) {
                return Err(Error::validation_invalid_argument(
                    "bump",
                    format!(
                        "Invalid --bump value '{}'. Use: major, minor, patch, or a version like 2.0.0",
                        override_value
                    ),
                    Some(override_value.clone()),
                    None,
                ));
            }

            let mut forced_lower_bump = false;
            if let Some(under_bump) =
                detect_lower_bump_override(&bump, &auto_bump_type, releasable_count)?
            {
                guard_lower_bump_override(
                    &input.component_id,
                    &under_bump,
                    input.force_lower_bump,
                    input.dry_run,
                )?;
                forced_lower_bump = input.force_lower_bump && !input.dry_run;
            }

            if forced_lower_bump {
                log_status!(
                    "release",
                    "Forced lower bump: requested {} while commits indicate {}",
                    bump,
                    auto_bump_type
                );
            }

            log_status!(
                "release",
                "Using --bump {} (overriding auto-detected {} from {} commit{})",
                bump,
                auto_bump_type,
                releasable_count,
                if releasable_count == 1 { "" } else { "s" }
            );
            bump
        }
    } else {
        // No override — use auto-detected bump type
        let mut bump_type = auto_bump_type;

        // Pre-1.0 semver: breaking changes bump minor, not major.
        // In semver, 0.x.y signals "initial development" where the public API is
        // not stable. Breaking changes are expected and land as minor bumps.
        // A major bump to 1.0.0 should only happen when the author explicitly
        // decides the API is stable (via --bump major).
        if bump_type == "major" {
            let current_version = super::version::read_version(Some(&input.component_id))
                .ok()
                .and_then(|v| v.version.split('.').next().map(String::from))
                .unwrap_or_default();
            if current_version == "0" {
                log_status!(
                    "release",
                    "Pre-1.0: downgrading major → minor (breaking changes are minor bumps in 0.x)"
                );
                bump_type = "minor".to_string();
            }
        }

        // Gate: auto-detected major requires explicit --bump major
        if bump_type == "major" {
            log_status!(
                "release",
                "⚠ Breaking changes detected — this requires a major version bump"
            );
            log_status!(
                "release",
                "Re-run with: homeboy release {} --bump major",
                input.component_id
            );
            return Ok((
                ReleaseCommandResult {
                    component_id: input.component_id.clone(),
                    bump_type: "major".to_string(),
                    dry_run: input.dry_run,
                    releasable_commits: releasable_count,
                    new_version: None,
                    tag: None,
                    skipped_reason: Some("major-requires-flag".to_string()),
                    plan: Some(skipped_release_plan(
                        &input.component_id,
                        "major-requires-flag",
                        "Breaking changes require an explicit major bump",
                        &format!(
                            "Re-run with: homeboy release {} --bump major",
                            input.component_id
                        ),
                    )),
                    run: None,
                    deployment: None,
                },
                0,
            ));
        }

        log_status!(
            "release",
            "Detected {} bump from {} releasable commit{}",
            bump_type,
            releasable_count,
            if releasable_count == 1 { "" } else { "s" }
        );

        bump_type
    };

    let options = ReleaseOptions {
        bump_type: bump_type.clone(),
        dry_run: input.dry_run,
        path_override: input.path_override,
        skip_checks: input.skip_checks,
        skip_publish: input.skip_publish,
        deploy: input.deploy,
        skip_github_release: input.skip_github_release,
    };

    if options.dry_run {
        let plan = super::plan(&input.component_id, &options)?;
        let new_version = extract_new_version_from_plan(&plan);
        let tag = new_version
            .as_ref()
            .map(|v| format_tag(v, monorepo.as_ref()));
        let deployment = input
            .deploy
            .then(|| super::deployment::plan_deployment(&input.component_id));

        return Ok((
            ReleaseCommandResult {
                component_id: input.component_id,
                bump_type,
                dry_run: true,
                releasable_commits: releasable_count,
                new_version,
                tag,
                skipped_reason: None,
                plan: Some(plan),
                run: None,
                deployment,
            },
            0,
        ));
    }

    let (plan, run_result) = super::pipeline::run_with_plan(&input.component_id, &options)?;
    display_release_summary(&run_result);

    let new_version = extract_new_version_from_run(&run_result);
    let tag = new_version
        .as_ref()
        .map(|v| format_tag(v, monorepo.as_ref()));
    let post_release_exit = if has_post_release_warnings(&run_result) {
        3
    } else {
        0
    };
    let deployment = super::deployment::extract_deployment_from_run(&run_result);
    let deploy_exit_code = deployment
        .as_ref()
        .filter(|deployment| deployment.summary.failed > 0)
        .map(|_| 1)
        .unwrap_or(0);
    let exit_code = if deploy_exit_code != 0 {
        // Deploy failed after the release was already tagged and pushed.
        // The tag cannot be rolled back safely, so warn the user to retry.
        if let Some(ref t) = tag {
            eprintln!();
            log_status!(
                "release",
                "⚠️  Release {} was tagged and pushed, but deploy FAILED.",
                t
            );
            log_status!(
                "release",
                "Run `homeboy deploy {}` to finish deploying.",
                input.component_id
            );
        }
        deploy_exit_code
    } else {
        post_release_exit
    };

    Ok((
        ReleaseCommandResult {
            component_id: input.component_id,
            bump_type,
            dry_run: false,
            releasable_commits: releasable_count,
            new_version,
            tag,
            skipped_reason: None,
            plan: Some(plan),
            run: Some(run_result),
            deployment,
        },
        exit_code,
    ))
}

fn resolve_bump(
    local_path: &str,
    monorepo: Option<&git::MonorepoContext>,
) -> Result<Option<(String, usize)>> {
    let (_latest_tag, commits) = super::pipeline::resolve_tag_and_commits(local_path, monorepo)?;

    if commits.is_empty() {
        return Ok(None);
    }

    match git::recommended_bump_from_commits(&commits) {
        Some(bump) => {
            let releasable = commits
                .iter()
                .filter(|c| c.category.to_changelog_entry_type().is_some())
                .count();
            Ok(Some((bump.as_str().to_string(), releasable)))
        }
        None => Ok(None),
    }
}

/// Format a version string as a tag name, using component prefix in monorepos.
fn format_tag(version: &str, monorepo: Option<&git::MonorepoContext>) -> String {
    match monorepo {
        Some(ctx) => ctx.format_tag(version),
        None => format!("v{}", version),
    }
}

fn ensure_release_on_default_branch(local_path: &str) -> Result<()> {
    let current_branch =
        command::run_in_optional(local_path, "git", &["symbolic-ref", "--short", "HEAD"])
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
        local_path,
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

fn extract_new_version_from_plan(plan: &ReleasePlan) -> Option<String> {
    plan.steps
        .iter()
        .find(|s| s.step_type == "version")
        .and_then(|s| s.config.get("to"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn skipped_release_plan(component_id: &str, reason: &str, label: &str, hint: &str) -> ReleasePlan {
    ReleasePlan {
        component_id: component_id.to_string(),
        enabled: false,
        steps: vec![ReleasePlanStep {
            id: "release.skip".to_string(),
            step_type: "release.skip".to_string(),
            label: Some(label.to_string()),
            needs: vec![],
            config: std::collections::HashMap::from([(
                "reason".to_string(),
                serde_json::Value::String(reason.to_string()),
            )]),
            status: ReleasePlanStatus::Disabled,
            missing: vec![],
        }],
        semver_recommendation: None,
        warnings: vec![],
        hints: vec![hint.to_string()],
    }
}

fn extract_new_version_from_run(run: &ReleaseRun) -> Option<String> {
    run.result
        .steps
        .iter()
        .find(|s| s.step_type == "version")
        .and_then(|s| s.data.as_ref())
        .and_then(|d| d.get("new_version").or_else(|| d.get("to")))
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn display_release_summary(run: &ReleaseRun) {
    if let Some(ref summary) = run.result.summary {
        if !summary.success_summary.is_empty() {
            eprintln!();
            for line in &summary.success_summary {
                log_status!("release", "{}", line);
            }
        }
    }
}

fn has_post_release_warnings(run: &ReleaseRun) -> bool {
    run.result.steps.iter().any(|step| {
        step.step_type == "post_release"
            && step
                .data
                .as_ref()
                .and_then(|d| d.get("all_succeeded"))
                .and_then(|v| v.as_bool())
                == Some(false)
    })
}

fn run_recover(input: &ReleaseCommandInput) -> Result<(ReleaseCommandResult, i32)> {
    let component = load_component(
        &input.component_id,
        &ReleaseOptions {
            path_override: input.path_override.clone(),
            ..Default::default()
        },
    )?;

    // Configure git identity for recovery commits/tags
    if let Some(ref identity_str) = input.git_identity {
        let identity = git::parse_git_identity(Some(identity_str));
        git::configure_identity(&component.local_path, &identity)?;
    }

    let monorepo = git::MonorepoContext::detect(&component.local_path, &input.component_id);
    let version_info = crate::version::read_component_version(&component)?;
    let current_version = &version_info.version;
    let tag_name = format_tag(current_version, monorepo.as_ref());

    // Surface the orphan-tag pattern from issue #2234. When the latest release
    // tag points at a commit whose subject is *not* `release: vX.Y.Z`, the
    // previous release was botched (tag without bump). Recover should warn
    // loudly so the operator can decide whether to delete the orphan tag, hand
    // back-fill a release: commit, or run `--recover` to commit the version
    // files at the tagged commit.
    if let Some(latest_tag) = latest_release_tag(&component.local_path, monorepo.as_ref()) {
        if let Some(diagnostic) = diagnose_orphan_tag(&component.local_path, &latest_tag) {
            log_status!("recover", "{}", diagnostic);
        }
    }

    let tag_exists_local =
        git::tag_exists_locally(&component.local_path, &tag_name).unwrap_or(false);
    let tag_exists_remote =
        git::tag_exists_on_remote(&component.local_path, &tag_name).unwrap_or(false);
    let uncommitted = git::get_uncommitted_changes(&component.local_path)?;

    let mut actions = Vec::new();

    if uncommitted.has_changes {
        log_status!("recover", "Committing uncommitted changes...");
        let msg = format!("release: v{}", current_version);
        let commit_result = git::commit(
            Some(&input.component_id),
            Some(msg.as_str()),
            git::CommitOptions {
                staged_only: false,
                files: None,
                exclude: None,
                amend: false,
            },
        )?;
        if !commit_result.success {
            return Err(Error::git_command_failed(format!(
                "Failed to commit: {}",
                commit_result.stderr
            )));
        }
        actions.push("committed version files".to_string());
    }

    if !tag_exists_local {
        log_status!("recover", "Creating tag {}...", tag_name);
        let tag_result = git::tag(
            Some(&input.component_id),
            Some(&tag_name),
            Some(&format!("Release {}", tag_name)),
        )?;
        if !tag_result.success {
            return Err(Error::git_command_failed(format!(
                "Failed to create tag: {}",
                tag_result.stderr
            )));
        }
        actions.push(format!("created tag {}", tag_name));
    }

    if !tag_exists_remote {
        log_status!("recover", "Pushing to remote...");
        let push_result = git::push(
            Some(&input.component_id),
            git::PushOptions {
                tags: true,
                force_with_lease: false,
            },
        )?;
        if !push_result.success {
            return Err(Error::git_command_failed(format!(
                "Failed to push: {}",
                push_result.stderr
            )));
        }
        actions.push("pushed commits and tags".to_string());
    }

    if actions.is_empty() {
        log_status!(
            "recover",
            "Release v{} appears complete — nothing to recover.",
            current_version
        );
    } else {
        log_status!(
            "recover",
            "Recovery complete for v{}: {}",
            current_version,
            actions.join(", ")
        );
    }

    Ok((
        ReleaseCommandResult {
            component_id: input.component_id.clone(),
            bump_type: "recover".to_string(),
            dry_run: false,
            releasable_commits: 0,
            new_version: None,
            tag: None,
            skipped_reason: None,
            plan: None,
            run: None,
            deployment: None,
        },
        0,
    ))
}

/// Resolve the most recent release-shaped tag for the component, honoring
/// monorepo prefixes. Returns `None` if no matching tag is found.
fn latest_release_tag(local_path: &str, monorepo: Option<&git::MonorepoContext>) -> Option<String> {
    match monorepo {
        Some(ctx) => git::get_latest_tag_with_prefix(&ctx.git_root, Some(&ctx.tag_prefix)).ok()?,
        None => git::get_latest_tag(local_path).ok()?,
    }
}

/// Inspect the latest release tag for the orphan-tag pattern (#2234): a tag
/// whose tagged commit subject is not `release: vX.Y.Z`. Returns a one-line
/// warning when the tag looks orphaned, otherwise `None`.
///
/// This is intentionally a soft warning — `--recover` may still be the
/// right move (re-commit the working tree), but the operator deserves to
/// know they're recovering on top of a misplaced tag before they push more
/// state to origin.
fn diagnose_orphan_tag(local_path: &str, tag: &str) -> Option<String> {
    let tag_commit = git::get_tag_commit(local_path, tag).ok()?;
    let subject_output =
        git::execute_git_for_release(local_path, &["log", "-1", "--format=%s", &tag_commit])
            .ok()?;
    if !subject_output.status.success() {
        return None;
    }
    let subject = String::from_utf8_lossy(&subject_output.stdout)
        .trim()
        .to_string();

    if subject.starts_with("release: v") || subject.starts_with("release:v") {
        return None;
    }

    Some(format!(
        "⚠ Latest tag {} points at commit {} ({}) — not a `release: v...` commit. \
         This matches the orphan-tag pattern from issue #2234. Inspect the tag/commit before recovering: \
         `git show {}`. To delete a misplaced tag locally and on origin: \
         `git tag -d {} && git push origin :refs/tags/{}`",
        tag,
        &tag_commit[..8.min(tag_commit.len())],
        subject,
        tag,
        tag,
        tag,
    ))
}

/// Run releases for multiple components sequentially.
///
/// Continue-on-error: if one component fails, the rest still run.
/// Each component releases independently (own tag, own push).
pub fn run_batch(
    component_ids: &[String],
    input_template: &ReleaseCommandInput,
) -> BatchReleaseResult {
    let mut results = Vec::new();
    let mut released: u32 = 0;
    let mut skipped: u32 = 0;
    let mut failed: u32 = 0;

    for component_id in component_ids {
        log_status!(
            "release",
            "--- Releasing '{}' ({}/{}) ---",
            component_id,
            results.len() + 1,
            component_ids.len()
        );

        let input = ReleaseCommandInput {
            component_id: component_id.clone(),
            path_override: None,
            dry_run: input_template.dry_run,
            deploy: input_template.deploy,
            recover: input_template.recover,
            skip_checks: input_template.skip_checks,
            bump_override: input_template.bump_override.clone(),
            force_lower_bump: input_template.force_lower_bump,
            skip_publish: input_template.skip_publish,
            skip_github_release: input_template.skip_github_release,
            git_identity: input_template.git_identity.clone(),
        };

        match run_command(input) {
            Ok((result, _exit_code)) => {
                let was_skipped = result.skipped_reason.is_some();
                let status = if was_skipped {
                    skipped += 1;
                    "skipped"
                } else {
                    released += 1;
                    "released"
                };

                results.push(BatchReleaseComponentResult {
                    component_id: component_id.clone(),
                    status: status.to_string(),
                    error: None,
                    result: Some(result),
                });
            }
            Err(e) => {
                log_status!("release", "Failed to release '{}': {}", component_id, e);
                failed += 1;
                results.push(BatchReleaseComponentResult {
                    component_id: component_id.clone(),
                    status: "failed".to_string(),
                    error: Some(e.to_string()),
                    result: None,
                });
            }
        }
    }

    let total = results.len() as u32;

    // Log summary
    if total > 1 {
        log_status!("release", "--- Batch summary ---");
        log_status!(
            "release",
            "{} component(s): {} released, {} skipped, {} failed",
            total,
            released,
            skipped,
            failed
        );
    }

    BatchReleaseResult {
        results,
        summary: BatchReleaseSummary {
            total,
            released,
            skipped,
            failed,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LowerBumpOverride {
    requested: String,
    detected: String,
    releasable_count: usize,
}

impl LowerBumpOverride {
    fn commit_suffix(&self) -> &'static str {
        if self.releasable_count == 1 {
            ""
        } else {
            "s"
        }
    }

    fn message(&self) -> String {
        format!(
            "Requested {} bump is lower than detected {} impact from {} releasable commit{}",
            self.requested,
            self.detected,
            self.releasable_count,
            self.commit_suffix()
        )
    }
}

fn detect_lower_bump_override(
    requested_bump: &str,
    detected_bump: &str,
    releasable_count: usize,
) -> Result<Option<LowerBumpOverride>> {
    let Some(requested) = git::SemverBump::parse(requested_bump) else {
        return Ok(None);
    };
    let Some(detected) = git::SemverBump::parse(detected_bump) else {
        return Ok(None);
    };

    if requested.rank() >= detected.rank() {
        return Ok(None);
    }

    Ok(Some(LowerBumpOverride {
        requested: requested.as_str().to_string(),
        detected: detected.as_str().to_string(),
        releasable_count,
    }))
}

fn guard_lower_bump_override(
    component_id: &str,
    under_bump: &LowerBumpOverride,
    force_lower_bump: bool,
    dry_run: bool,
) -> Result<()> {
    if dry_run {
        log_status!("release", "{}", under_bump.message());
        return Ok(());
    }

    if force_lower_bump {
        return Ok(());
    }

    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        log_status!("release", "{}.", under_bump.message());
        eprint!("Continue with lower bump? [y/N] ");
        io::stderr().flush().ok();

        let mut answer = String::new();
        io::stdin().lock().read_line(&mut answer).map_err(|e| {
            Error::internal_unexpected(format!("Failed to read confirmation: {}", e))
        })?;

        if matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
            return Ok(());
        }
    }

    Err(lower_bump_error(component_id, under_bump))
}

fn lower_bump_error(component_id: &str, under_bump: &LowerBumpOverride) -> Error {
    Error::validation_invalid_argument(
        "bump",
        under_bump.message(),
        Some(under_bump.requested.clone()),
        None,
    )
    .with_hint(format!(
        "Use the detected bump: homeboy release {} --bump {}",
        component_id, under_bump.detected
    ))
    .with_hint("If the lower release is intentional, re-run with --force-lower-bump")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::{ReleaseRunResult, ReleaseStepResult, ReleaseStepStatus};
    use std::collections::HashMap;

    #[test]
    fn detects_keyword_under_bump_override() {
        let under_bump = detect_lower_bump_override("patch", "minor", 3)
            .expect("valid bump comparison")
            .expect("patch should under-bump minor");

        assert_eq!(under_bump.requested, "patch");
        assert_eq!(under_bump.detected, "minor");
        assert_eq!(under_bump.releasable_count, 3);
    }

    #[test]
    fn lower_bump_detection_allows_equal_or_higher_bump() {
        assert!(detect_lower_bump_override("minor", "minor", 1)
            .unwrap()
            .is_none());
        assert!(detect_lower_bump_override("major", "minor", 1)
            .unwrap()
            .is_none());
    }

    #[test]
    fn lower_bump_detection_ignores_explicit_versions_and_none() {
        assert!(detect_lower_bump_override("2.0.0", "minor", 1)
            .unwrap()
            .is_none());
        assert!(detect_lower_bump_override("patch", "none", 0)
            .unwrap()
            .is_none());
    }

    #[test]
    fn lower_bump_error_points_to_detected_bump_and_force_flag() {
        let under_bump = LowerBumpOverride {
            requested: "patch".to_string(),
            detected: "minor".to_string(),
            releasable_count: 2,
        };

        let err = lower_bump_error("demo", &under_bump);

        assert_eq!(err.code.as_str(), "validation.invalid_argument");
        assert!(err
            .message
            .contains("Requested patch bump is lower than detected minor impact"));
        assert!(err
            .hints
            .iter()
            .any(|hint| hint.message.contains("homeboy release demo --bump minor")));
        assert!(err
            .hints
            .iter()
            .any(|hint| hint.message.contains("--force-lower-bump")));
    }

    #[test]
    fn extracts_new_version_from_plan() {
        let plan = ReleasePlan {
            component_id: "demo".to_string(),
            enabled: true,
            steps: vec![crate::release::ReleasePlanStep {
                id: "version".to_string(),
                step_type: "version".to_string(),
                label: None,
                needs: vec![],
                config: HashMap::from([(
                    "to".to_string(),
                    serde_json::Value::String("1.2.3".to_string()),
                )]),
                status: crate::release::ReleasePlanStatus::Ready,
                missing: vec![],
            }],
            semver_recommendation: None,
            warnings: vec![],
            hints: vec![],
        };

        assert_eq!(
            extract_new_version_from_plan(&plan).as_deref(),
            Some("1.2.3")
        );
    }

    #[test]
    fn skipped_release_plan_records_disabled_reason() {
        let plan = skipped_release_plan(
            "demo",
            "no-releasable-commits",
            "No releasable commits since last tag",
            "Use --bump to force a release when this is intentional",
        );

        assert!(!plan.enabled);
        assert_eq!(plan.component_id, "demo");
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].id, "release.skip");
        assert_eq!(plan.steps[0].step_type, "release.skip");
        assert_eq!(
            plan.steps[0].status,
            crate::release::ReleasePlanStatus::Disabled
        );
        assert_eq!(
            plan.steps[0].config.get("reason").and_then(|v| v.as_str()),
            Some("no-releasable-commits")
        );
        assert_eq!(
            plan.hints,
            vec!["Use --bump to force a release when this is intentional"]
        );
    }

    #[test]
    fn detects_post_release_warnings() {
        let run = ReleaseRun {
            component_id: "demo".to_string(),
            enabled: true,
            result: ReleaseRunResult {
                steps: vec![ReleaseStepResult {
                    id: "post_release".to_string(),
                    step_type: "post_release".to_string(),
                    status: ReleaseStepStatus::Success,
                    missing: vec![],
                    warnings: vec![],
                    hints: vec![],
                    data: Some(serde_json::json!({ "all_succeeded": false })),
                    error: None,
                }],
                status: ReleaseStepStatus::Success,
                warnings: vec![],
                summary: None,
            },
        };

        assert!(has_post_release_warnings(&run));
    }

    // ----- Recover-time orphan-tag warning (issue #2234 ask #3) -----

    fn run_in(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
        let output = std::process::Command::new(args[0])
            .args(&args[1..])
            .current_dir(dir)
            .output()
            .expect("spawn command");
        assert!(
            output.status.success(),
            "command {:?} failed: stdout={:?} stderr={:?}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        output
    }

    #[test]
    fn diagnose_orphan_tag_warns_when_tag_points_at_non_release_commit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path();
        run_in(dir, &["git", "init", "-q"]);
        run_in(dir, &["git", "config", "user.email", "test@example.com"]);
        run_in(dir, &["git", "config", "user.name", "Test"]);
        run_in(dir, &["git", "config", "commit.gpgsign", "false"]);
        std::fs::write(dir.join("README"), "x").expect("write");
        run_in(dir, &["git", "add", "."]);
        run_in(
            dir,
            &["git", "commit", "-q", "-m", "Update h2bc bundle to v0.6.14"],
        );
        run_in(dir, &["git", "tag", "v0.7.6"]);

        let warning = diagnose_orphan_tag(&dir.to_string_lossy(), "v0.7.6")
            .expect("orphan tag should produce a warning");

        assert!(warning.contains("v0.7.6"));
        assert!(warning.contains("issue #2234"));
        assert!(warning.contains("Update h2bc bundle"));
    }

    #[test]
    fn diagnose_orphan_tag_silent_when_tag_points_at_release_commit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path();
        run_in(dir, &["git", "init", "-q"]);
        run_in(dir, &["git", "config", "user.email", "test@example.com"]);
        run_in(dir, &["git", "config", "user.name", "Test"]);
        run_in(dir, &["git", "config", "commit.gpgsign", "false"]);
        std::fs::write(dir.join("README"), "x").expect("write");
        run_in(dir, &["git", "add", "."]);
        run_in(dir, &["git", "commit", "-q", "-m", "release: v0.7.4"]);
        run_in(dir, &["git", "tag", "v0.7.4"]);

        assert!(diagnose_orphan_tag(&dir.to_string_lossy(), "v0.7.4").is_none());
    }
}
