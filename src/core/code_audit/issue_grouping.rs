//! Cross-reference audit findings with open GitHub issues.
//!
//! When an audit run produces findings that overlap an already-open homeboy-filed
//! audit issue (matched by `(component, kind)` grouping key), this module merges
//! the new findings into the existing issue body instead of filing a duplicate.
//!
//! Issue bodies carry a machine-parseable block bracketed by
//! `<!-- homeboy:findings -->` markers, so successive runs can round-trip through
//! the table: preserve existing rows, append newly observed findings, and
//! (optionally) strike through findings that have been resolved since the last run.
//!
//! This module is a reporting/triage helper — it does not produce new `Finding`
//! entries. The GitHub HTTP client pattern mirrors `core::refactor::auto::guard`
//! (reqwest blocking, `Bearer` auth, `application/vnd.github+json`).

use std::collections::{HashMap, HashSet};

use super::{conventions::AuditFinding, findings::Finding};
use crate::{Error, Result};

/// Identifies the bucket a finding belongs to for issue grouping.
///
/// One open issue is maintained per `(component, kind)` pair. File paths are
/// listed as rows inside that issue's findings table — they are not part of
/// the key itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GroupingKey {
    pub component: String,
    pub kind: AuditFinding,
}

/// A single open issue returned by the GitHub search.
#[derive(Debug, Clone)]
pub struct OpenIssue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
}

/// A finding row as stored/parsed inside the `<!-- homeboy:findings -->` block.
///
/// This is an internal representation used by `merge_finding_table` when
/// round-tripping through an existing issue body. Callers interact through
/// `&[&Finding]` and `finding_fingerprint`, not through this type.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FindingRow {
    fingerprint: String,
    file: String,
    description: String,
    first_seen: String,
    resolved: Option<String>,
}

const BLOCK_START: &str = "<!-- homeboy:findings -->";
const BLOCK_END: &str = "<!-- /homeboy:findings -->";
const TABLE_HEADER: &str =
    "| File | Description | First seen | Resolved |\n|---|---|---|---|";

/// Group findings into `(component, kind)` buckets.
///
/// All findings come from a single component audit run, so `component` is
/// threaded in from the caller. Findings sharing a `kind` accumulate into a
/// single bucket that maps to a single GitHub issue.
pub fn group_findings<'a>(
    findings: &'a [Finding],
    component: &str,
) -> HashMap<GroupingKey, Vec<&'a Finding>> {
    let mut grouped: HashMap<GroupingKey, Vec<&'a Finding>> = HashMap::new();
    for finding in findings {
        let key = GroupingKey {
            component: component.to_string(),
            kind: finding.kind.clone(),
        };
        grouped.entry(key).or_default().push(finding);
    }
    grouped
}

