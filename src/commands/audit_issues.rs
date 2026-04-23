//! `homeboy audit-issues sync` — cross-reference audit findings with open GitHub issues.
//!
//! When an audit run produces findings that overlap an already-open homeboy-filed
//! audit issue (matched by `(component, kind)` grouping key), this command
//! updates the existing issue body in-place instead of filing a duplicate.
//!
//! Soft-fails cleanly when `GITHUB_REPOSITORY` / `GH_TOKEN` / `GITHUB_TOKEN`
//! are not set — the same pattern used by `core::refactor::auto::guard`.

use std::path::Path;

use clap::{Args, Subcommand};
use serde::Serialize;

use homeboy::code_audit::{self, issue_grouping, AuditFinding, Finding};
use homeboy::engine::execution_context::{self, ResolveOptions};

use super::CmdResult;

#[derive(Args)]
pub struct AuditIssuesArgs {
    #[command(subcommand)]
    pub command: AuditIssuesCommand,
}

#[derive(Subcommand)]
pub enum AuditIssuesCommand {
    /// Sync an audit run against open `audit` issues on GitHub.
    Sync(SyncArgs),
}

#[derive(Args)]
pub struct SyncArgs {
    /// Component ID (registered) or filesystem path to audit.
    pub target: String,

    /// Print the proposed new issue bodies to stdout instead of PATCHing them.
    #[arg(long)]
    pub dry_run: bool,

    /// Wrap rows in `~~...~~` when a finding that was previously recorded is no
    /// longer observed in this run.
    #[arg(long)]
    pub strike_resolved: bool,
}

#[derive(Serialize)]
#[serde(tag = "command")]
pub enum AuditIssuesOutput {
    #[serde(rename = "audit-issues.sync")]
    Sync(SyncResult),
    #[serde(rename = "audit-issues.skipped")]
    Skipped { reason: String },
}

#[derive(Serialize, Default)]
pub struct SyncResult {
    pub component_id: String,
    pub source_path: String,
    pub dry_run: bool,
    pub strike_resolved: bool,
    pub groups: Vec<SyncGroup>,
    /// Number of groups that actually updated an open issue.
    pub updated: usize,
    /// Number of groups that had no matching open issue (skipped).
    pub unmatched: usize,
}

#[derive(Serialize)]
pub struct SyncGroup {
    pub kind: String,
    pub finding_count: usize,
    pub issue_number: Option<u64>,
    pub issue_title: Option<String>,
    pub action: SyncAction,
    /// Proposed body — only populated in dry-run mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposed_body: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncAction {
    /// Found a matching issue; body would be (or was) PATCHed.
    Updated,
    /// Dry-run: showed the proposed body but didn't PATCH.
    WouldUpdate,
    /// No matching open issue for this `(component, kind)` bucket.
    NoMatch,
    /// PATCH request failed — reason recorded in the skipped field.
    Failed { reason: String },
}

pub fn run(args: AuditIssuesArgs, _global: &super::GlobalArgs) -> CmdResult<AuditIssuesOutput> {
    match args.command {
        AuditIssuesCommand::Sync(sync) => run_sync(sync),
    }
}

