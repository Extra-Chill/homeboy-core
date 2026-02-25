mod execution;
mod lifecycle;
mod manifest;
mod runner;
mod scope;

pub mod exec_context;

// Re-export runner types
pub use runner::{ModuleRunner, RunnerOutput};

// Re-export manifest types
pub use manifest::{
    ActionConfig, ActionType, AuditCapability, BuildConfig, CliConfig, DatabaseCliConfig,
    DatabaseConfig, DeployCapability, DeployOverride, DeployVerification, DiscoveryConfig,
    ExecutableCapability, HttpMethod, InputConfig, LintConfig, ModuleManifest, OutputConfig,
    OutputSchema, PlatformCapability, RequirementsConfig, RuntimeConfig, SelectOption,
    SettingConfig, SinceTagConfig, TestConfig, VersionPatternConfig,
};

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
