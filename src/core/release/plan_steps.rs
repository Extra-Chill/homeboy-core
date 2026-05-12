use crate::component::Component;
use crate::extension::ExtensionManifest;
use crate::git;
use crate::release::pipeline_capabilities::{
    get_publish_targets, has_package_capability, has_prepare_capability,
};
use crate::release::types::{ReleaseOptions, ReleasePlanStatus, ReleasePlanStep};
use crate::Result;

type StepConfig = std::collections::HashMap<String, serde_json::Value>;

pub(super) fn changelog_entries_to_json(
    entries: &std::collections::HashMap<String, Vec<String>>,
) -> serde_json::Value {
    serde_json::to_value(entries).unwrap_or_default()
}

/// Return true if this component should get a GitHub Release created.
///
/// Resolves the remote URL from the component config (preferred) or from
/// `git remote get-url origin` in the component's local_path, then parses
/// it as a GitHub URL. Non-GitHub remotes (GitLab, self-hosted, etc.) fall
/// through cleanly — the step simply isn't added to the plan.
pub(super) fn github_release_applies(component: &Component) -> bool {
    let remote_url = component.remote_url.clone().or_else(|| {
        crate::deploy::release_download::detect_remote_url(std::path::Path::new(
            &component.local_path,
        ))
    });

    remote_url
        .as_deref()
        .and_then(crate::deploy::release_download::parse_github_url)
        .is_some()
}

fn ready_step(
    id: &str,
    step_type: &str,
    label: impl Into<String>,
    needs: Vec<String>,
    config: StepConfig,
) -> ReleasePlanStep {
    ReleasePlanStep {
        id: id.to_string(),
        step_type: step_type.to_string(),
        label: Some(label.into()),
        needs,
        config,
        status: ReleasePlanStatus::Ready,
        missing: vec![],
    }
}

fn disabled_step(
    id: &str,
    step_type: &str,
    label: impl Into<String>,
    config: StepConfig,
) -> ReleasePlanStep {
    ReleasePlanStep {
        id: id.to_string(),
        step_type: step_type.to_string(),
        label: Some(label.into()),
        needs: vec![],
        config,
        status: ReleasePlanStatus::Disabled,
        missing: vec![],
    }
}

fn string_config(key: &str, value: impl Into<String>) -> StepConfig {
    let mut config = StepConfig::new();
    config.insert(key.to_string(), serde_json::Value::String(value.into()));
    config
}

pub(super) fn build_preflight_steps(options: &ReleaseOptions) -> Vec<ReleasePlanStep> {
    let mut steps = vec![
        ready_step(
            "preflight.default_branch",
            "preflight.default_branch",
            "Validate default branch",
            vec![],
            StepConfig::new(),
        ),
        ready_step(
            "preflight.working_tree",
            "preflight.working_tree",
            "Validate working tree",
            vec!["preflight.git_identity".to_string()],
            StepConfig::new(),
        ),
        ready_step(
            "preflight.remote_sync",
            "preflight.remote_sync",
            "Validate remote sync",
            vec!["preflight.working_tree".to_string()],
            StepConfig::new(),
        ),
    ];

    if let Some(identity) = options.git_identity.as_ref() {
        steps.insert(
            1,
            ready_step(
                "preflight.git_identity",
                "preflight.git_identity",
                "Configure git identity",
                vec!["preflight.default_branch".to_string()],
                string_config("identity", identity.as_str()),
            ),
        );
    } else {
        steps.insert(
            1,
            disabled_step(
                "preflight.git_identity",
                "preflight.git_identity",
                "Configure git identity",
                string_config("reason", "not-requested"),
            ),
        );
    }

    if options.skip_checks {
        steps.push(disabled_step(
            "preflight.quality",
            "preflight.quality",
            "Run release quality checks",
            string_config("reason", "--skip-checks"),
        ));
    } else {
        steps.push(ready_step(
            "preflight.quality",
            "preflight.quality",
            "Run release quality checks",
            vec!["preflight.remote_sync".to_string()],
            StepConfig::new(),
        ));
    }

    let mut changelog_config = StepConfig::new();
    changelog_config.insert(
        "dry_run".to_string(),
        serde_json::Value::Bool(options.dry_run),
    );
    steps.push(ready_step(
        "preflight.changelog_bootstrap",
        "preflight.changelog_bootstrap",
        "Ensure changelog exists",
        vec!["preflight.quality".to_string()],
        changelog_config,
    ));

    steps
}