fn run_sync(args: SyncArgs) -> CmdResult<AuditIssuesOutput> {
    // Soft-fail when CI env isn't wired up. Matches the pattern in
    // `core::refactor::auto::guard` — the command prints a short notice to
    // stderr and exits 0 so CI wrappers don't choke on local `--dry-run`.
    let repo = std::env::var("GITHUB_REPOSITORY").ok();
    let token = std::env::var("GH_TOKEN")
        .ok()
        .or_else(|| std::env::var("GITHUB_TOKEN").ok());

    let ci_env_missing = repo.as_deref().unwrap_or("").is_empty()
        || (!args.dry_run && token.as_deref().unwrap_or("").is_empty());

    if ci_env_missing && !args.dry_run {
        eprintln!("[audit-issues] CI env not set — skipping sync");
        return Ok((
            AuditIssuesOutput::Skipped {
                reason: "GITHUB_REPOSITORY and/or token env not set".to_string(),
            },
            0,
        ));
    }

    // Resolve target → (component_id, source_path). Same resolution pattern as
    // the `audit` command so registered IDs and raw paths both work.
    let (component_id, source_path) = resolve_target(&args.target)?;

    homeboy::log_status!(
        "audit-issues",
        "Auditing {} at {}",
        component_id,
        source_path
    );

    let audit_result = code_audit::audit_path_with_id(&component_id, &source_path)?;
    homeboy::log_status!(
        "audit-issues",
        "Audit produced {} finding(s)",
        audit_result.findings.len()
    );

    // Group by (component, kind). One issue per bucket.
    // HashMap keys are unique by construction, so we only need to sort for
    // deterministic output order — no dedup needed.
    let grouped = issue_grouping::group_findings(&audit_result.findings, &component_id);
    let mut kinds: Vec<(String, &AuditFinding)> = grouped
        .keys()
        .map(|k| (issue_grouping::audit_finding_slug(&k.kind), &k.kind))
        .collect();
    kinds.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = SyncResult {
        component_id: component_id.clone(),
        source_path: source_path.clone(),
        dry_run: args.dry_run,
        strike_resolved: args.strike_resolved,
        ..SyncResult::default()
    };

    for (slug, kind) in kinds {
        let key = issue_grouping::GroupingKey {
            component: component_id.clone(),
            kind: kind.clone(),
        };
        let findings_ref: Vec<&Finding> =
            grouped.get(&key).cloned().unwrap_or_default();

        let group_log = format!("{} ({} finding(s))", slug, findings_ref.len());

        // Look up matching open issues. Without a token we can still dry-run
        // against a synthetic "fresh body" scenario — GitHub won't be queried.
        let matching = match (repo.as_deref(), token.as_deref()) {
            (Some(r), Some(t)) if !r.is_empty() && !t.is_empty() => {
                issue_grouping::query_open_issues(r, t, kind)
            }
            _ => Vec::new(),
        };

        // Prefer the oldest issue (lowest number) so repeat runs don't bounce
        // between duplicates.
        let Some(target_issue) = matching.into_iter().min_by_key(|i| i.number) else {
            homeboy::log_status!("audit-issues", "{} — no matching open issue", group_log);
            out.groups.push(SyncGroup {
                kind: slug,
                finding_count: findings_ref.len(),
                issue_number: None,
                issue_title: None,
                action: SyncAction::NoMatch,
                proposed_body: None,
            });
            out.unmatched += 1;
            continue;
        };

        // Compute the new body. When `--strike-resolved` is off, pass a
        // sentinel (a single non-matching fingerprint) so no rows get struck —
        // the merge algorithm only strikes when `resolved_fingerprints` is
        // empty OR the missing row's fingerprint appears in the list.
        let resolved_fingerprints: Vec<String> = if args.strike_resolved {
            Vec::new()
        } else {
            vec!["__homeboy::no-strike-sentinel__".to_string()]
        };
        let new_body = issue_grouping::merge_finding_table(
            &target_issue.body,
            &findings_ref,
            &resolved_fingerprints,
        );

        if args.dry_run {
            println!(
                "### Issue #{} — {}\n{}\n",
                target_issue.number, target_issue.title, new_body
            );
            out.groups.push(SyncGroup {
                kind: slug,
                finding_count: findings_ref.len(),
                issue_number: Some(target_issue.number),
                issue_title: Some(target_issue.title),
                action: SyncAction::WouldUpdate,
                proposed_body: Some(new_body),
            });
            continue;
        }

        // Non-dry-run: PATCH the issue. Token is guaranteed non-empty above.
        let token = token.as_deref().unwrap_or("");
        let repo = repo.as_deref().unwrap_or("");
        match issue_grouping::sync_issue(repo, token, target_issue.number, &new_body) {
            Ok(()) => {
                homeboy::log_status!(
                    "audit-issues",
                    "{} — updated issue #{}",
                    group_log,
                    target_issue.number
                );
                out.groups.push(SyncGroup {
                    kind: slug,
                    finding_count: findings_ref.len(),
                    issue_number: Some(target_issue.number),
                    issue_title: Some(target_issue.title),
                    action: SyncAction::Updated,
                    proposed_body: None,
                });
                out.updated += 1;
            }
            Err(e) => {
                let reason = e.to_string();
                homeboy::log_status!(
                    "audit-issues",
                    "{} — PATCH failed: {}",
                    group_log,
                    reason
                );
                out.groups.push(SyncGroup {
                    kind: slug,
                    finding_count: findings_ref.len(),
                    issue_number: Some(target_issue.number),
                    issue_title: Some(target_issue.title),
                    action: SyncAction::Failed { reason },
                    proposed_body: None,
                });
            }
        }
    }

    Ok((AuditIssuesOutput::Sync(out), 0))
}

/// Resolve a `<component|path>` target to (component_id, absolute_source_path).
/// Mirrors the logic in `commands::audit::run` so behavior stays consistent.
fn resolve_target(target: &str) -> homeboy::Result<(String, String)> {
    if Path::new(target).is_dir() {
        let name = Path::new(target)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        return Ok((name, target.to_string()));
    }

    let ctx = execution_context::resolve(&ResolveOptions::source_only(target, None))?;
    Ok((
        ctx.component_id,
        ctx.source_path.to_string_lossy().to_string(),
    ))
}
