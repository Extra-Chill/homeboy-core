//! Lint workflow orchestration — runs lint, resolves changed-file scoping,
//! drives autofix, processes baseline lifecycle, and assembles results.
//!
//! Mirrors `core/extension/test/run.rs` — the command layer provides CLI args,
//! this module owns all business logic and returns a structured result.

use crate::component::Component;
use crate::engine::baseline::BaselineFlags;
use crate::engine::run_dir::{self, RunDir};
use crate::extension::lint::baseline::{self as lint_baseline, LintFinding};
use crate::extension::lint::build_lint_runner;
use crate::extension::{self, ExtensionCapability};
use crate::git;
use crate::refactor::AppliedRefactor;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Arguments for the main lint workflow — populated by the command layer from CLI flags.
#[derive(Debug, Clone)]
pub struct LintRunWorkflowArgs {
    pub component_label: String,
    pub component_id: String,
    pub path_override: Option<String>,
    pub settings: Vec<(String, String)>,
    pub summary: bool,
    pub file: Option<String>,
    pub glob: Option<String>,
    pub changed_only: bool,
    pub changed_since: Option<String>,
    pub errors_only: bool,
    pub sniffs: Option<String>,
    pub exclude_sniffs: Option<String>,
    pub category: Option<String>,
    pub baseline_flags: BaselineFlags,
}