/// Build all release steps: core steps (non-configurable) + publish steps (extension-derived).
pub(super) fn build_release_steps(
    component: &Component,
    extensions: &[ExtensionManifest],
    current_version: &str,
    new_version: &str,
    options: &ReleaseOptions,
    monorepo: Option<&git::MonorepoContext>,
    warnings: &mut Vec<String>,
    _hints: &mut Vec<String>,
) -> Result<Vec<ReleasePlanStep>> {
    let mut steps = Vec::new();
    let publish_targets = get_publish_targets(extensions);

    if !publish_targets.is_empty() && !has_package_capability(extensions) {
        warnings.push(
            "Publish targets derived from extensions but no extension provides 'release.package'. \
             Add an extension that provides packaging."
                .to_string(),
        );
    }

    steps.push(ready_step(
        "changelog.generate",
        "changelog.generate",
        "Generate changelog entries from commits",
        vec!["preflight.changelog_bootstrap".to_string()],
        string_config("policy", "generated"),
    ));

    let mut version_config = string_config("bump", options.bump_type.clone());
    version_config.insert(
        "from".to_string(),
        serde_json::Value::String(current_version.to_string()),
    );
    version_config.insert(
        "to".to_string(),
        serde_json::Value::String(new_version.to_string()),
    );
    steps.push(ready_step(
        "version",
        "version",
        format!(
            "Bump version {} → {} ({})",
            current_version, new_version, options.bump_type
        ),
        vec!["changelog.generate".to_string()],
        version_config,
    ));

    let commit_needs = if has_prepare_capability(extensions) {
        steps.push(ready_step(
            "release.prepare",
            "release.prepare",
            "Prepare release files",
            vec!["version".to_string()],
            StepConfig::new(),
        ));
        vec!["release.prepare".to_string()]
    } else {
        vec!["version".to_string()]
    };

    steps.push(ready_step(
        "git.commit",
        "git.commit",
        format!("Commit release: v{}", new_version),
        commit_needs,
        StepConfig::new(),
    ));

    let tag_needs = if !publish_targets.is_empty() && !options.skip_publish {
        steps.push(ready_step(
            "package",
            "package",
            "Package release artifacts",
            vec!["git.commit".to_string()],
            StepConfig::new(),
        ));
        vec!["package".to_string()]
    } else {
        vec!["git.commit".to_string()]
    };

    let tag_name = match monorepo {
        Some(ctx) => ctx.format_tag(new_version),
        None => format!("v{}", new_version),
    };
    steps.push(ready_step(
        "git.tag",
        "git.tag",
        format!("Tag {}", tag_name),
        tag_needs,
        string_config("name", tag_name),
    ));

    let mut push_config = StepConfig::new();
    push_config.insert("tags".to_string(), serde_json::Value::Bool(true));
    steps.push(ready_step(
        "git.push",
        "git.push",
        "Push to remote",
        vec!["git.tag".to_string()],
        push_config,
    ));

    if !options.skip_github_release && github_release_applies(component) {
        steps.push(ready_step(
            "github.release",
            "github.release",
            "Create GitHub Release",
            vec!["git.push".to_string()],
            StepConfig::new(),
        ));
    }

    let mut publish_step_ids: Vec<String> = Vec::new();
    if !publish_targets.is_empty() && !options.skip_publish {
        for target in &publish_targets {
            let step_id = format!("publish.{}", target);
            publish_step_ids.push(step_id.clone());
            steps.push(ready_step(
                &step_id,
                &step_id,
                format!("Publish to {}", target),
                vec!["git.push".to_string()],
                StepConfig::new(),
            ));
        }

        if !options.deploy {
            steps.push(ready_step(
                "cleanup",
                "cleanup",
                "Clean up release artifacts",
                publish_step_ids.clone(),
                StepConfig::new(),
            ));
        }
    } else if options.skip_publish && !publish_targets.is_empty() {
        log_status!("release", "Skipping publish/package steps (--skip-publish)");
    }

    let post_release_hooks =
        crate::engine::hooks::resolve_hooks(component, crate::engine::hooks::events::POST_RELEASE);
    if !post_release_hooks.is_empty() {
        let post_release_needs = if !options.skip_publish && !publish_targets.is_empty() {
            if options.deploy {
                publish_step_ids.clone()
            } else {
                vec!["cleanup".to_string()]
            }
        } else {
            vec!["git.push".to_string()]
        };

        steps.push(ready_step(
            "post_release",
            "post_release",
            "Run post-release hooks",
            post_release_needs,
            string_array_config("commands", &post_release_hooks),
        ));
    }

    if options.deploy {
        let deploy_needs = if !post_release_hooks.is_empty() {
            vec!["post_release".to_string()]
        } else if !options.skip_publish && !publish_step_ids.is_empty() {
            publish_step_ids
        } else {
            vec!["git.push".to_string()]
        };

        steps.push(ready_step(
            "deploy",
            "deploy",
            "Deploy released component",
            deploy_needs,
            string_config("execution", "release_plan"),
        ));
    }

    Ok(steps)
}

