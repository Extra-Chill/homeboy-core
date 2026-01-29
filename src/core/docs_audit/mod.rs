//! Documentation audit system for extracting and verifying claims from markdown files.
//!
//! This module provides a doc-centric approach to documentation auditing:
//! 1. Extract claims from documentation (file paths, identifiers, code examples)
//! 2. Verify claims against the actual codebase
//! 3. Correlate docs with git changes to identify priority docs needing review
//! 4. Build an alignment report focused on actionable items

mod claims;
mod tasks;
mod verify;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

pub use claims::{Claim, ClaimType};
pub use tasks::{AuditTask, AuditTaskStatus};
pub use verify::VerifyResult;

use crate::{component, git, module, Result};

/// A doc that needs content review due to referenced files changing.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PriorityDoc {
    pub doc: String,
    pub reason: String,
    pub changed_files_referenced: Vec<String>,
    pub code_examples: usize,
    pub action: String,
}

/// A broken reference that needs fixing.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BrokenReference {
    pub doc: String,
    pub line: usize,
    pub claim: String,
    pub action: String,
}

/// Summary counts for the alignment report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AlignmentSummary {
    pub docs_scanned: usize,
    pub priority_docs: usize,
    pub broken_references: usize,
    pub unchanged_docs: usize,
}

/// Result of auditing a component's documentation for content alignment.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditResult {
    pub component_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_ref: Option<String>,
    pub summary: AlignmentSummary,
    pub changed_files: Vec<String>,
    pub priority_docs: Vec<PriorityDoc>,
    pub broken_references: Vec<BrokenReference>,
}

/// Audit a component's documentation and return an alignment report.
pub fn audit_component(component_id: &str) -> Result<AuditResult> {
    let comp = component::load(component_id)?;
    let source_path = Path::new(&comp.local_path);
    let docs_path = source_path.join("docs");

    // Get changelog target to exclude from audit (historical references are expected)
    let changelog_exclude = comp.changelog_target.as_deref();

    // Collect ignore patterns from all linked modules
    let ignore_patterns = collect_module_ignore_patterns(&comp);

    // Find all documentation files (excluding changelog)
    let doc_files = find_doc_files(&docs_path, changelog_exclude);
    let docs_scanned = doc_files.len();

    // Extract claims from all docs
    let mut all_claims = Vec::new();
    for doc_file in &doc_files {
        let doc_path = docs_path.join(doc_file);
        if let Ok(content) = fs::read_to_string(&doc_path) {
            let claims = claims::extract_claims(&content, doc_file, &ignore_patterns);
            all_claims.extend(claims);
        }
    }

    // Verify claims and build tasks (internal only)
    let mut tasks = Vec::new();
    for claim in all_claims {
        let result = verify::verify_claim(&claim, source_path, &docs_path, Some(component_id));
        let task = tasks::build_task(claim, result);
        tasks.push(task);
    }

    // Get changed files from git (both committed and uncommitted)
    let (changed_files, baseline_ref) = get_changed_files(component_id);

    // Build doc-centric outputs
    let priority_docs = build_priority_docs(&tasks, &changed_files);
    let broken_references = extract_broken_references(&tasks);

    // Calculate unchanged docs (docs with no priority items and no broken refs)
    let docs_with_issues: HashSet<_> = priority_docs
        .iter()
        .map(|p| &p.doc)
        .chain(broken_references.iter().map(|b| &b.doc))
        .collect();
    let unchanged_docs = docs_scanned.saturating_sub(docs_with_issues.len());

    Ok(AuditResult {
        component_id: component_id.to_string(),
        baseline_ref,
        summary: AlignmentSummary {
            docs_scanned,
            priority_docs: priority_docs.len(),
            broken_references: broken_references.len(),
            unchanged_docs,
        },
        changed_files,
        priority_docs,
        broken_references,
    })
}

