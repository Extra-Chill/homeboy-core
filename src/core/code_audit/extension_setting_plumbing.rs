//! Repeated extension setting parse/serialize plumbing detection.
//!
//! This detector is deliberately contract-oriented: it looks for command-path
//! functions that independently read the same setting key while also doing
//! typed/string conversion or schema/default normalization nearby. It does not
//! know any ecosystem-specific setting names.

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;

use super::conventions::AuditFinding;
use super::findings::{Finding, Severity};
use super::fingerprint::FileFingerprint;

const CONTEXT_RADIUS: usize = 180;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct SettingSite {
    file: String,
    function: String,
    operations: BTreeSet<&'static str>,
}

pub(super) fn run(fingerprints: &[&FileFingerprint]) -> Vec<Finding> {
    let mut by_key: BTreeMap<String, Vec<SettingSite>> = BTreeMap::new();

    for fp in fingerprints {
        if !is_command_path(&fp.relative_path) || is_test_path(&fp.relative_path) {
            continue;
        }

        for function in extract_functions(&fp.content) {
            for (key, operations) in setting_operations(&function.body) {
                by_key.entry(key).or_default().push(SettingSite {
                    file: fp.relative_path.clone(),
                    function: function.name.clone(),
                    operations,
                });
            }
        }
    }

    let mut findings = Vec::new();
    for (key, sites) in by_key {
        let deduped = dedupe_sites(sites);
        let functions: BTreeSet<String> = deduped
            .iter()
            .map(|site| format!("{}::{}", site.file, site.function))
            .collect();
        if functions.len() < 2 {
            continue;
        }

        let operations: BTreeSet<&'static str> = deduped
            .iter()
            .flat_map(|site| site.operations.iter().copied())
            .collect();
        if !operations.contains("parse") && !operations.contains("serialize") {
            continue;
        }

        let Some(anchor) = deduped.first() else {
            continue;
        };
        let function_list = functions.into_iter().collect::<Vec<_>>().join(", ");
        let operation_list = operations.into_iter().collect::<Vec<_>>().join(", ");

        findings.push(Finding {
            convention: "extension_setting_plumbing".to_string(),
            severity: Severity::Info,
            file: anchor.file.clone(),
            description: format!(
                "Repeated extension setting plumbing for `{}` across command functions: {}; operations: {}",
                key, function_list, operation_list
            ),
            suggestion: format!(
                "Move `{}` parsing, serialization, defaults, and schema normalization into the shared extension settings contract instead of duplicating it in command modules",
                key
            ),
            kind: AuditFinding::ExtensionSettingPlumbing,
        });
    }

    findings.sort_by(|a, b| a.file.cmp(&b.file).then(a.description.cmp(&b.description)));
    findings
}

fn is_command_path(path: &str) -> bool {
    path.split(['/', '\\'])
        .any(|segment| segment == "commands" || segment == "command" || segment == "cmd")
}

fn is_test_path(path: &str) -> bool {
    super::walker::is_test_path(path)
        || path.ends_with("/tests.rs")
        || path.ends_with("_test.rs")
        || path.contains("/tests/")
}

fn dedupe_sites(mut sites: Vec<SettingSite>) -> Vec<SettingSite> {
    sites.sort();
    sites.dedup();
    sites
}

#[derive(Debug, Clone)]
struct FunctionBlock {
    name: String,
    body: String,
}

fn extract_functions(content: &str) -> Vec<FunctionBlock> {
    let patterns = [
        r"(?m)\b(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*[^;{]*\{",
        r"(?m)\bfunction\s+([A-Za-z_][A-Za-z0-9_]*)\s*\([^)]*\)\s*\{",
        r"(?m)\b(?:const|let|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(?:async\s*)?\([^)]*\)\s*=>\s*\{",
    ];

    let mut functions = Vec::new();
    for pattern in patterns {
        let Ok(regex) = Regex::new(pattern) else {
            continue;
        };
        for captures in regex.captures_iter(content) {
            let Some(full_match) = captures.get(0) else {
                continue;
            };
            let Some(name) = captures.get(1) else {
                continue;
            };
            let open = full_match.end().saturating_sub(1);
            if let Some(close) = matching_brace(content.as_bytes(), open) {
                functions.push(FunctionBlock {
                    name: name.as_str().to_string(),
                    body: content[open + 1..close].to_string(),
                });
            }
        }
    }
    functions.sort_by(|a, b| a.name.cmp(&b.name).then(a.body.len().cmp(&b.body.len())));
    functions.dedup_by(|a, b| a.name == b.name && a.body == b.body);
    functions
}

