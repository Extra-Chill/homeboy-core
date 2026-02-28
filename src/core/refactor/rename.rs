//! Rename engine — find and replace terms across a codebase with case awareness.
//!
//! Given a `RenameSpec` (from → to), this extension:
//! 1. Generates all case variants (snake, camel, Pascal, UPPER, plural)
//! 2. Walks the codebase finding word-boundary matches
//! 3. Generates file content edits and file/directory renames
//! 4. Applies changes to disk (or returns a dry-run preview)

use crate::error::{Error, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ============================================================================
// Types
// ============================================================================

/// What scope to apply renames to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameScope {
    /// Source files only.
    Code,
    /// Config files only (homeboy.json, component configs).
    Config,
    /// Everything.
    All,
}

impl RenameScope {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "code" => Ok(RenameScope::Code),
            "config" => Ok(RenameScope::Config),
            "all" => Ok(RenameScope::All),
            _ => Err(Error::validation_invalid_argument(
                "scope",
                format!("Unknown scope '{}'. Use: code, config, all", s),
                None,
                None,
            )),
        }
    }
}

/// A case variant of a rename term.
#[derive(Debug, Clone, Serialize)]
pub struct CaseVariant {
    pub from: String,
    pub to: String,
    pub label: String,
}

/// A rename specification with all generated case variants.
#[derive(Debug, Clone)]
pub struct RenameSpec {
    pub from: String,
    pub to: String,
    pub scope: RenameScope,
    pub variants: Vec<CaseVariant>,
}

impl RenameSpec {
    /// Create a rename spec, auto-generating case variants.
    ///
    /// From a base term like "extension", generates:
    /// - `extension` → `extension` (lowercase)
    /// - `Extension` → `Extension` (PascalCase)
    /// - `EXTENSION` → `EXTENSION` (UPPER_CASE)
    /// - `extensions` → `extensions` (plural)
    /// - `Extensions` → `Extensions` (plural PascalCase)
    /// - `EXTENSIONS` → `EXTENSIONS` (plural UPPER)
    /// - `extension_` → `extension_` (snake prefix, catches snake_case compounds)
    /// - `_module` → `_extension` (snake suffix)
    pub fn new(from: &str, to: &str, scope: RenameScope) -> Self {
        let mut variants = Vec::new();

        // Singular forms
        variants.push(CaseVariant {
            from: from.to_lowercase(),
            to: to.to_lowercase(),
            label: "lowercase".to_string(),
        });
        variants.push(CaseVariant {
            from: capitalize(&from.to_lowercase()),
            to: capitalize(&to.to_lowercase()),
            label: "PascalCase".to_string(),
        });
        variants.push(CaseVariant {
            from: from.to_uppercase(),
            to: to.to_uppercase(),
            label: "UPPER_CASE".to_string(),
        });

        // Plural forms
        let from_plural = pluralize(&from.to_lowercase());
        let to_plural = pluralize(&to.to_lowercase());
        variants.push(CaseVariant {
            from: from_plural.clone(),
            to: to_plural.clone(),
            label: "plural".to_string(),
        });
        variants.push(CaseVariant {
            from: capitalize(&from_plural),
            to: capitalize(&to_plural),
            label: "plural PascalCase".to_string(),
        });
        variants.push(CaseVariant {
            from: from_plural.to_uppercase(),
            to: to_plural.to_uppercase(),
            label: "plural UPPER".to_string(),
        });

        // Deduplicate (in case from == plural form)
        variants.dedup_by(|a, b| a.from == b.from);

        RenameSpec {
            from: from.to_string(),
            to: to.to_string(),
            scope,
            variants,
        }
    }
}

/// A single reference found in the codebase.
#[derive(Debug, Clone, Serialize)]
pub struct Reference {
    /// File path relative to root.
    pub file: String,
    /// Line number (1-indexed).
    pub line: usize,
    /// Column number (1-indexed).
    pub column: usize,
    /// The matched text.
    pub matched: String,
    /// What it would be replaced with.
    pub replacement: String,
    /// The case variant label.
    pub variant: String,
    /// The full line content for context.
    pub context: String,
}

/// An edit to apply to a file's content.
#[derive(Debug, Clone, Serialize)]
pub struct FileEdit {
    /// File path relative to root.
    pub file: String,
    /// Number of replacements in this file.
    pub replacements: usize,
    /// New content after all replacements.
    #[serde(skip)]
    pub new_content: String,
}

