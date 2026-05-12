use crate::component::Component;
use crate::error::{Error, Result};
use crate::extension::ExtensionManifest;
use crate::git;
use crate::release::executor;
use crate::release::types::{
    ReleaseOptions, ReleasePlanStatus, ReleasePlanStep, ReleaseState, ReleaseStepResult,
    ReleaseStepStatus,
};

pub(super) struct ReleaseExecutionContext<'a> {
    pub(super) component: &'a Component,
    pub(super) extensions: &'a [ExtensionManifest],
    pub(super) component_id: &'a str,
    pub(super) options: &'a ReleaseOptions,
    pub(super) state: ReleaseState,
    pub(super) publish_failed: bool,
}

pub(super) fn execute_release_plan_step(
    step: &ReleasePlanStep,
    context: &mut ReleaseExecutionContext,
) -> Result<Option<ReleaseStepResult>> {
    if matches!(step.status, ReleasePlanStatus::Disabled) || release_step_is_plan_only(step) {
        return Ok(None);
    }

    match step.step_type.as_str() {
        "preflight.default_branch" => Ok(Some(run_default_branch_preflight(step, context))),
        "preflight.git_identity" => configure_git_identity(step, context).map(Some),
        "preflight.working_tree" => Ok(Some(run_working_tree_preflight(step, context))),
        "changelog.finalize" => {
            executor::changelog::run_changelog_finalize(step, context.component, &mut context.state)
                .map(Some)
        }
        "version" => executor::run_version(
            context.component,
            &mut context.state,
            &context.options.bump_type,
        )
        .map(Some),
        "release.prepare" => Ok(Some(
            executor::prepare::run_prepare(
                context.extensions,
                &context.state,
                context.component_id,
                &context.component.local_path,
            )
            .unwrap_or_else(|err| failed_result("release.prepare", "release.prepare", err)),
        )),
        "git.commit" => {
            executor::run_git_commit(context.component, context.component_id, &context.state)
                .map(Some)
        }
        "package" => Ok(Some(
            executor::run_package(
                context.extensions,
                &mut context.state,
                context.component_id,
                &context.component.local_path,
            )
            .unwrap_or_else(|err| failed_result("package", "package", err)),
        )),
        "git.tag" => {
            let tag_name = step
                .config
                .get("name")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("v{}", context.state.version.as_deref().unwrap_or("")));
            executor::run_git_tag(
                context.component,
                context.component_id,
                &mut context.state,
                &tag_name,
            )
            .map(Some)
        }
        "git.push" => executor::run_git_push(context.component, context.component_id).map(Some),
        "github.release" => Ok(Some(
            executor::run_github_release(context.component, &context.state)
                .unwrap_or_else(|err| failed_result("github.release", "github.release", err)),
        )),
        "cleanup" => {
            if context.publish_failed {
                return Ok(None);
            }
            Ok(Some(
                executor::run_cleanup(context.component)
                    .unwrap_or_else(|err| failed_result("cleanup", "cleanup", err)),
            ))
        }
        "post_release" => {
            let commands = step_config_string_array(step, "commands");
            Ok(Some(
                executor::run_post_release(context.component, &commands)
                    .unwrap_or_else(|err| failed_result("post_release", "post_release", err)),
            ))
        }
        "deploy" => Ok(Some(super::deployment::run_deployment_step(
            context.component_id,
            &context.component.local_path,
        ))),
        step_type if step_type.starts_with("publish.") => {
            let target = step_type.strip_prefix("publish.").unwrap_or_default();
            let result = executor::run_publish(
                context.extensions,
                &context.state,
                context.component_id,
                &context.component.local_path,
                target,
            )
            .unwrap_or_else(|err| {
                context.publish_failed = true;
                failed_result(step_type, step_type, err)
            });

            if matches!(result.status, ReleaseStepStatus::Failed) {
                context.publish_failed = true;
            }

            Ok(Some(result))
        }
        _ => Err(Error::internal_unexpected(format!(
            "release plan contains unsupported executable step '{}'",
            step.step_type
        ))),
    }
}

fn release_step_is_plan_only(step: &ReleasePlanStep) -> bool {
    (step.step_type.starts_with("preflight.")
        && step.step_type != "preflight.default_branch"
        && step.step_type != "preflight.git_identity"
        && step.step_type != "preflight.working_tree")
        || step.step_type == "changelog.policy"
        || step.step_type == "changelog.generate"
}

fn run_default_branch_preflight(
    step: &ReleasePlanStep,
    context: &ReleaseExecutionContext,
) -> ReleaseStepResult {
    match super::pipeline::validate_default_branch(context.component) {
        Ok(()) => ReleaseStepResult {
            id: step.id.clone(),
            step_type: step.step_type.clone(),
            status: ReleaseStepStatus::Success,
            missing: Vec::new(),
            warnings: Vec::new(),
            hints: Vec::new(),
            data: None,
            error: None,
        },
        Err(err) => failed_result(&step.id, &step.step_type, err),
    }
}

