use clap::Args;
use serde::Serialize;
use std::path::Path;

use homeboy::code_audit::{self, fixer, CodeAuditResult};

use super::CmdResult;

#[derive(Args)]
pub struct AuditArgs {
    /// Component ID or direct filesystem path to audit
    pub component_id: String,

    /// Only show discovered conventions (skip findings)
    #[arg(long)]
    pub conventions: bool,

    /// Generate fix stubs for outlier files (dry run by default)
    #[arg(long)]
    pub fix: bool,

    /// Apply fixes to disk (requires --fix)
    #[arg(long, requires = "fix")]
    pub write: bool,
}

#[derive(Serialize)]
#[serde(tag = "command")]
pub enum AuditOutput {
    #[serde(rename = "audit")]
    Full(CodeAuditResult),

    #[serde(rename = "audit.conventions")]
    Conventions {
        component_id: String,
        conventions: Vec<homeboy::code_audit::ConventionReport>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        directory_conventions: Vec<homeboy::code_audit::DirectoryConvention>,
    },

    #[serde(rename = "audit.fix")]
    Fix {
        component_id: String,
        source_path: String,
        #[serde(flatten)]
        fix_result: fixer::FixResult,
        written: bool,
    },
}

pub fn run(args: AuditArgs, _global: &super::GlobalArgs) -> CmdResult<AuditOutput> {
    let result = if Path::new(&args.component_id).is_dir() {
        code_audit::audit_path(&args.component_id)?
    } else {
        code_audit::audit_component(&args.component_id)?
    };

    if args.conventions {
        return Ok((
            AuditOutput::Conventions {
                component_id: result.component_id,
                conventions: result.conventions,
                directory_conventions: result.directory_conventions,
            },
            0,
        ));
    }

    if args.fix {
        let root = Path::new(&result.source_path);
        let mut fix_result = fixer::generate_fixes(&result, root);
        let written = args.write;

        if written && !fix_result.fixes.is_empty() {
            let applied = fixer::apply_fixes(&mut fix_result.fixes, root);
            fix_result.files_modified = applied;
        }

        let exit_code = if fix_result.total_insertions > 0 { 1 } else { 0 };

        return Ok((
            AuditOutput::Fix {
                component_id: result.component_id,
                source_path: result.source_path,
                fix_result,
                written,
            },
            exit_code,
        ));
    }

    let exit_code = if result.summary.outliers_found > 0 {
        1
    } else {
        0
    };
    Ok((AuditOutput::Full(result), exit_code))
}