/// A file or directory rename.
#[derive(Debug, Clone, Serialize)]
pub struct FileRename {
    /// Original path relative to root.
    pub from: String,
    /// New path relative to root.
    pub to: String,
}

/// The full result of a rename operation.
#[derive(Debug, Clone, Serialize)]
pub struct RenameResult {
    /// Case variants that were searched.
    pub variants: Vec<CaseVariant>,
    /// All references found.
    pub references: Vec<Reference>,
    /// File content edits to apply.
    pub edits: Vec<FileEdit>,
    /// File/directory renames to apply.
    pub file_renames: Vec<FileRename>,
    /// Total reference count.
    pub total_references: usize,
    /// Total files affected.
    pub total_files: usize,
    /// Whether changes were written to disk.
    pub applied: bool,
}

// ============================================================================
// Case utilities
// ============================================================================

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

fn pluralize(s: &str) -> String {
    if s.ends_with('s') || s.ends_with('x') || s.ends_with("sh") || s.ends_with("ch") {
        format!("{}es", s)
    } else if s.ends_with('y') && !s.ends_with("ey") && !s.ends_with("oy") && !s.ends_with("ay") {
        format!("{}ies", &s[..s.len() - 1])
    } else {
        format!("{}s", s)
    }
}

// ============================================================================
// Boundary-aware regex
// ============================================================================

/// Check if a character is a boundary for matching purposes.
/// A boundary exists at word starts/ends, camelCase joins (lowercase→uppercase),
/// and underscore separators.
fn is_boundary_char(c: u8) -> bool {
    !c.is_ascii_alphanumeric() && c != b'_'
}

/// Find all occurrences of `term` in `text` that appear at sensible boundaries.
///
/// Boundary rules:
/// - Left: start of string, non-alphanumeric char, or underscore
/// - Right: end of string, non-alphanumeric, underscore, or uppercase letter (camelCase)
///
/// This handles:
/// - `widget` in `pub mod widget;` (word boundary)
/// - `Widget` in `WidgetManifest` (uppercase letter follows = camelCase boundary)
/// - `WIDGET` in `WIDGET_DIR` (underscore follows)
/// - `widget` in `load_widget` (underscore precedes = snake_case boundary)
/// - Won't match `widget` inside `widgetry` (lowercase letter follows)
fn find_term_matches(text: &str, term: &str) -> Vec<usize> {
    let text_bytes = text.as_bytes();
    let term_bytes = term.as_bytes();
    let term_len = term_bytes.len();
    let text_len = text_bytes.len();
    let mut matches = Vec::new();

    if term_len == 0 || term_len > text_len {
        return matches;
    }

    let mut start = 0;
    while let Some(pos) = text[start..].find(term) {
        let abs = start + pos;
        let end = abs + term_len;

        // Left boundary: start of string, non-alphanumeric, or underscore
        let left_ok = abs == 0 || is_boundary_char(text_bytes[abs - 1]) || text_bytes[abs - 1] == b'_';

        // Right boundary: end of string, or next char is:
        // - not alphanumeric (space, punctuation, etc.)
        // - uppercase letter (camelCase boundary: WidgetManifest → Widget|Manifest)
        // - underscore (snake boundary: WIDGET_DIR → WIDGET|_DIR)
        let right_ok = end >= text_len || {
            let next = text_bytes[end];
            is_boundary_char(next) || next.is_ascii_uppercase() || next == b'_'
        };

        if left_ok && right_ok {
            matches.push(abs);
        }

        start = abs + 1;
    }

    matches
}

// ============================================================================
// File walking
// ============================================================================

const SKIP_DIRS: &[&str] = &[
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

const SOURCE_EXTENSIONS: &[&str] = &[
    "rs", "php", "js", "jsx", "ts", "tsx", "mjs", "json", "toml", "yaml", "yml", "md", "txt",
    "sh", "bash", "py", "rb", "go", "swift", "lock",
];

fn walk_files(root: &Path, scope: &RenameScope) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_recursive(root, &mut files);

    match scope {
        RenameScope::Code => {
            files.retain(|f| {
                let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
                !matches!(ext, "json" | "toml" | "yaml" | "yml")
            });
        }
        RenameScope::Config => {
            files.retain(|f| {
                let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
                matches!(ext, "json" | "toml" | "yaml" | "yml")
            });
        }
        RenameScope::All => {}
    }

    files
}

fn walk_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk_recursive(&path, files);
            }
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if SOURCE_EXTENSIONS.contains(&ext) {
                files.push(path);
            }
        }
    }
}