fn run_working_tree_preflight(
    step: &ReleasePlanStep,
    context: &ReleaseExecutionContext,
) -> ReleaseStepResult {
    match super::pipeline::validate_working_tree_fail_fast(context.component) {
        Ok(()) => ReleaseStepResult {
            id: step.id.clone(),
            step_type: step.step_type.clone(),
            status: ReleaseStepStatus::Success,
            missing: Vec::new(),
            warnings: Vec::new(),
            hints: Vec::new(),
            data: None,
            error: None,
        },
        Err(err) => failed_result(&step.id, &step.step_type, err),
    }
}

fn configure_git_identity(
    step: &ReleasePlanStep,
    context: &ReleaseExecutionContext,
) -> Result<ReleaseStepResult> {
    let identity_value = step
        .config
        .get("identity")
        .and_then(|value| value.as_str())
        .ok_or_else(|| Error::internal_unexpected("release git identity step missing identity"))?;
    let identity = git::parse_git_identity(Some(identity_value));
    git::configure_identity(&context.component.local_path, &identity)?;
    log_status!(
        "release",
        "Git identity: {} <{}>",
        identity.name,
        identity.email
    );

    Ok(ReleaseStepResult {
        id: step.id.clone(),
        step_type: step.step_type.clone(),
        status: ReleaseStepStatus::Success,
        missing: Vec::new(),
        warnings: Vec::new(),
        hints: Vec::new(),
        data: Some(serde_json::json!({
            "name": identity.name,
            "email": identity.email,
        })),
        error: None,
    })
}

pub(super) fn release_step_is_show_stopper(result: &ReleaseStepResult) -> bool {
    if !matches!(result.status, ReleaseStepStatus::Failed) {
        return false;
    }

    matches!(
        result.step_type.as_str(),
        "changelog.finalize"
            | "preflight.default_branch"
            | "preflight.working_tree"
            | "version"
            | "release.prepare"
            | "git.commit"
            | "package"
            | "git.tag"
            | "git.push"
    )
}