fn matching_brace(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        if let Some(next) = skip_string_or_comment(bytes, i) {
            i = next;
            continue;
        }

        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn setting_operations(body: &str) -> BTreeMap<String, BTreeSet<&'static str>> {
    let mut settings = BTreeMap::new();
    for literal in string_literals(body) {
        if !looks_like_setting_key(&literal.value) {
            continue;
        }
        let context = lower_context(body, literal.start, literal.end);
        if !context.contains("extension") {
            continue;
        }
        if !contains_any(
            &context,
            &[
                "setting",
                "settings",
                "config",
                "schema",
                "default",
                "fallback",
                "extension",
            ],
        ) {
            continue;
        }

        let operations = operation_kinds(&context);
        if operations.is_empty() {
            continue;
        }

        settings
            .entry(literal.value)
            .or_insert_with(BTreeSet::new)
            .extend(operations);
    }
    settings
}

#[derive(Debug, Clone)]
struct StringLiteral {
    value: String,
    start: usize,
    end: usize,
}

fn string_literals(content: &str) -> Vec<StringLiteral> {
    let bytes = content.as_bytes();
    let mut literals = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\'' && bytes[i] != b'\"' {
            i += 1;
            continue;
        }
        let quote = bytes[i];
        let start = i;
        i += 1;
        let value_start = i;
        let mut escaped = false;
        while i < bytes.len() {
            if escaped {
                escaped = false;
            } else if bytes[i] == b'\\' {
                escaped = true;
            } else if bytes[i] == quote {
                if let Ok(value) = std::str::from_utf8(&bytes[value_start..i]) {
                    literals.push(StringLiteral {
                        value: value.to_string(),
                        start,
                        end: i + 1,
                    });
                }
                i += 1;
                break;
            }
            i += 1;
        }
    }
    literals
}

fn looks_like_setting_key(value: &str) -> bool {
    if value.len() < 3
        || value.len() > 96
        || value.starts_with("--")
        || value.contains(char::is_whitespace)
        || !value.chars().any(|ch| ch.is_ascii_alphabetic())
    {
        return false;
    }
    let has_key_separator = value.contains('.') || value.contains('_') || value.contains('-');
    let has_identifier_chars = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':' | '/'));
    has_key_separator && has_identifier_chars
}

fn lower_context(content: &str, start: usize, end: usize) -> String {
    let from = start.saturating_sub(CONTEXT_RADIUS);
    let to = content.len().min(end + CONTEXT_RADIUS);
    content[from..to].to_ascii_lowercase()
}

fn operation_kinds(context: &str) -> BTreeSet<&'static str> {
    let mut operations = BTreeSet::new();
    if contains_any(
        context,
        &[
            "parse",
            "from_str",
            "from_value",
            "as_str",
            "as_bool",
            "as_i64",
            "as_u64",
            "as_f64",
            "as_array",
            "as_object",
            "json_decode",
        ],
    ) {
        operations.insert("parse");
    }
    if contains_any(
        context,
        &[
            "to_string",
            "stringify",
            "to_value",
            "json_encode",
            "value::string",
            "insert(",
        ],
    ) {
        operations.insert("serialize");
    }
    if contains_any(
        context,
        &[
            "default",
            "fallback",
            "unwrap_or",
            "unwrap_or_else",
            "schema",
            "normalize",
            "normalise",
        ],
    ) {
        operations.insert("normalize");
    }
    operations
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn skip_string_or_comment(bytes: &[u8], i: usize) -> Option<usize> {
    match bytes.get(i).copied()? {
        b'\'' | b'\"' => skip_quoted(bytes, i),
        b'/' if bytes.get(i + 1) == Some(&b'/') => bytes[i + 2..]
            .iter()
            .position(|ch| *ch == b'\n')
            .map(|pos| i + 2 + pos + 1)
            .or(Some(bytes.len())),
        b'/' if bytes.get(i + 1) == Some(&b'*') => bytes[i + 2..]
            .windows(2)
            .position(|pair| pair == b"*/")
            .map(|pos| i + 2 + pos + 2)
            .or(Some(bytes.len())),
        _ => None,
    }
}