/// Result of the main lint workflow — ready for report assembly.
#[derive(Debug, Clone, Serialize)]
pub struct LintRunWorkflowResult {
    pub status: String,
    pub component: String,
    pub exit_code: i32,
    pub autofix: Option<AppliedRefactor>,
    pub hints: Option<Vec<String>>,
    pub baseline_comparison: Option<lint_baseline::BaselineComparison>,
    pub lint_findings: Option<Vec<LintFinding>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScopedLintRun {
    glob: String,
    step: Option<&'static str>,
}

/// Run the main lint workflow.
///
/// Handles changed-file scoping, autofix planning, lint runner execution,
/// baseline lifecycle, hint assembly, and result construction.
pub fn run_main_lint_workflow(
    component: &Component,
    source_path: &PathBuf,
    args: LintRunWorkflowArgs,
    run_dir: &RunDir,
) -> crate::Result<LintRunWorkflowResult> {
    let scoped_runs = resolve_scoped_lint_runs(component, &args)?;

    // Early exit if changed-file mode produced no files
    if let Some(ref runs) = scoped_runs {
        if runs.is_empty() {
            return Ok(LintRunWorkflowResult {
                status: "passed".to_string(),
                component: args.component_label,
                exit_code: 0,
                autofix: None,
                hints: None,
                baseline_comparison: None,
                lint_findings: None,
            });
        }
    }

    // Run lint
    let output = if let Some(runs) = scoped_runs {
        run_scoped_lint_runs(component, &args, run_dir, &runs)?
    } else {
        build_lint_runner(
            component,
            args.path_override.clone(),
            &args.settings,
            args.summary,
            args.file.as_deref(),
            args.glob.as_deref(),
            args.errors_only,
            args.sniffs.as_deref(),
            args.exclude_sniffs.as_deref(),
            args.category.as_deref(),
            None,
            run_dir,
        )?
        .run()?
    };

    let lint_findings_file = run_dir.step_file(run_dir::files::LINT_FINDINGS);
    let lint_findings = lint_baseline::parse_findings_file(&lint_findings_file)?;

    // Status computation — check findings first, exit code as fallback.
    // The extension runner uses passthrough mode (stdout goes to terminal),
    // so `output.success` only reflects the shell exit code. PHPCS/PHPStan
    // wrappers may exit 0 even when findings exist, so the sidecar findings
    // file is the canonical source of truth (mirrors test command pattern).
    let mut status = if !lint_findings.is_empty() {
        "failed"
    } else if output.success {
        "passed"
    } else {
        "failed"
    }
    .to_string();

    let mut hints = Vec::new();

    let lint_clean = lint_findings.is_empty() && output.success;

    // Baseline lifecycle
    let (baseline_comparison, baseline_exit_override) =
        process_baseline(source_path, &args, &lint_findings)?;

    // Hint assembly — point to the auto-fix CTA for autofixable findings.
    //
    // Per the contract under #1459 (issue #1507), autofixable findings never
    // fail the run; they nudge. The CTA is rendered here in core, not by each
    // extension's runner, so every language extension benefits from a single
    // consistent prose. `homeboy lint --fix` is the ergonomic alias and is
    // listed first; the canonical `homeboy refactor --from lint --write`
    // invocation follows for users who want the longer form.
    if !lint_clean {
        hints.push(format!(
            "Auto-fix: homeboy lint {} --fix (or homeboy refactor {} --from lint --write)",
            args.component_label, args.component_label
        ));
        hints.push("Some issues may require manual fixes".to_string());
    }

    if args.file.is_none()
        && args.glob.is_none()
        && !args.changed_only
        && args.changed_since.is_none()
    {
        hints.push(
            "For targeted linting: --file <path>, --glob <pattern>, --changed-only, or --changed-since <ref>".to_string(),
        );
    }

    hints.push("Full options: homeboy docs commands/lint".to_string());

    if !args.baseline_flags.baseline && baseline_comparison.is_none() {
        hints.push(format!(
            "Save lint baseline: homeboy lint {} --baseline",
            args.component_label
        ));
    }

    let hints = if hints.is_empty() { None } else { Some(hints) };
    let exit_code = baseline_exit_override.unwrap_or(output.exit_code);
    if exit_code != output.exit_code {
        status = "failed".to_string();
    }

    Ok(LintRunWorkflowResult {
        status,
        component: args.component_label,
        exit_code,
        autofix: None,
        hints,
        baseline_comparison,
        lint_findings: Some(lint_findings),
    })
}

fn run_scoped_lint_runs(
    component: &Component,
    args: &LintRunWorkflowArgs,
    run_dir: &RunDir,
    runs: &[ScopedLintRun],
) -> crate::Result<extension::RunnerOutput> {
    let mut success = true;
    let mut exit_code = 0;

    for (index, run) in runs.iter().enumerate() {
        let scoped_run_dir;
        let active_run_dir = if index == 0 {
            run_dir
        } else {
            scoped_run_dir = RunDir::create()?;
            &scoped_run_dir
        };

        let output = build_lint_runner(
            component,
            args.path_override.clone(),
            &args.settings,
            args.summary,
            args.file.as_deref(),
            Some(run.glob.as_str()),
            args.errors_only,
            args.sniffs.as_deref(),
            args.exclude_sniffs.as_deref(),
            args.category.as_deref(),
            run.step,
            active_run_dir,
        )?
        .run()?;

        if !output.success {
            success = false;
            if exit_code == 0 {
                exit_code = output.exit_code;
            }
        }
    }

    Ok(extension::RunnerOutput {
        exit_code,
        success,
        stdout: String::new(),
        stderr: String::new(),
    })
}

pub fn run_self_check_lint_workflow(
    component: &Component,
    source_path: &Path,
    component_label: String,
) -> crate::Result<LintRunWorkflowResult> {
    let output =
        extension::self_check::run_self_checks(component, ExtensionCapability::Lint, source_path)?;
    let status = if output.success { "passed" } else { "failed" }.to_string();
    let hints = (!output.success).then(|| {
        vec![format!(
            "Fix the failing self-check command declared in {}'s homeboy.json self_checks.lint",
            component.id
        )]
    });

    Ok(LintRunWorkflowResult {
        status,
        component: component_label,
        exit_code: output.exit_code,
        autofix: None,
        hints,
        baseline_comparison: None,
        lint_findings: Some(Vec::new()),
    })
}

/// Resolve runner-compatible scopes from --changed-only or --changed-since flags.
///
/// Returns `Some(Vec::new())` when changed-file mode is active but no compatible
/// files were found — the caller should treat this as an early "passed" exit.
/// Returns `None` when no changed-file scoping is active (use args.glob directly).
fn resolve_scoped_lint_runs(
    component: &Component,
    args: &LintRunWorkflowArgs,
) -> crate::Result<Option<Vec<ScopedLintRun>>> {
    if args.changed_only {
        let uncommitted = git::get_uncommitted_changes(&component.local_path)?;
        let mut changed_files: Vec<String> = Vec::new();
        changed_files.extend(uncommitted.staged);
        changed_files.extend(uncommitted.unstaged);
        changed_files.extend(uncommitted.untracked);

        if changed_files.is_empty() {
            println!("No files in working tree changes");
            return Ok(Some(Vec::new()));
        }

        Ok(Some(build_changed_lint_runs(component, &changed_files)))
    } else if let Some(ref git_ref) = args.changed_since {
        let changed_files = git::get_files_changed_since(&component.local_path, git_ref)?;

        if changed_files.is_empty() {
            println!("No files changed since {}", git_ref);
            return Ok(Some(Vec::new()));
        }

        Ok(Some(build_changed_lint_runs(component, &changed_files)))
    } else {
        Ok(None)
    }
}

fn build_changed_lint_runs(component: &Component, changed_files: &[String]) -> Vec<ScopedLintRun> {
    if !component_uses_extension(component, "wordpress") {
        return vec![ScopedLintRun {
            glob: glob_for_files(&component.local_path, changed_files),
            step: None,
        }];
    }

    let php_files: Vec<String> = changed_files
        .iter()
        .filter(|file| has_extension(file, &["php"]))
        .cloned()
        .collect();
    let js_files: Vec<String> = changed_files
        .iter()
        .filter(|file| has_extension(file, &["js", "jsx", "ts", "tsx"]))
        .cloned()
        .collect();

    let mut runs = Vec::new();
    if !php_files.is_empty() {
        runs.push(ScopedLintRun {
            glob: glob_for_files(&component.local_path, &php_files),
            step: Some("phpcs,phpstan"),
        });
    }
    if !js_files.is_empty() {
        runs.push(ScopedLintRun {
            glob: glob_for_files(&component.local_path, &js_files),
            step: Some("eslint"),
        });
    }
    runs
}

fn component_uses_extension(component: &Component, extension_id: &str) -> bool {
    component
        .extensions
        .as_ref()
        .is_some_and(|extensions| extensions.contains_key(extension_id))
}

fn has_extension(file: &str, extensions: &[&str]) -> bool {
    Path::new(file)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extensions.contains(&extension))
}

