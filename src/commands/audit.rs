use clap::Args;
use serde::Serialize;
use std::path::Path;

use homeboy::code_audit::{self, CodeAuditResult};

use super::CmdResult;

#[derive(Args)]
pub struct AuditArgs {
    /// Component ID or direct filesystem path to audit
    pub component_id: String,

    /// Only show discovered conventions (skip findings)
    #[arg(long)]
    pub conventions: bool,
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
    },
}

pub fn run(args: AuditArgs, _global: &super::GlobalArgs) -> CmdResult<AuditOutput> {
    let result = if Path::new(&args.component_id).is_dir() {
        code_audit::audit_path(&args.component_id)?
    } else {
        code_audit::audit_component(&args.component_id)?
    };

    if args.conventions {
        Ok((
            AuditOutput::Conventions {
                component_id: result.component_id,
                conventions: result.conventions,
            },
            0,
        ))
    } else {
        let exit_code = if result.summary.outliers_found > 0 {
            1
        } else {
            0
        };
        Ok((AuditOutput::Full(result), exit_code))
    }
}
