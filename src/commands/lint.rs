use clap::Args;
use serde::Serialize;
use std::collections::HashSet;

use homeboy::component::Component;
use homeboy::error::Error;
use homeboy::extension::{self, ExtensionRunner};
use homeboy::git;
use homeboy::utils::autofix::{self, AutofixMode};

use super::args::{HiddenJsonArgs, PositionalComponentArgs, SettingArgs};
use super::CmdResult;

#[derive(Args)]
pub struct LintArgs {
    #[command(flatten)]
    comp: PositionalComponentArgs,

    /// Auto-fix formatting issues before validating
    #[arg(long)]
    fix: bool,

    /// Show compact summary instead of full output
    #[arg(long)]
    summary: bool,

    /// Lint only a single file (path relative to component root)
    #[arg(long)]
    file: Option<String>,

    /// Lint only files matching glob pattern (e.g., "inc/**/*.php")
    #[arg(long)]
    glob: Option<String>,

    /// Lint only files modified in the working tree (staged, unstaged, untracked)
    #[arg(long, conflicts_with = "changed_since")]
    changed_only: bool,

    /// Lint only files changed since a git ref (branch, tag, or SHA) — CI-friendly
    #[arg(long, conflicts_with = "changed_only")]
    changed_since: Option<String>,

    /// Show only errors, suppress warnings
    #[arg(long)]
    errors_only: bool,

    /// Only check specific sniffs (comma-separated codes)
    #[arg(long)]
    sniffs: Option<String>,

    /// Exclude sniffs from checking (comma-separated codes)
    #[arg(long)]
    exclude_sniffs: Option<String>,

    /// Filter by category: security, i18n, yoda, whitespace
    #[arg(long)]
    category: Option<String>,

    #[command(flatten)]
    setting_args: SettingArgs,

    #[command(flatten)]
    _json: HiddenJsonArgs,
}

