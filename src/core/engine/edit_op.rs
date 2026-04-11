//! Shared edit operations — the canonical vocabulary for file modifications.
//!
//! Both the autofix fixer pipeline and manual refactor commands (rename,
//! transform, move, decompose) perform the same handful of mechanical
//! operations on files. `EditOp` is the shared type that captures these.
//!
//! ## Design
//!
//! Five atomic operations cover all current edit patterns:
//!
//! - `ReplaceText` — find-and-replace on a single line
//! - `RemoveLines` — delete a contiguous range of lines
//! - `InsertLines` — add code at a position (import, stub, etc.)
//! - `MoveFile` — rename/relocate a file
//! - `CreateFile` — write a new file from scratch
//!
//! Each operation carries an optional `RefactorPrimitive` tag so CI
//! summaries, undo, and debugging all speak the same vocabulary regardless
//! of whether the edit came from a fixer or a manual command.
//!
//! ## Current integration
//!
//! The fixer pipeline still uses `InsertionKind` internally for apply logic.
//! `EditOp` provides a parallel representation via `from_insertion()` for
//! reporting and cross-system compatibility. Manual commands will produce
//! `EditOp` directly in a future phase.

use crate::code_audit::AuditFinding;
use crate::core::refactor::auto::{Insertion, InsertionKind, RefactorPrimitive};

/// Atomic file edit operation.
///
/// The shared vocabulary for all file modifications in the refactor engine.
/// Fixer pipelines and manual commands both reduce to these operations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditOp {
    /// Find-and-replace text on a single line.
    ///
    /// Covers: `VisibilityChange`, `LineReplacement`, `DocReferenceUpdate`.
    ReplaceText {
        /// Relative file path.
        file: String,
        /// 1-indexed line number.
        line: usize,
        /// Text to find on that line.
        old_text: String,
        /// Replacement text.
        new_text: String,
    },

    /// Remove a contiguous range of lines (inclusive).
    ///
    /// Covers: `FunctionRemoval`, `DocLineRemoval`.
    RemoveLines {
        /// Relative file path.
        file: String,
        /// 1-indexed start line.
        start_line: usize,
        /// 1-indexed end line (inclusive).
        end_line: usize,
    },

    /// Insert code at a logical position in a file.
    ///
    /// Covers: `ImportAdd`, `MethodStub`, `RegistrationStub`,
    /// `ConstructorWithRegistration`, `TraitUse`, `TypeConformance`,
    /// `NamespaceDeclaration`, `TestModule`, `ReexportRemoval`.
    ///
    /// The `anchor` describes where to insert. The apply logic resolves
    /// the actual line number based on file content and language.
    InsertLines {
        /// Relative file path.
        file: String,
        /// Where in the file to insert.
        anchor: InsertAnchor,
        /// The code to insert.
        code: String,
    },

    /// Move a file to a new path.
    ///
    /// Covers: `FileMove`.
    MoveFile {
        /// Current relative path.
        from: String,
        /// Target relative path.
        to: String,
    },

    /// Create a new file with the given content.
    ///
    /// Covers: `NewFile` from the fixer pipeline.
    CreateFile {
        /// Relative file path to create.
        file: String,
        /// Full file content.
        content: String,
    },
}

/// Logical position for inserting code into a file.
///
/// The apply layer resolves these anchors to actual line numbers based
/// on file content and language rules.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertAnchor {
    /// After the last import/use statement.
    AfterImports,
    /// After the class/struct opening brace (for trait uses).
    AfterClassOpen,
    /// Inside the constructor body, after the opening brace.
    InConstructor,
    /// Before the last closing brace in the file (for method stubs).
    BeforeClosingBrace,
    /// Replace or insert at the top of the file (for namespace declarations).
    FileTop,
    /// Append to the end of the file (for test modules).
    FileEnd,
    /// Remove a symbol from a re-export block (structural edit).
    RemoveFromReexport {
        /// The symbol name to remove.
        symbol: String,
    },
    /// Add a type conformance to the primary type declaration.
    TypeDeclaration,
    /// Insert at a specific 1-indexed line number.
    ///
    /// Used by manual commands like `propagate` that compute exact
    /// insertion points from structural analysis.
    AtLine {
        /// 1-indexed line number to insert before.
        line: usize,
    },
}

/// An `EditOp` with metadata about its origin.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaggedEditOp {
    /// The edit operation.
    #[serde(flatten)]
    pub op: EditOp,
    /// The refactor primitive this operation represents, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primitive: Option<RefactorPrimitive>,
    /// The audit finding this operation addresses, if from the fixer pipeline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finding: Option<AuditFinding>,
    /// Human-readable description.
    pub description: String,
    /// Whether this operation requires manual review.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub manual_only: bool,
}

/// Translate a fixer pipeline `Insertion` into `EditOp`(s).
///
/// Most insertions map 1:1 to an EditOp. The `file` parameter is the
/// relative file path from the parent `Fix`.
pub fn from_insertion(insertion: &Insertion, file: &str) -> TaggedEditOp {
    let op = match &insertion.kind {
        InsertionKind::VisibilityChange { line, from, to } => EditOp::ReplaceText {
            file: file.to_string(),
            line: *line,
            old_text: from.clone(),
            new_text: to.clone(),
        },

        InsertionKind::LineReplacement {
            line,
            old_text,
            new_text,
        } => EditOp::ReplaceText {
            file: file.to_string(),
            line: *line,
            old_text: old_text.clone(),
            new_text: new_text.clone(),
        },

        InsertionKind::DocReferenceUpdate {
            line,
            old_ref,
            new_ref,
        } => EditOp::ReplaceText {
            file: file.to_string(),
            line: *line,
            old_text: old_ref.clone(),
            new_text: new_ref.clone(),
        },

        InsertionKind::FunctionRemoval {
            start_line,
            end_line,
        } => EditOp::RemoveLines {
            file: file.to_string(),
            start_line: *start_line,
            end_line: *end_line,
        },

        InsertionKind::DocLineRemoval { line } => EditOp::RemoveLines {
            file: file.to_string(),
            start_line: *line,
            end_line: *line,
        },

        InsertionKind::ImportAdd => EditOp::InsertLines {
            file: file.to_string(),
            anchor: InsertAnchor::AfterImports,
            code: insertion.code.clone(),
        },

        InsertionKind::TraitUse => EditOp::InsertLines {
            file: file.to_string(),
            anchor: InsertAnchor::AfterClassOpen,
            code: insertion.code.clone(),
        },

        InsertionKind::RegistrationStub => EditOp::InsertLines {
            file: file.to_string(),
            anchor: InsertAnchor::InConstructor,
            code: insertion.code.clone(),
        },

        InsertionKind::ConstructorWithRegistration => EditOp::InsertLines {
            file: file.to_string(),
            anchor: InsertAnchor::BeforeClosingBrace,
            code: insertion.code.clone(),
        },

        InsertionKind::MethodStub => EditOp::InsertLines {
            file: file.to_string(),
            anchor: InsertAnchor::BeforeClosingBrace,
            code: insertion.code.clone(),
        },

        InsertionKind::NamespaceDeclaration => EditOp::InsertLines {
            file: file.to_string(),
            anchor: InsertAnchor::FileTop,
            code: insertion.code.clone(),
        },

        InsertionKind::TypeConformance => EditOp::InsertLines {
            file: file.to_string(),
            anchor: InsertAnchor::TypeDeclaration,
            code: insertion.code.clone(),
        },

        InsertionKind::TestModule => EditOp::InsertLines {
            file: file.to_string(),
            anchor: InsertAnchor::FileEnd,
            code: insertion.code.clone(),
        },

        InsertionKind::ReexportRemoval { fn_name } => EditOp::InsertLines {
            file: file.to_string(),
            anchor: InsertAnchor::RemoveFromReexport {
                symbol: fn_name.clone(),
            },
            code: String::new(),
        },

        InsertionKind::FileMove { from, to } => EditOp::MoveFile {
            from: from.clone(),
            to: to.clone(),
        },
    };

    TaggedEditOp {
        op,
        primitive: insertion.primitive.clone(),
        finding: Some(insertion.finding.clone()),
        description: insertion.description.clone(),
        manual_only: insertion.manual_only,
    }
}