fn string_array_config(key: &str, values: &[String]) -> StepConfig {
    let mut config = StepConfig::new();
    config.insert(
        key.to_string(),
        serde_json::Value::Array(
            values
                .iter()
                .map(|value| serde_json::Value::String(value.clone()))
                .collect(),
        ),
    );
    config
}

#[cfg(test)]
mod tests {
    use super::{
        build_preflight_steps, build_release_steps, changelog_entries_to_json,
        github_release_applies,
    };
    use crate::component::Component;
    use crate::release::types::{ReleaseOptions, ReleasePlanStatus};

    #[test]
    fn test_build_preflight_steps() {
        let options = ReleaseOptions {
            bump_type: "patch".to_string(),
            ..Default::default()
        };

        let steps = build_preflight_steps(&options);
        let ids: Vec<&str> = steps.iter().map(|step| step.id.as_str()).collect();

        assert_eq!(
            ids,
            vec![
                "preflight.default_branch",
                "preflight.git_identity",
                "preflight.working_tree",
                "preflight.remote_sync",
                "preflight.quality",
                "preflight.changelog_bootstrap"
            ]
        );
        assert_eq!(steps[0].status, ReleasePlanStatus::Ready);
        assert_eq!(steps[1].status, ReleasePlanStatus::Disabled);
        assert_eq!(steps[2].needs, vec!["preflight.git_identity"]);
    }

    #[test]
    fn release_plan_marks_git_identity_ready_when_requested() {
        let options = ReleaseOptions {
            bump_type: "patch".to_string(),
            git_identity: Some("Release Bot <bot@example.com>".to_string()),
            ..Default::default()
        };

        let steps = build_preflight_steps(&options);
        let identity = steps
            .iter()
            .find(|step| step.id == "preflight.git_identity")
            .expect("git identity step");

        assert_eq!(identity.status, ReleasePlanStatus::Ready);
        assert_eq!(identity.needs, vec!["preflight.default_branch"]);
        assert_eq!(
            identity
                .config
                .get("identity")
                .and_then(|value| value.as_str()),
            Some("Release Bot <bot@example.com>")
        );
    }

