//! types — extracted from mod.rs.

use serde::Serialize;
use super::super::*;


/// A case variant of a rename term.
#[derive(Debug, Clone, Serialize)]
pub struct CaseVariant {
    pub from: String,
    pub to: String,
    pub label: String,
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

/// A warning about a potential collision or issue.
#[derive(Debug, Clone, Serialize)]
pub struct RenameWarning {
    /// Warning category.
    pub kind: String,
    /// File path relative to root.
    pub file: String,
    /// Line number (if applicable).
    pub line: Option<usize>,
    /// Human-readable description.
    pub message: String,
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
    /// Warnings about potential collisions or issues.
    pub warnings: Vec<RenameWarning>,
    /// Total reference count.
    pub total_references: usize,
    /// Total files affected.
    pub total_files: usize,
    /// Whether changes were written to disk.
    pub applied: bool,
}
use crate::error::{Error, Result};
use super::new;
use super::from_str;
use super::literal;

impl RenameScope {
    #[allow(clippy::should_implement_trait)]
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

impl RenameSpec {
    /// Create a rename spec, auto-generating cross-separator case variants.
    ///
    /// Splits the `from` and `to` terms into constituent words, then generates
    /// all standard naming convention variants:
    ///
    /// - `kebab-case` (e.g., `data-machine-agent`)
    /// - `snake_case` (e.g., `data_machine_agent`)
    /// - `UPPER_SNAKE` (e.g., `DATA_MACHINE_AGENT`)
    /// - `PascalCase` (e.g., `DataMachineAgent`)
    /// - `camelCase` (e.g., `dataMachineAgent`)
    /// - `Display Name` (e.g., `Data Machine Agent`)
    /// - Plus plural forms of each
    ///
    /// This means a single `--from wp-agent --to data-machine-agent` will also
    /// match and replace `wp_agent`, `WP_AGENT`, `WPAgent`, `wpAgent`, `WP Agent`,
    /// and all their plurals.
    pub fn new(from: &str, to: &str, scope: RenameScope) -> Self {
        let from_words = split_words(from);
        let to_words = split_words(to);

        let mut variants = Vec::new();

        // If word splitting produced words, generate cross-separator variants.
        // If it produced a single word (e.g., "widget"), the joins all collapse
        // to the same thing, and dedup handles it naturally.
        if !from_words.is_empty() && !to_words.is_empty() {
            // Singular forms — all naming conventions
            let join_fns: [fn(&[String]) -> String; 6] = [
                join_kebab,
                join_snake,
                join_upper_snake,
                join_pascal,
                join_camel,
                join_display,
            ];
            let labels = [
                "kebab",
                "snake_case",
                "UPPER_SNAKE",
                "PascalCase",
                "camelCase",
                "Display Name",
            ];

            for (label, join_fn) in labels.iter().zip(join_fns.iter()) {
                variants.push(CaseVariant {
                    from: join_fn(&from_words),
                    to: join_fn(&to_words),
                    label: label.to_string(),
                });
            }

            // Plural forms — pluralize the last word, then generate all conventions
            let mut from_words_plural = from_words.clone();
            let mut to_words_plural = to_words.clone();
            if let Some(last) = from_words_plural.last_mut() {
                *last = pluralize(last);
            }
            if let Some(last) = to_words_plural.last_mut() {
                *last = pluralize(last);
            }

            for (label, join_fn) in labels.iter().zip(join_fns.iter()) {
                variants.push(CaseVariant {
                    from: join_fn(&from_words_plural),
                    to: join_fn(&to_words_plural),
                    label: format!("plural {}", label),
                });
            }
        } else {
            // Fallback for empty/unparseable input — use the original simple logic
            variants.push(CaseVariant {
                from: from.to_lowercase(),
                to: to.to_lowercase(),
                label: "lowercase".to_string(),
            });
        }

        // Deduplicate — remove variants where from matches a previous one.
        // Sort by from length descending first so longer matches take priority.
        variants.sort_by(|a, b| b.from.len().cmp(&a.from.len()));
        let mut seen = std::collections::HashSet::new();
        variants.retain(|v| seen.insert(v.from.clone()));

        RenameSpec {
            from: from.to_string(),
            to: to.to_string(),
            scope,
            variants,
            literal: false,
        }
    }

    /// Create a literal rename spec — exact string match, no boundary detection,
    /// no case variant generation. The `from` string is matched as-is.
    pub fn literal(from: &str, to: &str, scope: RenameScope) -> Self {
        let variants = vec![CaseVariant {
            from: from.to_string(),
            to: to.to_string(),
            label: "literal".to_string(),
        }];

        RenameSpec {
            from: from.to_string(),
            to: to.to_string(),
            scope,
            variants,
            literal: true,
        }
    }
}
