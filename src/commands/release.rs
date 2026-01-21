use clap::{Args, ValueEnum};
use serde::Serialize;

use homeboy::release::{self, ReleasePlan, ReleaseRun};

use super::CmdResult;

#[derive(Clone, ValueEnum)]
pub enum BumpType {
    Patch,
    Minor,
    Major,
}

impl BumpType {
    pub fn as_str(&self) -> &'static str {
        match self {
            BumpType::Patch => "patch",
            BumpType::Minor => "minor",
            BumpType::Major => "major",
        }
    }
}

#[derive(Args)]
pub struct ReleaseArgs {
    /// Component ID
    #[arg(value_name = "COMPONENT")]
    component_id: String,

    /// Version bump type (patch, minor, major)
    #[arg(value_name = "BUMP_TYPE")]
    bump_type: BumpType,

    /// Preview what will happen without making changes
    #[arg(long)]
    dry_run: bool,

    /// Skip creating git tag
    #[arg(long)]
    no_tag: bool,

    /// Skip pushing to remote (implies no publish)
    #[arg(long)]
    no_push: bool,

    /// Skip auto-committing uncommitted changes (fail if dirty)
    #[arg(long)]
    no_commit: bool,

    /// Custom message for pre-release commit
    #[arg(long, value_name = "MESSAGE")]
    commit_message: Option<String>,

    /// Accept --json for compatibility (output is JSON by default)
    #[arg(long, hide = true)]
    json: bool,
}

#[derive(Serialize)]
#[serde(tag = "command", rename = "release")]
pub struct ReleaseOutput {
    pub result: ReleaseResult,
}

#[derive(Serialize)]
pub struct ReleaseResult {
    pub component_id: String,
    pub bump_type: String,
    pub dry_run: bool,
    pub no_tag: bool,
    pub no_push: bool,
    pub no_commit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<ReleasePlan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<ReleaseRun>,
}

pub fn run(args: ReleaseArgs, _global: &crate::commands::GlobalArgs) -> CmdResult<ReleaseOutput> {
    let options = release::ReleaseOptions {
        bump_type: args.bump_type.as_str().to_string(),
        dry_run: args.dry_run,
        no_tag: args.no_tag,
        no_push: args.no_push,
        no_commit: args.no_commit,
        commit_message: args.commit_message.clone(),
    };

    if args.dry_run {
        let plan = release::plan_unified(&args.component_id, &options)?;
        Ok((
            ReleaseOutput {
                result: ReleaseResult {
                    component_id: args.component_id,
                    bump_type: options.bump_type,
                    dry_run: true,
                    no_tag: args.no_tag,
                    no_push: args.no_push,
                    no_commit: args.no_commit,
                    commit_message: args.commit_message,
                    plan: Some(plan),
                    run: None,
                },
            },
            0,
        ))
    } else {
        let run_result = release::run(&args.component_id, &options)?;
        Ok((
            ReleaseOutput {
                result: ReleaseResult {
                    component_id: args.component_id,
                    bump_type: options.bump_type,
                    dry_run: false,
                    no_tag: args.no_tag,
                    no_push: args.no_push,
                    no_commit: args.no_commit,
                    commit_message: args.commit_message,
                    plan: None,
                    run: Some(run_result),
                },
            },
            0,
        ))
    }
}
