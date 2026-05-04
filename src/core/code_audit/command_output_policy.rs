//! Scattered command response/output policy detection.
//!
//! The audit is intentionally source-shape based: command modules should not
//! independently own response modes, artifact emission, or success/error
//! wrapping policy. The canonical output layer should own those contracts.

use std::collections::{BTreeSet, HashMap};

use regex::Regex;

use super::conventions::AuditFinding;
use super::findings::{Finding, Severity};
use super::fingerprint::FileFingerprint;

const MIN_FILES: usize = 2;

#[derive(Debug, Clone)]
struct PolicySite {
    file: String,
    functions: Vec<String>,
}

pub(super) fn run(fingerprints: &[&FileFingerprint]) -> Vec<Finding> {
    let mut groups: HashMap<String, Vec<PolicySite>> = HashMap::new();

    for fp in fingerprints {
        if !is_command_module(&fp.relative_path) || is_excluded_path(&fp.relative_path) {
            continue;
        }
        if is_intentionally_command_specific(&fp.content) {
            continue;
        }

        let signals = policy_signals(&fp.content);
        if signals.len() < 2 {
            continue;
        }

        groups
            .entry(signals.into_iter().collect::<Vec<_>>().join(" + "))
            .or_default()
            .push(PolicySite {
                file: fp.relative_path.clone(),
                functions: functions_with_policy(&fp.content),
            });
    }

    let mut findings = Vec::new();
    for (policy, mut sites) in groups {
        sites.sort_by(|a, b| a.file.cmp(&b.file));
        sites.dedup_by(|a, b| a.file == b.file);
        if sites.len() < MIN_FILES {
            continue;
        }

        let anchor = sites[0].file.clone();
        let details = sites
            .iter()
            .map(|site| {
                let functions = if site.functions.is_empty() {
                    "module scope".to_string()
                } else {
                    site.functions.join(", ")
                };
                format!("{} [{}]", site.file, functions)
            })
            .collect::<Vec<_>>()
            .join("; ");

        findings.push(Finding {
            convention: "command_output_policy".to_string(),
            severity: Severity::Warning,
            file: anchor,
            description: format!(
                "Repeated command response/output policy ({}) appears in {} command modules: {}",
                policy,
                sites.len(),
                details
            ),
            suggestion: "Move the shared response mode, artifact, or wrapper policy into the canonical command output layer; keep command-specific rendering declarative or mark intentional local rendering explicitly.".to_string(),
            kind: AuditFinding::CommandOutputPolicy,
        });
    }

    findings.sort_by(|a, b| a.file.cmp(&b.file).then(a.description.cmp(&b.description)));
    findings
}

fn is_command_module(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/command/")
        || lower.contains("/commands/")
        || lower.starts_with("command/")
        || lower.starts_with("commands/")
        || lower.contains("/cmd/")
        || lower.starts_with("cmd/")
}

fn is_excluded_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/test/")
        || lower.contains("/tests/")
        || lower.ends_with("/tests.rs")
        || lower.ends_with("_tests.rs")
        || lower.ends_with("/test_fixture.rs")
        || lower.contains("/fixtures/")
        || lower.contains("/vendor/")
        || lower.contains("/generated/")
        || lower.contains("/utils/response.")
        || lower.ends_with(".md")
}

fn is_intentionally_command_specific(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("intentional command-specific rendering")
        || lower.contains("intentionally command-specific rendering")
        || lower.contains("command-specific rendering is intentional")
        || lower.contains("local command rendering")
}

fn policy_signals(content: &str) -> BTreeSet<&'static str> {
    let scrubbed = scrub_comments(content);
    let mut signals = BTreeSet::new();

    if has_response_mode_branch(&scrubbed) {
        signals.insert("response mode branching");
    }
    if has_artifact_policy(&scrubbed) {
        signals.insert("artifact emission policy");
    }
    if has_response_wrapper(&scrubbed) {
        signals.insert("success/error wrapper policy");
    }
    if has_output_routing(&scrubbed) {
        signals.insert("output routing policy");
    }

    signals
}