fn glob_for_files(root: &str, files: &[String]) -> String {
    let abs_files: Vec<String> = files
        .iter()
        .map(|file| format!("{}/{}", root, file))
        .collect();

    if abs_files.len() == 1 {
        abs_files[0].clone()
    } else {
        format!("{{{}}}", abs_files.join(","))
    }
}

/// Process baseline lifecycle — save, load, compare.
fn process_baseline(
    source_path: &PathBuf,
    args: &LintRunWorkflowArgs,
    lint_findings: &[LintFinding],
) -> crate::Result<(Option<lint_baseline::BaselineComparison>, Option<i32>)> {
    let mut baseline_comparison = None;
    let mut baseline_exit_override = None;

    if args.baseline_flags.baseline {
        let saved = lint_baseline::save_baseline(source_path, &args.component_id, lint_findings)?;
        eprintln!(
            "[lint] Baseline saved to {} ({} findings)",
            saved.display(),
            lint_findings.len()
        );
    }

    if !args.baseline_flags.baseline && !args.baseline_flags.ignore_baseline {
        if let Some(existing) = lint_baseline::load_baseline(source_path) {
            let comparison = lint_baseline::compare(lint_findings, &existing);

            if comparison.drift_increased {
                eprintln!(
                    "[lint] DRIFT INCREASED: {} new finding(s) since baseline",
                    comparison.new_items.len()
                );
                baseline_exit_override = Some(1);
            } else if !comparison.resolved_fingerprints.is_empty() {
                eprintln!(
                    "[lint] Drift reduced: {} finding(s) resolved since baseline",
                    comparison.resolved_fingerprints.len()
                );
            } else {
                eprintln!("[lint] No change from baseline");
            }

            baseline_comparison = Some(comparison);
        }
    }

    Ok((baseline_comparison, baseline_exit_override))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::{ScopedExtensionConfig, SelfCheckConfig};
    use std::collections::HashMap;

    fn wordpress_component(root: &str) -> Component {
        let mut component = Component::new(
            "fixture".to_string(),
            root.to_string(),
            "".to_string(),
            None,
        );
        component.extensions = Some(HashMap::from([(
            "wordpress".to_string(),
            ScopedExtensionConfig::default(),
        )]));
        component
    }

    #[test]
    fn test_run_self_check_lint_workflow() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("lint.sh"), "printf lint-ok\n")
            .expect("script should be written");

        let mut component = Component::new(
            "fixture".to_string(),
            dir.path().to_string_lossy().to_string(),
            "".to_string(),
            None,
        );
        component.self_checks = Some(SelfCheckConfig {
            lint: vec!["sh lint.sh".to_string()],
            test: Vec::new(),
        });

        let result = run_self_check_lint_workflow(&component, dir.path(), "fixture".to_string())
            .expect("lint self-check should run");

        assert_eq!(result.status, "passed");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.component, "fixture");
    }

    #[test]
    fn wordpress_changed_php_files_route_to_php_steps_only() {
        let component = wordpress_component("/repo");
        let runs = build_changed_lint_runs(
            &component,
            &["data-machine.php".to_string(), "inc/Foo.php".to_string()],
        );

        assert_eq!(
            runs,
            vec![ScopedLintRun {
                glob: "{/repo/data-machine.php,/repo/inc/Foo.php}".to_string(),
                step: Some("phpcs,phpstan"),
            }]
        );
    }

    #[test]
    fn wordpress_changed_markdown_files_do_not_route_to_eslint() {
        let component = wordpress_component("/repo");
        let runs = build_changed_lint_runs(
            &component,
            &[
                "docs/core-system/agent-bundles.md".to_string(),
                "README.md".to_string(),
            ],
        );

        assert!(runs.is_empty());
    }

    #[test]
    fn wordpress_changed_mixed_php_and_js_files_split_by_runner() {
        let component = wordpress_component("/repo");
        let runs = build_changed_lint_runs(
            &component,
            &[
                "inc/Foo.php".to_string(),
                "docs/notes.md".to_string(),
                "assets/app.js".to_string(),
                "assets/view.tsx".to_string(),
            ],
        );

        assert_eq!(
            runs,
            vec![
                ScopedLintRun {
                    glob: "/repo/inc/Foo.php".to_string(),
                    step: Some("phpcs,phpstan"),
                },
                ScopedLintRun {
                    glob: "{/repo/assets/app.js,/repo/assets/view.tsx}".to_string(),
                    step: Some("eslint"),
                },
            ]
        );
    }

    #[test]
    fn non_wordpress_changed_files_keep_existing_single_runner_scope() {
        let component = Component::new(
            "fixture".to_string(),
            "/repo".to_string(),
            "".to_string(),
            None,
        );
        let runs = build_changed_lint_runs(
            &component,
            &["src/main.rs".to_string(), "README.md".to_string()],
        );

        assert_eq!(
            runs,
            vec![ScopedLintRun {
                glob: "{/repo/src/main.rs,/repo/README.md}".to_string(),
                step: None,
            }]
        );
    }
}
