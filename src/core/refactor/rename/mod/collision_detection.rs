//! collision_detection — extracted from mod.rs.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use super::super::*;


/// Count leading spaces on a line.
pub(crate) fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Extract the field/identifier name from a struct field or variable declaration line.
/// Returns the identifier if the line looks like a field declaration.
///
/// Matches patterns like:
/// - `pub field_name: Type,`
/// - `field_name: Type,`
/// - `pub(crate) field_name: Type,`
/// - `let field_name = ...`
/// - `fn field_name(...`
pub(crate) fn extract_field_identifier(trimmed: &str) -> Option<String> {
    // Skip attributes, comments, empty lines
    if trimmed.starts_with('#')
        || trimmed.starts_with("//")
        || trimmed.starts_with("/*")
        || trimmed.is_empty()
    {
        return None;
    }

    // Strip visibility modifiers
    let rest = trimmed
        .strip_prefix("pub(crate) ")
        .or_else(|| trimmed.strip_prefix("pub(super) "))
        .or_else(|| trimmed.strip_prefix("pub "))
        .unwrap_or(trimmed);

    // Strip let/fn/const/static
    let rest = rest
        .strip_prefix("let mut ")
        .or_else(|| rest.strip_prefix("let "))
        .or_else(|| rest.strip_prefix("fn "))
        .or_else(|| rest.strip_prefix("const "))
        .or_else(|| rest.strip_prefix("static "))
        .unwrap_or(rest);

    // Extract identifier (alphanumeric + underscore until : or ( or = or space)
    let ident: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();

    if ident.is_empty() {
        return None;
    }

    // Must be followed by : or ( or = or < (type params) to be an identifier
    let after = &rest[ident.len()..].trim_start();
    if after.starts_with(':')
        || after.starts_with('(')
        || after.starts_with('=')
        || after.starts_with('<')
    {
        Some(ident)
    } else {
        None
    }
}

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
    renames.sort_by(|a, b| {
        b.from
            .matches('/')
            .count()
            .cmp(&a.from.matches('/').count())
    });

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