/// Translate an entire `Fix` into a list of `TaggedEditOp`s.
pub fn fix_to_edit_ops(fix: &crate::core::refactor::auto::Fix) -> Vec<TaggedEditOp> {
    fix.insertions
        .iter()
        .map(|ins| from_insertion(ins, &fix.file))
        .collect()
}

/// Translate a `NewFile` into a `TaggedEditOp`.
pub fn new_file_to_edit_op(nf: &crate::core::refactor::auto::NewFile) -> TaggedEditOp {
    TaggedEditOp {
        op: EditOp::CreateFile {
            file: nf.file.clone(),
            content: nf.content.clone(),
        },
        primitive: nf.primitive.clone(),
        finding: Some(nf.finding.clone()),
        description: nf.description.clone(),
        manual_only: nf.manual_only,
    }
}

/// Translate an entire `FixResult` into a flat list of `TaggedEditOp`s.
///
/// This is the primary reporting/debugging surface — it shows every edit
/// the refactor engine would perform, in a unified format.
pub fn fix_result_to_edit_ops(
    result: &crate::core::refactor::auto::FixResult,
) -> Vec<TaggedEditOp> {
    let mut ops: Vec<TaggedEditOp> = result.fixes.iter().flat_map(fix_to_edit_ops).collect();

    for nf in &result.new_files {
        ops.push(new_file_to_edit_op(nf));
    }

    ops
}

// ============================================================================
// Manual command conversions
// ============================================================================

/// Translate a `PropagateEdit` into a `TaggedEditOp`.
pub fn propagate_edit_to_edit_op(
    edit: &crate::core::refactor::propagate::PropagateEdit,
) -> TaggedEditOp {
    TaggedEditOp {
        op: EditOp::InsertLines {
            file: edit.file.clone(),
            anchor: InsertAnchor::AtLine { line: edit.line },
            code: edit.insert_text.clone(),
        },
        primitive: None,
        finding: None,
        description: edit.description.clone(),
        manual_only: false,
    }
}

/// Translate a `PropagateResult` into a list of `TaggedEditOp`s.
pub fn propagate_result_to_edit_ops(
    result: &crate::core::refactor::propagate::PropagateResult,
) -> Vec<TaggedEditOp> {
    result.edits.iter().map(propagate_edit_to_edit_op).collect()
}

/// Translate a `TransformMatch` into a `TaggedEditOp`.
pub fn transform_match_to_edit_op(
    m: &crate::core::refactor::transform::TransformMatch,
) -> TaggedEditOp {
    TaggedEditOp {
        op: EditOp::ReplaceText {
            file: m.file.clone(),
            line: m.line,
            old_text: m.before.clone(),
            new_text: m.after.clone(),
        },
        primitive: None,
        finding: None,
        description: format!("Transform: {} → {}", m.before, m.after),
        manual_only: false,
    }
}

/// Translate a `TransformResult` into a list of `TaggedEditOp`s.
pub fn transform_result_to_edit_ops(
    result: &crate::core::refactor::transform::TransformResult,
) -> Vec<TaggedEditOp> {
    result
        .rules
        .iter()
        .flat_map(|rule| {
            rule.matches.iter().map(|m| {
                let mut op = transform_match_to_edit_op(m);
                op.description = format!("{}: {}", rule.description, op.description);
                op
            })
        })
        .collect()
}

// ============================================================================
// Apply logic — execute EditOps against file content and the filesystem
// ============================================================================

use crate::code_audit::conventions::Language;
use crate::error::Result;
use std::collections::HashMap;
use std::path::Path;

/// Report from applying edit operations.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ApplyReport {
    /// Number of files that were modified.
    pub files_modified: usize,
    /// Number of files created.
    pub files_created: usize,
    /// Number of files moved.
    pub files_moved: usize,
    /// Total number of ops successfully applied.
    pub ops_applied: usize,
    /// Total number of ops skipped (e.g. file not found, line out of range).
    pub ops_skipped: usize,
    /// Per-op errors (non-fatal — the op was skipped but processing continued).
    pub errors: Vec<ApplyError>,
}

/// A non-fatal error encountered while applying a single edit op.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ApplyError {
    /// Which file the error occurred in.
    pub file: String,
    /// Human-readable description of what went wrong.
    pub message: String,
}