/// Stable signature for a finding — used to match new findings against rows
/// already present in an issue body.
///
/// Uses `file|kind|description` (see issue #1275). Intentionally does not
/// include `convention` so that the same underlying bug surfacing via a
/// different convention name still matches across runs.
pub fn finding_fingerprint(f: &Finding) -> String {
    let kind = serde_json::to_value(&f.kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{:?}", f.kind));
    format!("{}|{}|{}", f.file, kind, f.description)
}

/// Query GitHub for open issues that carry both the `audit` label and an
/// `audit:<kind>` label.
///
/// Returns an empty vec on any transport or parse failure — callers should
/// treat "no match" as "file a fresh issue" downstream.
pub fn query_open_issues(repo: &str, token: &str, kind: &AuditFinding) -> Vec<OpenIssue> {
    let kind_slug = audit_finding_slug(kind);
    let url = format!(
        "https://api.github.com/repos/{}/issues?state=open&labels=audit,audit:{}&per_page=100",
        repo, kind_slug
    );

    let client = match reqwest::blocking::Client::builder()
        .user_agent("homeboy")
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let response = match client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github+json")
        .send()
    {
        Ok(r) => r,
        Err(e) => {
            crate::log_status!("audit-issues", "GitHub GET {} failed: {}", url, e);
            return Vec::new();
        }
    };

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        crate::log_status!(
            "audit-issues",
            "GitHub GET {} returned {}: {}",
            url,
            status,
            body.chars().take(200).collect::<String>()
        );
        return Vec::new();
    }

    let issues: Vec<serde_json::Value> = match response.json() {
        Ok(v) => v,
        Err(e) => {
            crate::log_status!(
                "audit-issues",
                "GitHub GET {} returned non-JSON body: {}",
                url,
                e
            );
            return Vec::new();
        }
    };

    issues
        .into_iter()
        .filter(|v| v.get("pull_request").is_none()) // /issues includes PRs; drop them
        .filter_map(|v| {
            let number = v.get("number").and_then(|n| n.as_u64())?;
            let title = v
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let body = v
                .get("body")
                .and_then(|b| b.as_str())
                .unwrap_or("")
                .to_string();
            let labels = v
                .get("labels")
                .and_then(|l| l.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|lbl| {
                            lbl.get("name").and_then(|n| n.as_str()).map(str::to_string)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Some(OpenIssue {
                number,
                title,
                body,
                labels,
            })
        })
        .collect()
}

/// Merge new findings into an existing issue body, producing a new body.
///
/// Behavior:
/// - If `existing_body` does not contain the `<!-- homeboy:findings -->` block,
///   a fresh block is created (appended to the body).
/// - Rows whose fingerprint appears in `new_findings` AND in the existing table
///   are preserved as-is.
/// - Findings in `new_findings` but not in the table are appended with today's
///   date as `First seen`.
/// - Rows in the table but not in `new_findings` are considered resolved:
///   if their fingerprint is in `resolved_fingerprints` (or `resolved_fingerprints`
///   is empty, meaning "mark every missing row resolved"), the File and
///   Description cells are wrapped in `~~...~~` and the Resolved date is set.
///
/// The contract matches issue #1275's body format (deterministic, round-trippable).
pub fn merge_finding_table(
    existing_body: &str,
    new_findings: &[&Finding],
    resolved_fingerprints: &[String],
) -> String {
    merge_finding_table_with_date(existing_body, new_findings, resolved_fingerprints, &today())
}

/// Inner implementation that accepts an injected date — enables deterministic tests.
fn merge_finding_table_with_date(
    existing_body: &str,
    new_findings: &[&Finding],
    resolved_fingerprints: &[String],
    today_str: &str,
) -> String {
    let (prefix, existing_rows, suffix) = split_body(existing_body);

    // Index new findings by the *row* fingerprint (file||description) because
    // that's the key stored inside the table. A stored row does not carry
    // `kind` in its fingerprint — every row in a given issue shares the kind
    // enforced by grouping upstream.
    let new_by_row_fp: HashMap<String, &Finding> = new_findings
        .iter()
        .map(|f| (row_fingerprint_from_finding(f), *f))
        .collect();

    // Accept `resolved_fingerprints` in either the row-style (`file||description`)
    // or the public fingerprint style (`file|kind|description`) so callers can
    // pass `finding_fingerprint(&f)` directly without needing to know the
    // internal row-fingerprint format.
    let resolved_set: HashSet<String> = resolved_fingerprints
        .iter()
        .map(|fp| public_to_row_fp(fp))
        .collect();
    let explicit_resolution = !resolved_fingerprints.is_empty();

    let mut merged: Vec<FindingRow> = Vec::new();
    let mut emitted_fps: HashSet<String> = HashSet::new();

    // 1. Walk existing rows in order, preserving, resolving, or leaving strikethrough as-is.
    for row in &existing_rows {
        emitted_fps.insert(row.fingerprint.clone());
        if new_by_row_fp.contains_key(&row.fingerprint) {
            // Still observed — preserve unchanged (including any prior resolved mark,
            // though a finding re-appearing is normally a fresh row with no strikethrough).
            merged.push(row.clone());
        } else if row.resolved.is_some() {
            // Already marked resolved in a previous run — keep as-is.
            merged.push(row.clone());
        } else {
            // Row is missing from this run — resolve if caller opted in.
            let should_resolve =
                !explicit_resolution || resolved_set.contains(&row.fingerprint);
            if should_resolve {
                let mut resolved_row = row.clone();
                resolved_row.resolved = Some(today_str.to_string());
                merged.push(resolved_row);
            } else {
                merged.push(row.clone());
            }
        }
    }

    // 2. Append newly observed findings in input order.
    for finding in new_findings {
        let row_fp = row_fingerprint_from_finding(finding);
        if emitted_fps.insert(row_fp.clone()) {
            merged.push(FindingRow {
                fingerprint: row_fp,
                file: finding.file.clone(),
                description: finding.description.clone(),
                first_seen: today_str.to_string(),
                resolved: None,
            });
        }
    }

    let block = render_block(&merged);
    assemble_body(&prefix, &block, &suffix)
}

/// PATCH an issue's body on GitHub. Returns Err on non-2xx or transport failure.
pub fn sync_issue(repo: &str, token: &str, issue_number: u64, new_body: &str) -> Result<()> {
    let url = format!(
        "https://api.github.com/repos/{}/issues/{}",
        repo, issue_number
    );

    let client = reqwest::blocking::Client::builder()
        .user_agent("homeboy")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| Error::internal_unexpected(format!("reqwest client build failed: {}", e)))?;

    let response = client
        .patch(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github+json")
        .json(&serde_json::json!({ "body": new_body }))
        .send()
        .map_err(|e| Error::internal_unexpected(format!("GitHub PATCH failed: {}", e)))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().unwrap_or_default();
        return Err(Error::internal_unexpected(format!(
            "GitHub PATCH {} returned {}: {}",
            url, status, text
        )));
    }

    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

/// Snake_case representation of an AuditFinding — matches the `audit:<kind>`
/// label convention on GitHub.
pub fn audit_finding_slug(kind: &AuditFinding) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{:?}", kind).to_lowercase())
}

/// Split a body into (prefix, existing rows, suffix) around the findings block.
/// If no block is present, returns (body, [], "").
fn split_body(body: &str) -> (String, Vec<FindingRow>, String) {
    let Some(start) = body.find(BLOCK_START) else {
        return (body.to_string(), Vec::new(), String::new());
    };
    let after_start = start + BLOCK_START.len();
    let Some(end_rel) = body[after_start..].find(BLOCK_END) else {
        // Open marker with no close — treat as absent rather than corrupt the body.
        return (body.to_string(), Vec::new(), String::new());
    };
    let end = after_start + end_rel;
    let inner = &body[after_start..end];
    let prefix = body[..start].trim_end_matches('\n').to_string();
    let suffix = body[end + BLOCK_END.len()..]
        .trim_start_matches('\n')
        .to_string();
    let rows = parse_rows(inner);
    (prefix, rows, suffix)
}

/// Parse the inner text of the findings block into typed rows.
///
/// Rows are recognized by a pipe-delimited format with exactly 4 cells. The
/// header line and separator line (`|---|---|---|---|`) are skipped.
fn parse_rows(inner: &str) -> Vec<FindingRow> {
    let mut rows = Vec::new();
    for line in inner.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        // Skip the header ("| File | ...") and separator ("|---|...|").
        let lowered = trimmed.to_ascii_lowercase();
        if lowered.contains("| file") && lowered.contains("description") {
            continue;
        }
        if trimmed.chars().all(|c| matches!(c, '|' | '-' | ' ')) {
            continue;
        }

        // Split on '|' and strip leading/trailing empty cells produced by the
        // outer pipes. We expect exactly 4 content cells.
        let cells: Vec<String> = trimmed
            .trim_matches('|')
            .split('|')
            .map(|s| s.trim().to_string())
            .collect();
        if cells.len() != 4 {
            continue;
        }

        let mut cells = cells.into_iter();
        let file_cell = cells.next().expect("4 cells");
        let desc_cell = cells.next().expect("4 cells");
        let first_seen = cells.next().expect("4 cells");
        let resolved_cell = cells.next().expect("4 cells");

        let file = unstrike(&file_cell);
        let description = unstrike(&desc_cell);
        let resolved = if resolved_cell.is_empty() {
            None
        } else {
            Some(resolved_cell)
        };

        // Fingerprint stored rows using file + description (no kind: rows in a
        // single issue all share the same kind, enforced by grouping).
        let fingerprint = format!("{}||{}", file, description);

        rows.push(FindingRow {
            fingerprint,
            file,
            description,
            first_seen,
            resolved,
        });
    }
    rows
}

