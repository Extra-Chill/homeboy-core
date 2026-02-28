mod execution;
mod lifecycle;
mod manifest;
mod runner;
mod scope;
pub mod version;

pub mod exec_context;

// Re-export runner types
pub use runner::{ModuleRunner, RunnerOutput};

// Re-export manifest types
pub use manifest::{
    ActionConfig, ActionType, AuditCapability, BuildConfig, CliConfig, DatabaseCliConfig,
    DatabaseConfig, DeployCapability, DeployOverride, DeployVerification, DiscoveryConfig,
    ExecutableCapability, HttpMethod, InputConfig, LintConfig, ModuleManifest, OutputConfig,
    OutputSchema, PlatformCapability, ProvidesConfig, RequirementsConfig, RuntimeConfig,
    ScriptsConfig, SelectOption, SettingConfig, SinceTagConfig, TestConfig, VersionPatternConfig,
};

// Re-export version types
pub use version::{VersionConstraint, parse_module_version};

// Re-export execution types and functions
pub(crate) use execution::execute_action;
pub use execution::{
    is_module_compatible, module_ready_status, run_action, run_module, run_setup,
    ModuleExecutionMode, ModuleReadyStatus, ModuleRunResult, ModuleSetupResult, ModuleStepFilter,
};

// Re-export scope types
pub use scope::ModuleScope;

// Re-export lifecycle types and functions
pub use lifecycle::{
    check_update_available, derive_id_from_url, install, is_git_url, slugify_id, uninstall, update,
    InstallResult, UpdateAvailable, UpdateResult,
};

// Module loader functions

use crate::config;
use crate::error::Result;
use crate::output::MergeOutput;
use crate::paths;
use std::path::PathBuf;

pub fn load_module(id: &str) -> Result<ModuleManifest> {
    let mut manifest = config::load::<ModuleManifest>(id)?;
    let module_dir = paths::module(id)?;
    manifest.module_path = Some(module_dir.to_string_lossy().to_string());
    Ok(manifest)
}

pub fn load_all_modules() -> Result<Vec<ModuleManifest>> {
    let modules = config::list::<ModuleManifest>()?;
    let mut modules_with_paths = Vec::new();
    for mut module in modules {
        let module_dir = paths::module(&module.id)?;
        module.module_path = Some(module_dir.to_string_lossy().to_string());
        modules_with_paths.push(module);
    }
    Ok(modules_with_paths)
}

pub fn find_module_by_tool(tool: &str) -> Option<ModuleManifest> {
    load_all_modules().ok().and_then(|modules| {
        modules
            .into_iter()
            .find(|m| m.cli.as_ref().is_some_and(|c| c.tool == tool))
    })
}

/// Find a module that handles a given file extension and has a specific capability script.
///
/// Looks through all installed modules for one whose `provides.file_extensions` includes
/// the given extension and whose `scripts` has the requested capability configured.
///
/// Returns the module manifest with `module_path` populated.
pub fn find_module_for_file_extension(ext: &str, capability: &str) -> Option<ModuleManifest> {
    load_all_modules().ok().and_then(|modules| {
        modules.into_iter().find(|m| {
            if !m.handles_file_extension(ext) {
                return false;
            }
            match capability {
                "fingerprint" => m.fingerprint_script().is_some(),
                "refactor" => m.refactor_script().is_some(),
                _ => false,
            }
        })
    })
}

/// Run a module's fingerprint script on file content.
///
/// The script receives a JSON object on stdin:
/// ```json
/// {"file_path": "src/core/foo.rs", "content": "...file content..."}
/// ```
///
/// The script must output a JSON object on stdout matching the FileFingerprint schema:
/// ```json
/// {
///   "methods": ["foo", "bar"],
///   "type_name": "MyStruct",
///   "implements": ["SomeTrait"],
///   "registrations": [],
///   "namespace": null,
///   "imports": ["crate::error::Result"]
/// }
/// ```
pub fn run_fingerprint_script(
    module: &ModuleManifest,
    file_path: &str,
    content: &str,
) -> Option<FingerprintOutput> {
    let module_path = module.module_path.as_deref()?;
    let script_rel = module.fingerprint_script()?;
    let script_path = std::path::Path::new(module_path).join(script_rel);

    if !script_path.exists() {
        return None;
    }

    let input = serde_json::json!({
        "file_path": file_path,
        "content": content,
    });

    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(script_path.to_string_lossy().as_ref())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(input.to_string().as_bytes());
            }
            child.wait_with_output().ok()
        })?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).ok()
}

