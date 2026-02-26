use std::path::{Path, PathBuf};

use crate::component::{self, Component, ScopedModuleConfig};
use crate::error::{Error, Result};
use crate::ssh::{execute_local_command_passthrough, CommandOutput};
use crate::utils::{io, shell};

/// Output from a module runner script execution.
pub struct RunnerOutput {
    pub exit_code: i32,
    pub success: bool,
}

/// Orchestrates module script execution for test/lint runners.
///
/// Encapsulates the shared logic for finding components, resolving modules,
/// loading manifests, merging settings, and executing runner scripts.
pub struct ModuleRunner {
    component_id: String,
    script_path: String, // Relative to module root (e.g., "scripts/lint/lint-runner.sh")
    settings_overrides: Vec<(String, String)>,
    env_vars: Vec<(String, String)>,
    script_args: Vec<String>,
    path_override: Option<String>,
}

impl ModuleRunner {
    /// Create a new ModuleRunner for a component and script.
    ///
    /// - `component_id`: The component to run the script for
    /// - `script_path`: Path to the script relative to module root (e.g., "scripts/lint/lint-runner.sh")
    pub fn new(component_id: &str, script_path: &str) -> Self {
        Self {
            component_id: component_id.to_string(),
            script_path: script_path.to_string(),
            settings_overrides: Vec::new(),
            env_vars: Vec::new(),
            script_args: Vec::new(),
            path_override: None,
        }
    }

    /// Override the component's `local_path` for this execution.
    ///
    /// Use this when running against a workspace clone or temporary checkout
    /// instead of the configured component path.
    pub fn path_override(mut self, path: Option<String>) -> Self {
        self.path_override = path;
        self
    }

    /// Add settings overrides from key=value pairs.
    pub fn settings(mut self, overrides: &[(String, String)]) -> Self {
        self.settings_overrides.extend(overrides.iter().cloned());
        self
    }