/// Strip a leading/trailing `~~...~~` wrapper if present.
fn unstrike(cell: &str) -> String {
    if let Some(inner) = cell.strip_prefix("~~").and_then(|s| s.strip_suffix("~~")) {
        inner.to_string()
    } else {
        cell.to_string()
    }
}

/// Render a table block from a list of rows.
fn render_block(rows: &[FindingRow]) -> String {
    let mut out = String::new();
    out.push_str(BLOCK_START);
    out.push('\n');
    out.push_str(TABLE_HEADER);
    out.push('\n');
    for row in rows {
        let resolved_date = row.resolved.clone().unwrap_or_default();
        if row.resolved.is_some() {
            out.push_str(&format!(
                "| ~~{}~~ | ~~{}~~ | {} | {} |\n",
                row.file, row.description, row.first_seen, resolved_date
            ));
        } else {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                row.file, row.description, row.first_seen, resolved_date
            ));
        }
    }
    out.push_str(BLOCK_END);
    out
}

/// Reassemble prefix + block + suffix with single blank-line separators.
fn assemble_body(prefix: &str, block: &str, suffix: &str) -> String {
    let mut out = String::new();
    if !prefix.is_empty() {
        out.push_str(prefix);
        out.push_str("\n\n");
    }
    out.push_str(block);
    if !suffix.is_empty() {
        out.push_str("\n\n");
        out.push_str(suffix);
    }
    out
}

