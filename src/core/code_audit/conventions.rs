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

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviationKind {
    MissingMethod,
    ExtraMethod,
    MissingRegistration,
    DifferentRegistration,
    NamingMismatch,
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
        conforming,
        outliers,
        total_files: total,
        confidence,
    })
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
}