    /// Add an environment variable.
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env_vars.push((key.to_string(), value.to_string()));
        self
    }

    /// Add an environment variable if condition is true.
    pub fn env_if(mut self, condition: bool, key: &str, value: &str) -> Self {
        if condition {
            self.env_vars.push((key.to_string(), value.to_string()));
        }
        self
    }

    /// Add an environment variable if the Option is Some.
    pub fn env_opt(mut self, key: &str, value: &Option<String>) -> Self {
        if let Some(v) = value {
            self.env_vars.push((key.to_string(), v.clone()));
        }
        self
    }

    /// Add arguments to pass to the script.
    pub fn script_args(mut self, args: &[String]) -> Self {
        self.script_args.extend(args.iter().cloned());
        self
    }

    /// Execute the module runner script.
    ///
    /// Performs the full orchestration:
    /// 1. Load component configuration
    /// 2. Determine module from component config
    /// 3. Find module path
    /// 4. Validate script exists
    /// 5. Load manifest
    /// 6. Merge settings (manifest defaults → component → overrides)
    /// 7. Prepare environment variables
    /// 8. Execute via shell
    pub fn run(&self) -> Result<RunnerOutput> {
        let component = self.find_component()?;
        let (module_name, module_settings) = self.determine_module(&component)?;
        let module_path = self.find_module_path(&module_name)?;

        self.validate_script_exists(&module_path)?;

        let manifest = self.load_module_manifest(&module_path)?;
        let settings_json = self.merge_settings(&manifest, &module_settings)?;
        let project_path = PathBuf::from(&component.local_path);
        let env_vars = self.prepare_env_vars(&module_path, &project_path, &settings_json);

        let output = self.execute_script(&module_path, &env_vars)?;

        Ok(RunnerOutput {
            exit_code: output.exit_code,
            success: output.success,
        })
    }

    fn find_component(&self) -> Result<Component> {
        let mut comp = component::load(&self.component_id)?;
        if let Some(ref path) = self.path_override {
            comp.local_path = path.clone();
        }
        Ok(comp)
    }

    fn determine_module(&self, component: &Component) -> Result<(String, Vec<(String, String)>)> {
        let modules = component.modules.as_ref().ok_or_else(|| {
            Error::validation_invalid_argument(
                "component",
                format!(
                    "Component '{}' has no modules configured for {}",
                    component.id,
                    self.script_description()
                ),
                None,
                None,
            )
            .with_hint(format!(
                "Add a module: homeboy component set {} --module <module_id>",
                component.id
            ))
        })?;

        // Prefer wordpress module if available
        if modules.contains_key("wordpress") {
            let settings = extract_module_settings(
                modules
                    .get("wordpress")
                    .expect("wordpress module checked above"),
            );
            return Ok(("wordpress".to_string(), settings));
        }

        // Otherwise use the first available module
        if let Some((module_name, module_config)) = modules.iter().next() {
            let settings = extract_module_settings(module_config);
            return Ok((module_name.clone(), settings));
        }

        Err(Error::validation_invalid_argument(
            "component",
            format!(
                "Component '{}' has no modules configured for {}",
                component.id,
                self.script_description()
            ),
            None,
            None,
        )
        .with_hint(format!(
            "Add a module: homeboy component set {} --module <module_id>",
            component.id
        )))
    }

    fn find_module_path(&self, module_name: &str) -> Result<PathBuf> {
        let module_path = super::module_path(module_name);

        if module_path.exists() {
            Ok(module_path)
        } else {
            Err(Error::validation_invalid_argument(
                "module",
                format!(
                    "Module '{}' not found in ~/.config/homeboy/modules/",
                    module_name
                ),
                None,
                None,
            ))
        }
    }

    fn validate_script_exists(&self, module_path: &Path) -> Result<()> {
        let script_path = module_path.join(&self.script_path);
        if !script_path.exists() {
            return Err(Error::validation_invalid_argument(
                "module",
                format!(
                    "Module at {} does not have {} infrastructure (missing {})",
                    module_path.display(),
                    self.script_description(),
                    self.script_path
                ),
                None,
                None,
            ));
        }
        Ok(())
    }

    fn load_module_manifest(&self, module_path: &Path) -> Result<serde_json::Value> {
        let module_name = module_path
            .file_name()
            .ok_or_else(|| Error::internal_io("Module path has no file name".to_string(), None))?
            .to_string_lossy();
        let manifest_path = module_path.join(format!("{}.json", module_name));

        if !manifest_path.exists() {
            return Err(Error::internal_io(
                format!("Module manifest not found: {}", manifest_path.display()),
                None,
            ));
        }

        let content = io::read_file(&manifest_path, &format!("read {}", manifest_path.display()))?;

        serde_json::from_str(&content).map_err(|e| {
            Error::validation_invalid_json(e, Some("parse manifest".to_string()), None)
        })
    }

    fn merge_settings(
        &self,
        manifest: &serde_json::Value,
        module_settings: &[(String, String)],
    ) -> Result<String> {
        let mut settings = serde_json::json!({});

        // Start with manifest defaults
        if let Some(manifest_settings) = manifest.get("settings") {
            if let Some(settings_array) = manifest_settings.as_array() {
                if let serde_json::Value::Object(ref mut obj) = settings {
                    for setting in settings_array {
                        if let (Some(id), Some(default)) = (
                            setting.get("id").and_then(|v| v.as_str()),
                            setting.get("default").and_then(|v| v.as_str()),
                        ) {
                            obj.insert(
                                id.to_string(),
                                serde_json::Value::String(default.to_string()),
                            );
                        }
                    }
                }
            }
        }

        if let serde_json::Value::Object(ref mut obj) = settings {
            // Add module settings from component config
            for (key, value) in module_settings {
                obj.insert(key.clone(), serde_json::Value::String(value.clone()));
            }

            // Add user overrides (these take precedence)
            for (key, value) in &self.settings_overrides {
                obj.insert(key.clone(), serde_json::Value::String(value.clone()));
            }
        }

        crate::config::to_json_string(&settings)
    }

    fn prepare_env_vars(
        &self,
        module_path: &Path,
        project_path: &Path,
        settings_json: &str,
    ) -> Vec<(String, String)> {
        let module_name = module_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".to_string());

        let component_path = project_path.to_string_lossy();
        let mut env = super::execution::build_exec_env(
            &module_name,
            None, // no project context in runner
            Some(&self.component_id),
            settings_json,
            Some(&module_path.to_string_lossy()),
            None, // no project base_path in runner
            None, // no individual settings
            Some(&component_path), // path_override (respects --path flag)
        );

        // Add command-specific environment variables (e.g. HOMEBOY_SKIP)
        env.extend(self.env_vars.iter().cloned());

        env
    }

    fn execute_script(
        &self,
        module_path: &Path,
        env_vars: &[(String, String)],
    ) -> Result<CommandOutput> {
        let script_path = module_path.join(&self.script_path);
        let mut command = shell::quote_path(&script_path.to_string_lossy());

        // Append script arguments if any
        if !self.script_args.is_empty() {
            command.push(' ');
            command.push_str(&shell::quote_args(&self.script_args));
        }

        let env_refs: Vec<(&str, &str)> = env_vars
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        Ok(execute_local_command_passthrough(
            &command,
            None,
            Some(&env_refs),
        ))
    }

    fn script_description(&self) -> &str {
        if self.script_path.contains("test") {
            "test"
        } else if self.script_path.contains("lint") {
            "lint"
        } else {
            "script"
        }
    }
}

fn extract_module_settings(module_config: &ScopedModuleConfig) -> Vec<(String, String)> {
    let mut settings = Vec::new();
    for (key, value) in &module_config.settings {
        if let Some(str_val) = value.as_str() {
            settings.push((key.clone(), str_val.to_string()));
        }
    }
    settings
}
