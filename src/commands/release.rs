use clap::{Args, Subcommand};
use serde::Serialize;

use homeboy::release::{self, ReleasePlan, ReleaseRun};

use super::CmdResult;

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ReleaseArgs {
    /// Component ID for direct release run (shorthand for `release run <component>`)
    #[arg(value_name = "COMPONENT")]
    component_id: Option<String>,

    #[command(subcommand)]
    command: Option<ReleaseCommand>,

    /// Accept --json for compatibility (output is JSON by default)
    #[arg(long, hide = true)]
    json: bool,
}

#[derive(Subcommand)]

enum ReleaseCommand {
    /// Plan a component release without executing steps
    Plan {
        /// Component ID to plan
        component_id: String,
    },
    /// Run a component release pipeline
    Run {
        /// Component ID to run
        component_id: String,
    },
}

#[derive(Serialize)]
#[serde(tag = "command")]

pub enum ReleaseOutput {
    #[serde(rename = "release.plan")]
    Plan { plan: ReleasePlan },
    #[serde(rename = "release.run")]
    Run { run: ReleaseRun },
}

pub fn run(args: ReleaseArgs, _global: &crate::commands::GlobalArgs) -> CmdResult<ReleaseOutput> {
    if let Some(command) = args.command {
        match command {
            ReleaseCommand::Plan { component_id } => {
                let plan = release::plan(&component_id, None)?;
                Ok((ReleaseOutput::Plan { plan }, 0))
            }
            ReleaseCommand::Run { component_id } => {
                let run = release::run(&component_id, None)?;
                Ok((ReleaseOutput::Run { run }, 0))
            }
        }
    } else if let Some(component_id) = args.component_id {
        let run = release::run(&component_id, None)?;
        Ok((ReleaseOutput::Run { run }, 0))
    } else {
        Err(homeboy::Error::validation_invalid_argument(
            "input",
            "Provide component ID or use `release plan|run <component>`",
            None,
            None,
        ))
    }
}
