use super::types::{ReleaseRunSummary, ReleaseStepResult, ReleaseStepStatus};

pub(super) fn derive_overall_status(results: &[ReleaseStepResult]) -> ReleaseStepStatus {
    let has_success = results
        .iter()
        .any(|r| matches!(r.status, ReleaseStepStatus::Success));
    let has_failed = results
        .iter()
        .any(|r| matches!(r.status, ReleaseStepStatus::Failed));

    if has_failed && has_success {
        ReleaseStepStatus::PartialSuccess
    } else if has_failed {
        ReleaseStepStatus::Failed
    } else {
        ReleaseStepStatus::Success
    }
}

pub(super) fn build_summary(
    results: &[ReleaseStepResult],
    status: &ReleaseStepStatus,
) -> ReleaseRunSummary {
    let succeeded = results
        .iter()
        .filter(|r| matches!(r.status, ReleaseStepStatus::Success))
        .count();
    let failed = results
        .iter()
        .filter(|r| matches!(r.status, ReleaseStepStatus::Failed))
        .count();
    let skipped = results
        .iter()
        .filter(|r| matches!(r.status, ReleaseStepStatus::Skipped))
        .count();
    let missing = results
        .iter()
        .filter(|r| matches!(r.status, ReleaseStepStatus::Missing))
        .count();

    let next_actions = match status {
        ReleaseStepStatus::PartialSuccess | ReleaseStepStatus::Failed => vec![
            "Fix the issue and re-run (idempotent - completed steps will succeed again)"
                .to_string(),
        ],
        ReleaseStepStatus::Missing => {
            vec!["Install missing extensions or actions to resolve missing steps".to_string()]
        }
        _ => Vec::new(),
    };

    let success_summary = if matches!(status, ReleaseStepStatus::Success) {
        results.iter().filter_map(build_step_summary_line).collect()
    } else {
        Vec::new()
    };

    ReleaseRunSummary {
        total_steps: results.len(),
        succeeded,
        failed,
        skipped,
        missing,
        next_actions,
        success_summary,
    }
}

fn build_step_summary_line(result: &ReleaseStepResult) -> Option<String> {
    if !matches!(result.status, ReleaseStepStatus::Success) {
        return None;
    }

    let data = result.data.as_ref();

    match result.step_type.as_str() {
        "version" => data
            .and_then(|d| d.get("new_version"))
            .and_then(|v| v.as_str())
            .map(|ver| format!("Version bumped to {}", ver)),
        "git.commit" => {
            let skipped = data
                .and_then(|d| d.get("skipped"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if skipped {
                Some("Working tree was clean".to_string())
            } else {
                Some("Committed release changes".to_string())
            }
        }
        "git.tag" => {
            let tag = data.and_then(|d| d.get("tag")).and_then(|v| v.as_str());
            let skipped = data
                .and_then(|d| d.get("skipped"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            match (tag, skipped) {
                (Some(t), true) => Some(format!("Tag {} already exists", t)),
                (Some(t), false) => Some(format!("Tagged {}", t)),
                (None, _) => Some("Tagged release".to_string()),
            }
        }
        "git.push" => Some("Pushed to origin (with tags)".to_string()),
        "release.prepare" => Some("Prepared release files".to_string()),
        "package" => Some("Created release artifacts".to_string()),
        "cleanup" => None,
        "github.release" => {
            let skipped = data
                .and_then(|d| d.get("skipped"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if skipped {
                None
            } else {
                data.and_then(|d| d.get("url"))
                    .and_then(|v| v.as_str())
                    .map(|url| format!("Created GitHub Release: {}", url))
            }
        }
        "post_release" => {
            let all_succeeded = data
                .and_then(|d| d.get("all_succeeded"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if all_succeeded {
                Some("Post-release commands completed".to_string())
            } else {
                Some("Post-release commands completed (with warnings)".to_string())
            }
        }
        step if step.starts_with("publish.") => {
            let target = step.strip_prefix("publish.").unwrap_or("registry");
            Some(format!("Published to {}", target))
        }
        _ => None,
    }
}