/// Find all markdown files in the docs directory.
///
/// Excludes the changelog file if configured, since changelogs contain
/// historical references to file paths that may no longer exist.
fn find_doc_files(docs_path: &Path, exclude_changelog: Option<&str>) -> Vec<String> {
    let mut docs = Vec::new();

    if !docs_path.exists() {
        return docs;
    }

    // Extract changelog filename for comparison
    let changelog_filename = exclude_changelog
        .and_then(|p| Path::new(p).file_name())
        .and_then(|n| n.to_str())
        .map(|s| s.to_lowercase());

    fn scan_docs(
        dir: &Path,
        prefix: &str,
        docs: &mut Vec<String>,
        changelog_filename: &Option<String>,
    ) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();

                if name.starts_with('.') {
                    continue;
                }

                if path.is_file() && name.ends_with(".md") {
                    // Skip changelog file if configured
                    if let Some(changelog) = changelog_filename {
                        if name.to_lowercase() == *changelog {
                            continue;
                        }
                    }

                    let relative = if prefix.is_empty() {
                        name
                    } else {
                        format!("{}/{}", prefix, name)
                    };
                    docs.push(relative);
                } else if path.is_dir() {
                    let new_prefix = if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{}", prefix, name)
                    };
                    scan_docs(&path, &new_prefix, docs, changelog_filename);
                }
            }
        }
    }

    scan_docs(docs_path, "", &mut docs, &changelog_filename);
    docs.sort();
    docs
}

/// Get changed files from git, including both uncommitted and committed changes.
/// Returns (changed_files, baseline_ref).
fn get_changed_files(component_id: &str) -> (Vec<String>, Option<String>) {
    // Request diff output to extract committed file changes
    let changes = match git::changes(Some(component_id), None, true) {
        Ok(c) => c,
        Err(_) => return (vec![], None),
    };

    let mut files: Vec<String> = Vec::new();

    // Uncommitted changes
    files.extend(changes.uncommitted.staged.iter().cloned());
    files.extend(changes.uncommitted.unstaged.iter().cloned());

    // Parse committed changes from diff output
    if let Some(ref diff) = changes.diff {
        files.extend(parse_diff_file_paths(diff));
    }

    files.sort();
    files.dedup();

    (files, changes.baseline_ref)
}

/// Parse git diff output to extract changed file paths.
fn parse_diff_file_paths(diff: &str) -> Vec<String> {
    diff.lines()
        .filter(|line| line.starts_with("diff --git "))
        .filter_map(|line| {
            // Format: "diff --git a/path/to/file b/path/to/file"
            line.split(" b/").nth(1).map(|s| s.to_string())
        })
        .collect()
}

/// Build priority docs by grouping tasks by doc and filtering for changed file references.
fn build_priority_docs(tasks: &[AuditTask], changed_files: &[String]) -> Vec<PriorityDoc> {
    // Group tasks by doc file
    let mut docs_map: HashMap<String, Vec<&AuditTask>> = HashMap::new();
    for task in tasks {
        docs_map.entry(task.doc.clone()).or_default().push(task);
    }

    let mut priority_docs: Vec<PriorityDoc> = Vec::new();

    for (doc, doc_tasks) in docs_map {
        // Find which changed files this doc references
        let referenced_changes: Vec<String> = changed_files
            .iter()
            .filter(|f| doc_tasks.iter().any(|t| references_file(&t.claim_value, f)))
            .cloned()
            .collect();

        if referenced_changes.is_empty() {
            continue; // Not a priority doc
        }

        // Count code examples in this doc
        let code_examples = doc_tasks
            .iter()
            .filter(|t| matches!(t.claim_type, ClaimType::CodeExample))
            .count();

        // Build action based on what needs review
        let action = build_doc_action(&referenced_changes, code_examples);

        priority_docs.push(PriorityDoc {
            doc,
            reason: format!("References {} changed file(s)", referenced_changes.len()),
            changed_files_referenced: referenced_changes,
            code_examples,
            action,
        });
    }

    // Sort by impact (most changed files referenced first)
    priority_docs.sort_by(|a, b| b.changed_files_referenced.len().cmp(&a.changed_files_referenced.len()));

    priority_docs
}

/// Check if a claim value references a changed file.
fn references_file(claim_value: &str, changed_file: &str) -> bool {
    let claim_normalized = claim_value.trim_start_matches('/');
    let file_normalized = changed_file.trim_start_matches('/');

    // Exact path match
    if claim_normalized == file_normalized {
        return true;
    }

    // Directory contains changed file (claim is a directory path like "inc/Engine/")
    if claim_value.ends_with('/') && file_normalized.starts_with(claim_normalized) {
        return true;
    }

    // Basename match (for code examples that reference "ToolExecutor" without full path)
    if let Some(basename) = Path::new(changed_file).file_stem() {
        if let Some(name) = basename.to_str() {
            // Only match if the claim contains the basename as a significant reference
            // Avoid false positives by requiring the name to appear as a word boundary
            if claim_value.contains(name) && name.len() >= 4 {
                return true;
            }
        }
    }

    false
}