/// Output from a fingerprint extension script.
/// Matches the structural data extracted from a source file.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FingerprintOutput {
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub type_name: Option<String>,
    #[serde(default)]
    pub implements: Vec<String>,
    #[serde(default)]
    pub registrations: Vec<String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub imports: Vec<String>,
}

pub fn module_path(id: &str) -> PathBuf {
    paths::module(id).unwrap_or_else(|_| PathBuf::from(id))
}

pub fn available_module_ids() -> Vec<String> {
    config::list_ids::<ModuleManifest>().unwrap_or_default()
}

pub fn save_manifest(manifest: &ModuleManifest) -> Result<()> {
    config::save(manifest)
}

pub fn merge(id: Option<&str>, json_spec: &str, replace_fields: &[String]) -> Result<MergeOutput> {
    config::merge::<ModuleManifest>(id, json_spec, replace_fields)
}

/// Check if a module is a symlink (linked, not installed).
pub fn is_module_linked(module_id: &str) -> bool {
    paths::module(module_id)
        .map(|p| p.is_symlink())
        .unwrap_or(false)
}

/// Validate that all modules declared in a component's `modules` field are installed.
///
/// If `component.modules` contains keys like `{"wordpress": {}}`, those modules
/// are implicitly required. Returns an actionable error with install commands
/// when any are missing.
pub fn validate_required_modules(component: &crate::component::Component) -> Result<()> {
    let modules = match &component.modules {
        Some(m) if !m.is_empty() => m,
        _ => return Ok(()),
    };

    let mut missing: Vec<String> = Vec::new();
    for module_id in modules.keys() {
        if load_module(module_id).is_err() {
            missing.push(module_id.clone());
        }
    }

    if missing.is_empty() {
        return Ok(());
    }

    missing.sort();

    let module_list = missing.join(", ");
    let install_hints: Vec<String> = missing
        .iter()
        .map(|id| {
            format!(
                "homeboy module install https://github.com/Extra-Chill/homeboy-modules --id {}",
                id
            )
        })
        .collect();

    let message = if missing.len() == 1 {
        format!(
            "Component '{}' requires module '{}' which is not installed",
            component.id, missing[0]
        )
    } else {
        format!(
            "Component '{}' requires modules not installed: {}",
            component.id, module_list
        )
    };

    let mut err = crate::error::Error::new(
        crate::error::ErrorCode::ModuleNotFound,
        message,
        serde_json::json!({
            "component_id": component.id,
            "missing_modules": missing,
        }),
    );

    for hint in &install_hints {
        err = err.with_hint(hint.to_string());
    }

    err = err.with_hint("Browse available modules: https://github.com/Extra-Chill/homeboy-modules".to_string());

    Err(err)
}

