//! Structural refactoring — rename concepts across a codebase.
//!
//! Walks source files, finds all references to a term (with word-boundary matching
//! and case-variant awareness), generates edits, and optionally applies them.

mod rename;

pub use rename::{
    find_references, generate_renames, apply_renames, CaseVariant,
    FileEdit, FileRename, Reference, RenameResult, RenameScope, RenameSpec,
};
