use clap::{Args, Subcommand};
use serde::Serialize;

use homeboy::git::{self, BulkGitOutput, GitOutput};

use crate::commands::version;

use super::CmdResult;

#[derive(Args)]
pub struct GitArgs {
    #[command(subcommand)]
    command: GitCommand,
}

#[derive(Subcommand)]
enum GitCommand {
    /// Show git status for a component
    Status {
        /// JSON input spec for bulk operations.
        /// Use "-" for stdin, "@file.json" for file, or inline JSON string.
        #[arg(long)]
        json: Option<String>,

        /// Component ID (non-JSON mode)
        component_id: Option<String>,
    },
    /// Stage all changes and commit
    Commit {
        /// JSON input spec for bulk operations.
        /// Use "-" for stdin, "@file.json" for file, or inline JSON string.
        #[arg(long)]
        json: Option<String>,

        /// Component ID (non-JSON mode)
        component_id: Option<String>,

        /// Commit message (non-JSON mode)
        message: Option<String>,
    },
    /// Push local commits to remote
    Push {
        /// JSON input spec for bulk operations.
        /// Use "-" for stdin, "@file.json" for file, or inline JSON string.
        #[arg(long)]
        json: Option<String>,

        /// Component ID (non-JSON mode)
        component_id: Option<String>,

        /// Push tags as well
        #[arg(long)]
        tags: bool,
    },
    /// Pull remote changes
    Pull {
        /// JSON input spec for bulk operations.
        /// Use "-" for stdin, "@file.json" for file, or inline JSON string.
        #[arg(long)]
        json: Option<String>,

        /// Component ID (non-JSON mode)
        component_id: Option<String>,
    },
    /// Create a git tag
    Tag {
        /// Component ID
        component_id: String,
        /// Tag name (e.g., v0.1.2)
        ///
        /// If omitted, tag defaults to v<component version>.
        tag_name: Option<String>,
        /// Tag message (creates annotated tag)
        #[arg(short, long)]
        message: Option<String>,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum GitCommandOutput {
    Single(GitOutput),
    Bulk(BulkGitOutput),
}

pub fn run(args: GitArgs, _global: &crate::commands::GlobalArgs) -> CmdResult<GitCommandOutput> {
    match args.command {
        GitCommand::Status { json, component_id } => {
            if let Some(spec) = json {
                let output = git::status_bulk(&spec)?;
                let exit_code = if output.summary.failed > 0 { 1 } else { 0 };
                return Ok((GitCommandOutput::Bulk(output), exit_code));
            }

            let id = component_id.ok_or_else(|| {
                homeboy::Error::validation_invalid_argument(
                    "componentId",
                    "Missing componentId (or use --json for bulk)",
                    None,
                    None,
                )
            })?;
            let output = git::status(&id)?;
            let exit_code = output.exit_code;
            Ok((GitCommandOutput::Single(output), exit_code))
        }
        GitCommand::Commit {
            json,
            component_id,
            message,
        } => {
            if let Some(spec) = json {
                let output = git::commit_bulk(&spec)?;
                let exit_code = if output.summary.failed > 0 { 1 } else { 0 };
                return Ok((GitCommandOutput::Bulk(output), exit_code));
            }

            let id = component_id.ok_or_else(|| {
                homeboy::Error::validation_invalid_argument(
                    "componentId",
                    "Missing componentId (or use --json for bulk)",
                    None,
                    None,
                )
            })?;
            let msg = message.ok_or_else(|| {
                homeboy::Error::validation_invalid_argument(
                    "message",
                    "Missing message (or use --json for bulk)",
                    None,
                    None,
                )
            })?;
            let output = git::commit(&id, &msg)?;
            let exit_code = output.exit_code;
            Ok((GitCommandOutput::Single(output), exit_code))
        }
        GitCommand::Push {
            json,
            component_id,
            tags,
        } => {
            if let Some(spec) = json {
                let output = git::push_bulk(&spec)?;
                let exit_code = if output.summary.failed > 0 { 1 } else { 0 };
                return Ok((GitCommandOutput::Bulk(output), exit_code));
            }

            let id = component_id.ok_or_else(|| {
                homeboy::Error::validation_invalid_argument(
                    "componentId",
                    "Missing componentId (or use --json for bulk)",
                    None,
                    None,
                )
            })?;
            let output = git::push(&id, tags)?;
            let exit_code = output.exit_code;
            Ok((GitCommandOutput::Single(output), exit_code))
        }
        GitCommand::Pull { json, component_id } => {
            if let Some(spec) = json {
                let output = git::pull_bulk(&spec)?;
                let exit_code = if output.summary.failed > 0 { 1 } else { 0 };
                return Ok((GitCommandOutput::Bulk(output), exit_code));
            }

            let id = component_id.ok_or_else(|| {
                homeboy::Error::validation_invalid_argument(
                    "componentId",
                    "Missing componentId (or use --json for bulk)",
                    None,
                    None,
                )
            })?;
            let output = git::pull(&id)?;
            let exit_code = output.exit_code;
            Ok((GitCommandOutput::Single(output), exit_code))
        }
        GitCommand::Tag {
            component_id,
            tag_name,
            message,
        } => {
            let derived_tag_name = match tag_name {
                Some(name) => name,
                None => {
                    let (out, _) = version::show_version_output(&component_id)?;
                    format!("v{}", out.version)
                }
            };

            let output = git::tag(&component_id, &derived_tag_name, message.as_deref())?;
            let exit_code = output.exit_code;
            Ok((GitCommandOutput::Single(output), exit_code))
        }
    }
}
