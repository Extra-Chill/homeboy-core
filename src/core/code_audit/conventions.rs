//! Convention discovery — detect structural patterns across similar files.
//!
//! Scans files matched by glob patterns, extracts structural fingerprints
//! (method names, registration calls, naming patterns), then groups them
//! to discover conventions and outliers.

use std::collections::HashMap;
use std::path::Path;

use regex::Regex;

/// A structural fingerprint extracted from a single source file.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FileFingerprint {
    /// Path relative to component root.
    pub relative_path: String,
    /// Language detected from extension.
    pub language: Language,
    /// Method/function names found in the file.
    pub methods: Vec<String>,
    /// Registration calls found (e.g., add_action, register_rest_route).
    pub registrations: Vec<String>,
    /// Class or struct name if found.
    pub type_name: Option<String>,
    /// Interfaces or traits implemented.
    pub implements: Vec<String>,
    /// Namespace declaration (PHP namespace, Rust mod path).
    pub namespace: Option<String>,
    /// Import/use statements.
    pub imports: Vec<String>,
    /// Raw file content (for import usage analysis).
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Php,
    Rust,
    JavaScript,
    TypeScript,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "php" => Language::Php,
            "rs" => Language::Rust,
            "js" | "jsx" | "mjs" => Language::JavaScript,
            "ts" | "tsx" => Language::TypeScript,
            _ => Language::Unknown,
        }
    }
}

/// A discovered convention: a pattern that most files in a group follow.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Convention {
    /// Human-readable name (auto-generated or from config).
    pub name: String,
    /// The glob pattern that groups these files.
    pub glob: String,
    /// The expected methods/functions that define the convention.
    pub expected_methods: Vec<String>,
    /// The expected registration calls.
    pub expected_registrations: Vec<String>,
    /// The expected interfaces/traits that files should implement.
    pub expected_interfaces: Vec<String>,
    /// The expected namespace pattern (if consistent across files).
    pub expected_namespace: Option<String>,
    /// The expected import/use statements.
    pub expected_imports: Vec<String>,
    /// Files that follow the convention.
    pub conforming: Vec<String>,
    /// Files that deviate from the convention.
    pub outliers: Vec<Outlier>,
    /// How many files were analyzed.
    pub total_files: usize,
    /// Confidence score (0.0 - 1.0).
    pub confidence: f32,
}

/// A file that deviates from a convention.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Outlier {
    /// Relative file path.
    pub file: String,
    /// What's missing or different.
    pub deviations: Vec<Deviation>,
}

/// A specific deviation from the convention.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Deviation {
    /// What kind of deviation.
    pub kind: DeviationKind,
    /// Human-readable description.
    pub description: String,
    /// Suggested fix.
    pub suggestion: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviationKind {
    MissingMethod,
    ExtraMethod,
    MissingRegistration,
    DifferentRegistration,
    MissingInterface,
    NamingMismatch,
    SignatureMismatch,
    NamespaceMismatch,
    MissingImport,
}

// ============================================================================
// Import Matching
// ============================================================================

/// Check whether an expected import is satisfied by a file's actual imports,
/// accounting for grouped imports, path equivalence, and actual usage.
///
/// Returns `true` (import present or unnecessary) when:
/// 1. Exact match exists in imports
/// 2. A grouped import covers it (e.g., `super::{CmdResult, X}` satisfies `super::CmdResult`)
/// 3. An equivalent path provides the same terminal name
///    (e.g., `crate::commands::CmdResult` satisfies `super::CmdResult`)
/// 4. The file doesn't reference the terminal name outside import lines
///    (the import would be unused — not a real convention violation)
fn has_import(expected: &str, actual_imports: &[String], file_content: &str) -> bool {
    // 1. Exact match
    if actual_imports.iter().any(|imp| imp == expected) {
        return true;
    }

    // Extract terminal name (last segment after :: or \)
    let terminal = expected
        .rsplit("::")
        .next()
        .unwrap_or(expected)
        .rsplit('\\')
        .next()
        .unwrap_or(expected);
    // Extract prefix (everything before the terminal name)
    let prefix_len = expected.len() - terminal.len();
    let prefix = if prefix_len > 2 {
        // Strip trailing :: or \
        let p = &expected[..prefix_len];
        let p = p.strip_suffix("::").or_else(|| p.strip_suffix('\\')).unwrap_or(p);
        Some(p)
    } else if prefix_len > 0 {
        Some(&expected[..prefix_len - 1])  // strip single separator char
    } else {
        None
    };

    // 2 & 3. Check all actual imports for grouped coverage or path equivalence
    for imp in actual_imports {
        // Grouped import with matching prefix: super::{CmdResult, X}
        if let Some(pfx) = prefix {
            for sep in &["::", "\\"] {
                let group_prefix = format!("{}{}{}", pfx, sep, "{");
                if imp.starts_with(&group_prefix) && grouped_import_contains(imp, terminal) {
                    return true;
                }
            }
        }

        // Grouped import from any path containing the terminal name
        if (imp.contains("::{") || imp.contains("\\{"))
            && grouped_import_contains(imp, terminal)
        {
            return true;
        }

        // Path equivalence: different path, same terminal name
        let imp_terminal = imp
            .rsplit("::")
            .next()
            .unwrap_or(imp)
            .rsplit('\\')
            .next()
            .unwrap_or(imp);
        if imp_terminal == terminal && !imp.contains("::{") && !imp.contains("\\{") {
            return true;
        }
    }

    // 4. Usage check: if the terminal name isn't referenced outside imports,
    //    the import would be unused — not a real convention violation
    if !terminal.is_empty() && !content_references_name(file_content, terminal) {
        return true;
    }

    false
}

/// Check if a grouped import (e.g., `serde::{Deserialize, Serialize}`) contains a name.
fn grouped_import_contains(import: &str, name: &str) -> bool {
    if let Some(brace_start) = import.find('{') {
        let brace_end = import.rfind('}').unwrap_or(import.len());
        let inner = &import[brace_start + 1..brace_end];
        inner.split(',').map(|s| s.trim()).any(|n| n == name)
    } else {
        false
    }
}

/// Check if file content references a name outside of import/use statements.
fn content_references_name(content: &str, name: &str) -> bool {
    for line in content.lines() {
        let trimmed = line.trim();
        // Skip import/use lines — we're looking for usage, not declarations
        if trimmed.starts_with("use ") || trimmed.starts_with("import ") {
            continue;
        }
        if contains_word(trimmed, name) {
            return true;
        }
    }
    false
}

/// Check if `text` contains `word` as a standalone word (not a substring).
fn contains_word(text: &str, word: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = text[start..].find(word) {
        let abs = start + pos;
        let before_ok = abs == 0
            || !text.as_bytes()[abs - 1].is_ascii_alphanumeric()
                && text.as_bytes()[abs - 1] != b'_';
        let after = abs + word.len();
        let after_ok = after >= text.len()
            || !text.as_bytes()[after].is_ascii_alphanumeric()
                && text.as_bytes()[after] != b'_';
        if before_ok && after_ok {
            return true;
        }
        start = abs + 1;
    }
    false
}

// ============================================================================
// Fingerprinting
// ============================================================================

/// Extract a structural fingerprint from a source file.
pub fn fingerprint_file(path: &Path, root: &Path) -> Option<FileFingerprint> {
    let ext = path.extension()?.to_str()?;
    let language = Language::from_extension(ext);
    if language == Language::Unknown {
        return None;
    }

    let content = std::fs::read_to_string(path).ok()?;
    let relative_path = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    let (methods, type_name, implements) = match language {
        Language::Php => extract_php(&content),
        Language::Rust => extract_rust(&content),
        Language::JavaScript | Language::TypeScript => extract_js(&content),
        Language::Unknown => return None,
    };

    let registrations = extract_registrations(&content, &language);
    let (namespace, imports) = extract_namespace_imports(&content, &language);

    Some(FileFingerprint {
        relative_path,
        language,
        methods,
        registrations,
        type_name,
        implements,
        namespace,
        imports,
        content,
    })
}