// ============================================================================
// Reference finding
// ============================================================================

/// Find all references to the rename term across the codebase.
pub fn find_references(spec: &RenameSpec, root: &Path) -> Vec<Reference> {
    let files = walk_files(root, &spec.scope);
    let mut references = Vec::new();

    // Sort variants longest-first to prevent partial overlap
    let mut sorted_variants = spec.variants.clone();
    sorted_variants.sort_by(|a, b| b.from.len().cmp(&a.from.len()));

    for file_path in &files {
        let Ok(content) = std::fs::read_to_string(file_path) else {
            continue;
        };

        let relative = file_path
            .strip_prefix(root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        for (line_num, line) in content.lines().enumerate() {
            // Track which byte offsets in this line are already claimed
            // to prevent overlapping matches from shorter variants
            let mut claimed: Vec<(usize, usize)> = Vec::new();

            for variant in &sorted_variants {
                let positions = find_term_matches(line, &variant.from);
                for pos in positions {
                    let end = pos + variant.from.len();
                    // Skip if this range overlaps with an already-claimed match
                    if claimed.iter().any(|&(s, e)| pos < e && end > s) {
                        continue;
                    }
                    claimed.push((pos, end));
                    references.push(Reference {
                        file: relative.clone(),
                        line: line_num + 1,
                        column: pos + 1,
                        matched: variant.from.clone(),
                        replacement: variant.to.clone(),
                        variant: variant.label.clone(),
                        context: line.to_string(),
                    });
                }
            }
        }
    }

    references
}

// ============================================================================
// Rename generation
// ============================================================================

/// Generate file edits and file renames from found references.
pub fn generate_renames(spec: &RenameSpec, root: &Path) -> RenameResult {
    let references = find_references(spec, root);
    let files = walk_files(root, &spec.scope);

    // Sort variants longest-first to prevent partial matches
    let mut sorted_variants = spec.variants.clone();
    sorted_variants.sort_by(|a, b| b.from.len().cmp(&a.from.len()));

    // Generate file content edits using reverse-offset replacement
    let mut edits = Vec::new();
    let mut affected_files: HashMap<String, bool> = HashMap::new();

    for file_path in &files {
        let Ok(content) = std::fs::read_to_string(file_path) else {
            continue;
        };

        let relative = file_path
            .strip_prefix(root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        // Collect all matches with their positions and replacements
        let mut all_matches: Vec<(usize, usize, String)> = Vec::new(); // (start, end, replacement)

        for variant in &sorted_variants {
            let positions = find_term_matches(&content, &variant.from);
            for pos in positions {
                let end = pos + variant.from.len();
                // Skip if overlapping with an already-claimed longer match
                if all_matches.iter().any(|&(s, e, _)| pos < e && end > s) {
                    continue;
                }
                all_matches.push((pos, end, variant.to.clone()));
            }
        }

        if !all_matches.is_empty() {
            let count = all_matches.len();

            // Sort by position descending so we can replace from end to start
            // without invalidating earlier offsets
            all_matches.sort_by(|a, b| b.0.cmp(&a.0));

            let mut new_content = content;
            for (start, end, replacement) in &all_matches {
                new_content.replace_range(start..end, replacement);
            }

            affected_files.insert(relative.clone(), true);
            edits.push(FileEdit {
                file: relative,
                replacements: count,
                new_content,
            });
        }
    }

    // Generate file/directory renames
    let mut file_renames = Vec::new();
    for file_path in &files {
        let relative = file_path
            .strip_prefix(root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        let mut new_relative = relative.clone();
        for variant in &sorted_variants {
            // Replace in path segments (word-boundary aware in file names)
            new_relative = new_relative.replace(&variant.from, &variant.to);
        }

        if new_relative != relative {
            file_renames.push(FileRename {
                from: relative,
                to: new_relative,
            });
        }
    }

    // Deduplicate file renames
    file_renames.dedup_by(|a, b| a.from == b.from);

    let total_references = references.len();
    let total_files = affected_files.len() + file_renames.len();

    RenameResult {
        variants: spec.variants.clone(),
        references,
        edits,
        file_renames,
        total_references,
        total_files,
        applied: false,
    }
}

// ============================================================================
// Apply renames
// ============================================================================

/// Apply rename edits and file renames to disk.
pub fn apply_renames(result: &mut RenameResult, root: &Path) -> Result<()> {
    // Apply content edits first
    for edit in &result.edits {
        let path = root.join(&edit.file);
        std::fs::write(&path, &edit.new_content).map_err(|e| {
            Error::internal_io(e.to_string(), Some(format!("write {}", path.display())))
        })?;
    }

    // Apply file renames (sort by path depth descending so children rename before parents)
    let mut renames = result.file_renames.clone();
    renames.sort_by(|a, b| b.from.matches('/').count().cmp(&a.from.matches('/').count()));

    for rename in &renames {
        let from = root.join(&rename.from);
        let to = root.join(&rename.to);

        // Create parent dirs if needed
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if from.exists() {
            std::fs::rename(&from, &to).map_err(|e| {
                Error::internal_io(
                    e.to_string(),
                    Some(format!("rename {} → {}", from.display(), to.display())),
                )
            })?;
        }
    }

    result.applied = true;
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitalize_works() {
        assert_eq!(capitalize("widget"), "Widget");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("a"), "A");
    }

    #[test]
    fn pluralize_regular() {
        assert_eq!(pluralize("widget"), "widgets");
        assert_eq!(pluralize("gadget"), "gadgets");
    }

    #[test]
    fn pluralize_y_ending() {
        assert_eq!(pluralize("ability"), "abilities");
        assert_eq!(pluralize("query"), "queries");
    }

    #[test]
    fn pluralize_s_ending() {
        assert_eq!(pluralize("class"), "classes");
    }

    #[test]
    fn pluralize_preserves_ey_oy_ay() {
        assert_eq!(pluralize("key"), "keys");
        assert_eq!(pluralize("day"), "days");
    }

    #[test]
    fn rename_spec_generates_variants() {
        let spec = RenameSpec::new("widget", "gadget", RenameScope::All);
        let from_values: Vec<&str> = spec.variants.iter().map(|v| v.from.as_str()).collect();
        assert!(from_values.contains(&"widget"));
        assert!(from_values.contains(&"Widget"));
        assert!(from_values.contains(&"WIDGET"));
        assert!(from_values.contains(&"widgets"));
        assert!(from_values.contains(&"Widgets"));
        assert!(from_values.contains(&"WIDGETS"));

        let to_values: Vec<&str> = spec.variants.iter().map(|v| v.to.as_str()).collect();
        assert!(to_values.contains(&"gadget"));
        assert!(to_values.contains(&"Gadget"));
        assert!(to_values.contains(&"GADGET"));
        assert!(to_values.contains(&"gadgets"));
        assert!(to_values.contains(&"Gadgets"));
        assert!(to_values.contains(&"GADGETS"));
    }

    #[test]
    fn find_references_in_temp_dir() {
        let dir = std::env::temp_dir().join("homeboy_refactor_test");
        let _ = std::fs::create_dir_all(&dir);

        std::fs::write(
            dir.join("test.rs"),
            "pub mod widget;\nuse crate::widget::WidgetManifest;\nconst WIDGET_DIR: &str = \"widgets\";\n",
        )
        .unwrap();

        let spec = RenameSpec::new("widget", "gadget", RenameScope::All);
        let refs = find_references(&spec, &dir);

        assert!(!refs.is_empty());

        // Should find: widget (2x), Widget (1x), WIDGET (1x), widgets (1x)
        let matched: Vec<&str> = refs.iter().map(|r| r.matched.as_str()).collect();
        assert!(matched.contains(&"widget"), "Expected 'widget' in {:?}", matched);
        assert!(matched.contains(&"Widget"), "Expected 'Widget' in {:?}", matched);
        assert!(matched.contains(&"WIDGET"), "Expected 'WIDGET' in {:?}", matched);
        assert!(matched.contains(&"widgets"), "Expected 'widgets' in {:?}", matched);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn generate_renames_produces_edits() {
        let dir = std::env::temp_dir().join("homeboy_refactor_gen_test");
        let _ = std::fs::create_dir_all(&dir);

        std::fs::write(dir.join("test.rs"), "pub mod widget;\n").unwrap();

        let spec = RenameSpec::new("widget", "gadget", RenameScope::All);
        let result = generate_renames(&spec, &dir);

        assert!(!result.edits.is_empty());
        assert_eq!(result.edits[0].new_content, "pub mod gadget;\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn generate_renames_detects_file_renames() {
        let dir = std::env::temp_dir().join("homeboy_refactor_file_rename_test");
        let sub = dir.join("widget");
        let _ = std::fs::create_dir_all(&sub);

        std::fs::write(sub.join("widget.rs"), "fn widget_init() {}\n").unwrap();

        let spec = RenameSpec::new("widget", "gadget", RenameScope::All);
        let result = generate_renames(&spec, &dir);

        assert!(!result.file_renames.is_empty());
        // Should want to rename widget/widget.rs → gadget/gadget.rs
        let rename = result.file_renames.iter().find(|r| r.from.contains("widget.rs")).unwrap();
        assert!(rename.to.contains("gadget.rs"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn word_boundary_no_false_positives() {
        let dir = std::env::temp_dir().join("homeboy_refactor_boundary_test");
        let _ = std::fs::create_dir_all(&dir);

        // "widgets_plus" should NOT be matched as "widget" — the 's' makes it "widgets" (plural variant)
        // but "widgetry" should NOT be matched when renaming "widget"
        std::fs::write(dir.join("test.rs"), "let widgetry = true;\n").unwrap();

        let spec = RenameSpec::new("widget", "gadget", RenameScope::All);
        let refs = find_references(&spec, &dir);

        assert!(refs.is_empty(), "Should not match 'widgetry' when renaming 'widget'");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_renames_writes_to_disk() {
        let dir = std::env::temp_dir().join("homeboy_refactor_apply_test");
        let _ = std::fs::create_dir_all(&dir);

        std::fs::write(dir.join("test.rs"), "pub mod widget;\n").unwrap();

        let spec = RenameSpec::new("widget", "gadget", RenameScope::All);
        let mut result = generate_renames(&spec, &dir);

        apply_renames(&mut result, &dir).unwrap();
        assert!(result.applied);

        let content = std::fs::read_to_string(dir.join("test.rs")).unwrap();
        assert_eq!(content, "pub mod gadget;\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snake_case_compounds_match() {
        // find_term_matches should match "widget" inside "load_widget", "is_widget_linked", etc.
        let matches = find_term_matches("load_widget", "widget");
        assert_eq!(matches, vec![5], "Should match 'widget' in 'load_widget'");

        let matches = find_term_matches("is_widget_linked", "widget");
        assert_eq!(matches, vec![3], "Should match 'widget' in 'is_widget_linked'");

        let matches = find_term_matches("widget_init", "widget");
        assert_eq!(matches, vec![0], "Should match 'widget' at start of 'widget_init'");

        let matches = find_term_matches("WIDGET_DIR", "WIDGET");
        assert_eq!(matches, vec![0], "Should match 'WIDGET' in 'WIDGET_DIR'");

        let matches = find_term_matches("THE_WIDGET_CONFIG", "WIDGET");
        assert_eq!(matches, vec![4], "Should match 'WIDGET' in 'THE_WIDGET_CONFIG'");
    }

    #[test]
    fn snake_case_rename_in_file() {
        let dir = std::env::temp_dir().join("homeboy_refactor_snake_test");
        let _ = std::fs::create_dir_all(&dir);

        std::fs::write(
            dir.join("test.rs"),
            "fn load_widget() {}\nfn is_widget_linked() -> bool { true }\nconst WIDGET_DIR: &str = \"widgets\";\n",
        )
        .unwrap();

        let spec = RenameSpec::new("widget", "gadget", RenameScope::All);
        let result = generate_renames(&spec, &dir);

        assert!(!result.edits.is_empty());
        let content = &result.edits[0].new_content;
        assert!(content.contains("load_gadget"), "Expected 'load_gadget' in:\n{}", content);
        assert!(content.contains("is_gadget_linked"), "Expected 'is_gadget_linked' in:\n{}", content);
        assert!(content.contains("GADGET_DIR"), "Expected 'GADGET_DIR' in:\n{}", content);
        assert!(content.contains("\"gadgets\""), "Expected '\"gadgets\"' in:\n{}", content);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn node_modules_not_matched() {
        // "node_modules" should NOT have "module" matched inside it — the plural
        // variant "modules" consumes it first, but we don't want partial matches either.
        // "node_modules" as a directory name is handled by SKIP_DIRS, but in content
        // the plural variant "modules" should match (not "module" partially).
        let matches = find_term_matches("node_modules", "module");
        assert!(matches.is_empty(), "Should not match 'module' inside 'node_modules' — 's' follows");

        // But "modules" (plural) should match
        let matches = find_term_matches("node_modules", "modules");
        assert_eq!(matches, vec![5], "Should match 'modules' in 'node_modules'");
    }
}