fn skip_quoted(bytes: &[u8], start: usize) -> Option<usize> {
    let quote = *bytes.get(start)?;
    let mut escaped = false;
    let mut i = start + 1;
    while i < bytes.len() {
        if escaped {
            escaped = false;
        } else if bytes[i] == b'\\' {
            escaped = true;
        } else if bytes[i] == quote {
            return Some(i + 1);
        }
        i += 1;
    }
    Some(bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_audit::conventions::Language;

    fn fp(path: &str, content: &str) -> FileFingerprint {
        FileFingerprint {
            relative_path: path.to_string(),
            language: Language::Rust,
            content: content.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn detects_repeated_typed_string_setting_plumbing_across_commands() {
        let first = fp(
            "src/commands/bench.rs",
            r#"
            pub fn run_bench_command(extension_settings: &serde_json::Value) {
                let raw = extension_settings.get("runner.timeout-ms").and_then(|value| value.as_str()).unwrap_or("30");
                let timeout: u64 = raw.parse().unwrap_or(30);
                let mut schema = serde_json::Map::new();
                schema.insert("runner.timeout-ms".to_string(), serde_json::Value::String(timeout.to_string()));
            }
            "#,
        );
        let second = fp(
            "src/commands/trace.rs",
            r#"
            pub fn run_trace_command(extension_settings: &serde_json::Value) {
                let fallback = "30";
                let timeout = extension_settings.get("runner.timeout-ms").and_then(|value| value.as_str()).unwrap_or(fallback);
                let parsed: u64 = timeout.parse().unwrap_or(30);
                let encoded = serde_json::Value::String(parsed.to_string());
                let _ = (encoded, "runner.timeout-ms");
            }
            "#,
        );

        let findings = run(&[&first, &second]);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, AuditFinding::ExtensionSettingPlumbing);
        assert!(findings[0].description.contains("runner.timeout-ms"));
        assert!(findings[0].description.contains("run_bench_command"));
        assert!(findings[0].description.contains("run_trace_command"));
        assert!(findings[0]
            .suggestion
            .contains("shared extension settings contract"));
    }

    #[test]
    fn ignores_one_off_local_setting_conversion() {
        let only = fp(
            "src/commands/bench.rs",
            r#"
            pub fn run_bench_command(settings: &serde_json::Value) {
                let raw = settings.get("runner.timeout-ms").and_then(|value| value.as_str()).unwrap_or("30");
                let timeout: u64 = raw.parse().unwrap_or(30);
                let _ = timeout.to_string();
            }
            "#,
        );

        assert!(run(&[&only]).is_empty());
    }

    #[test]
    fn ignores_non_command_paths() {
        let first = fp(
            "src/core/settings.rs",
            r#"
            pub fn parse_shared(settings: &serde_json::Value) {
                let raw = settings.get("runner.timeout-ms").and_then(|value| value.as_str()).unwrap_or("30");
                let _timeout: u64 = raw.parse().unwrap_or(30);
            }
            "#,
        );
        let second = fp(
            "src/core/runner.rs",
            r#"
            pub fn parse_runner(settings: &serde_json::Value) {
                let raw = settings.get("runner.timeout-ms").and_then(|value| value.as_str()).unwrap_or("30");
                let _timeout: u64 = raw.parse().unwrap_or(30);
            }
            "#,
        );

        assert!(run(&[&first, &second]).is_empty());
    }
}