#[derive(Serialize)]
pub struct LintOutput {
    status: String,
    component: String,
    exit_code: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    autofix: Option<LintAutofixOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hints: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct LintAutofixOutput {
    files_modified: usize,
    rerun_recommended: bool,
}

fn resolve_lint_script(component: &Component) -> homeboy::error::Result<String> {
    let extensions = component.extensions.as_ref().ok_or_else(|| {
        Error::validation_invalid_argument(
            "component",
            format!("Component '{}' has no extensions configured", component.id),
            None,
            None,
        )
        .with_hint(format!(
            "Add a extension: homeboy component set {} --extension <extension_id>",
            component.id
        ))
    })?;

    let extension_id = if extensions.contains_key("wordpress") {
        "wordpress"
    } else {
        extensions.keys().next().ok_or_else(|| {
            Error::validation_invalid_argument(
                "component",
                format!("Component '{}' has no extensions configured", component.id),
                None,
                None,
            )
            .with_hint(format!(
                "Add a extension: homeboy component set {} --extension <extension_id>",
                component.id
            ))
        })?
    };

    let manifest = extension::load_extension(extension_id)?;

    manifest
        .lint_script()
        .map(|s| s.to_string())
        .ok_or_else(|| {
            Error::validation_invalid_argument(
                "extension",
                format!(
                    "Extension '{}' does not have lint infrastructure configured (missing lint.extension_script)",
                    extension_id
                ),
                None,
                None,
            )
        })
}

pub fn run(args: LintArgs, _global: &super::GlobalArgs) -> CmdResult<LintOutput> {
    let component = args.comp.load()?;
    let script_path = resolve_lint_script(&component)?;

    let before_fix_files = if args.fix {
        Some(changed_file_set(&component.local_path)?)
    } else {
        None
    };

    // Resolve glob from --changed-only or --changed-since flags
    let effective_glob = if args.changed_only {
        let uncommitted = git::get_uncommitted_changes(&component.local_path)?;

        // Collect all changed files
        let mut changed_files: Vec<String> = Vec::new();
        changed_files.extend(uncommitted.staged);
        changed_files.extend(uncommitted.unstaged);
        changed_files.extend(uncommitted.untracked);

        if changed_files.is_empty() {
            println!("No files in working tree changes");
            return Ok((
                LintOutput {
                    status: "passed".to_string(),
                    component: args.comp.component,
                    exit_code: 0,
                    autofix: None,
                    hints: None,
                },
                0,
            ));
        }

        // Make paths absolute so lint runners can find files regardless of
        // the shell's working directory (git status returns repo-relative paths)
        let abs_files: Vec<String> = changed_files
            .iter()
            .map(|f| format!("{}/{}", component.local_path, f))
            .collect();

        // Pass ALL files to extension - let lint runner filter to relevant types
        if abs_files.len() == 1 {
            Some(abs_files[0].clone())
        } else {
            Some(format!("{{{}}}", abs_files.join(",")))
        }
    } else if let Some(ref git_ref) = args.changed_since {
        let changed_files = git::get_files_changed_since(&component.local_path, git_ref)?;

        if changed_files.is_empty() {
            println!("No files changed since {}", git_ref);
            return Ok((
                LintOutput {
                    status: "passed".to_string(),
                    component: args.comp.component,
                    exit_code: 0,
                    autofix: None,
                    hints: None,
                },
                0,
            ));
        }

        // Make paths absolute (git diff returns repo-relative paths)
        let abs_files: Vec<String> = changed_files
            .iter()
            .map(|f| format!("{}/{}", component.local_path, f))
            .collect();

        if abs_files.len() == 1 {
            Some(abs_files[0].clone())
        } else {
            Some(format!("{{{}}}", abs_files.join(",")))
        }
    } else {
        args.glob.clone()
    };

    let output = ExtensionRunner::new(args.comp.id(), &script_path)
        .path_override(args.comp.path.clone())
        .settings(&args.setting_args.setting)
        .env_if(args.fix, "HOMEBOY_AUTO_FIX", "1")
        .env_if(args.summary, "HOMEBOY_SUMMARY_MODE", "1")
        .env_opt("HOMEBOY_LINT_FILE", &args.file)
        .env_opt("HOMEBOY_LINT_GLOB", &effective_glob)
        .env_if(args.errors_only, "HOMEBOY_ERRORS_ONLY", "1")
        .env_opt("HOMEBOY_SNIFFS", &args.sniffs)
        .env_opt("HOMEBOY_EXCLUDE_SNIFFS", &args.exclude_sniffs)
        .env_opt("HOMEBOY_CATEGORY", &args.category)
        .run()?;

    let mut status = if output.success { "passed" } else { "failed" }.to_string();
    let mut autofix = None;

    let mut hints = Vec::new();

    if args.fix {
        let after_fix_files = changed_file_set(&component.local_path)?;
        let files_modified = before_fix_files
            .as_ref()
            .map(|before| count_newly_changed(before, &after_fix_files))
            .unwrap_or(0);

        let outcome = autofix::standard_outcome(
            AutofixMode::Write,
            files_modified,
            Some(format!("homeboy test {} --analyze", args.comp.component)),
            vec![],
        );

        if output.success && outcome.status == "auto_fixed" {
            status = outcome.status.clone();
        }

        hints.extend(outcome.hints.clone());
        autofix = Some(LintAutofixOutput {
            files_modified,
            rerun_recommended: outcome.rerun_recommended,
        });
    }

    // Fix hint when linting fails
    if !output.success && !args.fix {
        hints.push(format!(
            "Run 'homeboy lint {} --fix' to auto-fix formatting issues",
            args.comp.component
        ));
        hints.push("Some issues may require manual fixes".to_string());
    }

    // Capability hints when running component-wide lint (no targeting options used)
    if args.file.is_none()
        && args.glob.is_none()
        && !args.changed_only
        && args.changed_since.is_none()
    {
        hints.push(
            "For targeted linting: --file <path>, --glob <pattern>, --changed-only, or --changed-since <ref>".to_string(),
        );
    }

    // Always include docs reference
    hints.push("Full options: homeboy docs commands/lint".to_string());

    let hints = if hints.is_empty() { None } else { Some(hints) };

    Ok((
        LintOutput {
            status,
            component: args.comp.component,
            exit_code: output.exit_code,
            autofix,
            hints,
        },
        output.exit_code,
    ))
}

fn changed_file_set(local_path: &str) -> homeboy::Result<HashSet<String>> {
    let uncommitted = git::get_uncommitted_changes(local_path)?;
    let mut files = HashSet::new();
    files.extend(uncommitted.staged);
    files.extend(uncommitted.unstaged);
    files.extend(uncommitted.untracked);
    Ok(files)
}

fn count_newly_changed(before: &HashSet<String>, after: &HashSet<String>) -> usize {
    after.difference(before).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_newly_changed_only_counts_new_entries() {
        let before = HashSet::from([
            "src/a.rs".to_string(),
            "src/b.rs".to_string(),
            "README.md".to_string(),
        ]);
        let after = HashSet::from([
            "src/a.rs".to_string(),
            "src/b.rs".to_string(),
            "README.md".to_string(),
            "src/c.rs".to_string(),
            "tests/a_test.rs".to_string(),
        ]);

        assert_eq!(count_newly_changed(&before, &after), 2);
    }
}
