use crate::config::ConfigEntity;
use crate::error::{Error, Result};
use crate::paths;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Type of action that can be executed by a module.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActionType {
    Api,
    Command,
    Builtin,
}

/// Builtin action types for Desktop app (copy, export operations).
/// CLI parses these but does not execute them - Desktop implements the behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BuiltinAction {
    CopyColumn,
    ExportCsv,
    CopyJson,
}

/// HTTP method for API actions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

// ============================================================================
// Capability Groups
// ============================================================================

/// Deploy lifecycle: verification rules, install overrides, version patterns, @since tags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployCapability {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verifications: Vec<DeployVerification>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<DeployOverride>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub version_patterns: Vec<VersionPatternConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_tag: Option<SinceTagConfig>,
}

/// Docs audit: ignore patterns and feature detection patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditCapability {
    /// Glob patterns for paths to ignore during docs audit.
    /// Uses `*` for single segment and `**` for multiple segments (e.g., `/wp-json/**`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_claim_patterns: Vec<String>,
    /// Regex patterns to detect feature registrations in source code.
    /// Each pattern should have a capture group for the feature name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feature_patterns: Vec<String>,
}

/// Executable tool: runtime, inputs, and output schema.
/// Represents a module that can be run as a standalone tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableCapability {
    pub runtime: RuntimeConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<InputConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<OutputConfig>,
}

/// Desktop/platform UI config: pinned files, database, discovery, commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_pinned_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_pinned_logs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<DatabaseConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery: Option<DiscoveryConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
}

// ============================================================================
// ModuleManifest
// ============================================================================

/// Unified module manifest decomposed into capability groups.
///
/// Module JSON files use nested capability groups that map directly to these fields.
/// Convenience methods provide ergonomic access to nested data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleManifest {
    // Identity
    #[serde(default, skip_serializing)]
    pub id: String,
    pub name: String,
    pub version: String,

    // Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,

    // Capability groups
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deploy: Option<DeployCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit: Option<AuditCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<ExecutableCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<PlatformCapability>,

    // Standalone capabilities (already self-contained structs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli: Option<CliConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lint: Option<LintConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<TestConfig>,

    // Actions (cross-cutting: used by both platform and executable modules)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ActionConfig>,

    // Lifecycle hooks: event name -> list of shell commands.
    // Module hooks run before component hooks at each event.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub hooks: HashMap<String, Vec<String>>,

    // Shared
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<SettingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<RequirementsConfig>,

    // Extensibility: preserve unknown fields for external consumers (GUI, workflows)
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,

    // Internal path (not serialized)
    #[serde(skip)]
    pub module_path: Option<String>,
}

impl ModuleManifest {
    pub fn has_cli(&self) -> bool {
        self.cli.is_some()
    }

    pub fn has_build(&self) -> bool {
        self.build.is_some()
    }

    pub fn has_lint(&self) -> bool {
        self.lint
            .as_ref()
            .and_then(|c| c.module_script.as_ref())
            .is_some()
    }

    pub fn has_test(&self) -> bool {
        self.test
            .as_ref()
            .and_then(|c| c.module_script.as_ref())
            .is_some()
    }

    pub fn lint_script(&self) -> Option<&str> {
        self.lint.as_ref().and_then(|c| c.module_script.as_deref())
    }

    pub fn test_script(&self) -> Option<&str> {
        self.test.as_ref().and_then(|c| c.module_script.as_deref())
    }

    /// Convenience: get deploy verifications (empty if no deploy capability).
    pub fn deploy_verifications(&self) -> &[DeployVerification] {
        self.deploy
            .as_ref()
            .map(|d| d.verifications.as_slice())
            .unwrap_or(&[])
    }

    /// Convenience: get deploy overrides (empty if no deploy capability).
    pub fn deploy_overrides(&self) -> &[DeployOverride] {
        self.deploy
            .as_ref()
            .map(|d| d.overrides.as_slice())
            .unwrap_or(&[])
    }

    /// Convenience: get version patterns (empty if no deploy capability).
    pub fn version_patterns(&self) -> &[VersionPatternConfig] {
        self.deploy
            .as_ref()
            .map(|d| d.version_patterns.as_slice())
            .unwrap_or(&[])
    }

    /// Convenience: get since_tag config.
    pub fn since_tag(&self) -> Option<&SinceTagConfig> {
        self.deploy.as_ref().and_then(|d| d.since_tag.as_ref())
    }

    /// Convenience: get runtime config.
    pub fn runtime(&self) -> Option<&RuntimeConfig> {
        self.executable.as_ref().map(|e| &e.runtime)
    }

