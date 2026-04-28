use clap::{Args, Subcommand};

use homeboy::deps::{self, DependencyStatus, DependencyUpdateResult};

use super::CmdResult;

#[derive(Args)]
pub struct DepsArgs {
    #[command(subcommand)]
    command: DepsCommand,
}

#[derive(Subcommand)]
enum DepsCommand {
    /// Inspect dependency constraints and locked package versions
    Status {
        /// Component ID. When omitted, auto-detected from CWD.
        component: Option<String>,

        /// Limit output to one package.
        #[arg(long, value_name = "PACKAGE")]
        package: Option<String>,

        /// Workspace path to operate on directly.
        #[arg(long, value_name = "PATH")]
        path: Option<String>,
    },
    /// Update one Composer package explicitly
    Update {
        /// Composer package name, e.g. chubes4/block-format-bridge.
        package: String,

        /// Component ID. When omitted, auto-detected from CWD.
        component: Option<String>,

        /// New manifest constraint, e.g. ^0.4.
        #[arg(long, value_name = "CONSTRAINT")]
        to: Option<String>,

        /// Workspace path to operate on directly.
        #[arg(long, value_name = "PATH")]
        path: Option<String>,
    },
}

pub fn run(args: DepsArgs, _global: &crate::commands::GlobalArgs) -> CmdResult<serde_json::Value> {
    match args.command {
        DepsCommand::Status {
            component,
            package,
            path,
        } => {
            let output: DependencyStatus =
                deps::status(component.as_deref(), path.as_deref(), package.as_deref())?;
            Ok((
                serde_json::to_value(output).map_err(|e| {
                    homeboy::Error::internal_json(
                        e.to_string(),
                        Some("serialize deps status".to_string()),
                    )
                })?,
                0,
            ))
        }
        DepsCommand::Update {
            package,
            component,
            to,
            path,
        } => {
            let output: DependencyUpdateResult = deps::update(
                component.as_deref(),
                path.as_deref(),
                &package,
                to.as_deref(),
            )?;
            Ok((
                serde_json::to_value(output).map_err(|e| {
                    homeboy::Error::internal_json(
                        e.to_string(),
                        Some("serialize deps update".to_string()),
                    )
                })?,
                0,
            ))
        }
    }
}