/// Extract methods, class name, and implements from PHP.
fn extract_php(content: &str) -> (Vec<String>, Option<String>, Vec<String>) {
    let method_re = Regex::new(r"(?m)^\s*(?:public|protected|private|static)\s+function\s+(\w+)")
        .unwrap();
    let class_re =
        Regex::new(r"(?m)^\s*(?:abstract\s+)?class\s+(\w+)").unwrap();
    let implements_re =
        Regex::new(r"(?m)implements\s+([\w\\,\s]+)").unwrap();

    let methods: Vec<String> = method_re
        .captures_iter(content)
        .map(|c| c[1].to_string())
        .collect();

    let type_name = class_re
        .captures(content)
        .map(|c| c[1].to_string());

    let implements: Vec<String> = implements_re
        .captures(content)
        .map(|c| {
            c[1].split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    (methods, type_name, implements)
}

/// Extract functions, struct name, and trait impls from Rust.
fn extract_rust(content: &str) -> (Vec<String>, Option<String>, Vec<String>) {
    let fn_re = Regex::new(r"(?m)^\s*pub(?:\(crate\))?\s+fn\s+(\w+)").unwrap();
    let struct_re = Regex::new(r"(?m)^\s*pub\s+struct\s+(\w+)").unwrap();
    let impl_re = Regex::new(r"(?m)^\s*impl\s+(\w+)\s+for\s+").unwrap();

    let methods: Vec<String> = fn_re
        .captures_iter(content)
        .map(|c| c[1].to_string())
        .collect();

    let type_name = struct_re.captures(content).map(|c| c[1].to_string());

    let implements: Vec<String> = impl_re
        .captures_iter(content)
        .map(|c| c[1].to_string())
        .collect();

    (methods, type_name, implements)
}

/// Extract functions and class/export name from JS/TS.
fn extract_js(content: &str) -> (Vec<String>, Option<String>, Vec<String>) {
    let fn_re =
        Regex::new(r"(?m)(?:export\s+)?(?:async\s+)?function\s+(\w+)").unwrap();
    let method_re = Regex::new(r"(?m)^\s+(?:async\s+)?(\w+)\s*\(").unwrap();
    let class_re =
        Regex::new(r"(?m)(?:export\s+)?class\s+(\w+)").unwrap();
    let extends_re = Regex::new(r"extends\s+(\w+)").unwrap();

    let mut methods: Vec<String> = fn_re
        .captures_iter(content)
        .map(|c| c[1].to_string())
        .collect();

    // Also grab class methods
    for cap in method_re.captures_iter(content) {
        let name = cap[1].to_string();
        if !methods.contains(&name)
            && name != "if"
            && name != "for"
            && name != "while"
            && name != "switch"
            && name != "catch"
            && name != "return"
        {
            methods.push(name);
        }
    }

    let type_name = class_re.captures(content).map(|c| c[1].to_string());

    let implements: Vec<String> = extends_re
        .captures_iter(content)
        .map(|c| c[1].to_string())
        .collect();

    (methods, type_name, implements)
}

/// Extract registration/hook calls that indicate architectural patterns.
fn extract_registrations(content: &str, language: &Language) -> Vec<String> {
    let patterns: Vec<&str> = match language {
        Language::Php => vec![
            r#"add_action\s*\(\s*['"](\w+)['"]"#,
            r#"add_filter\s*\(\s*['"](\w+)['"]"#,
            r"register_rest_route\s*\(",
            r"register_post_type\s*\(",
            r"register_taxonomy\s*\(",
            r"register_block_type\s*\(",
            r"wp_enqueue_script\s*\(",
            r"wp_enqueue_style\s*\(",
        ],
        Language::Rust => vec![
            r"\.subcommand\s*\(",
            r"\.arg\s*\(",
            r"Command::new\s*\(",
        ],
        Language::JavaScript | Language::TypeScript => vec![
            r"module\.exports",
            r"export\s+default",
            r"registerBlockType\s*\(",
            r"addEventListener\s*\(",
        ],
        Language::Unknown => vec![],
    };

    let mut registrations = Vec::new();
    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            for cap in re.captures_iter(content) {
                let matched = if cap.len() > 1 {
                    cap.get(1).map(|m| m.as_str()).unwrap_or(pattern)
                } else {
                    pattern
                };
                let registration = matched.to_string();
                if !registrations.contains(&registration) {
                    registrations.push(registration);
                }
            }
        }
    }
    registrations
}

// ============================================================================
// Namespace and Import Extraction
// ============================================================================

/// Extract namespace declaration and import/use statements from source content.
fn extract_namespace_imports(content: &str, language: &Language) -> (Option<String>, Vec<String>) {
    match language {
        Language::Php => extract_php_namespace_imports(content),
        Language::Rust => extract_rust_namespace_imports(content),
        Language::JavaScript | Language::TypeScript => extract_js_namespace_imports(content),
        Language::Unknown => (None, vec![]),
    }
}

fn extract_php_namespace_imports(content: &str) -> (Option<String>, Vec<String>) {
    let ns_re = Regex::new(r"(?m)^\s*namespace\s+([\w\\]+)\s*;").unwrap();
    let use_re = Regex::new(r"(?m)^\s*use\s+([\w\\]+)(?:\s+as\s+\w+)?\s*;").unwrap();

    let namespace = ns_re.captures(content).map(|c| c[1].to_string());

    let imports: Vec<String> = use_re
        .captures_iter(content)
        .map(|c| c[1].to_string())
        .collect();

    (namespace, imports)
}

fn extract_rust_namespace_imports(content: &str) -> (Option<String>, Vec<String>) {
    // Rust doesn't have namespace declarations per-file, but we can track the module path
    // from `mod` declarations in the same directory
    let use_re = Regex::new(r"(?m)^\s*use\s+((?:\w+::)*(?:\w+|\{[^}]+\}))").unwrap();

    let imports: Vec<String> = use_re
        .captures_iter(content)
        .map(|c| c[1].to_string())
        .collect();

    (None, imports)
}

