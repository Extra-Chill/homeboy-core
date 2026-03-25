//! rename_scope — extracted from mod.rs.

use crate::error::{Error, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crate::core::refactor::*;


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
