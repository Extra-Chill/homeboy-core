use std::collections::HashSet;
use std::path::Path;

use regex::Regex;

use super::conventions::{AuditFinding, Language};
use super::findings::{Finding, Severity};
use super::fingerprint::FileFingerprint;

pub(crate) fn rust_crate_name(root: &Path) -> Option<String> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let package_start = manifest.find("[package]")?;
    let package = &manifest[package_start..];
    for line in package.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("name") {
            let value = value.trim_start();
            if let Some(value) = value.strip_prefix('=') {
                return Some(value.trim().trim_matches('"').replace('-', "_"));
            }
        }
    }
    None
}

pub(crate) fn find_vacuous_test_methods(
    findings: &mut Vec<Finding>,
    fp: &FileFingerprint,
    test_methods: &[String],
    source_methods: &HashSet<&str>,
    crate_name: Option<&str>,
) {
    if fp.language != Language::Rust || fp.content.trim().is_empty() {
        return;
    }

    let product_symbols = collect_rust_product_imports(&fp.content, crate_name);
    for method in test_methods {
        let Some(body) = extract_rust_function_body(&fp.content, method) else {
            continue;
        };
        let Some(reason) =
            classify_vacuous_rust_test(&body, &product_symbols, source_methods, crate_name)
        else {
            continue;
        };

        findings.push(Finding {
            convention: "test_coverage".to_string(),
            severity: Severity::Info,
            file: fp.relative_path.clone(),
            description: format!("Test method '{}' is vacuous: {}", method, reason),
            suggestion: format!(
                "Remove '{}' or replace it with a behavior test that exercises product code",
                method
            ),
            kind: AuditFinding::VacuousTest,
        });
    }
}

fn classify_vacuous_rust_test(
    body: &str,
    product_symbols: &HashSet<String>,
    source_methods: &HashSet<&str>,
    crate_name: Option<&str>,
) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    if lower.contains("compile contract") || lower.contains("compile-only") {
        return None;
    }
    if body.contains("assert_snapshot") || body.contains("assert_debug_snapshot") {
        return None;
    }

    let uncommented = strip_rust_comments(body);
    let compact: String = uncommented.chars().filter(|c| !c.is_whitespace()).collect();
    let has_assertion = uncommented.contains("assert") || uncommented.contains("panic!");
    let has_product_ref =
        has_rust_product_reference(&uncommented, product_symbols, source_methods, crate_name);

    if has_product_ref {
        return None;
    }

    if compact == "assert!(true);" || compact == "assert!(true)" {
        return Some("it only asserts true".to_string());
    }
    if compact.contains("assert!(true)") {
        return Some("it contains only placeholder assertion logic".to_string());
    }
    if lower.contains("audit") && (lower.contains("mapping") || lower.contains("coverage")) {
        return Some(
            "its comments describe audit coverage mapping instead of behavior".to_string(),
        );
    }
    if !has_assertion && compact.is_empty() {
        return Some("it has an empty body".to_string());
    }
    None
}

fn has_rust_product_reference(
    body: &str,
    product_symbols: &HashSet<String>,
    source_methods: &HashSet<&str>,
    crate_name: Option<&str>,
) -> bool {
    if body.contains("crate::") || body.contains("super::") || body.contains("self::") {
        return true;
    }
    if let Some(name) = crate_name {
        if body.contains(&format!("{}::", name)) {
            return true;
        }
    }
    product_symbols
        .iter()
        .any(|symbol| contains_word_call(body, symbol) || body.contains(&format!("{}::", symbol)))
        || source_methods
            .iter()
            .any(|method| contains_word_call(body, method))
}

fn contains_word_call(haystack: &str, needle: &str) -> bool {
    let pattern = format!(r"\b{}\s*\(", regex::escape(needle));
    Regex::new(&pattern)
        .ok()
        .is_some_and(|re| re.is_match(haystack))
}

fn collect_rust_product_imports(content: &str, crate_name: Option<&str>) -> HashSet<String> {
    let mut symbols = HashSet::new();
    let Some(crate_name) = crate_name else {
        return symbols;
    };
    let simple = Regex::new(&format!(
        r"(?m)^\s*use\s+{}::[^;:]+::([A-Za-z_][A-Za-z0-9_]*)\s*;",
        regex::escape(crate_name)
    ))
    .unwrap();
    for cap in simple.captures_iter(content) {
        symbols.insert(cap[1].to_string());
    }

    let grouped = Regex::new(&format!(
        r"(?m)^\s*use\s+{}::[^;]*\{{([^}}]+)\}}\s*;",
        regex::escape(crate_name)
    ))
    .unwrap();
    for cap in grouped.captures_iter(content) {
        for raw in cap[1].split(',') {
            let symbol = raw.trim().trim_start_matches("self::");
            let symbol = symbol.split_whitespace().next().unwrap_or("");
            if !symbol.is_empty()
                && symbol
                    .chars()
                    .all(|c| c == '_' || c.is_ascii_alphanumeric())
            {
                symbols.insert(symbol.to_string());
            }
        }
    }

    symbols
}

fn extract_rust_function_body(content: &str, fn_name: &str) -> Option<String> {
    let pattern = Regex::new(&format!(
        r"(?m)\bfn\s+{}\s*\([^)]*\)\s*(?:->[^{{]+)?\{{",
        regex::escape(fn_name)
    ))
    .ok()?;
    let mat = pattern.find(content)?;
    let open = content[mat.end() - 1..].chars().next()?;
    if open != '{' {
        return None;
    }
    let mut depth = 0_i32;
    let mut end = None;
    for (idx, ch) in content[mat.end() - 1..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(mat.end() - 1 + idx);
                    break;
                }
            }
            _ => {}
        }
    }
    end.map(|end| content[mat.end()..end].to_string())
}

fn strip_rust_comments(content: &str) -> String {
    let without_blocks = Regex::new(r"(?s)/\*.*?\*/")
        .unwrap()
        .replace_all(content, "");
    without_blocks
        .lines()
        .map(|line| line.split_once("//").map(|(code, _)| code).unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}