    /// Convenience: get inputs (empty if no executable capability).
    pub fn inputs(&self) -> &[InputConfig] {
        self.executable
            .as_ref()
            .map(|e| e.inputs.as_slice())
            .unwrap_or(&[])
    }

    /// Convenience: get audit ignore claim patterns (empty if no audit capability).
    pub fn audit_ignore_claim_patterns(&self) -> &[String] {
        self.audit
            .as_ref()
            .map(|a| a.ignore_claim_patterns.as_slice())
            .unwrap_or(&[])
    }

    /// Convenience: get audit feature patterns (empty if no audit capability).
    pub fn audit_feature_patterns(&self) -> &[String] {
        self.audit
            .as_ref()
            .map(|a| a.feature_patterns.as_slice())
            .unwrap_or(&[])
    }

    /// Convenience: get database config from platform capability.
    pub fn database(&self) -> Option<&DatabaseConfig> {
        self.platform.as_ref().and_then(|p| p.database.as_ref())
    }
}

impl ConfigEntity for ModuleManifest {
    const ENTITY_TYPE: &'static str = "module";
    const DIR_NAME: &'static str = "modules";

    fn id(&self) -> &str {
        &self.id
    }
    fn set_id(&mut self, id: String) {
        self.id = id;
    }
    fn not_found_error(id: String, suggestions: Vec<String>) -> Error {
        Error::module_not_found(id, suggestions)
    }

    /// Override: modules use `{dir}/{id}/{id}.json` pattern.
    fn config_path(id: &str) -> Result<PathBuf> {
        paths::module_manifest(id)
    }
}

// ============================================================================
// Sub-structs (unchanged from original)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementsConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli: Option<DatabaseCliConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseCliConfig {
    pub tables_command: String,
    pub describe_command: String,
    pub query_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CliHelpConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id_help: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub args_help: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    pub tool: String,
    pub display_name: String,
    pub command_template: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_cli_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir_template: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub settings_flags: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<CliHelpConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    pub find_command: String,
    pub base_path_transform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployVerification {
    pub path_pattern: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_error_message: Option<String>,
}

fn default_staging_path() -> String {
    "/tmp/homeboy-staging".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployOverride {
    pub path_pattern: String,
    #[serde(default = "default_staging_path")]
    pub staging_path: String,
    pub install_command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup_command: Option<String>,
    #[serde(default)]
    pub skip_permissions_fix: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionPatternConfig {
    pub extension: String,
    pub pattern: String,
}

/// Configuration for replacing `@since` placeholder tags during version bump.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SinceTagConfig {
    /// File extensions to scan (e.g., [".php"]).
    pub extensions: Vec<String>,
    /// Regex pattern matching placeholder versions in `@since` tags.
    /// Default: `0\.0\.0|NEXT|TBD|TODO|UNRELEASED|x\.x\.x`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder_pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub script_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_build_script: Option<String>,
    /// Default artifact path pattern with template support.
    /// Supports: {component_id}, {local_path}
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_pattern: Option<String>,
    /// Paths to clean up after successful deploy (e.g., node_modules, vendor, target)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cleanup_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_script: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_script: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Desktop app runtime type (python/shell/cli). CLI ignores this field.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub runtime_type: Option<String>,

    /// Shell command to execute when running the module.
    /// Template variables: {{entrypoint}}, {{args}}, {{modulePath}}, plus project context vars.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_command: Option<String>,

    /// Shell command to set up the module (e.g., create venv, install deps).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_command: Option<String>,

    /// Shell command to check if module is ready. Exit 0 = ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_check: Option<String>,

    /// Environment variables to set when running the module.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,

    /// Entry point file (used in template substitution).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<String>,

    /// Default args template (used in template substitution).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<String>,

    /// Default site for this module (used by some CLI modules).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_site: Option<String>,

    /// Desktop app: Python dependencies to install.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<String>>,

    /// Desktop app: Playwright browsers to install.
    #[serde(rename = "playwrightBrowsers", skip_serializing_if = "Option::is_none")]
    pub playwright_browsers: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub input_type: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<SelectOption>>,
    pub arg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub schema: OutputSchema,
    pub display: String,
    pub selectable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionConfig {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub action_type: ActionType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<HttpMethod>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_auth: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Builtin action type (Desktop app only). CLI parses but does not execute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub builtin: Option<BuiltinAction>,
    /// Column identifier for copy-column builtin action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub setting_type: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}