fn extract_js_namespace_imports(content: &str) -> (Option<String>, Vec<String>) {
    // JS/TS import statements
    let import_re =
        Regex::new(r#"(?m)^\s*import\s+.*?\s+from\s+['"]([@\w/.!-]+)['"]"#).unwrap();
    let require_re =
        Regex::new(r#"(?m)(?:const|let|var)\s+\w+\s*=\s*require\s*\(\s*['"]([@\w/.!-]+)['"]"#).unwrap();

    let mut imports: Vec<String> = import_re
        .captures_iter(content)
        .map(|c| c[1].to_string())
        .collect();

    for cap in require_re.captures_iter(content) {
        let imp = cap[1].to_string();
        if !imports.contains(&imp) {
            imports.push(imp);
        }
    }

    (None, imports)
}

// ============================================================================
// Convention Discovery
// ============================================================================

/// Discover conventions from a set of fingerprints that share a common grouping.
///
/// The algorithm:
/// 1. Find methods that appear in ≥ 60% of files (the "convention")
/// 2. Find files that are missing any of those methods (the "outliers")
pub fn discover_conventions(
    group_name: &str,
    glob_pattern: &str,
    fingerprints: &[FileFingerprint],
) -> Option<Convention> {
    if fingerprints.len() < 2 {
        return None; // Need at least 2 files to detect a pattern
    }

    let total = fingerprints.len();
    let threshold = (total as f32 * 0.6).ceil() as usize;

    // Count method frequency
    let mut method_counts: HashMap<String, usize> = HashMap::new();
    for fp in fingerprints {
        for method in &fp.methods {
            *method_counts.entry(method.clone()).or_insert(0) += 1;
        }
    }

    // Methods appearing in ≥ threshold files are "expected"
    let expected_methods: Vec<String> = method_counts
        .iter()
        .filter(|(_, count)| **count >= threshold)
        .map(|(name, _)| name.clone())
        .collect();

    if expected_methods.is_empty() {
        return None; // No convention found
    }

    // Count registration frequency
    let mut reg_counts: HashMap<String, usize> = HashMap::new();
    for fp in fingerprints {
        for reg in &fp.registrations {
            *reg_counts.entry(reg.clone()).or_insert(0) += 1;
        }
    }

    let expected_registrations: Vec<String> = reg_counts
        .iter()
        .filter(|(_, count)| **count >= threshold)
        .map(|(name, _)| name.clone())
        .collect();

    // Count interface/trait frequency
    let mut interface_counts: HashMap<String, usize> = HashMap::new();
    for fp in fingerprints {
        for iface in &fp.implements {
            *interface_counts.entry(iface.clone()).or_insert(0) += 1;
        }
    }

    let expected_interfaces: Vec<String> = interface_counts
        .iter()
        .filter(|(_, count)| **count >= threshold)
        .map(|(name, _)| name.clone())
        .collect();

    // Discover namespace convention (most common namespace)
    let mut ns_counts: HashMap<String, usize> = HashMap::new();
    for fp in fingerprints {
        if let Some(ns) = &fp.namespace {
            *ns_counts.entry(ns.clone()).or_insert(0) += 1;
        }
    }
    let expected_namespace = ns_counts
        .iter()
        .filter(|(_, count)| **count >= threshold)
        .max_by_key(|(_, count)| *count)
        .map(|(ns, _)| ns.clone());

    // Discover import conventions (imports appearing in ≥ threshold files)
    let mut import_counts: HashMap<String, usize> = HashMap::new();
    for fp in fingerprints {
        for imp in &fp.imports {
            *import_counts.entry(imp.clone()).or_insert(0) += 1;
        }
    }
    let expected_imports: Vec<String> = import_counts
        .iter()
        .filter(|(_, count)| **count >= threshold)
        .map(|(name, _)| name.clone())
        .collect();

    // Classify files
    let mut conforming = Vec::new();
    let mut outliers = Vec::new();

    for fp in fingerprints {
        let mut deviations = Vec::new();

        // Check missing methods
        for expected in &expected_methods {
            if !fp.methods.contains(expected) {
                deviations.push(Deviation {
                    kind: DeviationKind::MissingMethod,
                    description: format!("Missing method: {}", expected),
                    suggestion: format!(
                        "Add {}() to match the convention in {}",
                        expected, group_name
                    ),
                });
            }
        }

        // Check missing registrations
        for expected in &expected_registrations {
            if !fp.registrations.contains(expected) {
                deviations.push(Deviation {
                    kind: DeviationKind::MissingRegistration,
                    description: format!("Missing registration: {}", expected),
                    suggestion: format!(
                        "Add {} call to match the convention in {}",
                        expected, group_name
                    ),
                });
            }
        }

        // Check missing interfaces/traits
        for expected in &expected_interfaces {
            if !fp.implements.contains(expected) {
                deviations.push(Deviation {
                    kind: DeviationKind::MissingInterface,
                    description: format!("Missing interface: {}", expected),
                    suggestion: format!(
                        "Implement {} to match the convention in {}",
                        expected, group_name
                    ),
                });
            }
        }

        // Check namespace mismatch
        if let Some(expected_ns) = &expected_namespace {
            if let Some(actual_ns) = &fp.namespace {
                if actual_ns != expected_ns {
                    deviations.push(Deviation {
                        kind: DeviationKind::NamespaceMismatch,
                        description: format!(
                            "Namespace mismatch: expected `{}`, found `{}`",
                            expected_ns, actual_ns
                        ),
                        suggestion: format!(
                            "Change namespace to `{}`",
                            expected_ns
                        ),
                    });
                }
            }
            // Missing namespace when others have one is also a deviation
            if fp.namespace.is_none() {
                deviations.push(Deviation {
                    kind: DeviationKind::NamespaceMismatch,
                    description: format!(
                        "Missing namespace declaration (expected `{}`)",
                        expected_ns
                    ),
                    suggestion: format!(
                        "Add `namespace {};`",
                        expected_ns
                    ),
                });
            }
        }

        // Check missing imports (aware of grouped imports, path equivalence, and usage)
        for expected_imp in &expected_imports {
            if !has_import(expected_imp, &fp.imports, &fp.content) {
                deviations.push(Deviation {
                    kind: DeviationKind::MissingImport,
                    description: format!("Missing import: {}", expected_imp),
                    suggestion: format!(
                        "Add `use {};` to match the convention in {}",
                        expected_imp, group_name
                    ),
                });
            }
        }

        if deviations.is_empty() {
            conforming.push(fp.relative_path.clone());
        } else {
            outliers.push(Outlier {
                file: fp.relative_path.clone(),
                deviations,
            });
        }
    }

    let conforming_count = conforming.len();
    let confidence = conforming_count as f32 / total as f32;

    log_status!(
        "audit",
        "Convention '{}': {}/{} files conform (confidence: {:.0}%)",
        group_name,
        conforming_count,
        total,
        confidence * 100.0
    );

    Some(Convention {
        name: group_name.to_string(),
        glob: glob_pattern.to_string(),
        expected_methods,
        expected_registrations,
        expected_interfaces,
        expected_namespace,
        expected_imports,
        conforming,
        outliers,
        total_files: total,
        confidence,
    })
}

// ============================================================================
// Signature Consistency
// ============================================================================

/// Normalize a signature string before tokenization.
///
/// Collapses whitespace/newlines, removes trailing commas before closing
/// parens, and normalizes module path references to just the final segment.
/// This is language-agnostic — works on any signature string.
fn normalize_signature(sig: &str) -> String {
    // Collapse all whitespace (including newlines) into single spaces
    let normalized: String = sig.split_whitespace().collect::<Vec<_>>().join(" ");

    // Remove trailing comma before closing paren: ", )" → ")"
    let normalized = Regex::new(r",\s*\)")
        .unwrap()
        .replace_all(&normalized, ")")
        .to_string();

    // Normalize module paths to final segment: crate::commands::GlobalArgs → GlobalArgs
    // Also handles super::GlobalArgs → GlobalArgs
    // This is generic: any sequence of word::word::...::Word keeps only the last part
    let normalized = Regex::new(r"\b(?:\w+::)+(\w+)")
        .unwrap()
        .replace_all(&normalized, "$1")
        .to_string();

    // Strip parameter modifiers that don't affect the structural contract.
    // "mut" before a parameter name is a local annotation, not part of the
    // function's external signature. E.g., "fn run(mut args: T)" → "fn run(args: T)"
    let normalized = Regex::new(r"\bmut\s+")
        .unwrap()
        .replace_all(&normalized, "")
        .to_string();

    normalized
}

/// Split a signature string into tokens for structural comparison.
///
/// Splits on whitespace and punctuation boundaries while preserving the
/// punctuation as separate tokens. This is language-agnostic — it works
/// on any signature string regardless of language.
///
/// Example: `pub fn run(args: FooArgs, _global: &GlobalArgs) -> CmdResult<FooOutput>`
/// becomes: `["pub", "fn", "run", "(", "args", ":", "FooArgs", ",", "_global", ":", "&", "GlobalArgs", ")", "->", "CmdResult", "<", "FooOutput", ">"]`
fn tokenize_signature(sig: &str) -> Vec<String> {
    let sig = normalize_signature(sig);
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in sig.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch);
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            // Punctuation: flush current word, then emit punctuation as token
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            // Group -> as a single token
            if ch == '-' {
                current.push(ch);
            } else if ch == '>' && current == "-" {
                current.push(ch);
                tokens.push(std::mem::take(&mut current));
            } else {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                tokens.push(ch.to_string());
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Compute the structural skeleton of a set of signatures for the same method.
///
/// Given multiple tokenized signatures, identifies which token positions are
/// constant (same across all signatures) vs. variable (differ per file).
/// Returns the skeleton as a vec of `Some(token)` for constant positions
/// and `None` for variable positions, plus the expected token count.
///
/// If signatures have different token counts (different arity/structure),
/// returns `None` — those are real structural mismatches.
fn compute_signature_skeleton(tokenized_sigs: &[Vec<String>]) -> Option<Vec<Option<String>>> {
    if tokenized_sigs.is_empty() {
        return None;
    }

    let expected_len = tokenized_sigs[0].len();

    // All signatures must have the same number of tokens
    if !tokenized_sigs.iter().all(|t| t.len() == expected_len) {
        // Different token counts = structural mismatch, can't build skeleton
        return None;
    }

    let mut skeleton = Vec::with_capacity(expected_len);
    for i in 0..expected_len {
        let first = &tokenized_sigs[0][i];
        if tokenized_sigs.iter().all(|t| &t[i] == first) {
            skeleton.push(Some(first.clone()));
        } else {
            skeleton.push(None); // This position varies — it's a "type parameter"
        }
    }

    Some(skeleton)
}

/// Check method signatures across all files in a convention for consistency.
///
/// Uses structural comparison: signatures are tokenized and compared
/// position-by-position. Positions where tokens vary across files are treated
/// as "type parameters" (expected to differ). Only structural differences
/// (different token count, different constant tokens) are flagged.
pub fn check_signature_consistency(conventions: &mut [Convention], root: &Path) {
    for conv in conventions.iter_mut() {
        if conv.expected_methods.is_empty() {
            continue;
        }

        // Detect language from the glob pattern
        let lang = if conv.glob.ends_with(".php") || conv.glob.ends_with("/*") {
            // Check first conforming file extension
            conv.conforming
                .first()
                .and_then(|f| f.rsplit('.').next())
                .map(Language::from_extension)
                .unwrap_or(Language::Unknown)
        } else {
            Language::Unknown
        };

        if lang == Language::Unknown {
            continue;
        }

        // Collect signatures for each method across ALL files (conforming + outliers)
        let all_files: Vec<String> = conv
            .conforming
            .iter()
            .chain(conv.outliers.iter().map(|o| &o.file))
            .cloned()
            .collect();

        // method_name -> [(file, raw_signature)]
        let mut method_sigs: HashMap<String, Vec<(String, String)>> = HashMap::new();

        for file in &all_files {
            let full_path = root.join(file);
            let content = match std::fs::read_to_string(&full_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let sigs = super::fixer::extract_signatures(&content, &lang);
            for sig in &sigs {
                if conv.expected_methods.contains(&sig.name) {
                    method_sigs
                        .entry(sig.name.clone())
                        .or_default()
                        .push((file.clone(), sig.signature.clone()));
                }
            }
        }

        // For each method, compute the structural skeleton and find mismatches
        let mut new_outlier_deviations: HashMap<String, Vec<Deviation>> = HashMap::new();

        for (method, file_sigs) in &method_sigs {
            if file_sigs.len() < 2 {
                continue;
            }

            let tokenized: Vec<Vec<String>> = file_sigs
                .iter()
                .map(|(_, sig)| tokenize_signature(sig))
                .collect();

            match compute_signature_skeleton(&tokenized) {
                Some(skeleton) => {
                    // Skeleton computed — all signatures have the same structure.
                    // Check each file against the skeleton's constant positions.
                    for (i, (file, sig)) in file_sigs.iter().enumerate() {
                        let tokens = &tokenized[i];
                        let mut mismatches = Vec::new();
                        for (j, expected) in skeleton.iter().enumerate() {
                            if let Some(expected_token) = expected {
                                if j < tokens.len() && &tokens[j] != expected_token {
                                    mismatches.push((expected_token.clone(), tokens[j].clone()));
                                }
                            }
                        }
                        if !mismatches.is_empty() {
                            // This file's constant tokens differ — real mismatch
                            let canonical_sig = skeleton
                                .iter()
                                .map(|s| s.as_deref().unwrap_or("<_>"))
                                .collect::<Vec<_>>()
                                .join(" ");
                            new_outlier_deviations
                                .entry(file.clone())
                                .or_default()
                                .push(Deviation {
                                    kind: DeviationKind::SignatureMismatch,
                                    description: format!(
                                        "Signature mismatch for {}: expected structure `{}`, found `{}`",
                                        method, canonical_sig, sig
                                    ),
                                    suggestion: format!(
                                        "Update {}() to match the structural pattern: `{}`",
                                        method, canonical_sig
                                    ),
                                });
                        }
                    }
                }
                None => {
                    // Different token counts — structural mismatch.
                    // Find the majority token count and flag files that differ.
                    let mut len_counts: HashMap<usize, usize> = HashMap::new();
                    for t in &tokenized {
                        *len_counts.entry(t.len()).or_insert(0) += 1;
                    }
                    let majority_len = len_counts
                        .iter()
                        .max_by_key(|(_, count)| *count)
                        .map(|(len, _)| *len)
                        .unwrap_or(0);

                    // Build canonical from majority-length sigs
                    let majority_sigs: Vec<&Vec<String>> = tokenized
                        .iter()
                        .filter(|t| t.len() == majority_len)
                        .collect();

                    let canonical_display = if let Some(first) = majority_sigs.first() {
                        first.join(" ")
                    } else {
                        continue;
                    };

                    for (i, (file, sig)) in file_sigs.iter().enumerate() {
                        if tokenized[i].len() != majority_len {
                            new_outlier_deviations
                                .entry(file.clone())
                                .or_default()
                                .push(Deviation {
                                    kind: DeviationKind::SignatureMismatch,
                                    description: format!(
                                        "Signature mismatch for {}: different structure — expected {} tokens, found {}. Example: `{}`",
                                        method, majority_len, tokenized[i].len(), sig
                                    ),
                                    suggestion: format!(
                                        "Update {}() to match the structural pattern: `{}`",
                                        method, canonical_display
                                    ),
                                });
                        }
                    }
                }
            }
        }

        if new_outlier_deviations.is_empty() {
            continue;
        }

        // Move conforming files with mismatches to outliers
        let mut moved_files = Vec::new();
        for file in &conv.conforming {
            if let Some(devs) = new_outlier_deviations.remove(file) {
                moved_files.push(file.clone());
                conv.outliers.push(Outlier {
                    file: file.clone(),
                    deviations: devs,
                });
            }
        }
        conv.conforming.retain(|f| !moved_files.contains(f));

        // Add deviations to existing outliers
        for outlier in &mut conv.outliers {
            if let Some(devs) = new_outlier_deviations.remove(&outlier.file) {
                outlier.deviations.extend(devs);
            }
        }

        // Recalculate confidence
        conv.confidence = conv.conforming.len() as f32 / conv.total_files as f32;
    }
}

// ============================================================================
// Auto-Discovery
// ============================================================================

/// Auto-discover file groups by scanning directories for clusters of similar files.
///
/// Returns (group_name, glob_pattern, files) tuples for directories that
/// contain 2+ files of the same language.
pub fn auto_discover_groups(root: &Path) -> Vec<(String, String, Vec<FileFingerprint>)> {
    let mut groups: Vec<(String, String, Vec<FileFingerprint>)> = Vec::new();

    // Walk directories, group files by parent dir + language
    let mut dir_files: HashMap<(String, Language), Vec<FileFingerprint>> = HashMap::new();

    if let Ok(walker) = walk_source_files(root) {
        for path in walker {
            if let Some(fp) = fingerprint_file(&path, root) {
                let parent = path
                    .parent()
                    .and_then(|p| p.strip_prefix(root).ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                let key = (parent, fp.language.clone());
                dir_files.entry(key).or_default().push(fp);
            }
        }
    }

    for ((dir, _lang), fingerprints) in dir_files {
        if fingerprints.len() < 2 {
            continue;
        }

        let glob_pattern = if dir.is_empty() {
            "*".to_string()
        } else {
            format!("{}/*", dir)
        };

        // Generate a name from the directory
        let name = if dir.is_empty() {
            "Root Files".to_string()
        } else {
            dir.split('/')
                .last()
                .unwrap_or(&dir)
                .replace('-', " ")
                .replace('_', " ")
                .split_whitespace()
                .map(|w| {
                    let mut chars = w.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };

        groups.push((name, glob_pattern, fingerprints));
    }

    // Sort by group name for deterministic output
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    groups
}

// ============================================================================
// Cross-Directory Discovery
// ============================================================================

/// Discover cross-directory conventions by analyzing sibling subdirectories.
///
/// Groups discovered conventions by their grandparent directory, then checks
/// if sibling subdirectories share the same expected methods/registrations.
///
/// Example: if `inc/Abilities/Flow/` and `inc/Abilities/Job/` both expect
/// `execute`, `registerAbility`, `__construct` — that's a cross-directory
/// convention for `inc/Abilities/`.
pub fn discover_cross_directory(
    conventions: &[super::ConventionReport],
) -> Vec<super::DirectoryConvention> {
    // Group conventions by their parent directory (one level up from glob)
    let mut parent_groups: HashMap<String, Vec<&super::ConventionReport>> = HashMap::new();

    for conv in conventions {
        // Extract parent from glob: "inc/Abilities/Flow/*" → "inc/Abilities"
        let parts: Vec<&str> = conv.glob.trim_end_matches("/*").rsplitn(2, '/').collect();
        if parts.len() == 2 {
            let parent = parts[1].to_string();
            parent_groups.entry(parent).or_default().push(conv);
        }
    }

    let mut results = Vec::new();

    for (parent, child_convs) in &parent_groups {
        if child_convs.len() < 2 {
            continue; // Need at least 2 sibling dirs to detect a pattern
        }

        let total = child_convs.len();
        let threshold = (total as f32 * 0.6).ceil() as usize;

        // Count method frequency across sibling conventions
        let mut method_counts: HashMap<&str, usize> = HashMap::new();
        for conv in child_convs {
            for method in &conv.expected_methods {
                *method_counts.entry(method.as_str()).or_insert(0) += 1;
            }
        }

        let expected_methods: Vec<String> = method_counts
            .iter()
            .filter(|(_, count)| **count >= threshold)
            .map(|(name, _)| name.to_string())
            .collect();

        // Count registration frequency across sibling conventions
        let mut reg_counts: HashMap<&str, usize> = HashMap::new();
        for conv in child_convs {
            for reg in &conv.expected_registrations {
                *reg_counts.entry(reg.as_str()).or_insert(0) += 1;
            }
        }

        let expected_registrations: Vec<String> = reg_counts
            .iter()
            .filter(|(_, count)| **count >= threshold)
            .map(|(name, _)| name.to_string())
            .collect();

        if expected_methods.is_empty() && expected_registrations.is_empty() {
            continue; // No shared pattern across siblings
        }

        // Classify sibling directories
        let mut conforming_dirs = Vec::new();
        let mut outlier_dirs = Vec::new();

        for conv in child_convs {
            let dir_name = conv.glob.trim_end_matches("/*").to_string();

            let missing_methods: Vec<String> = expected_methods
                .iter()
                .filter(|m| !conv.expected_methods.contains(m))
                .cloned()
                .collect();

            let missing_registrations: Vec<String> = expected_registrations
                .iter()
                .filter(|r| !conv.expected_registrations.contains(r))
                .cloned()
                .collect();

            if missing_methods.is_empty() && missing_registrations.is_empty() {
                conforming_dirs.push(dir_name);
            } else {
                outlier_dirs.push(super::DirectoryOutlier {
                    dir: dir_name,
                    missing_methods,
                    missing_registrations,
                });
            }
        }

        let confidence = conforming_dirs.len() as f32 / total as f32;

        results.push(super::DirectoryConvention {
            parent: parent.clone(),
            expected_methods,
            expected_registrations,
            conforming_dirs,
            outlier_dirs,
            total_dirs: total,
            confidence,
        });
    }

    results.sort_by(|a, b| a.parent.cmp(&b.parent));
    results
}

/// Module index/entry-point filenames that should be excluded from convention
/// sibling detection. These files organize other files rather than being
/// peers — including them produces false "missing method" findings.
const INDEX_FILES: &[&str] = &[
    "mod.rs",
    "lib.rs",
    "main.rs",
    "index.js",
    "index.jsx",
    "index.ts",
    "index.tsx",
    "index.mjs",
    "__init__.py",
];

/// Returns true if the filename is a module index/entry-point file.
fn is_index_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|name| INDEX_FILES.contains(&name))
        .unwrap_or(false)
}

/// Walk source files under a root, skipping common non-source directories
/// and module index files.
fn walk_source_files(root: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let skip_dirs = [
        "node_modules",
        "vendor",
        ".git",
        "build",
        "dist",
        "target",
        ".svn",
        ".hg",
        "cache",
        "tmp",
    ];
    let source_extensions = ["php", "rs", "js", "jsx", "ts", "tsx", "mjs"];

    let mut files = Vec::new();
    walk_recursive(root, &skip_dirs, &source_extensions, &mut files)?;

    // Exclude module index files from convention sibling detection
    files.retain(|f| !is_index_file(f));

    Ok(files)
}

fn walk_recursive(
    dir: &Path,
    skip_dirs: &[&str],
    extensions: &[&str],
    files: &mut Vec<std::path::PathBuf>,
) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if !skip_dirs.contains(&name.as_str()) {
                walk_recursive(&path, skip_dirs, extensions, files)?;
            }
        } else if let Some(ext) = path.extension() {
            if extensions.contains(&ext.to_str().unwrap_or("")) {
                files.push(path);
            }
        }
    }
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_php_methods() {
        let content = r#"
class MyStep {
    public function register() {}
    protected function validate($input) {}
    public function execute($context) {}
    private function helper() {}
}
"#;
        let (methods, type_name, _) = extract_php(content);
        assert_eq!(methods, vec!["register", "validate", "execute", "helper"]);
        assert_eq!(type_name, Some("MyStep".to_string()));
    }

    #[test]
    fn extract_php_implements() {
        let content = r#"
class MyStep extends Base implements StepInterface, Loggable {
    public function run() {}
}
"#;
        let (_, _, implements) = extract_php(content);
        assert!(implements.contains(&"StepInterface".to_string()));
        assert!(implements.contains(&"Loggable".to_string()));
    }

    #[test]
    fn extract_rust_functions() {
        let content = r#"
pub struct MyCommand;

impl MyCommand {
    pub fn run() {}
    pub(crate) fn validate() {}
    fn private_helper() {}
}

impl Display for MyCommand {}
"#;
        let (methods, type_name, implements) = extract_rust(content);
        assert!(methods.contains(&"run".to_string()));
        assert!(methods.contains(&"validate".to_string()));
        assert!(!methods.contains(&"private_helper".to_string()));
        assert_eq!(type_name, Some("MyCommand".to_string()));
        assert!(implements.contains(&"Display".to_string()));
    }

    #[test]
    fn extract_php_registrations() {
        let content = r#"
add_action('init', [$this, 'register']);
add_filter('the_content', [$this, 'filter']);
register_rest_route('api/v1', '/data', []);
"#;
        let regs = extract_registrations(content, &Language::Php);
        assert!(regs.contains(&"init".to_string()));
        assert!(regs.contains(&"the_content".to_string()));
        assert!(regs.iter().any(|r| r.contains("register_rest_route")));
    }

    #[test]
    fn discover_convention_from_fingerprints() {
        let fingerprints = vec![
            FileFingerprint {
                relative_path: "steps/ai-chat.php".to_string(),
                language: Language::Php,
                methods: vec![
                    "register".to_string(),
                    "validate".to_string(),
                    "execute".to_string(),
                ],
                registrations: vec![],
                type_name: Some("AiChat".to_string()),
                implements: vec![],
                namespace: None,
                imports: vec![],
            content: String::new(),
            },
            FileFingerprint {
                relative_path: "steps/webhook.php".to_string(),
                language: Language::Php,
                methods: vec![
                    "register".to_string(),
                    "validate".to_string(),
                    "execute".to_string(),
                ],
                registrations: vec![],
                type_name: Some("Webhook".to_string()),
                implements: vec![],
                namespace: None,
                imports: vec![],
            content: String::new(),
            },
            FileFingerprint {
                relative_path: "steps/agent-ping.php".to_string(),
                language: Language::Php,
                methods: vec!["register".to_string(), "execute".to_string()],
                registrations: vec![],
                type_name: Some("AgentPing".to_string()),
                implements: vec![],
                namespace: None,
                imports: vec![],
            content: String::new(),
            },
        ];

        let convention =
            discover_conventions("Step Types", "steps/*.php", &fingerprints).unwrap();

        assert_eq!(convention.name, "Step Types");
        assert!(convention.expected_methods.contains(&"register".to_string()));
        assert!(convention.expected_methods.contains(&"execute".to_string()));
        assert_eq!(convention.conforming.len(), 2);
        assert_eq!(convention.outliers.len(), 1);
        assert_eq!(convention.outliers[0].file, "steps/agent-ping.php");
        assert!(convention.outliers[0]
            .deviations
            .iter()
            .any(|d| d.description.contains("validate")));
    }

    #[test]
    fn convention_needs_minimum_two_files() {
        let fingerprints = vec![FileFingerprint {
            relative_path: "single.php".to_string(),
            language: Language::Php,
            methods: vec!["run".to_string()],
            registrations: vec![],
            type_name: None,
            implements: vec![],
            namespace: None,
            imports: vec![],
        content: String::new(),
        }];

        assert!(discover_conventions("Single", "*.php", &fingerprints).is_none());
    }

    #[test]
    fn language_from_extension() {
        assert_eq!(Language::from_extension("php"), Language::Php);
        assert_eq!(Language::from_extension("rs"), Language::Rust);
        assert_eq!(Language::from_extension("ts"), Language::TypeScript);
        assert_eq!(Language::from_extension("jsx"), Language::JavaScript);
        assert_eq!(Language::from_extension("txt"), Language::Unknown);
    }

    #[test]
    fn discover_interface_convention() {
        let fingerprints = vec![
            FileFingerprint {
                relative_path: "abilities/create.php".to_string(),
                language: Language::Php,
                methods: vec!["execute".to_string(), "register".to_string()],
                registrations: vec![],
                type_name: Some("CreateAbility".to_string()),
                implements: vec!["AbilityInterface".to_string()],
                namespace: None,
                imports: vec![],
            content: String::new(),
            },
            FileFingerprint {
                relative_path: "abilities/update.php".to_string(),
                language: Language::Php,
                methods: vec!["execute".to_string(), "register".to_string()],
                registrations: vec![],
                type_name: Some("UpdateAbility".to_string()),
                implements: vec!["AbilityInterface".to_string()],
                namespace: None,
                imports: vec![],
            content: String::new(),
            },
            FileFingerprint {
                relative_path: "abilities/helpers.php".to_string(),
                language: Language::Php,
                methods: vec!["execute".to_string(), "register".to_string()],
                registrations: vec![],
                type_name: Some("Helpers".to_string()),
                implements: vec![], // Missing interface
                namespace: None,
                imports: vec![],
            content: String::new(),
            },
        ];

        let convention =
            discover_conventions("Abilities", "abilities/*.php", &fingerprints).unwrap();

        // Should detect AbilityInterface as expected
        assert!(convention.expected_interfaces.contains(&"AbilityInterface".to_string()));

        // helpers.php should be an outlier due to missing interface
        assert_eq!(convention.outliers.len(), 1);
        assert_eq!(convention.outliers[0].file, "abilities/helpers.php");
        assert!(convention.outliers[0]
            .deviations
            .iter()
            .any(|d| matches!(d.kind, DeviationKind::MissingInterface)
                && d.description.contains("AbilityInterface")));
    }

    #[test]
    fn no_interface_convention_when_none_shared() {
        let fingerprints = vec![
            FileFingerprint {
                relative_path: "a.php".to_string(),
                language: Language::Php,
                methods: vec!["run".to_string()],
                registrations: vec![],
                type_name: None,
                implements: vec!["FooInterface".to_string()],
                namespace: None,
                imports: vec![],
            content: String::new(),
            },
            FileFingerprint {
                relative_path: "b.php".to_string(),
                language: Language::Php,
                methods: vec!["run".to_string()],
                registrations: vec![],
                type_name: None,
                implements: vec!["BarInterface".to_string()],
                namespace: None,
                imports: vec![],
            content: String::new(),
            },
            FileFingerprint {
                relative_path: "c.php".to_string(),
                language: Language::Php,
                methods: vec!["run".to_string()],
                registrations: vec![],
                type_name: None,
                implements: vec![],
                namespace: None,
                imports: vec![],
            content: String::new(),
            },
        ];

        let convention =
            discover_conventions("Mixed", "*.php", &fingerprints).unwrap();

        // No interface appears in ≥60% of files
        assert!(convention.expected_interfaces.is_empty());
    }

    // ========================================================================
    // Cross-directory convention tests
    // ========================================================================

    use super::super::checks::CheckStatus;
    use super::super::ConventionReport;

    fn make_convention(
        name: &str,
        glob: &str,
        methods: &[&str],
        registrations: &[&str],
    ) -> ConventionReport {
        ConventionReport {
            name: name.to_string(),
            glob: glob.to_string(),
            status: CheckStatus::Clean,
            expected_methods: methods.iter().map(|s| s.to_string()).collect(),
            expected_registrations: registrations.iter().map(|s| s.to_string()).collect(),
            expected_interfaces: vec![],
            expected_namespace: None,
            expected_imports: vec![],
            conforming: vec![],
            outliers: vec![],
            total_files: 3,
            confidence: 1.0,
        }
    }

    #[test]
    fn cross_directory_detects_shared_methods() {
        let conventions = vec![
            make_convention("Flow", "inc/Abilities/Flow/*", &["execute", "__construct", "registerAbility"], &[]),
            make_convention("Job", "inc/Abilities/Job/*", &["execute", "__construct", "registerAbility"], &[]),
            make_convention("Data", "inc/Abilities/Data/*", &["execute", "__construct", "registerAbility"], &[]),
        ];

        let results = discover_cross_directory(&conventions);

        assert_eq!(results.len(), 1);
        let result = &results[0];
        assert_eq!(result.parent, "inc/Abilities");
        assert!(result.expected_methods.contains(&"execute".to_string()));
        assert!(result.expected_methods.contains(&"__construct".to_string()));
        assert!(result.expected_methods.contains(&"registerAbility".to_string()));
        assert_eq!(result.conforming_dirs.len(), 3);
        assert!(result.outlier_dirs.is_empty());
        assert_eq!(result.total_dirs, 3);
        assert!((result.confidence - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn cross_directory_detects_outlier_missing_method() {
        let conventions = vec![
            make_convention("Flow", "inc/Abilities/Flow/*", &["execute", "__construct", "registerAbility"], &[]),
            make_convention("Job", "inc/Abilities/Job/*", &["execute", "__construct", "registerAbility"], &[]),
            make_convention("Data", "inc/Abilities/Data/*", &["execute", "__construct"], &[]), // missing registerAbility
        ];

        let results = discover_cross_directory(&conventions);

        assert_eq!(results.len(), 1);
        let result = &results[0];
        assert_eq!(result.conforming_dirs.len(), 2);
        assert_eq!(result.outlier_dirs.len(), 1);
        assert_eq!(result.outlier_dirs[0].dir, "inc/Abilities/Data");
        assert!(result.outlier_dirs[0].missing_methods.contains(&"registerAbility".to_string()));
    }

    #[test]
    fn cross_directory_needs_at_least_two_siblings() {
        // Only one subdirectory — no cross-directory convention possible
        let conventions = vec![
            make_convention("Flow", "inc/Abilities/Flow/*", &["execute", "__construct"], &[]),
        ];

        let results = discover_cross_directory(&conventions);
        assert!(results.is_empty());
    }

    #[test]
    fn cross_directory_skips_when_no_shared_methods() {
        // Sibling directories have completely different method sets
        let conventions = vec![
            make_convention("Flow", "inc/Modules/Flow/*", &["run_flow", "validate_flow"], &[]),
            make_convention("Job", "inc/Modules/Job/*", &["dispatch_job", "cancel_job"], &[]),
        ];

        let results = discover_cross_directory(&conventions);
        // No method appears in ≥60% of siblings (each appears in 1 of 2 = 50%)
        assert!(results.is_empty());
    }

    #[test]
    fn cross_directory_threshold_allows_partial_overlap() {
        // 3 of 4 siblings share "execute" (75% > 60% threshold) — should detect
        let conventions = vec![
            make_convention("A", "app/Services/A/*", &["execute", "validate"], &[]),
            make_convention("B", "app/Services/B/*", &["execute", "validate"], &[]),
            make_convention("C", "app/Services/C/*", &["execute", "validate"], &[]),
            make_convention("D", "app/Services/D/*", &["process"], &[]), // outlier
        ];

        let results = discover_cross_directory(&conventions);

        assert_eq!(results.len(), 1);
        let result = &results[0];
        assert!(result.expected_methods.contains(&"execute".to_string()));
        assert!(result.expected_methods.contains(&"validate".to_string()));
        assert_eq!(result.conforming_dirs.len(), 3);
        assert_eq!(result.outlier_dirs.len(), 1);
        assert_eq!(result.outlier_dirs[0].dir, "app/Services/D");
    }

    #[test]
    fn cross_directory_includes_shared_registrations() {
        let conventions = vec![
            make_convention("Flow", "inc/Abilities/Flow/*", &["execute"], &["wp_abilities_api_init"]),
            make_convention("Job", "inc/Abilities/Job/*", &["execute"], &["wp_abilities_api_init"]),
        ];

        let results = discover_cross_directory(&conventions);

        assert_eq!(results.len(), 1);
        assert!(results[0].expected_registrations.contains(&"wp_abilities_api_init".to_string()));
    }

    #[test]
    fn cross_directory_separate_parents_produce_separate_conventions() {
        let conventions = vec![
            make_convention("Flow", "inc/Abilities/Flow/*", &["execute", "register"], &[]),
            make_convention("Job", "inc/Abilities/Job/*", &["execute", "register"], &[]),
            make_convention("Auth", "inc/Middleware/Auth/*", &["handle", "boot"], &[]),
            make_convention("Cache", "inc/Middleware/Cache/*", &["handle", "boot"], &[]),
        ];

        let results = discover_cross_directory(&conventions);

        assert_eq!(results.len(), 2);
        let parents: Vec<&str> = results.iter().map(|r| r.parent.as_str()).collect();
        assert!(parents.contains(&"inc/Abilities"));
        assert!(parents.contains(&"inc/Middleware"));
    }

    #[test]
    fn cross_directory_ignores_top_level_globs() {
        // Glob "steps/*" has no parent directory — rsplitn won't find 2 parts
        let conventions = vec![
            make_convention("Steps", "steps/*", &["execute"], &[]),
            make_convention("Jobs", "jobs/*", &["execute"], &[]),
        ];

        let results = discover_cross_directory(&conventions);
        assert!(results.is_empty()); // These aren't siblings under a common parent
    }

    // ========================================================================
    // Signature consistency tests
    // ========================================================================

    #[test]
    fn signature_check_detects_mismatch() {
        let dir = std::env::temp_dir().join("homeboy_sig_mismatch_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("steps")).unwrap();

        // Two conforming files with matching signatures
        std::fs::write(
            dir.join("steps/AiChat.php"),
            r#"<?php
class AiChat {
    public function execute(array $config): array { return []; }
    public function register(): void {}
}
"#,
        ).unwrap();

        std::fs::write(
            dir.join("steps/Webhook.php"),
            r#"<?php
class Webhook {
    public function execute(array $config): array { return []; }
    public function register(): void {}
}
"#,
        ).unwrap();

        // One file with different signature (missing type hints)
        std::fs::write(
            dir.join("steps/AgentPing.php"),
            r#"<?php
class AgentPing {
    public function execute($config) { return []; }
    public function register(): void {}
}
"#,
        ).unwrap();

        let mut conventions = vec![Convention {
            name: "Steps".to_string(),
            glob: "steps/*".to_string(),
            expected_methods: vec!["execute".to_string(), "register".to_string()],
            expected_registrations: vec![],
            expected_interfaces: vec![],
            expected_namespace: None,
            expected_imports: vec![],
            conforming: vec![
                "steps/AiChat.php".to_string(),
                "steps/Webhook.php".to_string(),
                "steps/AgentPing.php".to_string(),
            ],
            outliers: vec![],
            total_files: 3,
            confidence: 1.0,
        }];

        check_signature_consistency(&mut conventions, &dir);

        let conv = &conventions[0];
        // AgentPing should be moved to outliers
        assert_eq!(conv.conforming.len(), 2);
        assert_eq!(conv.outliers.len(), 1);
        assert_eq!(conv.outliers[0].file, "steps/AgentPing.php");
        assert!(conv.outliers[0].deviations.iter().any(|d| {
            d.kind == DeviationKind::SignatureMismatch
                && d.description.contains("execute")
        }));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn signature_check_adds_to_existing_outliers() {
        let dir = std::env::temp_dir().join("homeboy_sig_existing_outlier_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("steps")).unwrap();

        std::fs::write(
            dir.join("steps/AiChat.php"),
            "<?php\nclass AiChat {\n    public function execute(array $config): array { return []; }\n    public function register(): void {}\n}\n",
        ).unwrap();

        std::fs::write(
            dir.join("steps/Webhook.php"),
            "<?php\nclass Webhook {\n    public function execute(array $config): array { return []; }\n    public function register(): void {}\n}\n",
        ).unwrap();

        // File already an outlier (missing register) AND has wrong execute signature
        std::fs::write(
            dir.join("steps/Bad.php"),
            "<?php\nclass Bad {\n    public function execute($config) { return []; }\n}\n",
        ).unwrap();

        let mut conventions = vec![Convention {
            name: "Steps".to_string(),
            glob: "steps/*".to_string(),
            expected_methods: vec!["execute".to_string(), "register".to_string()],
            expected_registrations: vec![],
            expected_interfaces: vec![],
            expected_namespace: None,
            expected_imports: vec![],
            conforming: vec![
                "steps/AiChat.php".to_string(),
                "steps/Webhook.php".to_string(),
            ],
            outliers: vec![Outlier {
                file: "steps/Bad.php".to_string(),
                deviations: vec![Deviation {
                    kind: DeviationKind::MissingMethod,
                    description: "Missing method: register".to_string(),
                    suggestion: "Add register()".to_string(),
                }],
            }],
            total_files: 3,
            confidence: 0.67,
        }];

        check_signature_consistency(&mut conventions, &dir);

        let conv = &conventions[0];
        assert_eq!(conv.conforming.len(), 2);
        assert_eq!(conv.outliers.len(), 1);
        // Should have BOTH the original MissingMethod AND the new SignatureMismatch
        assert!(conv.outliers[0].deviations.len() >= 2);
        assert!(conv.outliers[0].deviations.iter().any(|d| d.kind == DeviationKind::MissingMethod));
        assert!(conv.outliers[0].deviations.iter().any(|d| d.kind == DeviationKind::SignatureMismatch));
    }

    #[test]
    fn signature_check_no_change_when_all_match() {
        let dir = std::env::temp_dir().join("homeboy_sig_all_match_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("steps")).unwrap();

        std::fs::write(
            dir.join("steps/A.php"),
            "<?php\nclass A {\n    public function execute(array $config): array { return []; }\n}\n",
        ).unwrap();

        std::fs::write(
            dir.join("steps/B.php"),
            "<?php\nclass B {\n    public function execute(array $config): array { return []; }\n}\n",
        ).unwrap();

        let mut conventions = vec![Convention {
            name: "Steps".to_string(),
            glob: "steps/*".to_string(),
            expected_methods: vec!["execute".to_string()],
            expected_registrations: vec![],
            expected_interfaces: vec![],
            expected_namespace: None,
            expected_imports: vec![],
            conforming: vec!["steps/A.php".to_string(), "steps/B.php".to_string()],
            outliers: vec![],
            total_files: 2,
            confidence: 1.0,
        }];

        check_signature_consistency(&mut conventions, &dir);

        let conv = &conventions[0];
        assert_eq!(conv.conforming.len(), 2);
        assert!(conv.outliers.is_empty());
        assert!((conv.confidence - 1.0).abs() < f32::EPSILON);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn signature_check_skips_unknown_language() {
        let dir = std::env::temp_dir().join("homeboy_sig_unknown_lang_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();

        std::fs::write(dir.join("data/a.txt"), "some text\n").unwrap();
        std::fs::write(dir.join("data/b.txt"), "some text\n").unwrap();

        let mut conventions = vec![Convention {
            name: "Data".to_string(),
            glob: "data/*".to_string(),
            expected_methods: vec!["process".to_string()],
            expected_registrations: vec![],
            expected_interfaces: vec![],
            expected_namespace: None,
            expected_imports: vec![],
            conforming: vec!["data/a.txt".to_string(), "data/b.txt".to_string()],
            outliers: vec![],
            total_files: 2,
            confidence: 1.0,
        }];

        check_signature_consistency(&mut conventions, &dir);

        // Should not change anything for unknown language
        assert_eq!(conventions[0].conforming.len(), 2);
        assert!(conventions[0].outliers.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn signature_check_majority_wins() {
        // 2 files have one signature, 1 file has another — the 2-file version is canonical
        let dir = std::env::temp_dir().join("homeboy_sig_majority_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("steps")).unwrap();

        std::fs::write(
            dir.join("steps/A.php"),
            "<?php\nclass A {\n    public function run(string $input): bool { return true; }\n}\n",
        ).unwrap();

        std::fs::write(
            dir.join("steps/B.php"),
            "<?php\nclass B {\n    public function run(string $input): bool { return true; }\n}\n",
        ).unwrap();

        std::fs::write(
            dir.join("steps/C.php"),
            "<?php\nclass C {\n    public function run($input) { return true; }\n}\n",
        ).unwrap();

        let mut conventions = vec![Convention {
            name: "Steps".to_string(),
            glob: "steps/*".to_string(),
            expected_methods: vec!["run".to_string()],
            expected_registrations: vec![],
            expected_interfaces: vec![],
            expected_namespace: None,
            expected_imports: vec![],
            conforming: vec![
                "steps/A.php".to_string(),
                "steps/B.php".to_string(),
                "steps/C.php".to_string(),
            ],
            outliers: vec![],
            total_files: 3,
            confidence: 1.0,
        }];

        check_signature_consistency(&mut conventions, &dir);

        let conv = &conventions[0];
        assert_eq!(conv.conforming.len(), 2);
        assert_eq!(conv.outliers.len(), 1);
        assert_eq!(conv.outliers[0].file, "steps/C.php");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ========================================================================
    // Namespace and import tests
    // ========================================================================

    #[test]
    fn extract_php_namespace() {
        let content = r#"<?php
namespace DataMachine\Abilities\Flow;

use DataMachine\Core\BaseAbility;
use DataMachine\Traits\Registrable;

class CreateFlowAbility extends BaseAbility {
    public function execute() {}
}
"#;
        let (ns, imports) = extract_php_namespace_imports(content);
        assert_eq!(ns, Some("DataMachine\\Abilities\\Flow".to_string()));
        assert_eq!(imports.len(), 2);
        assert!(imports.contains(&"DataMachine\\Core\\BaseAbility".to_string()));
        assert!(imports.contains(&"DataMachine\\Traits\\Registrable".to_string()));
    }

    #[test]
    fn extract_php_no_namespace() {
        let content = "<?php\nclass SimpleClass {}\n";
        let (ns, imports) = extract_php_namespace_imports(content);
        assert!(ns.is_none());
        assert!(imports.is_empty());
    }

    #[test]
    fn extract_js_imports() {
        let content = r#"
import React from 'react';
import { useState } from 'react';
import FlowCard from '../components/FlowCard';

const App = () => {};
export default App;
"#;
        let (ns, imports) = extract_js_namespace_imports(content);
        assert!(ns.is_none());
        assert!(imports.contains(&"react".to_string()));
        assert!(imports.contains(&"../components/FlowCard".to_string()));
    }

    #[test]
    fn namespace_mismatch_detected_in_convention() {
        let fingerprints = vec![
            FileFingerprint {
                relative_path: "abilities/CreateFlow.php".to_string(),
                language: Language::Php,
                methods: vec!["execute".to_string()],
                registrations: vec![],
                type_name: Some("CreateFlow".to_string()),
                implements: vec![],
                namespace: Some("DataMachine\\Abilities\\Flow".to_string()),
                imports: vec![],
            content: String::new(),
            },
            FileFingerprint {
                relative_path: "abilities/UpdateFlow.php".to_string(),
                language: Language::Php,
                methods: vec!["execute".to_string()],
                registrations: vec![],
                type_name: Some("UpdateFlow".to_string()),
                implements: vec![],
                namespace: Some("DataMachine\\Abilities\\Flow".to_string()),
                imports: vec![],
            content: String::new(),
            },
            FileFingerprint {
                relative_path: "abilities/DeleteFlow.php".to_string(),
                language: Language::Php,
                methods: vec!["execute".to_string()],
                registrations: vec![],
                type_name: Some("DeleteFlow".to_string()),
                implements: vec![],
                namespace: Some("DataMachine\\Flow".to_string()), // WRONG namespace
                imports: vec![],
            content: String::new(),
            },
        ];

        let convention =
            discover_conventions("Flow", "abilities/*", &fingerprints).unwrap();

        assert_eq!(convention.expected_namespace, Some("DataMachine\\Abilities\\Flow".to_string()));
        assert_eq!(convention.conforming.len(), 2);
        assert_eq!(convention.outliers.len(), 1);
        assert_eq!(convention.outliers[0].file, "abilities/DeleteFlow.php");
        assert!(convention.outliers[0].deviations.iter().any(|d| {
            d.kind == DeviationKind::NamespaceMismatch
        }));
    }

    #[test]
    fn missing_import_detected_in_convention() {
        let fingerprints = vec![
            FileFingerprint {
                relative_path: "abilities/A.php".to_string(),
                language: Language::Php,
                methods: vec!["execute".to_string()],
                registrations: vec![],
                type_name: None,
                implements: vec![],
                namespace: None,
                imports: vec!["DataMachine\\Core\\Base".to_string()],
            content: String::new(),
            },
            FileFingerprint {
                relative_path: "abilities/B.php".to_string(),
                language: Language::Php,
                methods: vec!["execute".to_string()],
                registrations: vec![],
                type_name: None,
                implements: vec![],
                namespace: None,
                imports: vec!["DataMachine\\Core\\Base".to_string()],
            content: String::new(),
            },
            FileFingerprint {
                relative_path: "abilities/C.php".to_string(),
                language: Language::Php,
                methods: vec!["execute".to_string()],
                registrations: vec![],
                type_name: None,
                implements: vec![],
                namespace: None,
                imports: vec![],
                // File uses Base but doesn't import it
                content: "class C extends Base {\n    public function execute() {}\n}".to_string(),
            },
        ];

        let convention =
            discover_conventions("Abilities", "abilities/*", &fingerprints).unwrap();

        assert!(convention.expected_imports.contains(&"DataMachine\\Core\\Base".to_string()));
        assert_eq!(convention.outliers.len(), 1);
        assert!(convention.outliers[0].deviations.iter().any(|d| {
            d.kind == DeviationKind::MissingImport
        }));
    }

    #[test]
    fn missing_namespace_detected() {
        let fingerprints = vec![
            FileFingerprint {
                relative_path: "steps/A.php".to_string(),
                language: Language::Php,
                methods: vec!["run".to_string()],
                registrations: vec![],
                type_name: None,
                implements: vec![],
                namespace: Some("App\\Steps".to_string()),
                imports: vec![],
            content: String::new(),
            },
            FileFingerprint {
                relative_path: "steps/B.php".to_string(),
                language: Language::Php,
                methods: vec!["run".to_string()],
                registrations: vec![],
                type_name: None,
                implements: vec![],
                namespace: Some("App\\Steps".to_string()),
                imports: vec![],
            content: String::new(),
            },
            FileFingerprint {
                relative_path: "steps/C.php".to_string(),
                language: Language::Php,
                methods: vec!["run".to_string()],
                registrations: vec![],
                type_name: None,
                implements: vec![],
                namespace: None, // Missing namespace entirely
                imports: vec![],
            content: String::new(),
            },
        ];

        let convention =
            discover_conventions("Steps", "steps/*", &fingerprints).unwrap();

        assert_eq!(convention.expected_namespace, Some("App\\Steps".to_string()));
        assert_eq!(convention.outliers.len(), 1);
        assert!(convention.outliers[0].deviations.iter().any(|d| {
            d.kind == DeviationKind::NamespaceMismatch
                && d.description.contains("Missing namespace")
        }));
    }

    // ========================================================================
    // has_import tests
    // ========================================================================

    #[test]
    fn has_import_exact_match() {
        let imports = vec!["super::CmdResult".to_string()];
        assert!(has_import("super::CmdResult", &imports, "use super::CmdResult;\nfn run() -> CmdResult<T> {}"));
    }

    #[test]
    fn has_import_grouped_import() {
        // super::{CmdResult, DynamicSetArgs} should satisfy super::CmdResult
        let imports = vec!["super::{CmdResult, DynamicSetArgs}".to_string()];
        assert!(has_import("super::CmdResult", &imports, "fn run() -> CmdResult<T> {}"));
    }

    #[test]
    fn has_import_grouped_serde() {
        // serde::{Deserialize, Serialize} should satisfy serde::Serialize
        let imports = vec!["serde::{Deserialize, Serialize}".to_string()];
        assert!(has_import("serde::Serialize", &imports, "#[derive(Serialize)]\nstruct Foo {}"));
    }

    #[test]
    fn has_import_path_equivalence() {
        // crate::commands::CmdResult should satisfy super::CmdResult
        let imports = vec!["crate::commands::CmdResult".to_string()];
        assert!(has_import("super::CmdResult", &imports, "fn run() -> CmdResult<T> {}"));
    }

    #[test]
    fn has_import_unused_name_skipped() {
        // File doesn't use Serialize at all — missing import is irrelevant
        let imports = vec![];
        let content = "pub fn run() -> SomeOutput {}\n";
        assert!(has_import("serde::Serialize", &imports, content));
    }

    #[test]
    fn has_import_used_name_flagged() {
        // File uses Serialize but doesn't import it — real finding
        let imports = vec![];
        let content = "#[derive(Serialize)]\npub struct Output {}\n";
        assert!(!has_import("serde::Serialize", &imports, content));
    }

    #[test]
    fn has_import_grouped_from_alternate_path() {
        // crate::commands::{CmdResult, GlobalArgs} should satisfy super::CmdResult
        let imports = vec!["crate::commands::{CmdResult, GlobalArgs}".to_string()];
        assert!(has_import("super::CmdResult", &imports, "fn run() -> CmdResult<T> {}"));
    }

    #[test]
    fn contains_word_matches_standalone() {
        assert!(contains_word("derive(Serialize)", "Serialize"));
        assert!(contains_word("use Serialize;", "Serialize"));
        assert!(!contains_word("SerializeMe", "Serialize"));
        assert!(!contains_word("MySerialize", "Serialize"));
        assert!(!contains_word("_Serialize_ext", "Serialize"));
    }

    #[test]
    fn grouped_import_contains_finds_name() {
        assert!(grouped_import_contains("super::{CmdResult, DynamicSetArgs}", "CmdResult"));
        assert!(grouped_import_contains("super::{CmdResult, DynamicSetArgs}", "DynamicSetArgs"));
        assert!(!grouped_import_contains("super::{CmdResult, DynamicSetArgs}", "GlobalArgs"));
        assert!(grouped_import_contains("serde::{Deserialize, Serialize}", "Serialize"));
    }
}