fn has_response_mode_branch(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let has_mode_subject = [
        "response_mode",
        "output_mode",
        "output_format",
        "format",
        "mode",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    let mode_count = ["json", "markdown", "text", "plain"]
        .iter()
        .filter(|needle| lower.contains(**needle))
        .count();

    has_mode_subject && mode_count >= 2 && contains_branch_keyword(&lower)
}

fn has_artifact_policy(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("artifact")
        && (lower.contains("write")
            || lower.contains("emit")
            || lower.contains("save")
            || lower.contains("attach"))
}

fn has_response_wrapper(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let success = Regex::new(r#"[\"']success[\"']\s*[:=>]"#).expect("regex compiles");
    let error = Regex::new(r#"[\"']error[\"']\s*[:=>]"#).expect("regex compiles");
    let message = Regex::new(r#"[\"']message[\"']\s*[:=>]"#).expect("regex compiles");

    (success.is_match(content) && (error.is_match(content) || message.is_match(content)))
        || (lower.contains("success")
            && lower.contains("error")
            && (lower.contains("wrap") || lower.contains("envelope") || lower.contains("response")))
}

fn has_output_routing(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let has_output_subject = lower.contains("output") || lower.contains("report");
    let has_route = lower.contains("stdout")
        || lower.contains("stderr")
        || lower.contains("file")
        || lower.contains("path")
        || lower.contains("destination");

    has_output_subject && has_route && contains_branch_keyword(&lower)
}

fn contains_branch_keyword(lower: &str) -> bool {
    lower.contains("if ")
        || lower.contains("if(")
        || lower.contains("match ")
        || lower.contains("switch")
        || lower.contains("case ")
        || lower.contains("else")
}

fn functions_with_policy(content: &str) -> Vec<String> {
    let scrubbed = scrub_comments(content);
    let function_regex = Regex::new(
        r#"(?m)\b(?:fn|function|def)\s+([A-Za-z_][A-Za-z0-9_]*)|\b([A-Za-z_][A-Za-z0-9_]*)\s*[:=]\s*(?:async\s*)?\([^\n;{}]*\)\s*(?:=>|\{)"#,
    )
    .expect("regex compiles");
    let policy_regex = Regex::new(
        r#"(?i)response_mode|output_mode|output_format|artifact|success|error|markdown|stdout|stderr|destination"#,
    )
    .expect("regex compiles");

    let mut functions = BTreeSet::new();
    let matches: Vec<_> = function_regex.captures_iter(&scrubbed).collect();
    for (idx, captures) in matches.iter().enumerate() {
        let Some(matched) = captures.get(0) else {
            continue;
        };
        let end = matches
            .get(idx + 1)
            .and_then(|next| next.get(0))
            .map(|next| next.start())
            .unwrap_or(scrubbed.len());
        if policy_regex.is_match(&scrubbed[matched.start()..end]) {
            let name = captures
                .get(1)
                .or_else(|| captures.get(2))
                .map(|name| name.as_str())
                .unwrap_or("<anonymous>");
            functions.insert(name.to_string());
        }
    }

    functions.into_iter().collect()
}

fn scrub_comments(content: &str) -> String {
    let line_comments = Regex::new(r#"(?m)//.*$"#).expect("regex compiles");
    let block_comments = Regex::new(r#"(?s)/\*.*?\*/"#).expect("regex compiles");
    strip_cfg_test_tail(
        block_comments
            .replace_all(&line_comments.replace_all(content, ""), "")
            .as_ref(),
    )
}

fn strip_cfg_test_tail(content: &str) -> String {
    let mut kept = Vec::new();
    for line in content.lines() {
        if line.trim() == "#[cfg(test)]" {
            break;
        }
        kept.push(line);
    }
    kept.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(path: &str, content: &str) -> FileFingerprint {
        FileFingerprint {
            relative_path: path.to_string(),
            content: content.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_run() {
        let one = fp(
            "src/commands/alpha.ext",
            r#"
function run_alpha(ctx) {
  if (ctx.output_format == "json") return { "success": true, "message": ctx.value };
  if (ctx.output_format == "markdown") return renderMarkdown(ctx.value);
  if (ctx.artifact_path) writeArtifact(ctx.artifact_path, ctx.value);
}
"#,
        );
        let two = fp(
            "src/commands/beta.ext",
            r#"
function execute_beta(ctx) {
  switch (ctx.output_format) {
    case "json": return { "success": false, "error": ctx.err };
    case "markdown": return formatMarkdown(ctx.err);
  }
  if (ctx.artifact_path) saveArtifact(ctx.artifact_path, ctx.err);
}
"#,
        );

        let findings = run(&[&one, &two]);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, AuditFinding::CommandOutputPolicy);
        assert!(findings[0].description.contains("alpha.ext"));
        assert!(findings[0].description.contains("execute_beta"));
        assert!(findings[0].description.contains("artifact emission policy"));
    }

    #[test]
    fn skips_intentional_command_specific_rendering_fixtures() {
        let one = fp(
            "src/commands/alpha.ext",
            r#"
// Intentional command-specific rendering: this command streams a custom table.
function run_alpha(ctx) {
  if (ctx.output_format == "json") return { "success": true, "message": ctx.value };
  if (ctx.output_format == "markdown") return renderMarkdown(ctx.value);
  if (ctx.artifact_path) writeArtifact(ctx.artifact_path, ctx.value);
}
"#,
        );
        let two = fp(
            "src/commands/beta.ext",
            r#"
function run_beta(ctx) {
  if (ctx.output_format == "json") return { "success": false, "error": ctx.err };
  if (ctx.output_format == "markdown") return formatMarkdown(ctx.err);
  if (ctx.artifact_path) saveArtifact(ctx.artifact_path, ctx.err);
}
"#,
        );

        assert!(run(&[&one, &two]).is_empty());
    }

    #[test]
    fn ignores_non_command_modules_and_single_policy_sites() {
        let helper = fp(
            "src/shared/output.ext",
            r#"
function wrap(ctx) {
  if (ctx.output_format == "json") return { "success": true, "message": ctx.value };
  if (ctx.output_format == "markdown") return renderMarkdown(ctx.value);
  if (ctx.artifact_path) writeArtifact(ctx.artifact_path, ctx.value);
}
"#,
        );
        let command = fp(
            "src/commands/alpha.ext",
            r#"
function run_alpha(ctx) {
  if (ctx.output_format == "json") return { "success": true, "message": ctx.value };
  if (ctx.output_format == "markdown") return renderMarkdown(ctx.value);
}
"#,
        );

        assert!(run(&[&helper, &command]).is_empty());
    }

    #[test]
    fn skips_command_test_modules_and_response_helpers() {
        let test_module = fp(
            "src/commands/example/tests.ext",
            r#"
function fixture(ctx) {
  if (ctx.output_format == "json") return { "success": true, "message": ctx.value };
  if (ctx.output_format == "markdown") return renderMarkdown(ctx.value);
  if (ctx.artifact_path) writeArtifact(ctx.artifact_path, ctx.value);
}
"#,
        );
        let response_helper = fp(
            "src/commands/utils/response.ext",
            r#"
function canonical(ctx) {
  if (ctx.output_format == "json") return { "success": true, "message": ctx.value };
  if (ctx.output_format == "markdown") return renderMarkdown(ctx.value);
  if (ctx.artifact_path) writeArtifact(ctx.artifact_path, ctx.value);
}
"#,
        );

        assert!(run(&[&test_module, &response_helper]).is_empty());
    }

    #[test]
    fn ignores_cfg_test_tail_inside_command_modules() {
        let one = fp(
            "src/commands/alpha.rs",
            r#"
pub fn run(ctx: Context) -> Result<()> {
    run_shared(ctx)
}

#[cfg(test)]
mod tests {
    fn fixture(ctx: Context) {
        if ctx.output_format == "json" { return json!({ "success": true, "message": ctx.value }); }
        if ctx.output_format == "markdown" { render_markdown(ctx.value); }
        if ctx.artifact_path.is_some() { write_artifact(ctx.artifact_path, ctx.value); }
    }
}
"#,
        );
        let two = fp(
            "src/commands/beta.rs",
            r#"
pub fn run(ctx: Context) -> Result<()> {
    run_shared(ctx)
}

#[cfg(test)]
mod tests {
    fn fixture(ctx: Context) {
        if ctx.output_format == "json" { return json!({ "success": false, "error": ctx.err }); }
        if ctx.output_format == "markdown" { render_markdown(ctx.err); }
        if ctx.artifact_path.is_some() { write_artifact(ctx.artifact_path, ctx.err); }
    }
}
"#,
        );

        assert!(run(&[&one, &two]).is_empty());
    }
}