/// Resolve an `InsertAnchor` to a 1-indexed line number in the given content.
///
/// Returns `None` if the anchor cannot be resolved (e.g. no imports found
/// for `AfterImports`, no class open for `AfterClassOpen`, etc.).
pub fn resolve_anchor(content: &str, anchor: &InsertAnchor, language: &Language) -> Option<usize> {
    let lines: Vec<&str> = content.lines().collect();

    match anchor {
        InsertAnchor::AtLine { line } => Some(*line),

        InsertAnchor::FileTop => Some(1),

        InsertAnchor::FileEnd => Some(lines.len() + 1),

        InsertAnchor::AfterImports => {
            let import_prefix = match language {
                Language::Rust => "use ",
                Language::Php => "use ",
                Language::JavaScript | Language::TypeScript => "import ",
                Language::Unknown => "use ",
            };

            // For Rust, stop scanning at definition starts to avoid matching
            // `use` inside function bodies.
            let rust_definition_starts = [
                "fn ", "pub fn ", "pub(crate) fn ", "pub(super) fn ",
                "struct ", "pub struct ", "pub(crate) struct ",
                "enum ", "pub enum ", "pub(crate) enum ",
                "impl ", "impl<",
                "mod ", "pub mod ", "pub(crate) mod ",
                "trait ", "pub trait ", "pub(crate) trait ",
                "const ", "pub const ", "pub(crate) const ",
                "static ", "pub static ", "pub(crate) static ",
                "type ", "pub type ", "pub(crate) type ",
                "#[cfg(test)]",
            ];

            let mut last_import_line = None;
            for (i, line) in lines.iter().enumerate() {
                let trimmed = line.trim();

                if *language == Language::Rust
                    && rust_definition_starts.iter().any(|prefix| trimmed.starts_with(prefix))
                {
                    break;
                }

                if trimmed.starts_with(import_prefix)
                    || (trimmed.starts_with("use ") && *language == Language::Rust)
                {
                    last_import_line = Some(i + 1); // 1-indexed
                }
            }

            // If no imports found, insert after header comments / <?php tag
            if last_import_line.is_none() {
                let mut first_code = 0;
                for (i, line) in lines.iter().enumerate() {
                    let trimmed = line.trim();
                    if trimmed.is_empty()
                        || trimmed.starts_with("//")
                        || trimmed.starts_with("/*")
                        || trimmed.starts_with('*')
                        || trimmed.starts_with('#')
                        || trimmed == "<?php"
                    {
                        first_code = i + 1;
                    } else {
                        break;
                    }
                }
                return Some(first_code + 1);
            }

            // Insert after the last import line
            last_import_line.map(|l| l + 1)
        }

        InsertAnchor::AfterClassOpen => {
            let class_re = match language {
                Language::Php => regex::Regex::new(r"(?:class|trait|interface)\s+\w+[^\{]*\{").ok()?,
                Language::Rust => regex::Regex::new(r"(?:pub\s+)?(?:struct|enum|impl)\s+\w+[^\{]*\{").ok()?,
                Language::JavaScript | Language::TypeScript => {
                    regex::Regex::new(r"class\s+\w+[^\{]*\{").ok()?
                }
                Language::Unknown => return None,
            };

            // Find the line containing the class opening brace
            let full_content = lines.join("\n");
            let m = class_re.find(&full_content)?;
            let line_num = full_content[..m.end()].matches('\n').count() + 1;
            Some(line_num + 1)
        }

        InsertAnchor::InConstructor => {
            let constructor_re = match language {
                Language::Php => regex::Regex::new(r"function\s+__construct\s*\([^)]*\)\s*\{").ok()?,
                Language::Rust => regex::Regex::new(r"fn\s+new\s*\([^)]*\)\s*(?:->[^{]*)?\{").ok()?,
                Language::JavaScript | Language::TypeScript => {
                    regex::Regex::new(r"constructor\s*\([^)]*\)\s*\{").ok()?
                }
                Language::Unknown => return None,
            };

            let full_content = lines.join("\n");
            let m = constructor_re.find(&full_content)?;
            let line_num = full_content[..m.end()].matches('\n').count() + 1;
            Some(line_num + 1)
        }

        InsertAnchor::BeforeClosingBrace => {
            // Find the last `}` in the file
            for (i, line) in lines.iter().enumerate().rev() {
                if line.contains('}') {
                    return Some(i + 1); // Insert before this line (1-indexed)
                }
            }
            None
        }

        InsertAnchor::TypeDeclaration => {
            // Find the primary type declaration line. For Rust this is struct/enum,
            // for PHP/TS this is class.
            let type_re = match language {
                Language::Php => regex::Regex::new(
                    r"^\s*(?:abstract\s+)?(?:class|interface|trait)\s+\w+"
                ).ok()?,
                Language::Rust => regex::Regex::new(
                    r"^\s*(?:pub(?:\(crate\))?\s+)?(?:struct|enum|trait)\s+\w+"
                ).ok()?,
                Language::JavaScript | Language::TypeScript => regex::Regex::new(
                    r"^\s*(?:export\s+)?(?:abstract\s+)?class\s+\w+"
                ).ok()?,
                Language::Unknown => return None,
            };

            for (i, line) in lines.iter().enumerate() {
                if type_re.is_match(line) {
                    return Some(i + 1); // 1-indexed
                }
            }
            None
        }

        InsertAnchor::RemoveFromReexport { .. } => {
            // This is a structural edit, not a positional one.
            // Handled specially in apply_edit_ops_to_content.
            None
        }
    }
}

/// Apply edit operations to a content string (no I/O).
///
/// This is the testable core. All 5 `EditOp` variants are handled:
/// - `ReplaceText` — find-and-replace on a single line
/// - `RemoveLines` — delete a contiguous range of lines
/// - `InsertLines` — add code at a resolved anchor position
/// - `MoveFile` — no-op at content level (handled by filesystem layer)
/// - `CreateFile` — no-op at content level (handled by filesystem layer)
///
/// Line-level edits are sorted bottom-to-top to avoid line number drift.
pub fn apply_edit_ops_to_content(
    content: &str,
    ops: &[&EditOp],
    language: &Language,
) -> std::result::Result<String, String> {
    // Separate ops into categories
    let mut replace_ops: Vec<(&str, usize, &str, &str)> = Vec::new(); // (file, line, old, new)
    let mut remove_ops: Vec<(usize, usize)> = Vec::new(); // (start, end) 1-indexed inclusive
    let mut insert_ops: Vec<(usize, &str)> = Vec::new(); // (resolved_line, code)
    let mut reexport_removals: Vec<&str> = Vec::new();

    for op in ops {
        match op {
            EditOp::ReplaceText {
                line,
                old_text,
                new_text,
                ..
            } => {
                replace_ops.push(("", *line, old_text, new_text));
            }
            EditOp::RemoveLines {
                start_line,
                end_line,
                ..
            } => {
                remove_ops.push((*start_line, *end_line));
            }
            EditOp::InsertLines { anchor, code, .. } => {
                // Handle RemoveFromReexport specially
                if let InsertAnchor::RemoveFromReexport { symbol } = anchor {
                    reexport_removals.push(symbol.as_str());
                } else if let Some(line) = resolve_anchor(content, anchor, language) {
                    insert_ops.push((line, code.as_str()));
                }
            }
            EditOp::MoveFile { .. } | EditOp::CreateFile { .. } => {
                // No-op at content level
            }
        }
    }

    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let had_trailing_newline = content.ends_with('\n');

    // 1. Apply ReplaceText ops (order doesn't matter — each targets a specific line)
    for (_file, line_num, old_text, new_text) in &replace_ops {
        let idx = line_num.saturating_sub(1);
        if idx < lines.len() {
            if lines[idx].contains(*old_text) {
                lines[idx] = lines[idx].replacen(*old_text, *new_text, 1);
            } else {
                return Err(format!(
                    "ReplaceText: old_text {:?} not found on line {}",
                    old_text, line_num
                ));
            }
        } else {
            return Err(format!(
                "ReplaceText: line {} out of range (file has {} lines)",
                line_num,
                lines.len()
            ));
        }
    }

    // 2. Apply RemoveFromReexport (structural edit on pub use blocks)
    if !reexport_removals.is_empty() {
        for fn_name in &reexport_removals {
            remove_from_reexport_block(&mut lines, fn_name);
        }
    }

    // 3. Apply RemoveLines — sort bottom-to-top to avoid drift
    remove_ops.sort_by(|a, b| b.0.cmp(&a.0));
    for (start, end) in &remove_ops {
        let start_idx = start.saturating_sub(1);
        let end_idx = (*end).min(lines.len());
        if start_idx < lines.len() {
            // Also remove a trailing blank line if present (matches existing behavior)
            let remove_end = if end_idx < lines.len() && lines[end_idx].trim().is_empty() {
                end_idx + 1
            } else {
                end_idx
            };
            lines.drain(start_idx..remove_end);
        }
    }

    // Collapse consecutive blank lines left by removals
    if !remove_ops.is_empty() {
        let mut collapsed = Vec::with_capacity(lines.len());
        let mut prev_blank = false;
        for line in &lines {
            let is_blank = line.trim().is_empty();
            if is_blank && prev_blank {
                continue;
            }
            collapsed.push(line.clone());
            prev_blank = is_blank;
        }
        lines = collapsed;
    }

    // 4. Apply InsertLines — sort bottom-to-top to avoid drift
    insert_ops.sort_by(|a, b| b.0.cmp(&a.0));
    for (target_line, code) in &insert_ops {
        let idx = target_line.saturating_sub(1).min(lines.len());
        // Split the code into individual lines and insert them
        let code_lines: Vec<String> = code.lines().map(String::from).collect();
        for (offset, code_line) in code_lines.iter().enumerate() {
            let insert_at = (idx + offset).min(lines.len());
            lines.insert(insert_at, code_line.clone());
        }
    }

    let mut result = lines.join("\n");
    if had_trailing_newline && !result.ends_with('\n') {
        result.push('\n');
    }

    Ok(result)
}

