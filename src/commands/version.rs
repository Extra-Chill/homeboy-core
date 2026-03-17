use clap::{Args, Subcommand};
use serde::Serialize;

use homeboy::component;
use homeboy::release::{self, ReleaseCommandInput, ReleaseCommandResult};
use homeboy::version::{read_component_version, read_version, VersionTargetInfo};

use super::utils::args::{DryRunArgs, HiddenJsonArgs, PositionalComponentArgs};
use super::CmdResult;

#[derive(Serialize)]
#[serde(untagged)]
pub enum VersionOutput {
    Show(VersionShowOutput),
    Bump(VersionBumpOutput),
}

#[derive(Args)]
pub struct VersionArgs {
    #[command(subcommand)]
    command: VersionCommand,
}

#[derive(Subcommand)]
enum VersionCommand {
    /// Show current version (default: homeboy binary)
    Show {
        /// Component ID (optional - shows homeboy binary version when omitted)
        component_id: Option<String>,

        /// Override local_path for version file lookup
        #[arg(long)]
        path: Option<String>,
    },

    /// Bump version and release (alias for `homeboy release`)
    Bump {
        #[command(flatten)]
        comp: PositionalComponentArgs,

        #[command(flatten)]
        dry_run_args: DryRunArgs,

        #[command(flatten)]
        _json: HiddenJsonArgs,

        /// Deploy to all projects using this component after release
        #[arg(long)]
        deploy: bool,

        /// Recover from an interrupted release (tag + push current version)
        #[arg(long)]
        recover: bool,

        /// Skip pre-release lint and test checks
        #[arg(long)]
        skip_checks: bool,

        /// Allow a major version bump. Required when commits contain breaking changes.
        /// Without this flag, homeboy will warn and exit instead of releasing a major bump.
        #[arg(long)]
        major: bool,

        /// Skip publish/package steps (version bump + tag + push only).
        /// Use when CI handles publishing after the tag is pushed.
        #[arg(long)]
        skip_publish: bool,
    },
}

#[derive(Serialize)]
pub struct VersionShowOutput {
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    component_id: Option<String>,
    pub version: String,
    targets: Vec<VersionTargetInfo>,
}

#[derive(Serialize)]
#[serde(tag = "command", rename = "release")]
pub struct VersionBumpOutput {
    pub result: ReleaseCommandResult,
}

pub fn run(args: VersionArgs, _global: &crate::commands::GlobalArgs) -> CmdResult<VersionOutput> {
    match args.command {
        VersionCommand::Show { component_id, path } => {
            let info = if let Some(ref p) = path {
                let comp = component::resolve_effective(component_id.as_deref(), Some(p), None)?;
                read_component_version(&comp)?
            } else if component_id.is_some() {
                // Explicit component ID or CWD discovery
                let comp = component::resolve_effective(component_id.as_deref(), None, None)?;
                read_component_version(&comp)?
            } else {
                // No component ID and no --path: try CWD discovery first,
                // fall back to showing homeboy binary version
                match component::resolve_effective(None, None, None) {
                    Ok(comp) => read_component_version(&comp)?,
                    Err(_) => read_version(None)?,
                }
            };

            let display_id = component_id.or_else(|| {
                // Include discovered component ID in output
                if info.targets.is_empty() {
                    None
                } else {
                    component::resolve_effective(None, None, None)
                        .ok()
                        .map(|c| c.id)
                }
            });

            Ok((
                VersionOutput::Show(VersionShowOutput {
                    command: "version.show".to_string(),
                    component_id: display_id,
                    version: info.version,
                    targets: info.targets,
                }),
                0,
            ))
        }
        VersionCommand::Bump {
            comp,
            dry_run_args,
            _json: _,
            deploy,
            recover,
            skip_checks,
            major,
            skip_publish,
        } => {
            let (result, exit_code) = release::run_command(ReleaseCommandInput {
                component_id: comp.id().to_string(),
                path_override: comp.path.clone(),
                dry_run: dry_run_args.dry_run,
                deploy,
                recover,
                skip_checks,
                major,
                skip_publish,
            })?;

            Ok((VersionOutput::Bump(VersionBumpOutput { result }), exit_code))
        }
    }
}

pub fn show_version_output(component_id: &str) -> CmdResult<VersionShowOutput> {
    let info = read_version(Some(component_id))?;

    Ok((
        VersionShowOutput {
            command: "version.show".to_string(),
            component_id: Some(component_id.to_string()),
            version: info.version,
            targets: info.targets,
        },
        0,
    ))
}
