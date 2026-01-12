use crate::module::{load_module, ModuleManifest};
use crate::template::{render, TemplateVars};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildCommandSource {
    Module,
}

pub struct BuildCommandCandidate {
    pub source: BuildCommandSource,
    pub command: String,
}

fn file_exists(path: &Path) -> bool {
    std::fs::metadata(path).is_ok()
}

/// Detect build command using module configuration.
pub fn detect_build_command(
    local_path: &str,
    build_artifact: &str,
    modules: &[String],
) -> Option<BuildCommandCandidate> {
    let root = PathBuf::from(local_path);

    // Check modules for matching build config
    for module_id in modules {
        if let Some(module) = load_module(module_id) {
            if let Some(candidate) = detect_build_from_module(&root, build_artifact, &module) {
                return Some(candidate);
            }
        }
    }

    None
}

fn detect_build_from_module(
    root: &Path,
    build_artifact: &str,
    module: &ModuleManifest,
) -> Option<BuildCommandCandidate> {
    let build_config = module.build.as_ref()?;

    // Check if artifact matches any configured extension
    let artifact_lower = build_artifact.to_ascii_lowercase();
    let matches_artifact = build_config
        .artifact_extensions
        .iter()
        .any(|ext| artifact_lower.ends_with(&ext.to_ascii_lowercase()));

    if !matches_artifact {
        return None;
    }

    // Look for any of the configured script names
    for script_name in &build_config.script_names {
        let script_path = root.join(script_name);
        if file_exists(&script_path) {
            let command = build_config
                .command_template
                .as_ref()
                .map(|tpl| render(tpl, &[(TemplateVars::SCRIPT, script_name)]))
                .unwrap_or_else(|| format!("sh {}", script_name));

            return Some(BuildCommandCandidate {
                source: BuildCommandSource::Module,
                command,
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_detection_without_modules() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::write(temp_dir.path().join("build.sh"), "#!/bin/sh\necho ok\n").unwrap();

        // Without modules, no build command is detected (module-driven detection)
        let candidate =
            detect_build_command(temp_dir.path().to_str().unwrap(), "dist/app.zip", &[]);
        assert!(candidate.is_none());
    }
}
