use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use homeboy_core::config::{ConfigManager, VersionTarget};
use homeboy_core::json::{read_json_file, set_json_pointer, write_json_file_pretty};
use homeboy_core::version::{
    default_pattern_for_file, increment_version, parse_versions, replace_versions,
};
use homeboy_core::Error;

#[derive(Args)]
pub struct VersionArgs {
    #[command(subcommand)]
    command: VersionCommand,
}

#[derive(Subcommand)]
enum VersionCommand {
    /// Show current version of a component
    Show {
        /// Component ID
        component_id: String,
    },
    /// Bump version of a component
    Bump {
        /// Component ID
        component_id: String,
        /// Version bump type
        bump_type: BumpType,
    },
}

#[derive(Clone, ValueEnum)]
enum BumpType {
    Patch,
    Minor,
    Major,
}

impl BumpType {
    fn as_str(&self) -> &'static str {
        match self {
            BumpType::Patch => "patch",
            BumpType::Minor => "minor",
            BumpType::Major => "major",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionTargetOutput {
    version_file: String,
    version_pattern: String,
    full_path: String,
    match_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionOutput {
    command: String,
    component_id: String,
    version: Option<String>,
    old_version: Option<String>,
    new_version: Option<String>,
    targets: Vec<VersionTargetOutput>,
}

pub fn run(args: VersionArgs) -> homeboy_core::Result<(VersionOutput, i32)> {
    match args.command {
        VersionCommand::Show { component_id } => show(&component_id),
        VersionCommand::Bump {
            component_id,
            bump_type,
        } => bump(&component_id, bump_type),
    }
}

fn resolve_target_full_path(component_local_path: &str, version_file: &str) -> String {
    if version_file.starts_with('/') {
        version_file.to_string()
    } else {
        format!("{}/{}", component_local_path, version_file)
    }
}

fn resolve_target_pattern(target: &VersionTarget) -> String {
    target
        .pattern
        .clone()
        .unwrap_or_else(|| default_pattern_for_file(&target.file).to_string())
}

fn extract_versions_from_content(
    content: &str,
    pattern: &str,
) -> homeboy_core::Result<Vec<String>> {
    parse_versions(content, pattern)
        .ok_or_else(|| Error::Other(format!("Invalid version regex pattern '{}'", pattern)))
}

fn validate_single_version(
    versions: Vec<String>,
    version_file: &str,
    expected: &str,
) -> homeboy_core::Result<(String, usize)> {
    if versions.is_empty() {
        return Err(Error::Other(format!(
            "Could not find version in {}",
            version_file
        )));
    }

    let unique: BTreeSet<String> = versions.iter().cloned().collect();

    if unique.len() != 1 {
        return Err(Error::Other(format!(
            "Multiple different versions found in {}: {}",
            version_file,
            unique.into_iter().collect::<Vec<_>>().join(", ")
        )));
    }

    let found = versions[0].clone();
    if found != expected {
        return Err(Error::Other(format!(
            "Version mismatch in {}: found {}, expected {}",
            version_file, found, expected
        )));
    }

    Ok((found, versions.len()))
}

fn replace_versions_in_content(
    content: &str,
    pattern: &str,
    expected_old: &str,
    new_version: &str,
) -> homeboy_core::Result<(String, usize)> {
    let all_versions = extract_versions_from_content(content, pattern)?;
    let _ = validate_single_version(all_versions, "<content>", expected_old)?;

    let (replaced, replaced_count) = replace_versions(content, pattern, new_version)
        .ok_or_else(|| Error::Other(format!("Invalid version regex pattern '{}'", pattern)))?;

    Ok((replaced, replaced_count))
}

fn write_updated_version(
    full_path: &str,
    version_pattern: &str,
    old_version: &str,
    new_version: &str,
) -> homeboy_core::Result<usize> {
    if Path::new(full_path)
        .extension()
        .is_some_and(|ext| ext == "json")
        && version_pattern == default_pattern_for_file(full_path)
    {
        let mut json = read_json_file(full_path)?;
        let Some(current) = json.get("version").and_then(|v| v.as_str()) else {
            return Err(Error::Other(format!(
                "Could not find JSON key 'version' in {}",
                full_path
            )));
        };

        if current != old_version {
            return Err(Error::Other(format!(
                "Version mismatch in {}: found {}, expected {}",
                full_path, current, old_version
            )));
        }

        set_json_pointer(
            &mut json,
            "/version",
            serde_json::Value::String(new_version.to_string()),
        )?;
        write_json_file_pretty(full_path, &json)?;
        return Ok(1);
    }

    let content = fs::read_to_string(full_path)?;
    let (new_content, replaced_count) =
        replace_versions_in_content(&content, version_pattern, old_version, new_version)?;
    fs::write(full_path, &new_content)?;
    Ok(replaced_count)
}

fn show(component_id: &str) -> homeboy_core::Result<(VersionOutput, i32)> {
    let component = ConfigManager::load_component(component_id)?;
    let targets = component.version_targets.ok_or_else(|| {
        Error::Config(format!(
            "Component '{}' has no versionTargets configured",
            component_id
        ))
    })?;

    if targets.is_empty() {
        return Err(Error::Config(format!(
            "Component '{}' has no versionTargets configured",
            component_id
        )));
    }

    let primary = &targets[0];
    let primary_pattern = resolve_target_pattern(primary);
    let primary_full_path = resolve_target_full_path(&component.local_path, &primary.file);

    let content = fs::read_to_string(&primary_full_path)?;
    let versions = extract_versions_from_content(&content, &primary_pattern)?;

    if versions.is_empty() {
        return Err(Error::Other(format!(
            "Could not parse version from {} using pattern: {}",
            primary.file, primary_pattern
        )));
    }

    let unique: BTreeSet<String> = versions.iter().cloned().collect();
    if unique.len() != 1 {
        return Err(Error::Other(format!(
            "Multiple different versions found in {}: {}",
            primary.file,
            unique.into_iter().collect::<Vec<_>>().join(", ")
        )));
    }

    let version = versions[0].clone();

    Ok((
        VersionOutput {
            command: "version.show".to_string(),
            component_id: component_id.to_string(),
            version: Some(version),
            old_version: None,
            new_version: None,
            targets: vec![VersionTargetOutput {
                version_file: primary.file.clone(),
                version_pattern: primary_pattern,
                full_path: primary_full_path,
                match_count: versions.len(),
            }],
        },
        0,
    ))
}

fn bump(component_id: &str, bump_type: BumpType) -> homeboy_core::Result<(VersionOutput, i32)> {
    let component = ConfigManager::load_component(component_id)?;
    let targets = component.version_targets.clone().ok_or_else(|| {
        Error::Config(format!(
            "Component '{}' has no versionTargets configured",
            component_id
        ))
    })?;

    if targets.is_empty() {
        return Err(Error::Config(format!(
            "Component '{}' has no versionTargets configured",
            component_id
        )));
    }

    let primary = &targets[0];
    let primary_pattern = resolve_target_pattern(primary);
    let primary_full_path = resolve_target_full_path(&component.local_path, &primary.file);

    let primary_content = fs::read_to_string(&primary_full_path)?;
    let primary_versions = extract_versions_from_content(&primary_content, &primary_pattern)?;

    if primary_versions.is_empty() {
        return Err(Error::Other(format!(
            "Could not parse version from {} using pattern: {}",
            primary.file, primary_pattern
        )));
    }

    let unique_primary: BTreeSet<String> = primary_versions.iter().cloned().collect();
    if unique_primary.len() != 1 {
        return Err(Error::Other(format!(
            "Multiple different versions found in {}: {}",
            primary.file,
            unique_primary.into_iter().collect::<Vec<_>>().join(", ")
        )));
    }

    let old_version = primary_versions[0].clone();
    let new_version = increment_version(&old_version, bump_type.as_str())
        .ok_or_else(|| Error::Other(format!("Invalid version format: {}", old_version)))?;

    let mut outputs = Vec::new();

    for target in targets {
        let version_pattern = resolve_target_pattern(&target);
        let full_path = resolve_target_full_path(&component.local_path, &target.file);
        let content = fs::read_to_string(&full_path)?;

        let versions = extract_versions_from_content(&content, &version_pattern)?;
        let (_, match_count) = validate_single_version(versions, &target.file, &old_version)?;

        let replaced_count =
            write_updated_version(&full_path, &version_pattern, &old_version, &new_version)?;

        if replaced_count != match_count {
            return Err(Error::Other(format!(
                "Unexpected replacement count in {}",
                target.file
            )));
        }

        outputs.push(VersionTargetOutput {
            version_file: target.file,
            version_pattern,
            full_path,
            match_count,
        });
    }

    Ok((
        VersionOutput {
            command: "version.bump".to_string(),
            component_id: component_id.to_string(),
            version: None,
            old_version: Some(old_version),
            new_version: Some(new_version),
            targets: outputs,
        },
        0,
    ))
}