/// Remove a symbol from `pub use { ... }` re-export blocks.
///
/// This is extracted from the existing `remove_from_pub_use_block()` in
/// `auto/apply.rs` — same logic, operating on `Vec<String>` lines.
fn remove_from_reexport_block(lines: &mut Vec<String>, fn_name: &str) {
    let word_pattern = format!(r"\b{}\b", regex::escape(fn_name));
    let word_re = match regex::Regex::new(&word_pattern) {
        Ok(re) => re,
        Err(_) => return,
    };

    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim().to_string();

        // Single-line: pub use module::{a, b, c};
        if trimmed.starts_with("pub use") && trimmed.contains('{') && trimmed.contains('}') {
            if word_re.is_match(&trimmed) {
                let cleaned = word_re
                    .replace(&lines[i], "")
                    .to_string()
                    .replace(", ,", ",")
                    .replace("{, ", "{ ")
                    .replace("{,", "{")
                    .replace(", }", " }")
                    .replace(",}", "}");

                if let Some(start) = cleaned.find('{') {
                    if let Some(end) = cleaned.find('}') {
                        let inside = cleaned[start + 1..end].trim();
                        if inside.is_empty() {
                            lines.remove(i);
                            continue;
                        }
                    }
                }
                lines[i] = cleaned;
            }
            i += 1;
            continue;
        }

        // Multi-line block: pub use module::{
        if trimmed.starts_with("pub use") && trimmed.contains('{') && !trimmed.contains('}') {
            let block_start = i;
            i += 1;
            while i < lines.len() {
                let inner = lines[i].trim().to_string();
                if word_re.is_match(&inner) {
                    let cleaned = word_re
                        .replace(&inner, "")
                        .to_string()
                        .replace(", ,", ",")
                        .trim()
                        .to_string();
                    let cleaned = cleaned
                        .trim_start_matches(',')
                        .trim_end_matches(',')
                        .trim()
                        .to_string();
                    if cleaned.is_empty() {
                        lines.remove(i);
                        continue;
                    }
                    let needs_trailing_comma = !cleaned.contains('}');
                    let final_cleaned = if needs_trailing_comma && !cleaned.ends_with(',') {
                        format!("{},", cleaned)
                    } else {
                        cleaned
                    };
                    let indent = " ".repeat(lines[i].len() - lines[i].trim_start().len());
                    lines[i] = format!("{}{}", indent, final_cleaned);
                }
                if lines[i].trim().contains('}') {
                    break;
                }
                i += 1;
            }

            let block_end = i.min(lines.len().saturating_sub(1));
            let has_items = (block_start + 1..block_end)
                .any(|j| !lines[j].trim().is_empty() && lines[j].trim() != ",");
            if !has_items {
                for _ in block_start..=block_end.min(lines.len().saturating_sub(1)) {
                    if block_start < lines.len() {
                        lines.remove(block_start);
                    }
                }
                i = block_start;
                continue;
            }
        }

        i += 1;
    }
}

