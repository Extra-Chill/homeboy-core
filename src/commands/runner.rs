use std::collections::HashMap;

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::Value;

use homeboy::runner::{self, Runner, RunnerKind};
use homeboy::{EntityCrudOutput, MergeOutput};

use super::{CmdResult, DynamicSetArgs};

mod doctor;

#[derive(Debug, Default, Serialize)]
pub struct RunnerExtra {}

pub type RunnerOutput = EntityCrudOutput<Runner, RunnerExtra>;

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum RunnerCommandOutput {
    Registry(RunnerOutput),
    Doctor(doctor::RunnerDoctorOutput),
}

#[derive(Args)]
pub struct RunnerArgs {
    #[command(subcommand)]
    command: RunnerCommand,
}

#[derive(Subcommand)]
enum RunnerCommand {
    /// Register a local or SSH execution runner
    Add {
        /// JSON input spec for add/update (supports single or bulk)
        #[arg(long)]
        json: Option<String>,

        /// Skip items that already exist (JSON mode only)
        #[arg(long)]
        skip_existing: bool,

        /// Runner ID
        id: Option<String>,

        /// Runner kind. Defaults to ssh when --server is set, otherwise local.
        #[arg(long, value_enum)]
        kind: Option<RunnerKindArg>,

        /// Existing server ID for SSH runners
        #[arg(long)]
        server: Option<String>,

        /// Root directory where this runner checks out or owns workspaces
        #[arg(long)]
        workspace_root: Option<String>,

        /// Homeboy binary path on the runner machine
        #[arg(long)]
        homeboy_path: Option<String>,

        /// Prefer daemon-backed execution for future runner commands
        #[arg(long)]
        daemon: bool,

        /// Maximum concurrent workflows this runner should accept
        #[arg(long)]
        concurrency_limit: Option<usize>,

        /// Artifact retention/copying policy label for future execution commands
        #[arg(long)]
        artifact_policy: Option<String>,
    },
    /// List all configured runners
    List,
    /// Display runner configuration
    Show {
        /// Runner ID
        id: String,
    },
    /// Modify runner settings
    #[command(visible_aliases = ["edit", "merge"])]
    Set {
        #[command(flatten)]
        args: DynamicSetArgs,
    },
    /// Remove a runner configuration
    Remove {
        /// Runner ID
        id: String,
    },
    /// Diagnose a local or configured SSH runner without mutating it
    Doctor {
        /// Runner ID. Use `local`, `localhost`, or `self` for this machine;
        /// other values resolve through `homeboy runner` configuration.
        runner_id: String,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum RunnerKindArg {
    Local,
    Ssh,
}

impl From<RunnerKindArg> for RunnerKind {
    fn from(value: RunnerKindArg) -> Self {
        match value {
            RunnerKindArg::Local => RunnerKind::Local,
            RunnerKindArg::Ssh => RunnerKind::Ssh,
        }
    }
}

pub fn run(
    args: RunnerArgs,
    _global: &crate::commands::GlobalArgs,
) -> CmdResult<RunnerCommandOutput> {
    match args.command {
        RunnerCommand::Add {
            json,
            skip_existing,
            id,
            kind,
            server,
            workspace_root,
            homeboy_path,
            daemon,
            concurrency_limit,
            artifact_policy,
        } => map_registry(add(RunnerAddInput {
            json,
            skip_existing,
            id,
            kind,
            server,
            workspace_root,
            homeboy_path,
            daemon,
            concurrency_limit,
            artifact_policy,
        })),
        RunnerCommand::List => map_registry(list()),
        RunnerCommand::Show { id } => map_registry(show(&id)),
        RunnerCommand::Set { args } => map_registry(set(args)),
        RunnerCommand::Remove { id } => map_registry(remove(&id)),
        RunnerCommand::Doctor { runner_id } => map_doctor(doctor::run(&runner_id)),
    }
}

fn map_registry(result: CmdResult<RunnerOutput>) -> CmdResult<RunnerCommandOutput> {
    result.map(|(output, exit_code)| (RunnerCommandOutput::Registry(output), exit_code))
}

fn map_doctor(result: CmdResult<doctor::RunnerDoctorOutput>) -> CmdResult<RunnerCommandOutput> {
    result.map(|(output, exit_code)| (RunnerCommandOutput::Doctor(output), exit_code))
}

struct RunnerAddInput {
    json: Option<String>,
    skip_existing: bool,
    id: Option<String>,
    kind: Option<RunnerKindArg>,
    server: Option<String>,
    workspace_root: Option<String>,
    homeboy_path: Option<String>,
    daemon: bool,
    concurrency_limit: Option<usize>,
    artifact_policy: Option<String>,
}

fn add(input: RunnerAddInput) -> CmdResult<RunnerOutput> {
    let json_spec = if let Some(spec) = input.json {
        spec
    } else {
        let id = input.id.ok_or_else(|| {
            homeboy::Error::validation_invalid_argument(
                "id",
                "Missing required argument: id",
                None,
                None,
            )
        })?;
        let kind = input.kind.map(RunnerKind::from).unwrap_or_else(|| {
            if input.server.is_some() {
                RunnerKind::Ssh
            } else {
                RunnerKind::Local
            }
        });
        let new_runner = Runner {
            id,
            kind,
            server_id: input.server,
            workspace_root: input.workspace_root,
            homeboy_path: input.homeboy_path,
            daemon: input.daemon,
            concurrency_limit: input.concurrency_limit,
            artifact_policy: input.artifact_policy,
            env: HashMap::new(),
            resources: HashMap::<String, Value>::new(),
        };

        homeboy::config::to_json_string(&new_runner)?
    };

    match runner::create(&json_spec, input.skip_existing)? {
        homeboy::CreateOutput::Single(result) => Ok((
            RunnerOutput {
                command: "runner.add".to_string(),
                id: Some(result.id),
                entity: Some(result.entity),
                updated_fields: vec!["created".to_string()],
                ..Default::default()
            },
            0,
        )),
        homeboy::CreateOutput::Bulk(summary) => {
            let exit_code = summary.exit_code();
            Ok((
                RunnerOutput {
                    command: "runner.add".to_string(),
                    import: Some(summary),
                    ..Default::default()
                },
                exit_code,
            ))
        }
    }
}

fn list() -> CmdResult<RunnerOutput> {
    Ok((
        RunnerOutput {
            command: "runner.list".to_string(),
            entities: runner::list()?,
            ..Default::default()
        },
        0,
    ))
}

fn show(id: &str) -> CmdResult<RunnerOutput> {
    let runner = runner::load(id)?;
    Ok((
        RunnerOutput {
            command: "runner.show".to_string(),
            id: Some(runner.id.clone()),
            entity: Some(runner),
            ..Default::default()
        },
        0,
    ))
}

fn set(args: DynamicSetArgs) -> CmdResult<RunnerOutput> {
    let merged = super::merge_dynamic_args(&args)?.ok_or_else(|| {
        homeboy::Error::validation_invalid_argument(
            "spec",
            "Provide JSON spec, --json flag, --base64 flag, or --key value flags",
            None,
            None,
        )
    })?;
    let (json_string, replace_fields) = super::finalize_set_spec(&merged, &args.replace)?;

    match runner::merge(args.id.as_deref(), &json_string, &replace_fields)? {
        MergeOutput::Single(result) => {
            let entity = runner::load(&result.id)?;
            Ok((
                RunnerOutput {
                    command: "runner.set".to_string(),
                    id: Some(result.id),
                    entity: Some(entity),
                    updated_fields: result.updated_fields,
                    ..Default::default()
                },
                0,
            ))
        }
        MergeOutput::Bulk(summary) => {
            let exit_code = summary.exit_code();
            Ok((
                RunnerOutput {
                    command: "runner.set".to_string(),
                    batch: Some(summary),
                    ..Default::default()
                },
                exit_code,
            ))
        }
    }
}

fn remove(id: &str) -> CmdResult<RunnerOutput> {
    runner::delete_safe(id)?;
    Ok((
        RunnerOutput {
            command: "runner.remove".to_string(),
            id: Some(id.to_string()),
            deleted: vec![id.to_string()],
            ..Default::default()
        },
        0,
    ))
}
