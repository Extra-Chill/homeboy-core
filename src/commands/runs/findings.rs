use clap::Args;
use serde::Serialize;

use homeboy::observation::{FindingListFilter, FindingRecord, ObservationStore};
use homeboy::Error;

use crate::commands::{runs::RunsOutput, CmdResult};

#[derive(Args, Clone, Default)]
pub struct RunsFindingsArgs {
    /// Observation run ID
    pub run_id: String,
    /// Finding tool, for example lint
    #[arg(long)]
    pub tool: Option<String>,
    /// Finding file path
    #[arg(long)]
    pub file: Option<String>,
    /// Finding fingerprint
    #[arg(long)]
    pub fingerprint: Option<String>,
    /// Maximum findings to return
    #[arg(long, default_value_t = 100)]
    pub limit: i64,
}

#[derive(Serialize)]
pub struct RunsFindingsOutput {
    pub command: &'static str,
    pub run_id: String,
    pub findings: Vec<FindingRecord>,
}

#[derive(Serialize)]
pub struct RunsFindingOutput {
    pub command: &'static str,
    pub finding: FindingRecord,
}

pub fn findings(args: RunsFindingsArgs) -> CmdResult<RunsOutput> {
    let store = ObservationStore::open_initialized()?;
    require_run(&store, &args.run_id)?;
    let findings = store.list_findings(FindingListFilter {
        run_id: Some(args.run_id.clone()),
        tool: args.tool,
        file: args.file,
        fingerprint: args.fingerprint,
        limit: Some(args.limit),
    })?;

    Ok((
        RunsOutput::Findings(RunsFindingsOutput {
            command: "runs.findings",
            run_id: args.run_id,
            findings,
        }),
        0,
    ))
}

pub fn finding(finding_id: &str) -> CmdResult<RunsOutput> {
    let store = ObservationStore::open_initialized()?;
    let finding = store.get_finding(finding_id)?.ok_or_else(|| {
        Error::validation_invalid_argument(
            "finding_id",
            format!("finding not found: {finding_id}"),
            Some(finding_id.to_string()),
            None,
        )
    })?;

    Ok((
        RunsOutput::Finding(RunsFindingOutput {
            command: "runs.finding",
            finding,
        }),
        0,
    ))
}

fn require_run(store: &ObservationStore, run_id: &str) -> homeboy::Result<()> {
    if store.get_run(run_id)?.is_some() {
        return Ok(());
    }
    Err(Error::validation_invalid_argument(
        "run_id",
        format!("run record not found: {run_id}"),
        Some(run_id.to_string()),
        None,
    ))
}
