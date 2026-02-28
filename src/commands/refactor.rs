use clap::{Args, Subcommand};
use serde::Serialize;

use homeboy::component;
use homeboy::refactor::{self, RenameScope, RenameSpec};

use crate::commands::CmdResult;

#[derive(Args)]
pub struct RefactorArgs {
    #[command(subcommand)]
    command: RefactorCommand,
}

#[derive(Subcommand)]
enum RefactorCommand {
    /// Rename a term across the codebase with case-variant awareness
    Rename {
        /// Term to rename from
        #[arg(long)]
        from: String,
        /// Term to rename to
        #[arg(long)]
        to: String,
        /// Component ID (uses its local_path as the root)
        #[arg(short, long)]
        component: Option<String>,
        /// Directory path to refactor (alternative to --component)
        #[arg(long)]
        path: Option<String>,
        /// Scope: code, config, all (default: all)
        #[arg(long, default_value = "all")]
        scope: String,
        /// Exact string matching (no boundary detection, no case variants)
        #[arg(long)]
        literal: bool,
        /// Apply changes to disk (default is dry-run)
        #[arg(long)]
        write: bool,
    },
}

pub fn run(args: RefactorArgs, _global: &crate::commands::GlobalArgs) -> CmdResult<RefactorOutput> {
    match args.command {
        RefactorCommand::Rename {
            from,
            to,
            component: component_id,
            path,
            scope,
            literal,
            write,
        } => run_rename(&from, &to, component_id.as_deref(), path.as_deref(), &scope, literal, write),
    }
}

#[derive(Serialize)]
#[serde(tag = "command")]
pub enum RefactorOutput {
    #[serde(rename = "refactor.rename")]
    Rename {
        from: String,
        to: String,
        scope: String,
        dry_run: bool,
        variants: Vec<VariantSummary>,
        total_references: usize,
        total_files: usize,
        edits: Vec<EditSummary>,
        file_renames: Vec<RenameSummary>,
        warnings: Vec<WarningSummary>,
        applied: bool,
    },
}

#[derive(Serialize)]
pub struct VariantSummary {
    pub from: String,
    pub to: String,
    pub label: String,
}

#[derive(Serialize)]
pub struct EditSummary {
    pub file: String,
    pub replacements: usize,
}

#[derive(Serialize)]
pub struct RenameSummary {
    pub from: String,
    pub to: String,
}

#[derive(Serialize)]
pub struct WarningSummary {
    pub kind: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    pub message: String,
}

fn run_rename(
    from: &str,
    to: &str,
    component_id: Option<&str>,
    path: Option<&str>,
    scope: &str,
    literal: bool,
    write: bool,
) -> CmdResult<RefactorOutput> {
    let scope = RenameScope::from_str(scope)?;

    // Resolve root directory
    let root = if let Some(p) = path {
        std::path::PathBuf::from(p)
    } else {
        let comp = component::resolve(component_id)?;
        let validated = component::validate_local_path(&comp)?;
        validated
    };

    let spec = if literal {
        RenameSpec::literal(from, to, scope.clone())
    } else {
        RenameSpec::new(from, to, scope.clone())
    };
    let mut result = refactor::generate_renames(&spec, &root);

    // Print warnings to stderr before applying
    for warning in &result.warnings {
        let location = warning
            .line
            .map(|l| format!("{}:{}", warning.file, l))
            .unwrap_or_else(|| warning.file.clone());
        homeboy::log_status!("warning", "{}: {}", location, warning.message);
    }

    if write {
        if !result.warnings.is_empty() {
            homeboy::log_status!(
                "warning",
                "{} collision warning(s) detected — applying anyway",
                result.warnings.len()
            );
        }
        refactor::apply_renames(&mut result, &root)?;
    }

    let scope_str = match scope {
        RenameScope::Code => "code",
        RenameScope::Config => "config",
        RenameScope::All => "all",
    };

    let exit_code = if result.total_references == 0 { 1 } else { 0 };

    Ok((
        RefactorOutput::Rename {
            from: from.to_string(),
            to: to.to_string(),
            scope: scope_str.to_string(),
            dry_run: !write,
            variants: result
                .variants
                .iter()
                .map(|v| VariantSummary {
                    from: v.from.clone(),
                    to: v.to.clone(),
                    label: v.label.clone(),
                })
                .collect(),
            total_references: result.total_references,
            total_files: result.total_files,
            edits: result
                .edits
                .iter()
                .map(|e| EditSummary {
                    file: e.file.clone(),
                    replacements: e.replacements,
                })
                .collect(),
            file_renames: result
                .file_renames
                .iter()
                .map(|r| RenameSummary {
                    from: r.from.clone(),
                    to: r.to.clone(),
                })
                .collect(),
            warnings: result
                .warnings
                .iter()
                .map(|w| WarningSummary {
                    kind: w.kind.clone(),
                    file: w.file.clone(),
                    line: w.line,
                    message: w.message.clone(),
                })
                .collect(),
            applied: result.applied,
        },
        exit_code,
    ))
}