/// Apply a list of `TaggedEditOp`s to the filesystem.
///
/// Groups ops by file, reads each file once, applies all ops, writes once.
/// `MoveFile` and `CreateFile` ops are handled separately after content edits.
pub fn apply_edit_ops(ops: &[TaggedEditOp], root: &Path) -> Result<ApplyReport> {
    let mut report = ApplyReport::default();

    // Separate file-level ops from content-level ops
    let mut content_ops_by_file: HashMap<&str, Vec<&EditOp>> = HashMap::new();
    let mut move_ops: Vec<(&str, &str)> = Vec::new();
    let mut create_ops: Vec<(&str, &str)> = Vec::new();

    for tagged in ops {
        match &tagged.op {
            EditOp::ReplaceText { file, .. }
            | EditOp::RemoveLines { file, .. }
            | EditOp::InsertLines { file, .. } => {
                content_ops_by_file
                    .entry(file.as_str())
                    .or_default()
                    .push(&tagged.op);
            }
            EditOp::MoveFile { from, to } => {
                move_ops.push((from.as_str(), to.as_str()));
            }
            EditOp::CreateFile { file, content } => {
                create_ops.push((file.as_str(), content.as_str()));
            }
        }
    }

    // Apply content edits: read → transform → write per file
    for (file, file_ops) in &content_ops_by_file {
        let abs_path = root.join(file);
        let content = match std::fs::read_to_string(&abs_path) {
            Ok(c) => c,
            Err(e) => {
                report.errors.push(ApplyError {
                    file: file.to_string(),
                    message: format!("Failed to read: {}", e),
                });
                report.ops_skipped += file_ops.len();
                continue;
            }
        };

        let language = Language::from_path(&abs_path);
        let op_refs: Vec<&EditOp> = file_ops.iter().copied().collect();

        match apply_edit_ops_to_content(&content, &op_refs, &language) {
            Ok(modified) => {
                if modified != content {
                    if let Err(e) = std::fs::write(&abs_path, &modified) {
                        report.errors.push(ApplyError {
                            file: file.to_string(),
                            message: format!("Failed to write: {}", e),
                        });
                        report.ops_skipped += file_ops.len();
                        continue;
                    }
                    report.files_modified += 1;
                }
                report.ops_applied += file_ops.len();
            }
            Err(msg) => {
                report.errors.push(ApplyError {
                    file: file.to_string(),
                    message: msg,
                });
                report.ops_skipped += file_ops.len();
            }
        }
    }

    // Apply MoveFile ops
    for (from, to) in &move_ops {
        let from_abs = root.join(from);
        let to_abs = root.join(to);

        if !from_abs.exists() {
            report.errors.push(ApplyError {
                file: from.to_string(),
                message: format!("Source file does not exist: {}", from),
            });
            report.ops_skipped += 1;
            continue;
        }

        if to_abs.exists() {
            report.errors.push(ApplyError {
                file: to.to_string(),
                message: format!("Destination already exists: {}", to),
            });
            report.ops_skipped += 1;
            continue;
        }

        if let Some(parent) = to_abs.parent() {
            if !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    report.errors.push(ApplyError {
                        file: to.to_string(),
                        message: format!("Failed to create directory: {}", e),
                    });
                    report.ops_skipped += 1;
                    continue;
                }
            }
        }

        match std::fs::rename(&from_abs, &to_abs) {
            Ok(_) => {
                report.files_moved += 1;
                report.ops_applied += 1;
            }
            Err(e) => {
                report.errors.push(ApplyError {
                    file: from.to_string(),
                    message: format!("Move failed: {}", e),
                });
                report.ops_skipped += 1;
            }
        }
    }

    // Apply CreateFile ops
    for (file, file_content) in &create_ops {
        let abs_path = root.join(file);

        if abs_path.exists() {
            report.errors.push(ApplyError {
                file: file.to_string(),
                message: format!("File already exists: {}", file),
            });
            report.ops_skipped += 1;
            continue;
        }

        if let Some(parent) = abs_path.parent() {
            if !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    report.errors.push(ApplyError {
                        file: file.to_string(),
                        message: format!("Failed to create directory: {}", e),
                    });
                    report.ops_skipped += 1;
                    continue;
                }
            }
        }

        match std::fs::write(&abs_path, file_content) {
            Ok(_) => {
                report.files_created += 1;
                report.ops_applied += 1;
            }
            Err(e) => {
                report.errors.push(ApplyError {
                    file: file.to_string(),
                    message: format!("Failed to create file: {}", e),
                });
                report.ops_skipped += 1;
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_audit::AuditFinding;
    use crate::core::refactor::auto::{Fix, Insertion, InsertionKind, RefactorPrimitive};

    fn test_insertion(kind: InsertionKind) -> Insertion {
        Insertion {
            primitive: None,
            kind,
            finding: AuditFinding::UnreferencedExport,
            manual_only: false,
            auto_apply: false,
            blocked_reason: None,
            code: String::new(),
            description: "test".to_string(),
        }
    }

    #[test]
    fn visibility_change_maps_to_replace_text() {
        let ins = test_insertion(InsertionKind::VisibilityChange {
            line: 10,
            from: "pub fn".to_string(),
            to: "pub(crate) fn".to_string(),
        });
        let tagged = from_insertion(&ins, "src/lib.rs");
        assert!(matches!(tagged.op, EditOp::ReplaceText { line: 10, .. }));
    }

    #[test]
    fn line_replacement_maps_to_replace_text() {
        let ins = test_insertion(InsertionKind::LineReplacement {
            line: 5,
            old_text: "old_name".to_string(),
            new_text: "new_name".to_string(),
        });
        let tagged = from_insertion(&ins, "src/lib.rs");
        assert!(matches!(tagged.op, EditOp::ReplaceText { line: 5, .. }));
    }

    #[test]
    fn function_removal_maps_to_remove_lines() {
        let ins = test_insertion(InsertionKind::FunctionRemoval {
            start_line: 10,
            end_line: 20,
        });
        let tagged = from_insertion(&ins, "src/lib.rs");
        assert!(matches!(
            tagged.op,
            EditOp::RemoveLines {
                start_line: 10,
                end_line: 20,
                ..
            }
        ));
    }

    #[test]
    fn doc_line_removal_maps_to_remove_single_line() {
        let ins = test_insertion(InsertionKind::DocLineRemoval { line: 42 });
        let tagged = from_insertion(&ins, "docs/api.md");
        assert!(matches!(
            tagged.op,
            EditOp::RemoveLines {
                start_line: 42,
                end_line: 42,
                ..
            }
        ));
    }

    #[test]
    fn import_add_maps_to_insert_lines() {
        let mut ins = test_insertion(InsertionKind::ImportAdd);
        ins.code = "use crate::foo;".to_string();
        let tagged = from_insertion(&ins, "src/lib.rs");
        assert!(matches!(
            tagged.op,
            EditOp::InsertLines {
                anchor: InsertAnchor::AfterImports,
                ..
            }
        ));
    }

    #[test]
    fn file_move_maps_to_move_file() {
        let ins = test_insertion(InsertionKind::FileMove {
            from: "tests/old_test.rs".to_string(),
            to: "tests/new_test.rs".to_string(),
        });
        let tagged = from_insertion(&ins, "tests/old_test.rs");
        assert!(matches!(tagged.op, EditOp::MoveFile { .. }));
    }

    #[test]
    fn primitive_tag_is_preserved() {
        let mut ins = test_insertion(InsertionKind::FunctionRemoval {
            start_line: 1,
            end_line: 5,
        });
        ins.primitive = Some(RefactorPrimitive::RemoveOrphanedTest);
        let tagged = from_insertion(&ins, "src/lib.rs");
        assert_eq!(
            tagged.primitive,
            Some(RefactorPrimitive::RemoveOrphanedTest)
        );
    }

    #[test]
    fn manual_only_is_preserved() {
        let mut ins = test_insertion(InsertionKind::DocLineRemoval { line: 1 });
        ins.manual_only = true;
        let tagged = from_insertion(&ins, "src/lib.rs");
        assert!(tagged.manual_only);
    }

    #[test]
    fn fix_to_edit_ops_produces_one_per_insertion() {
        let fix = Fix {
            file: "src/lib.rs".to_string(),
            required_methods: vec![],
            required_registrations: vec![],
            insertions: vec![
                test_insertion(InsertionKind::FunctionRemoval {
                    start_line: 1,
                    end_line: 5,
                }),
                test_insertion(InsertionKind::ImportAdd),
            ],
            applied: false,
        };
        let ops = fix_to_edit_ops(&fix);
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0].op, EditOp::RemoveLines { .. }));
        assert!(matches!(ops[1].op, EditOp::InsertLines { .. }));
    }

    // ── Manual command conversion tests ───────────────────────────────

    #[test]
    fn propagate_edit_maps_to_insert_at_line() {
        use crate::core::refactor::propagate::PropagateEdit;

        let edit = PropagateEdit {
            file: "src/config.rs".to_string(),
            line: 42,
            insert_text: "    new_field: Default::default(),".to_string(),
            description: "Insert missing field `new_field`".to_string(),
        };
        let tagged = propagate_edit_to_edit_op(&edit);
        assert!(matches!(
            tagged.op,
            EditOp::InsertLines {
                anchor: InsertAnchor::AtLine { line: 42 },
                ..
            }
        ));
        assert!(tagged.description.contains("new_field"));
        assert!(tagged.primitive.is_none());
        assert!(tagged.finding.is_none());
    }

    #[test]
    fn propagate_result_produces_one_op_per_edit() {
        use crate::core::refactor::propagate::{PropagateEdit, PropagateResult};

        let result = PropagateResult {
            struct_name: "Config".to_string(),
            definition_file: "src/config.rs".to_string(),
            fields: vec![],
            files_scanned: 10,
            instantiations_found: 3,
            instantiations_needing_fix: 2,
            edits: vec![
                PropagateEdit {
                    file: "src/a.rs".to_string(),
                    line: 10,
                    insert_text: "    field: 0,".to_string(),
                    description: "Insert `field`".to_string(),
                },
                PropagateEdit {
                    file: "src/b.rs".to_string(),
                    line: 20,
                    insert_text: "    field: 0,".to_string(),
                    description: "Insert `field`".to_string(),
                },
            ],
            applied: false,
        };
        let ops = propagate_result_to_edit_ops(&result);
        assert_eq!(ops.len(), 2);
        assert!(matches!(
            &ops[0].op,
            EditOp::InsertLines {
                file,
                anchor: InsertAnchor::AtLine { line: 10 },
                ..
            } if file == "src/a.rs"
        ));
        assert!(matches!(
            &ops[1].op,
            EditOp::InsertLines {
                file,
                anchor: InsertAnchor::AtLine { line: 20 },
                ..
            } if file == "src/b.rs"
        ));
    }

    #[test]
    fn transform_match_maps_to_replace_text() {
        use crate::core::refactor::transform::TransformMatch;

        let m = TransformMatch {
            file: "src/lib.rs".to_string(),
            line: 15,
            before: "old_name".to_string(),
            after: "new_name".to_string(),
        };
        let tagged = transform_match_to_edit_op(&m);
        assert!(matches!(tagged.op, EditOp::ReplaceText { line: 15, .. }));
        assert!(tagged.description.contains("old_name"));
        assert!(tagged.description.contains("new_name"));
    }

    #[test]
    fn transform_result_flattens_rules_into_ops() {
        use crate::core::refactor::transform::{RuleResult, TransformMatch, TransformResult};

        let result = TransformResult {
            name: "test".to_string(),
            rules: vec![
                RuleResult {
                    id: "rule1".to_string(),
                    description: "Rename foo".to_string(),
                    matches: vec![TransformMatch {
                        file: "src/a.rs".to_string(),
                        line: 1,
                        before: "foo".to_string(),
                        after: "bar".to_string(),
                    }],
                    replacement_count: 1,
                },
                RuleResult {
                    id: "rule2".to_string(),
                    description: "Rename baz".to_string(),
                    matches: vec![
                        TransformMatch {
                            file: "src/b.rs".to_string(),
                            line: 5,
                            before: "baz".to_string(),
                            after: "qux".to_string(),
                        },
                        TransformMatch {
                            file: "src/c.rs".to_string(),
                            line: 10,
                            before: "baz".to_string(),
                            after: "qux".to_string(),
                        },
                    ],
                    replacement_count: 2,
                },
            ],
            total_replacements: 3,
            total_files: 3,
            written: false,
        };
        let ops = transform_result_to_edit_ops(&result);
        assert_eq!(ops.len(), 3);
        // First op is from rule1
        assert!(ops[0].description.contains("Rename foo"));
        // Second and third are from rule2
        assert!(ops[1].description.contains("Rename baz"));
        assert!(ops[2].description.contains("Rename baz"));
    }

    // ── Apply logic tests ─────────────────────────────────────────────

    #[test]
    fn resolve_anchor_at_line() {
        let content = "line1\nline2\nline3\n";
        let anchor = InsertAnchor::AtLine { line: 2 };
        assert_eq!(resolve_anchor(content, &anchor, &Language::Rust), Some(2));
    }

    #[test]
    fn resolve_anchor_file_top() {
        let content = "use foo;\nfn main() {}\n";
        assert_eq!(
            resolve_anchor(content, &InsertAnchor::FileTop, &Language::Rust),
            Some(1)
        );
    }

    #[test]
    fn resolve_anchor_file_end() {
        let content = "line1\nline2\nline3\n";
        assert_eq!(
            resolve_anchor(content, &InsertAnchor::FileEnd, &Language::Rust),
            Some(4)
        );
    }

    #[test]
    fn resolve_anchor_after_imports_rust() {
        let content = "use std::io;\nuse std::path::Path;\n\nfn main() {}\n";
        let resolved = resolve_anchor(content, &InsertAnchor::AfterImports, &Language::Rust);
        assert_eq!(resolved, Some(3)); // After line 2 (last import)
    }

    #[test]
    fn resolve_anchor_after_imports_php() {
        let content = "<?php\n\nnamespace App;\n\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nclass MyClass {}\n";
        let resolved = resolve_anchor(content, &InsertAnchor::AfterImports, &Language::Php);
        assert_eq!(resolved, Some(7)); // After line 6 (last use)
    }

    #[test]
    fn resolve_anchor_after_imports_no_imports_rust() {
        let content = "// header comment\n\nfn main() {}\n";
        let resolved = resolve_anchor(content, &InsertAnchor::AfterImports, &Language::Rust);
        // Should insert after header comments
        assert!(resolved.is_some());
        assert!(resolved.unwrap() <= 3);
    }

    #[test]
    fn resolve_anchor_after_class_open_php() {
        let content = "<?php\n\nclass MyClass {\n    private $x;\n}\n";
        let resolved = resolve_anchor(content, &InsertAnchor::AfterClassOpen, &Language::Php);
        assert_eq!(resolved, Some(4)); // Line after "class MyClass {"
    }

    #[test]
    fn resolve_anchor_in_constructor_php() {
        let content =
            "<?php\n\nclass MyClass {\n    function __construct() {\n        $this->x = 1;\n    }\n}\n";
        let resolved = resolve_anchor(content, &InsertAnchor::InConstructor, &Language::Php);
        assert_eq!(resolved, Some(5)); // Line after constructor opening brace
    }

    #[test]
    fn resolve_anchor_before_closing_brace() {
        let content = "struct Foo {\n    x: i32,\n}\n\nimpl Foo {\n    fn bar() {}\n}\n";
        let resolved = resolve_anchor(content, &InsertAnchor::BeforeClosingBrace, &Language::Rust);
        assert_eq!(resolved, Some(7)); // The line with the last `}`
    }

    #[test]
    fn resolve_anchor_type_declaration_rust() {
        let content = "use std::io;\n\npub struct Config {\n    pub name: String,\n}\n";
        let resolved = resolve_anchor(content, &InsertAnchor::TypeDeclaration, &Language::Rust);
        assert_eq!(resolved, Some(3)); // "pub struct Config {"
    }

    #[test]
    fn resolve_anchor_type_declaration_php() {
        let content = "<?php\n\nnamespace App;\n\nclass UserService {\n    private $db;\n}\n";
        let resolved = resolve_anchor(content, &InsertAnchor::TypeDeclaration, &Language::Php);
        assert_eq!(resolved, Some(5)); // "class UserService {"
    }

    #[test]
    fn resolve_anchor_reexport_returns_none() {
        let content = "pub use module::{foo, bar};\n";
        let anchor = InsertAnchor::RemoveFromReexport {
            symbol: "foo".to_string(),
        };
        assert_eq!(resolve_anchor(content, &anchor, &Language::Rust), None);
    }

    // ── apply_edit_ops_to_content tests ───────────────────────────────

    #[test]
    fn apply_replace_text() {
        let content = "fn old_name() {}\nfn other() {}\n";
        let op = EditOp::ReplaceText {
            file: "test.rs".to_string(),
            line: 1,
            old_text: "old_name".to_string(),
            new_text: "new_name".to_string(),
        };
        let result = apply_edit_ops_to_content(content, &[&op], &Language::Rust).unwrap();
        assert!(result.contains("fn new_name() {}"));
        assert!(result.contains("fn other() {}"));
    }

    #[test]
    fn apply_replace_text_not_found_errors() {
        let content = "fn something() {}\n";
        let op = EditOp::ReplaceText {
            file: "test.rs".to_string(),
            line: 1,
            old_text: "nonexistent".to_string(),
            new_text: "replacement".to_string(),
        };
        let result = apply_edit_ops_to_content(content, &[&op], &Language::Rust);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found on line"));
    }

    #[test]
    fn apply_replace_text_line_out_of_range() {
        let content = "fn something() {}\n";
        let op = EditOp::ReplaceText {
            file: "test.rs".to_string(),
            line: 99,
            old_text: "something".to_string(),
            new_text: "other".to_string(),
        };
        let result = apply_edit_ops_to_content(content, &[&op], &Language::Rust);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("out of range"));
    }

    #[test]
    fn apply_remove_lines() {
        let content = "line1\nline2\nline3\nline4\nline5\n";
        let op = EditOp::RemoveLines {
            file: "test.rs".to_string(),
            start_line: 2,
            end_line: 3,
        };
        let result = apply_edit_ops_to_content(content, &[&op], &Language::Rust).unwrap();
        assert!(result.contains("line1"));
        assert!(!result.contains("line2"));
        assert!(!result.contains("line3"));
        assert!(result.contains("line4"));
    }

    #[test]
    fn apply_insert_lines_at_line() {
        let content = "line1\nline2\nline3\n";
        let op = EditOp::InsertLines {
            file: "test.rs".to_string(),
            anchor: InsertAnchor::AtLine { line: 2 },
            code: "inserted_line".to_string(),
        };
        let result = apply_edit_ops_to_content(content, &[&op], &Language::Rust).unwrap();
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines[0], "line1");
        assert_eq!(lines[1], "inserted_line");
        assert_eq!(lines[2], "line2");
        assert_eq!(lines[3], "line3");
    }

    #[test]
    fn apply_insert_lines_after_imports() {
        let content = "use std::io;\nuse std::path::Path;\n\nfn main() {}\n";
        let op = EditOp::InsertLines {
            file: "test.rs".to_string(),
            anchor: InsertAnchor::AfterImports,
            code: "use crate::config;".to_string(),
        };
        let result = apply_edit_ops_to_content(content, &[&op], &Language::Rust).unwrap();
        assert!(result.contains("use crate::config;"));
        // The new import should appear after existing imports
        let config_pos = result.find("use crate::config;").unwrap();
        let path_pos = result.find("use std::path::Path;").unwrap();
        assert!(config_pos > path_pos);
    }

    #[test]
    fn apply_insert_lines_file_end() {
        let content = "line1\nline2\n";
        let op = EditOp::InsertLines {
            file: "test.rs".to_string(),
            anchor: InsertAnchor::FileEnd,
            code: "// end of file".to_string(),
        };
        let result = apply_edit_ops_to_content(content, &[&op], &Language::Rust).unwrap();
        assert!(result.ends_with("// end of file\n"));
    }

    #[test]
    fn apply_reexport_removal() {
        let content = "pub use sources::{alpha, beta, gamma};\n\nfn main() {}\n";
        let op = EditOp::InsertLines {
            file: "test.rs".to_string(),
            anchor: InsertAnchor::RemoveFromReexport {
                symbol: "beta".to_string(),
            },
            code: String::new(),
        };
        let result = apply_edit_ops_to_content(content, &[&op], &Language::Rust).unwrap();
        assert!(!result.contains("beta"));
        assert!(result.contains("alpha"));
        assert!(result.contains("gamma"));
    }

    #[test]
    fn apply_multiple_ops_same_file() {
        let content = "use std::io;\n\npub fn old_func() {\n    let x = 1;\n    let y = 2;\n}\n\nfn other() {}\n";
        let replace_op = EditOp::ReplaceText {
            file: "test.rs".to_string(),
            line: 3,
            old_text: "old_func".to_string(),
            new_text: "new_func".to_string(),
        };
        let insert_op = EditOp::InsertLines {
            file: "test.rs".to_string(),
            anchor: InsertAnchor::AfterImports,
            code: "use crate::config;".to_string(),
        };
        let ops: Vec<&EditOp> = vec![&replace_op, &insert_op];
        let result = apply_edit_ops_to_content(content, &ops, &Language::Rust).unwrap();
        assert!(result.contains("new_func"));
        assert!(result.contains("use crate::config;"));
    }

    #[test]
    fn apply_multiple_removals_bottom_to_top() {
        // Removing lines 7-8 and lines 3-4 should work correctly
        // regardless of order — they should be sorted bottom-to-top internally.
        let content = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\n";
        let op1 = EditOp::RemoveLines {
            file: "test.rs".to_string(),
            start_line: 3,
            end_line: 4,
        };
        let op2 = EditOp::RemoveLines {
            file: "test.rs".to_string(),
            start_line: 7,
            end_line: 8,
        };
        // Pass in "wrong" order — op2 should still be applied first (higher lines)
        let result = apply_edit_ops_to_content(content, &[&op1, &op2], &Language::Rust).unwrap();
        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
        assert!(!result.contains("line3"));
        assert!(!result.contains("line4"));
        assert!(result.contains("line5"));
        assert!(result.contains("line6"));
        assert!(!result.contains("line7"));
        assert!(!result.contains("line8"));
        assert!(result.contains("line9"));
    }

    #[test]
    fn apply_move_file_is_noop_for_content() {
        let content = "fn main() {}\n";
        let op = EditOp::MoveFile {
            from: "old.rs".to_string(),
            to: "new.rs".to_string(),
        };
        let result = apply_edit_ops_to_content(content, &[&op], &Language::Rust).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn apply_create_file_is_noop_for_content() {
        let content = "fn main() {}\n";
        let op = EditOp::CreateFile {
            file: "new.rs".to_string(),
            content: "fn new_fn() {}".to_string(),
        };
        let result = apply_edit_ops_to_content(content, &[&op], &Language::Rust).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn apply_preserves_trailing_newline() {
        let content = "line1\nline2\n";
        let op = EditOp::ReplaceText {
            file: "test.rs".to_string(),
            line: 1,
            old_text: "line1".to_string(),
            new_text: "replaced".to_string(),
        };
        let result = apply_edit_ops_to_content(content, &[&op], &Language::Rust).unwrap();
        assert!(result.ends_with('\n'));
    }

    #[test]
    fn apply_insert_multiline_code() {
        let content = "fn main() {\n}\n";
        let op = EditOp::InsertLines {
            file: "test.rs".to_string(),
            anchor: InsertAnchor::AtLine { line: 2 },
            code: "    let x = 1;\n    let y = 2;".to_string(),
        };
        let result = apply_edit_ops_to_content(content, &[&op], &Language::Rust).unwrap();
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines[0], "fn main() {");
        assert_eq!(lines[1], "    let x = 1;");
        assert_eq!(lines[2], "    let y = 2;");
        assert_eq!(lines[3], "}");
    }

    #[test]
    fn apply_edit_ops_filesystem() {
        use std::fs;

        let tmp = std::env::temp_dir().join("homeboy_edit_op_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Create a test file
        fs::write(
            tmp.join("test.rs"),
            "use std::io;\n\npub fn old_name() {}\n",
        )
        .unwrap();

        let ops = vec![
            TaggedEditOp {
                op: EditOp::ReplaceText {
                    file: "test.rs".to_string(),
                    line: 3,
                    old_text: "old_name".to_string(),
                    new_text: "new_name".to_string(),
                },
                primitive: None,
                finding: None,
                description: "Rename function".to_string(),
                manual_only: false,
            },
            TaggedEditOp {
                op: EditOp::CreateFile {
                    file: "new_file.rs".to_string(),
                    content: "// new file\npub fn created() {}\n".to_string(),
                },
                primitive: None,
                finding: None,
                description: "Create new file".to_string(),
                manual_only: false,
            },
        ];

        let report = apply_edit_ops(&ops, &tmp).unwrap();
        assert_eq!(report.files_modified, 1);
        assert_eq!(report.files_created, 1);
        assert_eq!(report.ops_applied, 2);
        assert!(report.errors.is_empty());

        // Verify file content
        let modified = fs::read_to_string(tmp.join("test.rs")).unwrap();
        assert!(modified.contains("new_name"));
        assert!(!modified.contains("old_name"));

        let created = fs::read_to_string(tmp.join("new_file.rs")).unwrap();
        assert!(created.contains("pub fn created()"));

        // Clean up
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn apply_edit_ops_move_file() {
        use std::fs;

        let tmp = std::env::temp_dir().join("homeboy_edit_op_move_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        fs::write(tmp.join("old.rs"), "fn moved() {}\n").unwrap();

        let ops = vec![TaggedEditOp {
            op: EditOp::MoveFile {
                from: "old.rs".to_string(),
                to: "subdir/new.rs".to_string(),
            },
            primitive: None,
            finding: None,
            description: "Move file".to_string(),
            manual_only: false,
        }];

        let report = apply_edit_ops(&ops, &tmp).unwrap();
        assert_eq!(report.files_moved, 1);
        assert_eq!(report.ops_applied, 1);
        assert!(!tmp.join("old.rs").exists());
        assert!(tmp.join("subdir/new.rs").exists());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn apply_edit_ops_reports_missing_file() {
        let tmp = std::env::temp_dir().join("homeboy_edit_op_missing_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let ops = vec![TaggedEditOp {
            op: EditOp::ReplaceText {
                file: "nonexistent.rs".to_string(),
                line: 1,
                old_text: "foo".to_string(),
                new_text: "bar".to_string(),
            },
            primitive: None,
            finding: None,
            description: "Edit missing file".to_string(),
            manual_only: false,
        }];

        let report = apply_edit_ops(&ops, &tmp).unwrap();
        assert_eq!(report.ops_skipped, 1);
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].message.contains("Failed to read"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_anchor_in_constructor_rust() {
        let content = "pub struct Foo;\n\nimpl Foo {\n    pub fn new(x: i32) -> Self {\n        Foo\n    }\n}\n";
        let resolved = resolve_anchor(content, &InsertAnchor::InConstructor, &Language::Rust);
        assert_eq!(resolved, Some(5)); // Line after "pub fn new(x: i32) -> Self {"
    }

    #[test]
    fn resolve_anchor_after_class_open_rust() {
        let content = "pub struct Config {\n    pub name: String,\n}\n";
        let resolved = resolve_anchor(content, &InsertAnchor::AfterClassOpen, &Language::Rust);
        assert_eq!(resolved, Some(2)); // Line after "pub struct Config {"
    }

    #[test]
    fn apply_combined_remove_and_insert() {
        // Remove a function, then insert an import — tests that both ops
        // apply correctly with bottom-to-top ordering.
        let content = "\
use std::io;

fn to_remove() {
    println!(\"remove me\");
}

fn keep_me() {}
";
        let remove_op = EditOp::RemoveLines {
            file: "test.rs".to_string(),
            start_line: 3,
            end_line: 5,
        };
        let insert_op = EditOp::InsertLines {
            file: "test.rs".to_string(),
            anchor: InsertAnchor::AfterImports,
            code: "use crate::new_dep;".to_string(),
        };
        let result =
            apply_edit_ops_to_content(content, &[&remove_op, &insert_op], &Language::Rust)
                .unwrap();
        assert!(!result.contains("to_remove"));
        assert!(result.contains("use crate::new_dep;"));
        assert!(result.contains("fn keep_me()"));
    }
}