/// Build an action description for a priority doc.
fn build_doc_action(changed_files: &[String], code_examples: usize) -> String {
    let files_desc = if changed_files.len() == 1 {
        changed_files[0].clone()
    } else {
        format!("{} files", changed_files.len())
    };

    if code_examples > 0 {
        format!(
            "Verify {} code example(s) match current {} implementation",
            code_examples, files_desc
        )
    } else {
        format!("Review documentation against {} changes", files_desc)
    }
}

/// Extract broken references from tasks into a separate list.
fn extract_broken_references(tasks: &[AuditTask]) -> Vec<BrokenReference> {
    tasks
        .iter()
        .filter(|t| matches!(t.status, AuditTaskStatus::Broken))
        .map(|t| BrokenReference {
            doc: t.doc.clone(),
            line: t.line,
            claim: t.claim.clone(),
            action: t
                .action
                .clone()
                .unwrap_or_else(|| "Fix broken reference".to_string()),
        })
        .collect()
}

/// Collect audit ignore patterns from all linked modules.
fn collect_module_ignore_patterns(comp: &component::Component) -> Vec<String> {
    let mut patterns = Vec::new();
    if let Some(ref modules) = comp.modules {
        for module_id in modules.keys() {
            if let Ok(manifest) = module::load_module(module_id) {
                patterns.extend(manifest.audit_ignore_claim_patterns.clone());
            }
        }
    }
    patterns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_diff_file_paths_basic() {
        let diff = r#"diff --git a/src/main.rs b/src/main.rs
index abc123..def456 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
+// New comment
 fn main() {}
diff --git a/src/lib.rs b/src/lib.rs
index 111222..333444 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1 @@
-old
+new
"#;
        let files = parse_diff_file_paths(diff);
        assert_eq!(files, vec!["src/main.rs", "src/lib.rs"]);
    }

    #[test]
    fn test_parse_diff_file_paths_empty() {
        let diff = "";
        let files = parse_diff_file_paths(diff);
        assert!(files.is_empty());
    }

    #[test]
    fn test_parse_diff_file_paths_no_diff_lines() {
        let diff = "Some random text\nwithout diff headers";
        let files = parse_diff_file_paths(diff);
        assert!(files.is_empty());
    }

    #[test]
    fn test_references_file_exact_match() {
        assert!(references_file("src/main.rs", "src/main.rs"));
        assert!(references_file("/src/main.rs", "src/main.rs"));
        assert!(references_file("src/main.rs", "/src/main.rs"));
    }

    #[test]
    fn test_references_file_directory_contains() {
        assert!(references_file("inc/Engine/", "inc/Engine/AI/Tools.php"));
        assert!(references_file("/inc/Engine/", "inc/Engine/AI/Tools.php"));
    }

    #[test]
    fn test_references_file_directory_no_match() {
        // Directory path should not match files outside it
        assert!(!references_file("inc/Engine/", "inc/Other/file.php"));
        // Non-directory paths should not use directory matching
        assert!(!references_file("inc/Engine", "inc/Engine/AI/Tools.php"));
    }

    #[test]
    fn test_references_file_basename_match() {
        // Code examples often reference class names without full paths
        assert!(references_file("ToolExecutor::run()", "inc/Engine/ToolExecutor.php"));
        assert!(references_file("new BaseTool()", "src/tools/BaseTool.rs"));
    }

    #[test]
    fn test_references_file_basename_short_name_no_match() {
        // Short names (< 4 chars) should not match to avoid false positives
        assert!(!references_file("use AI;", "src/AI.php"));
    }

    #[test]
    fn test_references_file_no_match() {
        assert!(!references_file("totally/different.rs", "src/main.rs"));
        assert!(!references_file("random text", "src/file.rs"));
    }

    #[test]
    fn test_build_doc_action_single_file() {
        let action = build_doc_action(&["src/main.rs".to_string()], 0);
        assert!(action.contains("src/main.rs"));
        assert!(action.contains("Review"));
    }

    #[test]
    fn test_build_doc_action_multiple_files() {
        let action = build_doc_action(
            &["src/main.rs".to_string(), "src/lib.rs".to_string()],
            0,
        );
        assert!(action.contains("2 files"));
    }

    #[test]
    fn test_build_doc_action_with_code_examples() {
        let action = build_doc_action(&["src/main.rs".to_string()], 3);
        assert!(action.contains("3 code example(s)"));
        assert!(action.contains("Verify"));
    }

    #[test]
    fn test_extract_broken_references() {
        let tasks = vec![
            AuditTask {
                doc: "api/index.md".to_string(),
                line: 10,
                claim: "file path `src/old.rs`".to_string(),
                claim_type: ClaimType::FilePath,
                claim_value: "src/old.rs".to_string(),
                status: AuditTaskStatus::Broken,
                action: Some("File not found".to_string()),
            },
            AuditTask {
                doc: "api/index.md".to_string(),
                line: 20,
                claim: "file path `src/main.rs`".to_string(),
                claim_type: ClaimType::FilePath,
                claim_value: "src/main.rs".to_string(),
                status: AuditTaskStatus::Verified,
                action: None,
            },
        ];

        let broken = extract_broken_references(&tasks);
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].doc, "api/index.md");
        assert_eq!(broken[0].line, 10);
        assert_eq!(broken[0].action, "File not found");
    }

    #[test]
    fn test_build_priority_docs_filters_by_changed_files() {
        let tasks = vec![
            AuditTask {
                doc: "api/tools.md".to_string(),
                line: 10,
                claim: "file path `inc/ToolExecutor.php`".to_string(),
                claim_type: ClaimType::FilePath,
                claim_value: "inc/ToolExecutor.php".to_string(),
                status: AuditTaskStatus::Verified,
                action: None,
            },
            AuditTask {
                doc: "api/other.md".to_string(),
                line: 5,
                claim: "file path `inc/Unrelated.php`".to_string(),
                claim_type: ClaimType::FilePath,
                claim_value: "inc/Unrelated.php".to_string(),
                status: AuditTaskStatus::Verified,
                action: None,
            },
        ];

        let changed_files = vec!["inc/ToolExecutor.php".to_string()];
        let priority = build_priority_docs(&tasks, &changed_files);

        // Only api/tools.md should be a priority doc
        assert_eq!(priority.len(), 1);
        assert_eq!(priority[0].doc, "api/tools.md");
        assert_eq!(priority[0].changed_files_referenced, vec!["inc/ToolExecutor.php"]);
    }

    #[test]
    fn test_build_priority_docs_sorts_by_impact() {
        let tasks = vec![
            AuditTask {
                doc: "doc_one.md".to_string(),
                line: 1,
                claim_type: ClaimType::FilePath,
                claim: "".to_string(),
                claim_value: "file1.rs".to_string(),
                status: AuditTaskStatus::Verified,
                action: None,
            },
            AuditTask {
                doc: "doc_two.md".to_string(),
                line: 1,
                claim_type: ClaimType::FilePath,
                claim: "".to_string(),
                claim_value: "file1.rs".to_string(),
                status: AuditTaskStatus::Verified,
                action: None,
            },
            AuditTask {
                doc: "doc_two.md".to_string(),
                line: 2,
                claim_type: ClaimType::FilePath,
                claim: "".to_string(),
                claim_value: "file2.rs".to_string(),
                status: AuditTaskStatus::Verified,
                action: None,
            },
        ];

        let changed_files = vec!["file1.rs".to_string(), "file2.rs".to_string()];
        let priority = build_priority_docs(&tasks, &changed_files);

        // doc_two.md references 2 files, doc_one.md references 1
        // So doc_two.md should come first
        assert_eq!(priority.len(), 2);
        assert_eq!(priority[0].doc, "doc_two.md");
        assert_eq!(priority[0].changed_files_referenced.len(), 2);
        assert_eq!(priority[1].doc, "doc_one.md");
        assert_eq!(priority[1].changed_files_referenced.len(), 1);
    }

    #[test]
    fn test_build_priority_docs_empty_when_no_changes() {
        let tasks = vec![AuditTask {
            doc: "api/tools.md".to_string(),
            line: 10,
            claim: "file path `inc/Tool.php`".to_string(),
            claim_type: ClaimType::FilePath,
            claim_value: "inc/Tool.php".to_string(),
            status: AuditTaskStatus::Verified,
            action: None,
        }];

        let changed_files: Vec<String> = vec![];
        let priority = build_priority_docs(&tasks, &changed_files);

        assert!(priority.is_empty());
    }
}
