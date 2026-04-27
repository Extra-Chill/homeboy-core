//! Installed rig source lifecycle.

use crate::error::{Error, Result};
use crate::paths;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::install::RigSourceMetadata;

mod types;
pub use types::{
    InvalidRigSourceMetadata, RemovedRigSourceRig, RigSourceGroup, RigSourceListResult,
    RigSourceRemoveResult, RigSourceRig, SkippedRigSourceRig,
};

pub fn list_sources() -> Result<RigSourceListResult> {
    read_source_entries().map(group_source_entries)
}

pub fn remove_source(selector: &str) -> Result<RigSourceRemoveResult> {
    let list = list_sources()?;
    let matches = list
        .sources
        .into_iter()
        .filter(|source| source_matches(source, selector))
        .collect::<Vec<_>>();

    if matches.is_empty() {
        return Err(Error::validation_invalid_argument(
            "source",
            format!("No installed rig source matches '{}'", selector),
            Some(selector.to_string()),
            None,
        ));
    }
    if matches.len() > 1 {
        let tried = matches
            .iter()
            .map(|source| source.package_id.clone())
            .collect::<Vec<_>>();
        return Err(Error::validation_invalid_argument(
            "source",
            format!(
                "Selector '{}' matches multiple rig sources; use the full source or package path",
                selector
            ),
            Some(selector.to_string()),
            Some(tried),
        ));
    }

    let source = matches.into_iter().next().expect("checked non-empty");
    let mut removed = Vec::new();
    let mut skipped = Vec::new();
    for rig in &source.rigs {
        let metadata_path = paths::rig_source_metadata(&rig.id)?;
        let config_path = PathBuf::from(&rig.config_path);
        let remove_config = rig.config_present && rig.config_owned;
        if remove_config {
            fs::remove_file(&config_path)
                .map_err(|e| Error::internal_io(e.to_string(), Some("remove rig config".into())))?;
        }

        remove_source_metadata(&metadata_path)?;
        if remove_config || !rig.config_present {
            removed.push(removed_rig_source_rig(rig, &metadata_path));
        } else {
            skipped.push(SkippedRigSourceRig {
                id: rig.id.clone(),
                config_path: rig.config_path.clone(),
                reason: "config file no longer points at the recorded rig source".to_string(),
            });
        }
    }

    let removed_package_path = if !source.linked {
        let package_path = PathBuf::from(&source.package_path);
        if package_path.exists() {
            fs::remove_dir_all(&package_path).map_err(|e| {
                Error::internal_io(e.to_string(), Some("remove rig package".into()))
            })?;
            Some(source.package_path.clone())
        } else {
            None
        }
    } else {
        None
    };

    Ok(RigSourceRemoveResult {
        selector: selector.to_string(),
        source,
        removed,
        skipped,
        removed_package_path,
    })
}

fn removed_rig_source_rig(rig: &RigSourceRig, metadata_path: &Path) -> RemovedRigSourceRig {
    RemovedRigSourceRig {
        id: rig.id.clone(),
        config_path: rig.config_path.clone(),
        metadata_path: metadata_path.to_string_lossy().to_string(),
    }
}

fn remove_source_metadata(path: &Path) -> Result<()> {
    fs::remove_file(path)
        .map_err(|e| Error::internal_io(e.to_string(), Some("remove rig source metadata".into())))
}

#[derive(Debug)]
struct RigSourceEntry {
    id: String,
    metadata: RigSourceMetadata,
}

#[derive(Debug)]
struct SourceEntries {
    valid: Vec<RigSourceEntry>,
    invalid: Vec<InvalidRigSourceMetadata>,
}

fn read_source_entries() -> Result<SourceEntries> {
    let dir = paths::rig_sources()?;
    if !dir.exists() {
        return Ok(SourceEntries {
            valid: Vec::new(),
            invalid: Vec::new(),
        });
    }

    let mut valid = Vec::new();
    let mut invalid = Vec::new();
    for entry in fs::read_dir(&dir)
        .map_err(|e| Error::internal_io(e.to_string(), Some("read rig sources dir".into())))?
    {
        let entry = entry
            .map_err(|e| Error::internal_io(e.to_string(), Some("read rig source entry".into())))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                invalid.push(InvalidRigSourceMetadata {
                    id,
                    metadata_path: path.to_string_lossy().to_string(),
                    error: err.to_string(),
                });
                continue;
            }
        };
        match serde_json::from_str::<RigSourceMetadata>(&content) {
            Ok(metadata) => valid.push(RigSourceEntry { id, metadata }),
            Err(err) => invalid.push(InvalidRigSourceMetadata {
                id,
                metadata_path: path.to_string_lossy().to_string(),
                error: err.to_string(),
            }),
        }
    }

    valid.sort_by(|a, b| a.id.cmp(&b.id));
    invalid.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(SourceEntries { valid, invalid })
}

fn group_source_entries(entries: SourceEntries) -> RigSourceListResult {
    let mut groups: BTreeMap<String, RigSourceGroup> = BTreeMap::new();
    for entry in entries.valid {
        let key = format!("{}\0{}", entry.metadata.source, entry.metadata.package_path);
        let config_path = paths::rig_config(&entry.id).ok();
        let config_present = config_path.as_ref().is_some_and(|path| path.exists());
        let config_owned = config_path
            .as_ref()
            .is_some_and(|path| rig_config_matches_source(path, &entry.metadata.rig_path));

        groups
            .entry(key)
            .or_insert_with(|| RigSourceGroup {
                source: entry.metadata.source.clone(),
                package_id: package_id_from_path(&entry.metadata.package_path),
                package_path: entry.metadata.package_path.clone(),
                linked: entry.metadata.linked,
                source_revision: entry.metadata.source_revision.clone(),
                rigs: Vec::new(),
            })
            .rigs
            .push(RigSourceRig {
                id: entry.id,
                rig_path: entry.metadata.rig_path,
                config_path: config_path
                    .map(|path| path.to_string_lossy().to_string())
                    .unwrap_or_default(),
                config_present,
                config_owned,
            });
    }

    let mut sources = groups.into_values().collect::<Vec<_>>();
    for source in &mut sources {
        source.rigs.sort_by(|a, b| a.id.cmp(&b.id));
    }

    RigSourceListResult {
        sources,
        invalid: entries.invalid,
    }
}

fn source_matches(source: &RigSourceGroup, selector: &str) -> bool {
    source.source == selector || source.package_path == selector || source.package_id == selector
}

fn package_id_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

fn rig_config_matches_source(config_path: &Path, rig_path: &str) -> bool {
    let rig_path = Path::new(rig_path);
    if fs::read_link(config_path).is_ok_and(|target| target == rig_path) {
        return true;
    }

    match (config_path.canonicalize(), rig_path.canonicalize()) {
        (Ok(config), Ok(rig)) if config == rig => true,
        (Ok(_), Ok(_)) => files_match(config_path, rig_path),
        _ => false,
    }
}

fn files_match(left: &Path, right: &Path) -> bool {
    match (fs::read(left), fs::read(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
#[path = "../../../tests/core/rig/source_test.rs"]
mod source_test;
