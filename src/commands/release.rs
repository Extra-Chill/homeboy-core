use clap::{Args, Subcommand};
use serde::Serialize;

use homeboy::release::{self, ReleasePlan};

use super::CmdResult;

#[derive(Args)]

pub struct ReleaseArgs {
    #[command(subcommand)]
    command: ReleaseCommand,
}

#[derive(Subcommand)]

enum ReleaseCommand {
    /// Plan a component release without executing steps
    Plan {
        /// Component ID to plan
        component_id: String,
        /// Module ID to source release defaults/actions (optional)
        #[arg(long)]
        module: Option<String>,
    },
}

#[derive(Serialize)]
#[serde(tag = "command")]

pub enum ReleaseOutput {
    #[serde(rename = "release.plan")]
    Plan { plan: ReleasePlan },
}

pub fn run(args: ReleaseArgs, _global: &crate::commands::GlobalArgs) -> CmdResult<ReleaseOutput> {
    match args.command {
        ReleaseCommand::Plan {
            component_id,
            module,
        } => {
            let plan = release::plan(&component_id, module.as_deref())?;
            Ok((ReleaseOutput::Plan { plan }, 0))
        }
    }
}
