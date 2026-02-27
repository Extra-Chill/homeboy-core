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

    Some(FileFingerprint {
        relative_path,
        language,
        methods,
        registrations,
        type_name,
        implements,
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
        conforming,
        outliers,
        total_files: total,
        confidence,
    })
}

// ============================================================================
// Signature Consistency
// ============================================================================

/// Check method signatures across all files in a convention for consistency.
///
/// For each expected method, builds a canonical signature from the majority of
/// conforming files. Files with different signatures get a `SignatureMismatch`
/// deviation and may be moved from conforming to outlier.
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

        // Build canonical signatures from conforming files (majority wins)
        let mut method_sig_counts: HashMap<String, HashMap<String, usize>> = HashMap::new();

        for file in &conv.conforming {
            let full_path = root.join(file);
            let content = match std::fs::read_to_string(&full_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let sigs = super::fixer::extract_signatures(&content, &lang);
            for sig in &sigs {
                if conv.expected_methods.contains(&sig.name) {
                    method_sig_counts
                        .entry(sig.name.clone())
                        .or_default()
                        .entry(sig.signature.clone())
                        .and_modify(|c| *c += 1)
                        .or_insert(1);
                }
            }
        }

        // Pick canonical: the most frequent signature for each method
        let mut canonical: HashMap<String, String> = HashMap::new();
        for (method, sig_counts) in &method_sig_counts {
            if let Some((sig, _)) = sig_counts.iter().max_by_key(|(_, count)| *count) {
                canonical.insert(method.clone(), sig.clone());
            }
        }

        if canonical.is_empty() {
            continue;
        }

        // Check ALL files (conforming + outliers) for signature mismatches
        let all_files: Vec<String> = conv
            .conforming
            .iter()
            .chain(conv.outliers.iter().map(|o| &o.file))
            .cloned()
            .collect();

        let mut new_outlier_deviations: HashMap<String, Vec<Deviation>> = HashMap::new();

        for file in &all_files {
            let full_path = root.join(file);
            let content = match std::fs::read_to_string(&full_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let sigs = super::fixer::extract_signatures(&content, &lang);
            let sig_map: HashMap<&str, &str> = sigs
                .iter()
                .map(|s| (s.name.as_str(), s.signature.as_str()))
                .collect();

            for (method, expected_sig) in &canonical {
                if let Some(actual_sig) = sig_map.get(method.as_str()) {
                    if *actual_sig != expected_sig.as_str() {
                        new_outlier_deviations
                            .entry(file.clone())
                            .or_default()
                            .push(Deviation {
                                kind: DeviationKind::SignatureMismatch,
                                description: format!(
                                    "Signature mismatch for {}: expected `{}`, found `{}`",
                                    method, expected_sig, actual_sig
                                ),
                                suggestion: format!(
                                    "Update {}() signature to match: `{}`",
                                    method, expected_sig
                                ),
                            });
                    }
                }
                // If the method is absent, that's already caught as MissingMethod
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

/// Walk source files under a root, skipping common non-source directories.
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
            },
            FileFingerprint {
                relative_path: "steps/agent-ping.php".to_string(),
                language: Language::Php,
                methods: vec!["register".to_string(), "execute".to_string()],
                registrations: vec![],
                type_name: Some("AgentPing".to_string()),
                implements: vec![],
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
            },
            FileFingerprint {
                relative_path: "abilities/update.php".to_string(),
                language: Language::Php,
                methods: vec!["execute".to_string(), "register".to_string()],
                registrations: vec![],
                type_name: Some("UpdateAbility".to_string()),
                implements: vec!["AbilityInterface".to_string()],
            },
            FileFingerprint {
                relative_path: "abilities/helpers.php".to_string(),
                language: Language::Php,
                methods: vec!["execute".to_string(), "register".to_string()],
                registrations: vec![],
                type_name: Some("Helpers".to_string()),
                implements: vec![], // Missing interface
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
            },
            FileFingerprint {
                relative_path: "b.php".to_string(),
                language: Language::Php,
                methods: vec!["run".to_string()],
                registrations: vec![],
                type_name: None,
                implements: vec!["BarInterface".to_string()],
            },
            FileFingerprint {
                relative_path: "c.php".to_string(),
                language: Language::Php,
                methods: vec!["run".to_string()],
                registrations: vec![],
                type_name: None,
                implements: vec![],
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
}