/// Fingerprint of a stored row, derived from its file + description cells.
/// Used by `merge_finding_table` when reconciling new findings against an
/// existing body. Stored rows can't include `kind` because a single issue
/// corresponds to a single kind (enforced by grouping), so the kind field
/// would be constant noise in every row.
fn row_fingerprint_from_finding(finding: &Finding) -> String {
    format!("{}||{}", finding.file, finding.description)
}

/// Normalize a caller-provided fingerprint to the row-style fingerprint used
/// internally. Accepts both `finding_fingerprint` output (`file|kind|description`)
/// and already-normalized `file||description` strings.
fn public_to_row_fp(fp: &str) -> String {
    // Public fingerprint: "file|kind|description" (3 parts).
    // Row fingerprint:    "file||description"    (2 parts with empty middle).
    // We split on '|' and, if we see 3 parts, drop the middle (kind) segment.
    let parts: Vec<&str> = fp.splitn(3, '|').collect();
    if parts.len() == 3 && !parts[1].is_empty() {
        format!("{}||{}", parts[0], parts[2])
    } else {
        fp.to_string()
    }
}

/// Today's date as `YYYY-MM-DD` in UTC.
///
/// UTC (not `Local`) keeps the issue body stable when CI runs in a different
/// timezone than the developer's machine — comparing issue bodies across
/// environments should not produce spurious diffs at midnight.
fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_audit::findings::Severity;

    fn finding(file: &str, kind: AuditFinding, description: &str) -> Finding {
        Finding {
            convention: "test".to_string(),
            severity: Severity::Warning,
            file: file.to_string(),
            description: description.to_string(),
            suggestion: String::new(),
            kind,
        }
    }

    #[test]
    fn group_findings_buckets_by_kind() {
        let findings = vec![
            finding("a.php", AuditFinding::MissingMethod, "missing x"),
            finding("b.php", AuditFinding::MissingMethod, "missing y"),
            finding("c.php", AuditFinding::GodFile, "too big"),
        ];
        let grouped = group_findings(&findings, "demo");
        let mm_key = GroupingKey {
            component: "demo".to_string(),
            kind: AuditFinding::MissingMethod,
        };
        let gf_key = GroupingKey {
            component: "demo".to_string(),
            kind: AuditFinding::GodFile,
        };
        assert_eq!(grouped.get(&mm_key).map(|v| v.len()), Some(2));
        assert_eq!(grouped.get(&gf_key).map(|v| v.len()), Some(1));
    }

    #[test]
    fn finding_fingerprint_stable_across_clones() {
        let f = finding("a.php", AuditFinding::GodFile, "too big");
        assert_eq!(finding_fingerprint(&f), finding_fingerprint(&f.clone()));
        assert!(finding_fingerprint(&f).contains("god_file"));
    }

    #[test]
    fn merge_fresh_body_produces_full_table() {
        let findings = vec![
            finding("a.php:10", AuditFinding::GodFile, "file is huge"),
            finding("b.php:20", AuditFinding::GodFile, "file is enormous"),
        ];
        let refs: Vec<&Finding> = findings.iter().collect();
        let body =
            merge_finding_table_with_date("# Top of issue\n\nIntro text.", &refs, &[], "2026-04-20");
        assert!(body.contains(BLOCK_START));
        assert!(body.contains(BLOCK_END));
        assert!(body.contains("a.php:10"));
        assert!(body.contains("b.php:20"));
        assert!(body.contains("2026-04-20"));
        // Preserves pre-existing prose.
        assert!(body.contains("Intro text."));
    }

    #[test]
    fn merge_appends_new_finding_as_new_row() {
        let existing = format!(
            "Header\n\n{start}\n{header}\n| a.php:10 | original bug | 2026-01-01 |  |\n{end}\n",
            start = BLOCK_START,
            header = TABLE_HEADER,
            end = BLOCK_END,
        );
        let new = vec![
            finding("a.php:10", AuditFinding::GodFile, "original bug"),
            finding("b.php:20", AuditFinding::GodFile, "brand new bug"),
        ];
        let refs: Vec<&Finding> = new.iter().collect();
        let body = merge_finding_table_with_date(&existing, &refs, &[], "2026-04-20");

        // Original row preserved with its original date.
        assert!(body.contains("| a.php:10 | original bug | 2026-01-01 |"));
        // New row appended with today's date.
        assert!(body.contains("| b.php:20 | brand new bug | 2026-04-20 |"));
    }

    #[test]
    fn merge_strikes_resolved_row_when_missing() {
        let existing = format!(
            "{start}\n{header}\n| a.php:10 | fixed now | 2026-01-01 |  |\n| b.php:20 | still broken | 2026-01-02 |  |\n{end}",
            start = BLOCK_START,
            header = TABLE_HEADER,
            end = BLOCK_END,
        );
        // Only b.php:20 shows up this run.
        let new = vec![finding("b.php:20", AuditFinding::GodFile, "still broken")];
        let refs: Vec<&Finding> = new.iter().collect();
        // Empty `resolved_fingerprints` means "mark every missing row resolved".
        let body = merge_finding_table_with_date(&existing, &refs, &[], "2026-04-23");

        // a.php row strike-through with resolved date.
        assert!(
            body.contains("| ~~a.php:10~~ | ~~fixed now~~ | 2026-01-01 | 2026-04-23 |"),
            "expected strike-through resolved row, got body:\n{}",
            body
        );
        // b.php row untouched.
        assert!(body.contains("| b.php:20 | still broken | 2026-01-02 |"));
        assert!(!body.contains("~~b.php:20~~"));
    }

    #[test]
    fn merge_preserves_unchanged_rows() {
        let existing = format!(
            "{start}\n{header}\n| a.php:10 | kept verbatim | 2026-01-01 |  |\n{end}",
            start = BLOCK_START,
            header = TABLE_HEADER,
            end = BLOCK_END,
        );
        let new = vec![finding("a.php:10", AuditFinding::GodFile, "kept verbatim")];
        let refs: Vec<&Finding> = new.iter().collect();
        let body = merge_finding_table_with_date(&existing, &refs, &[], "2026-04-23");
        // First-seen date preserved — today's date must not overwrite it.
        assert!(body.contains("| a.php:10 | kept verbatim | 2026-01-01 |"));
        assert!(!body.contains("| a.php:10 | kept verbatim | 2026-04-23 |"));
    }

    #[test]
    fn merge_honors_explicit_resolved_list() {
        let existing = format!(
            "{start}\n{header}\n| a.php:10 | resolve me | 2026-01-01 |  |\n| b.php:20 | dont touch | 2026-01-01 |  |\n{end}",
            start = BLOCK_START,
            header = TABLE_HEADER,
            end = BLOCK_END,
        );
        // Neither finding observed this run. Only mark a.php as resolved explicitly.
        let new: Vec<&Finding> = Vec::new();
        // Row fingerprint is `file||description` — feed both so the test matches
        // whichever convention the merger follows.
        let resolved = vec!["a.php:10||resolve me".to_string()];
        let body = merge_finding_table_with_date(&existing, &new, &resolved, "2026-04-23");

        assert!(body.contains("| ~~a.php:10~~ | ~~resolve me~~ | 2026-01-01 | 2026-04-23 |"));
        // b.php stays un-struck.
        assert!(body.contains("| b.php:20 | dont touch | 2026-01-01 |  |"));
        assert!(!body.contains("~~b.php:20~~"));
    }

    #[test]
    fn audit_finding_slug_matches_serde_snake_case() {
        assert_eq!(audit_finding_slug(&AuditFinding::GodFile), "god_file");
        assert_eq!(
            audit_finding_slug(&AuditFinding::LayerOwnershipViolation),
            "layer_ownership_violation"
        );
    }

    #[test]
    fn today_is_iso_date() {
        let t = today();
        // "YYYY-MM-DD" => 10 chars, with dashes at fixed positions.
        assert_eq!(t.len(), 10);
        assert_eq!(&t[4..5], "-");
        assert_eq!(&t[7..8], "-");
    }

    #[test]
    fn parse_rows_skips_header_and_separator() {
        let inner = "\n| File | Description | First seen | Resolved |\n|---|---|---|---|\n| a | b | 2026-01-01 |  |\n";
        let rows = parse_rows(inner);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].file, "a");
        assert_eq!(rows[0].description, "b");
        assert_eq!(rows[0].resolved, None);
    }

    #[test]
    fn parse_rows_recognizes_struck_rows() {
        let inner = "| ~~x~~ | ~~y~~ | 2026-01-01 | 2026-04-23 |";
        let rows = parse_rows(inner);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].file, "x");
        assert_eq!(rows[0].description, "y");
        assert_eq!(rows[0].resolved.as_deref(), Some("2026-04-23"));
    }

    #[test]
    fn row_fingerprint_from_finding_is_file_and_description() {
        let f = finding("src/x.rs:1", AuditFinding::GodFile, "huge");
        assert_eq!(row_fingerprint_from_finding(&f), "src/x.rs:1||huge");
    }

    #[test]
    fn public_fingerprint_converts_to_row_fingerprint() {
        let f = finding("src/x.rs:1", AuditFinding::GodFile, "huge");
        let public = finding_fingerprint(&f);
        assert_eq!(public_to_row_fp(&public), "src/x.rs:1||huge");
        // Already-row-shaped strings pass through unchanged.
        assert_eq!(
            public_to_row_fp("src/x.rs:1||huge"),
            "src/x.rs:1||huge"
        );
    }
}