fn step_config_string_array(step: &ReleasePlanStep, key: &str) -> Vec<String> {
    step.config
        .get(key)
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Convert a step error into a failed `ReleaseStepResult`.
fn failed_result(id: &str, step_type: &str, err: Error) -> ReleaseStepResult {
    ReleaseStepResult {
        id: id.to_string(),
        step_type: step_type.to_string(),
        status: ReleaseStepStatus::Failed,
        missing: Vec::new(),
        warnings: Vec::new(),
        hints: err.hints.clone(),
        data: Some(serde_json::json!({ "error_details": err.details })),
        error: Some(err.message),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        execute_release_plan_step, release_step_is_plan_only, release_step_is_show_stopper,
        ReleaseExecutionContext,
    };
    use crate::component::Component;
    use crate::release::types::{
        ReleaseOptions, ReleasePlanStatus, ReleasePlanStep, ReleaseState, ReleaseStepResult,
        ReleaseStepStatus,
    };

    #[test]
    fn test_release_step_is_plan_only() {
        let steps = [
            plan_step("preflight.audit"),
            plan_step("preflight.lint"),
            plan_step("preflight.test"),
            plan_step("changelog.policy"),
            plan_step("changelog.generate"),
        ];

        assert!(steps.iter().all(release_step_is_plan_only));
        assert!(!release_step_is_plan_only(&plan_step(
            "preflight.default_branch"
        )));
        assert!(!release_step_is_plan_only(&plan_step(
            "preflight.git_identity"
        )));
        assert!(!release_step_is_plan_only(&plan_step(
            "preflight.working_tree"
        )));
        assert!(!release_step_is_plan_only(&plan_step("changelog.finalize")));
        assert!(!release_step_is_plan_only(&plan_step("deploy")));
    }

    #[test]
    fn test_execute_release_plan_step() {
        let component = Component {
            id: "fixture".to_string(),
            local_path: "/tmp/fixture".to_string(),
            ..Default::default()
        };
        let options = ReleaseOptions {
            bump_type: "patch".to_string(),
            ..Default::default()
        };
        let mut context = ReleaseExecutionContext {
            component: &component,
            extensions: &[],
            component_id: "fixture",
            options: &options,
            state: ReleaseState::default(),
            publish_failed: true,
        };
        let step = plan_step("cleanup");

        let result = execute_release_plan_step(&step, &mut context).expect("dispatch");
        assert!(result.is_none());
    }

    #[test]
    fn preflight_default_branch_returns_failed_step_on_feature_branch() {
        let temp = tempfile::tempdir().expect("tempdir");
        run_in(temp.path(), &["git", "init", "-q"]);
        run_in(
            temp.path(),
            &["git", "config", "user.email", "test@example.com"],
        );
        run_in(temp.path(), &["git", "config", "user.name", "Test"]);
        std::fs::write(temp.path().join("README.md"), "fixture\n").expect("write fixture");
        run_in(temp.path(), &["git", "add", "."]);
        run_in(
            temp.path(),
            &["git", "commit", "-q", "-m", "Initial commit"],
        );
        run_in(temp.path(), &["git", "checkout", "-q", "-b", "feature"]);

        let component = Component {
            id: "fixture".to_string(),
            local_path: temp.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let options = ReleaseOptions::default();
        let mut context = ReleaseExecutionContext {
            component: &component,
            extensions: &[],
            component_id: "fixture",
            options: &options,
            state: ReleaseState::default(),
            publish_failed: false,
        };

        let result =
            execute_release_plan_step(&plan_step("preflight.default_branch"), &mut context)
                .expect("dispatch")
                .expect("result");

        assert_eq!(result.status, ReleaseStepStatus::Failed);
        assert!(release_step_is_show_stopper(&result));
        assert!(result
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("non-default branch"));
    }

    #[test]
    fn preflight_working_tree_returns_success_step_for_clean_tree() {
        let temp = tempfile::tempdir().expect("tempdir");
        run_in(temp.path(), &["git", "init", "-q"]);
        run_in(
            temp.path(),
            &["git", "config", "user.email", "test@example.com"],
        );
        run_in(temp.path(), &["git", "config", "user.name", "Test"]);
        std::fs::write(temp.path().join("README.md"), "fixture\n").expect("write fixture");
        run_in(temp.path(), &["git", "add", "."]);
        run_in(
            temp.path(),
            &["git", "commit", "-q", "-m", "Initial commit"],
        );

        let component = Component {
            id: "fixture".to_string(),
            local_path: temp.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let options = ReleaseOptions::default();
        let mut context = ReleaseExecutionContext {
            component: &component,
            extensions: &[],
            component_id: "fixture",
            options: &options,
            state: ReleaseState::default(),
            publish_failed: false,
        };

        let result = execute_release_plan_step(&plan_step("preflight.working_tree"), &mut context)
            .expect("dispatch")
            .expect("result");

        assert_eq!(result.status, ReleaseStepStatus::Success);
        assert!(!release_step_is_show_stopper(&result));
    }

    #[test]
    fn test_release_step_is_show_stopper() {
        let version_failure = failed_step_result("version");
        let default_branch_failure = failed_step_result("preflight.default_branch");
        let working_tree_failure = failed_step_result("preflight.working_tree");
        let changelog_failure = failed_step_result("changelog.finalize");
        let publish_failure = failed_step_result("publish.crates");

        assert!(release_step_is_show_stopper(&version_failure));
        assert!(release_step_is_show_stopper(&default_branch_failure));
        assert!(release_step_is_show_stopper(&working_tree_failure));
        assert!(release_step_is_show_stopper(&changelog_failure));
        assert!(!release_step_is_show_stopper(&publish_failure));
    }

    #[test]
    fn test_step_config_string_array() {
        let mut step = plan_step("post_release");
        step.config.insert(
            "commands".to_string(),
            serde_json::json!(["git tag -f stable", 123, "git push"]),
        );

        assert_eq!(
            super::step_config_string_array(&step, "commands"),
            vec!["git tag -f stable", "git push"]
        );
    }

    #[test]
    fn test_failed_result() {
        let err = crate::error::Error::internal_unexpected("boom".to_string());

        let result = super::failed_result("package", "package", err);

        assert_eq!(result.id, "package");
        assert_eq!(result.status, ReleaseStepStatus::Failed);
        assert_eq!(result.error.as_deref(), Some("boom"));
    }

    fn plan_step(step_type: &str) -> ReleasePlanStep {
        ReleasePlanStep {
            id: step_type.to_string(),
            step_type: step_type.to_string(),
            label: None,
            needs: vec![],
            config: std::collections::HashMap::new(),
            status: ReleasePlanStatus::Ready,
            missing: vec![],
        }
    }

    fn failed_step_result(step_type: &str) -> ReleaseStepResult {
        ReleaseStepResult {
            id: step_type.to_string(),
            step_type: step_type.to_string(),
            status: ReleaseStepStatus::Failed,
            missing: vec![],
            warnings: vec![],
            hints: vec![],
            data: None,
            error: Some("failed".to_string()),
        }
    }

    fn run_in(dir: &std::path::Path, args: &[&str]) {
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
    }
}
