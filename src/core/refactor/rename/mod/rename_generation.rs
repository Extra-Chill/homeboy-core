//! rename_generation — extracted from mod.rs.

use std::path::{Path, PathBuf};
use super::default;
use super::RenameSpec;
use super::generate_renames_with_targeting;
use super::RenameTargeting;
use super::RenameResult;
use super::super::*;


/// Generate file edits and file renames from found references.
pub fn generate_renames(spec: &RenameSpec, root: &Path) -> RenameResult {
    generate_renames_with_targeting(spec, root, &RenameTargeting::default())
}

pub(crate) fn target_files(files: Vec<PathBuf>, root: &Path, targeting: &RenameTargeting) -> Vec<PathBuf> {
    files
        .into_iter()
        .filter(|file| {
            let relative = file
                .strip_prefix(root)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/");

            if !targeting.include_globs.is_empty()
                && !targeting
                    .include_globs
                    .iter()
                    .any(|glob| glob_match::glob_match(glob, &relative))
            {
                return false;
            }

            if targeting
                .exclude_globs
                .iter()
                .any(|glob| glob_match::glob_match(glob, &relative))
            {
                return false;
            }

            true
        })
        .collect()
}
