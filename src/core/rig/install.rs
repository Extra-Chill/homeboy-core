//! Rig package install lifecycle.
//!
//! A rig package is a directory or git repository with specs at
//! `rigs/<id>/rig.json` (or a single rig directory containing `rig.json`).
//! Installed rigs stay loadable through the existing flat rig config path by
//! linking `~/.config/homeboy/rigs/<id>.json` to the package spec.

use crate::error::{Error, Result};
use crate::{extension, git, paths};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredRig {
    pub id: String,
    pub description: String,
    pub rig_path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct RigInstallResult {
    pub source: String,
    pub package_path: PathBuf,
    pub linked: bool,
    pub installed: Vec<InstalledRig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledRig {
    pub id: String,
    pub description: String,
    pub path: PathBuf,
    pub spec_path: PathBuf,
    pub source_revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigSourceMetadata {
    pub source: String,
    pub package_path: String,
    pub rig_path: String,
    pub linked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RigSourceListResult {
    pub sources: Vec<RigSourceGroup>,
    pub invalid: Vec<InvalidRigSourceMetadata>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RigSourceGroup {
    pub source: String,
    pub package_path: String,
    pub package_id: String,
    pub linked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
    pub rigs: Vec<RigSourceRig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RigSourceRig {
    pub id: String,
    pub rig_path: String,
    pub config_path: String,
    pub config_present: bool,
    pub config_owned: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InvalidRigSourceMetadata {
    pub id: String,
    pub metadata_path: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RigSourceRemoveResult {
    pub selector: String,
    pub source: RigSourceGroup,
    pub removed: Vec<RemovedRigSourceRig>,
    pub skipped: Vec<SkippedRigSourceRig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_package_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemovedRigSourceRig {
    pub id: String,
    pub config_path: String,
    pub metadata_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkippedRigSourceRig {
    pub id: String,
    pub config_path: String,
    pub reason: String,
}

pub fn install(source: &str, id: Option<&str>, all: bool) -> Result<RigInstallResult> {
    let prepared = prepare_source(source)?;
    let discovered = discover_rigs(&prepared.package_path)?;
    let selected = select_rigs(discovered, id, all, source)?;

    fs::create_dir_all(paths::rigs()?)
        .map_err(|e| Error::internal_io(e.to_string(), Some("create rigs dir".into())))?;
    fs::create_dir_all(paths::rig_sources()?)
        .map_err(|e| Error::internal_io(e.to_string(), Some("create rig sources dir".into())))?;

    let mut installed = Vec::new();
    for rig in selected {
        let target = paths::rig_config(&rig.id)?;
        if target.exists() || fs::symlink_metadata(&target).is_ok() {
            return Err(Error::validation_invalid_argument(
                "rig_id",
                format!("Rig '{}' already exists at {}", rig.id, target.display()),
                Some(rig.id),
                None,
            ));
        }

        link_or_copy_file(&rig.rig_path, &target)?;

        let metadata = RigSourceMetadata {
            source: prepared.source.clone(),
            package_path: prepared.package_path.to_string_lossy().to_string(),
            rig_path: rig.rig_path.to_string_lossy().to_string(),
            linked: prepared.linked,
            source_revision: prepared.source_revision.clone(),
        };
        write_source_metadata(&rig.id, &metadata)?;

        installed.push(InstalledRig {
            id: rig.id,
            description: rig.description,
            path: target,
            spec_path: rig.rig_path,
            source_revision: prepared.source_revision.clone(),
        });
    }

    Ok(RigInstallResult {
        source: prepared.source,
        package_path: prepared.package_path,
        linked: prepared.linked,
        installed,
    })
}

pub fn read_source_metadata(id: &str) -> Option<RigSourceMetadata> {
    let path = paths::rig_source_metadata(id).ok()?;
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

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
        if rig.config_present && rig.config_owned {
            fs::remove_file(&config_path)
                .map_err(|e| Error::internal_io(e.to_string(), Some("remove rig config".into())))?;
            fs::remove_file(&metadata_path).map_err(|e| {
                Error::internal_io(e.to_string(), Some("remove rig source metadata".into()))
            })?;
            removed.push(RemovedRigSourceRig {
                id: rig.id.clone(),
                config_path: rig.config_path.clone(),
                metadata_path: metadata_path.to_string_lossy().to_string(),
            });
        } else if !rig.config_present {
            fs::remove_file(&metadata_path).map_err(|e| {
                Error::internal_io(e.to_string(), Some("remove rig source metadata".into()))
            })?;
            removed.push(RemovedRigSourceRig {
                id: rig.id.clone(),
                config_path: rig.config_path.clone(),
                metadata_path: metadata_path.to_string_lossy().to_string(),
            });
        } else {
            skipped.push(SkippedRigSourceRig {
                id: rig.id.clone(),
                config_path: rig.config_path.clone(),
                reason: "config file no longer points at the recorded rig source".to_string(),
            });
        }
    }

    let removed_package_path = if !source.linked && skipped.is_empty() {
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
        (Ok(config), Ok(rig)) => config == rig,
        _ => false,
    }
}

struct PreparedSource {
    source: String,
    package_path: PathBuf,
    linked: bool,
    source_revision: Option<String>,
}

fn prepare_source(source: &str) -> Result<PreparedSource> {
    if extension::is_git_url(source) {
        prepare_git_source(source)
    } else {
        prepare_local_source(source)
    }
}

fn prepare_git_source(source: &str) -> Result<PreparedSource> {
    let trimmed = source.trim_end_matches('/').trim_end_matches(".git");
    let parts = trimmed.rsplit(['/', ':']).take(2).collect::<Vec<_>>();
    let package_id = if parts.len() == 2 {
        extension::slugify_id(&format!("{}-{}", parts[1], parts[0]))?
    } else {
        extension::slugify_id(parts.first().copied().unwrap_or(trimmed))?
    };
    let package_path = paths::rig_package(&package_id)?;
    if package_path.exists() {
        return Err(Error::validation_invalid_argument(
            "source",
            format!(
                "Rig package '{}' already exists at {}",
                package_id,
                package_path.display()
            ),
            Some(source.to_string()),
            None,
        ));
    }
    fs::create_dir_all(paths::rig_packages()?)
        .map_err(|e| Error::internal_io(e.to_string(), Some("create rig packages dir".into())))?;
    git::clone_repo(source, &package_path)?;
    Ok(PreparedSource {
        source: source.to_string(),
        package_path,
        linked: false,
        source_revision: None,
    })
}

fn prepare_local_source(source: &str) -> Result<PreparedSource> {
    let source_path = Path::new(source);
    let package_path = if source_path.is_absolute() {
        source_path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| Error::internal_io(e.to_string(), Some("get current dir".into())))?
            .join(source_path)
    };
    if !package_path.exists() {
        return Err(Error::validation_invalid_argument(
            "source",
            format!("Path does not exist: {}", package_path.display()),
            Some(source.to_string()),
            None,
        ));
    }
    Ok(PreparedSource {
        source: package_path.to_string_lossy().to_string(),
        package_path,
        linked: true,
        source_revision: None,
    })
}

pub fn discover_rigs(package_path: &Path) -> Result<Vec<DiscoveredRig>> {
    let mut rigs = Vec::new();

    let single = package_path.join("rig.json");
    if single.is_file() {
        rigs.push(discovered_from_path(&single, package_path.file_name())?);
    }

    let rigs_dir = package_path.join("rigs");
    if rigs_dir.is_dir() {
        for entry in fs::read_dir(&rigs_dir)
            .map_err(|e| Error::internal_io(e.to_string(), Some("read rigs dir".into())))?
        {
            let entry = entry.map_err(|e| {
                Error::internal_io(e.to_string(), Some("read rig dir entry".into()))
            })?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let rig_path = path.join("rig.json");
            if rig_path.is_file() {
                rigs.push(discovered_from_path(&rig_path, path.file_name())?);
            }
        }
    }

    rigs.sort_by(|a, b| a.id.cmp(&b.id));
    rigs.dedup_by(|a, b| a.id == b.id);

    if rigs.is_empty() {
        return Err(Error::validation_invalid_argument(
            "source",
            format!(
                "No rig specs found at {} (expected rig.json or rigs/<id>/rig.json)",
                package_path.display()
            ),
            Some(package_path.to_string_lossy().to_string()),
            None,
        ));
    }

    Ok(rigs)
}

fn discovered_from_path(
    path: &Path,
    fallback_name: Option<&std::ffi::OsStr>,
) -> Result<DiscoveredRig> {
    let content = fs::read_to_string(path)
        .map_err(|e| Error::internal_io(e.to_string(), Some("read rig spec".into())))?;
    let mut spec: super::RigSpec = serde_json::from_str(&content).map_err(|e| {
        Error::validation_invalid_json(
            e,
            Some(format!("parse rig spec {}", path.display())),
            Some(content.chars().take(200).collect()),
        )
    })?;
    if spec.id.is_empty() {
        spec.id = fallback_name
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                Error::validation_invalid_argument(
                    "rig_id",
                    "Rig spec has no id and no directory name fallback",
                    None,
                    None,
                )
            })?
            .to_string();
    }
    Ok(DiscoveredRig {
        id: extension::slugify_id(&spec.id)?,
        description: spec.description,
        rig_path: path.to_path_buf(),
    })
}

fn select_rigs(
    rigs: Vec<DiscoveredRig>,
    id: Option<&str>,
    all: bool,
    source: &str,
) -> Result<Vec<DiscoveredRig>> {
    if all {
        return Ok(rigs);
    }
    if let Some(id) = id {
        let id = extension::slugify_id(id)?;
        let found: Vec<_> = rigs.into_iter().filter(|rig| rig.id == id).collect();
        if found.is_empty() {
            return Err(Error::validation_invalid_argument(
                "id",
                format!("Rig '{}' not found in package", id),
                Some(id),
                None,
            ));
        }
        return Ok(found);
    }
    if rigs.len() == 1 {
        return Ok(rigs);
    }
    let available = rigs.iter().map(|rig| rig.id.clone()).collect::<Vec<_>>();
    Err(Error::validation_invalid_argument(
        "id",
        format!(
            "Package contains multiple rigs; pass --id <rig> or --all. Available: {}",
            available.join(", ")
        ),
        Some(source.to_string()),
        Some(available),
    ))
}

fn write_source_metadata(id: &str, metadata: &RigSourceMetadata) -> Result<()> {
    let path = paths::rig_source_metadata(id)?;
    let content = serde_json::to_string_pretty(metadata)
        .map_err(|e| Error::internal_json(e.to_string(), Some("serialize rig source".into())))?;
    fs::write(&path, format!("{}\n", content))
        .map_err(|e| Error::internal_io(e.to_string(), Some("write rig source".into())))
}

fn link_or_copy_file(source: &Path, target: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, target)
            .map_err(|e| Error::internal_io(e.to_string(), Some("create rig symlink".into())))
    }

    #[cfg(windows)]
    {
        fs::copy(source, target)
            .map(|_| ())
            .map_err(|e| Error::internal_io(e.to_string(), Some("copy rig spec".into())))
    }
}

#[cfg(test)]
#[path = "../../../tests/core/rig/install_test.rs"]
mod install_test;