/// Validate that all extensions declared in a component's `extensions` field are installed
/// and satisfy the declared version constraints.
///
/// Returns an actionable error listing every unsatisfied requirement with install/update hints.
pub fn validate_extension_requirements(component: &crate::component::Component) -> Result<()> {
    let extensions = match &component.extensions {
        Some(e) if !e.is_empty() => e,
        _ => return Ok(()),
    };

    let mut errors: Vec<String> = Vec::new();
    let mut hints: Vec<String> = Vec::new();

    for (module_id, constraint_str) in extensions {
        let constraint = match version::VersionConstraint::parse(constraint_str) {
            Ok(c) => c,
            Err(_) => {
                errors.push(format!(
                    "Invalid version constraint '{}' for extension '{}'",
                    constraint_str, module_id
                ));
                continue;
            }
        };

        match load_module(module_id) {
            Ok(module) => {
                match module.semver() {
                    Ok(installed_version) => {
                        if !constraint.matches(&installed_version) {
                            errors.push(format!(
                                "'{}' requires {}, but {} is installed",
                                module_id, constraint, installed_version
                            ));
                            hints.push(format!(
                                "Run `homeboy module update {}` to get the latest version",
                                module_id
                            ));
                        }
                    }
                    Err(_) => {
                        errors.push(format!(
                            "Extension '{}' has invalid version '{}'",
                            module_id, module.version
                        ));
                    }
                }
            }
            Err(_) => {
                errors.push(format!("Extension '{}' is not installed", module_id));
                hints.push(format!(
                    "homeboy module install https://github.com/Extra-Chill/homeboy-modules --id {}",
                    module_id
                ));
            }
        }
    }

    if errors.is_empty() {
        return Ok(());
    }

    let message = if errors.len() == 1 {
        format!(
            "Component '{}' has an unsatisfied extension requirement: {}",
            component.id, errors[0]
        )
    } else {
        format!(
            "Component '{}' has {} unsatisfied extension requirements:\n  - {}",
            component.id,
            errors.len(),
            errors.join("\n  - ")
        )
    };

    let mut err = crate::error::Error::new(
        crate::error::ErrorCode::ModuleNotFound,
        message,
        serde_json::json!({
            "component_id": component.id,
            "unsatisfied": errors,
        }),
    );

    for hint in &hints {
        err = err.with_hint(hint.to_string());
    }

    Err(err)
}

/// Check if any of the component's linked modules provide build configuration.
/// When true, the component's explicit build_command becomes optional.
pub fn module_provides_build(component: &crate::component::Component) -> bool {
    let modules = match &component.modules {
        Some(m) => m,
        None => return false,
    };

    for module_id in modules.keys() {
        if let Ok(module) = load_module(module_id) {
            if module.has_build() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::{Component, ScopedModuleConfig};
    use std::collections::HashMap;

    #[test]
    fn validate_required_modules_passes_with_no_modules() {
        let comp = Component {
            id: "test-component".to_string(),
            ..Default::default()
        };
        assert!(validate_required_modules(&comp).is_ok());
    }

    #[test]
    fn validate_required_modules_passes_with_empty_modules() {
        let comp = Component {
            id: "test-component".to_string(),
            modules: Some(HashMap::new()),
            ..Default::default()
        };
        assert!(validate_required_modules(&comp).is_ok());
    }

    #[test]
    fn validate_required_modules_fails_with_missing_module() {
        let mut modules = HashMap::new();
        modules.insert(
            "nonexistent-module-abc123".to_string(),
            ScopedModuleConfig::default(),
        );
        let comp = Component {
            id: "test-component".to_string(),
            modules: Some(modules),
            ..Default::default()
        };
        let err = validate_required_modules(&comp).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::ModuleNotFound);
        assert!(err.message.contains("nonexistent-module-abc123"));
        assert!(err.message.contains("test-component"));
        // Should have install hint + browse hint
        assert!(err.hints.len() >= 2);
        assert!(err.hints.iter().any(|h| h.message.contains("homeboy module install")));
        assert!(err
            .hints
            .iter()
            .any(|h| h.message.contains("homeboy-modules")));
    }

    #[test]
    fn validate_required_modules_reports_all_missing() {
        let mut modules = HashMap::new();
        modules.insert(
            "missing-mod-a".to_string(),
            ScopedModuleConfig::default(),
        );
        modules.insert(
            "missing-mod-b".to_string(),
            ScopedModuleConfig::default(),
        );
        let comp = Component {
            id: "multi-dep".to_string(),
            modules: Some(modules),
            ..Default::default()
        };
        let err = validate_required_modules(&comp).unwrap_err();
        // Error should mention both missing modules
        assert!(err.message.contains("missing-mod-a"));
        assert!(err.message.contains("missing-mod-b"));
        // Should have install hint for each + browse hint
        assert!(err.hints.len() >= 3);
    }
}