    #[test]
    fn release_plan_marks_quality_preflight_disabled_when_checks_are_skipped() {
        let options = ReleaseOptions {
            bump_type: "patch".to_string(),
            skip_checks: true,
            ..Default::default()
        };

        let steps = build_preflight_steps(&options);
        let quality = steps
            .iter()
            .find(|step| step.id == "preflight.quality")
            .expect("quality step");

        assert_eq!(quality.status, ReleasePlanStatus::Disabled);
        assert_eq!(
            quality
                .config
                .get("reason")
                .and_then(|value| value.as_str()),
            Some("--skip-checks")
        );
    }

    #[test]
    fn test_build_release_steps() {
        let component = fixture_component();
        let extension = serde_json::from_value(serde_json::json!({
            "name": "Fixture",
            "version": "1.0.0",
            "actions": [
                {
                    "id": "release.prepare",
                    "label": "Prepare release",
                    "type": "command",
                    "command": "true"
                }
            ]
        }))
        .expect("extension manifest");
        let mut warnings = Vec::new();
        let mut hints = Vec::new();
        let options = ReleaseOptions {
            bump_type: "patch".to_string(),
            ..Default::default()
        };

        let steps = build_release_steps(
            &component,
            &[extension],
            "1.0.0",
            "1.0.1",
            &options,
            None,
            &mut warnings,
            &mut hints,
        )
        .expect("steps");

        let ids: Vec<&str> = steps.iter().map(|step| step.id.as_str()).collect();
        let changelog_index = step_index(&ids, "changelog.generate");
        let version_index = step_index(&ids, "version");
        let prepare_index = step_index(&ids, "release.prepare");
        let commit_index = step_index(&ids, "git.commit");

        assert!(changelog_index < version_index);
        assert!(version_index < prepare_index);
        assert!(prepare_index < commit_index);
        assert_eq!(steps[version_index].needs, vec!["changelog.generate"]);
        assert_eq!(steps[prepare_index].needs, vec!["version"]);
        assert_eq!(steps[commit_index].needs, vec!["release.prepare"]);
    }

    #[test]
    fn release_plan_includes_deploy_intent_when_requested() {
        let component = fixture_component();
        let mut warnings = Vec::new();
        let mut hints = Vec::new();
        let options = ReleaseOptions {
            bump_type: "patch".to_string(),
            deploy: true,
            ..Default::default()
        };

        let steps = build_release_steps(
            &component,
            &[],
            "1.0.0",
            "1.0.1",
            &options,
            None,
            &mut warnings,
            &mut hints,
        )
        .expect("steps");

        let deploy = steps
            .iter()
            .find(|step| step.id == "deploy")
            .expect("deploy step");
        assert_eq!(deploy.needs, vec!["git.push"]);
        assert_eq!(
            deploy
                .config
                .get("execution")
                .and_then(|value| value.as_str()),
            Some("release_plan")
        );
    }

    #[test]
    fn test_changelog_entries_to_json() {
        let entries = std::collections::HashMap::from([(
            "added".to_string(),
            vec!["release plan previews".to_string()],
        )]);

        let json = changelog_entries_to_json(&entries);

        assert_eq!(json["added"][0], "release plan previews");
    }

    #[test]
    fn test_github_release_applies() {
        let mut github_component = fixture_component();
        github_component.remote_url =
            Some("https://github.com/Extra-Chill/homeboy.git".to_string());
        let mut non_github_component = fixture_component();
        non_github_component.remote_url =
            Some("https://gitlab.example.com/acme/tool.git".to_string());

        assert!(github_release_applies(&github_component));
        assert!(!github_release_applies(&non_github_component));
    }

    fn fixture_component() -> Component {
        Component {
            id: "fixture".to_string(),
            local_path: "/tmp/fixture".to_string(),
            ..Default::default()
        }
    }

    fn step_index(ids: &[&str], id: &str) -> usize {
        ids.iter()
            .position(|candidate| *candidate == id)
            .unwrap_or_else(|| panic!("missing {id} step"))
    }
}
