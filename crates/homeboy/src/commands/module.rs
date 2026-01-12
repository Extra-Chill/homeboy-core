use arboard::Clipboard;
use clap::{Args, Subcommand};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use homeboy_core::config::ModuleScope;
use homeboy_core::config::{AppPaths, ConfigManager, InstalledModuleConfig, ProjectConfiguration};
use homeboy_core::http::ApiClient;
use homeboy_core::module::{load_all_modules, load_module, ModuleManifest};
use homeboy_core::ssh::execute_local_command_interactive;
use homeboy_core::template;

use crate::commands::CmdResult;

struct ModuleExecContext {
    module_id: String,
    project_id: Option<String>,
    component_id: Option<String>,
    settings_json: String,
}

fn module_exec_context_env(context: &ModuleExecContext) -> Vec<(&'static str, String)> {
    use homeboy_core::module::exec_context;

    let mut env: Vec<(&'static str, String)> = vec![
        (exec_context::VERSION, exec_context::CURRENT_VERSION.to_string()),
        (exec_context::MODULE_ID, context.module_id.clone()),
        (exec_context::SETTINGS_JSON, context.settings_json.clone()),
    ];

    if let Some(ref project_id) = context.project_id {
        env.push((exec_context::PROJECT_ID, project_id.clone()));
    }

    if let Some(ref component_id) = context.component_id {
        env.push((exec_context::COMPONENT_ID, component_id.clone()));
    }

    env
}

#[derive(Args)]
pub struct ModuleArgs {
    #[command(subcommand)]
    command: ModuleCommand,
}

#[derive(Subcommand)]
enum ModuleCommand {
    /// Show available modules with compatibility status
    List {
        /// Project ID to filter compatible modules
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Execute a module
    Run {
        /// Module ID
        module_id: String,
        /// Project ID (defaults to active project)
        #[arg(short, long)]
        project: Option<String>,
        /// Component ID (required when ambiguous)
        #[arg(short, long)]
        component: Option<String>,
        /// Input values as key=value pairs
        #[arg(short, long, value_parser = parse_key_val)]
        input: Vec<(String, String)>,
        /// Arguments to pass to the module (for CLI modules)
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Run the module's setup command (if defined)
    Setup {
        /// Module ID
        module_id: String,
    },
    /// Install a module from a git repository URL
    Install {
        /// Git repository URL
        url: String,
        /// Override module id (directory name)
        #[arg(long)]
        id: Option<String>,
    },
    /// Update an installed module (git pull)
    Update {
        /// Module ID
        module_id: String,
        /// Force update even if module has local changes
        #[arg(long)]
        force: bool,
    },
    /// Uninstall a module (remove its directory)
    Uninstall {
        /// Module ID
        module_id: String,
        /// Delete without confirmation
        #[arg(long)]
        force: bool,
    },
    /// Symlink a local module for development
    Link {
        /// Path to local module directory
        path: String,
        /// Override module id (defaults to manifest id)
        #[arg(long)]
        id: Option<String>,
    },
    /// Remove a symlinked module (preserves source directory)
    Unlink {
        /// Module ID
        module_id: String,
    },
    /// Execute a module action (API call or builtin)
    Action {
        /// Module ID
        module_id: String,
        /// Action ID
        action_id: String,
        /// Project ID (required for API actions)
        #[arg(short, long)]
        project: Option<String>,
        /// JSON array of selected data rows
        #[arg(long)]
        data: Option<String>,
    },
}

fn parse_key_val(s: &str) -> Result<(String, String), String> {
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=value: no `=` found in `{s}`"))?;
    Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
}

pub fn run(args: ModuleArgs, _global: &crate::commands::GlobalArgs) -> CmdResult<ModuleOutput> {
    match args.command {
        ModuleCommand::List { project } => list(project),
        ModuleCommand::Run {
            module_id,
            project,
            component,
            input,
            args,
        } => run_module(&module_id, project, component, input, args),
        ModuleCommand::Setup { module_id } => setup_module(&module_id),
        ModuleCommand::Install { url, id } => install_module(&url, id),
        ModuleCommand::Update { module_id, force } => update_module(&module_id, force),
        ModuleCommand::Uninstall { module_id, force } => uninstall_module(&module_id, force),
        ModuleCommand::Link { path, id } => link_module(&path, id),
        ModuleCommand::Unlink { module_id } => unlink_module(&module_id),
        ModuleCommand::Action {
            module_id,
            action_id,
            project,
            data,
        } => run_action(&module_id, &action_id, project, data),
    }
}

#[derive(Serialize)]
#[serde(tag = "command", rename_all = "camelCase")]
pub enum ModuleOutput {
    #[serde(rename = "module.list")]
    List {
        #[serde(skip_serializing_if = "Option::is_none")]
        project_id: Option<String>,
        modules: Vec<ModuleEntry>,
    },
    #[serde(rename = "module.run")]
    Run {
        module_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        project_id: Option<String>,
    },
    #[serde(rename = "module.setup")]
    Setup { module_id: String },
    #[serde(rename = "module.install")]
    Install {
        module_id: String,
        url: String,
        path: String,
    },
    #[serde(rename = "module.update")]
    Update {
        module_id: String,
        url: String,
        path: String,
    },
    #[serde(rename = "module.uninstall")]
    Uninstall { module_id: String, path: String },
    #[serde(rename = "module.link")]
    Link {
        module_id: String,
        source_path: String,
        symlink_path: String,
    },
    #[serde(rename = "module.unlink")]
    Unlink { module_id: String, path: String },
    #[serde(rename = "module.action")]
    Action {
        module_id: String,
        action_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        project_id: Option<String>,
        response: serde_json::Value,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub runtime: String,
    pub compatible: bool,
    pub ready: bool,
    pub configured: bool,
    pub linked: bool,
    pub path: String,
}

fn list(project: Option<String>) -> CmdResult<ModuleOutput> {
    let modules = load_all_modules();

    let app_config = ConfigManager::load_app_config().ok();
    let project_config: Option<ProjectConfiguration> = project
        .as_ref()
        .and_then(|id| ConfigManager::load_project(id).ok());

    let entries: Vec<ModuleEntry> = modules
        .iter()
        .map(|module| {
            let ready = is_module_ready(module);
            let compatible = is_module_compatible(module, project_config.as_ref());

            let configured = app_config
                .as_ref()
                .and_then(|app| app.installed_modules.as_ref())
                .is_some_and(|installed| installed.contains_key(&module.id));

            let linked = is_module_linked(&module.id);

            ModuleEntry {
                id: module.id.clone(),
                name: module.name.clone(),
                version: module.version.clone(),
                description: module
                    .description
                    .as_ref()
                    .and_then(|d| d.lines().next())
                    .unwrap_or("")
                    .to_string(),
                runtime: if module.runtime.is_some() {
                    "executable".to_string()
                } else {
                    "platform".to_string()
                },
                compatible,
                ready,
                configured,
                linked,
                path: module.module_path.clone().unwrap_or_default(),
            }
        })
        .collect();

    Ok((
        ModuleOutput::List {
            project_id: project,
            modules: entries,
        },
        0,
    ))
}

fn run_module(
    module_id: &str,
    project: Option<String>,
    component: Option<String>,
    inputs: Vec<(String, String)>,
    args: Vec<String>,
) -> CmdResult<ModuleOutput> {
    let module = load_module(module_id)
        .ok_or_else(|| homeboy_core::Error::other(format!("Module '{}' not found", module_id)))?;

    let runtime = module.runtime.as_ref().ok_or_else(|| {
        homeboy_core::Error::other(format!(
            "Module '{}' does not have a runtime configuration and cannot be executed",
            module_id
        ))
    })?;

    let run_command = runtime.run_command.as_ref().ok_or_else(|| {
        homeboy_core::Error::other(format!(
            "Module '{}' does not have a runCommand defined",
            module_id
        ))
    })?;

    let app_config = ConfigManager::load_app_config()?;
    let installed_module = app_config
        .installed_modules
        .as_ref()
        .and_then(|m| m.get(module_id));

    if installed_module.is_none() {
        return Err(homeboy_core::Error::config(format!(
            "Module '{}' is not configured. Install it with `homeboy module install <git-url>`.",
            module_id
        )));
    }

    let module_path = module
        .module_path
        .as_ref()
        .ok_or_else(|| homeboy_core::Error::other("module_path not set".to_string()))?;

    let input_values: HashMap<String, String> = inputs.into_iter().collect();

    // Build args string from inputs and trailing args
    let mut argv = Vec::new();
    for input in &module.inputs {
        if let Some(value) = input_values.get(&input.id) {
            if !value.is_empty() {
                argv.push(input.arg.clone());
                argv.push(value.clone());
            }
        }
    }
    argv.extend(args);
    let args_str = argv.join(" ");

    // Check if project context is required
    let requires_project = module.requires.is_some()
        || template::is_present(run_command, "projectId")
        || template::is_present(run_command, "sitePath")
        || template::is_present(run_command, "cliPath")
        || template::is_present(run_command, "domain");

    let mut resolved_project_id: Option<String> = None;
    let mut resolved_component_id: Option<String> = None;
    let mut project_config: Option<ProjectConfiguration> = None;
    let mut component_config = None;

    if requires_project {
        let project_id = project.ok_or_else(|| {
            homeboy_core::Error::other(
                "This module requires a project; pass --project <id>".to_string(),
            )
        })?;

        let loaded_project = ConfigManager::load_project(&project_id)?;
        ModuleScope::validate_project_compatibility(&module, &loaded_project)?;

        resolved_component_id =
            ModuleScope::resolve_component_scope(&module, &loaded_project, component.as_deref())?;

        if let Some(ref comp_id) = resolved_component_id {
            component_config = Some(ConfigManager::load_component(comp_id).map_err(|_| {
                homeboy_core::Error::config(format!(
                    "Component '{}' required by module '{}' is not configured",
                    comp_id, module.id
                ))
            })?);
        }

        resolved_project_id = Some(project_id);
        project_config = Some(loaded_project);
    }

    let effective_settings = ModuleScope::effective_settings(
        module_id,
        installed_module,
        project_config.as_ref(),
        component_config.as_ref(),
    );

    let settings_json = serde_json::to_string(&effective_settings)
        .map_err(|e| homeboy_core::Error::other(e.to_string()))?;

    let exec_context = ModuleExecContext {
        module_id: module_id.to_string(),
        project_id: resolved_project_id.clone(),
        component_id: resolved_component_id.clone(),
        settings_json,
    };

    // Build template variables
    let entrypoint = runtime.entrypoint.clone().unwrap_or_default();
    let domain: String;
    let site_path: String;

    let vars: Vec<(&str, &str)> = if let Some(ref proj) = project_config {
        domain = proj.domain.clone();
        site_path = proj.base_path.clone().unwrap_or_default();

        vec![
            ("modulePath", module_path.as_str()),
            ("entrypoint", entrypoint.as_str()),
            ("args", args_str.as_str()),
            ("projectId", resolved_project_id.as_deref().unwrap_or("")),
            ("domain", domain.as_str()),
            ("sitePath", site_path.as_str()),
        ]
    } else {
        vec![
            ("modulePath", module_path.as_str()),
            ("entrypoint", entrypoint.as_str()),
            ("args", args_str.as_str()),
        ]
    };

    let command = template::render(run_command, &vars);

    // Build environment with module-defined env vars + exec context
    let mut env = module_exec_context_env(&exec_context);
    if let Some(ref module_env) = runtime.env {
        for (key, value) in module_env {
            let rendered_value = template::render(value, &vars);
            env.push((Box::leak(key.clone().into_boxed_str()), rendered_value));
        }
    }
    let env_pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let exit_code =
        execute_local_command_interactive(&command, Some(module_path), Some(&env_pairs));

    Ok((
        ModuleOutput::Run {
            module_id: module_id.to_string(),
            project_id: resolved_project_id,
        },
        exit_code,
    ))
}

fn slugify_module_id(value: &str) -> homeboy_core::Result<String> {
    let mut output = String::new();
    let mut last_was_dash = false;

    for ch in value.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            output.push(lower);
            last_was_dash = false;
            continue;
        }

        if !last_was_dash && !output.is_empty() {
            output.push('-');
            last_was_dash = true;
        }
    }

    while output.ends_with('-') {
        output.pop();
    }

    if output.is_empty() {
        return Err(homeboy_core::Error::other(
            "Unable to derive module id".to_string(),
        ));
    }

    Ok(output)
}

#[derive(Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModuleInstallMetadata {
    source_url: String,
    #[serde(default)]
    linked: bool,
}

fn install_metadata_path(module_id: &str) -> homeboy_core::Result<std::path::PathBuf> {
    Ok(AppPaths::module(module_id)?.join(".install.json"))
}

fn write_install_metadata(module_id: &str, url: &str) -> homeboy_core::Result<()> {
    let path = install_metadata_path(module_id)?;
    let content = serde_json::to_string_pretty(&ModuleInstallMetadata {
        source_url: url.to_string(),
        linked: false,
    })
    .map_err(|err| {
        homeboy_core::Error::internal_json(
            err.to_string(),
            Some("serialize module install metadata".to_string()),
        )
    })?;

    fs::write(path, content).map_err(|err| {
        homeboy_core::Error::internal_io(
            err.to_string(),
            Some("write module install metadata".to_string()),
        )
    })?;
    Ok(())
}

fn read_install_metadata(module_id: &str) -> homeboy_core::Result<ModuleInstallMetadata> {
    let path = install_metadata_path(module_id)?;
    if !path.exists() {
        return Err(homeboy_core::Error::other(format!(
            "No .install.json found for module '{module_id}'. Reinstall it with `homeboy module install`.",
        )));
    }

    let content = fs::read_to_string(path).map_err(|err| {
        homeboy_core::Error::internal_io(
            err.to_string(),
            Some("read module install metadata".to_string()),
        )
    })?;

    serde_json::from_str(&content).map_err(|err| {
        homeboy_core::Error::internal_json(
            err.to_string(),
            Some("parse module install metadata".to_string()),
        )
    })
}

fn derive_module_id_from_url(url: &str) -> homeboy_core::Result<String> {
    let trimmed = url.trim_end_matches('/');
    let segment = trimmed
        .split('/')
        .next_back()
        .unwrap_or(trimmed)
        .trim_end_matches(".git");

    slugify_module_id(segment)
}

fn confirm_dangerous_action(force: bool, message: &str) -> homeboy_core::Result<()> {
    if force {
        return Ok(());
    }

    Err(homeboy_core::Error::other(format!(
        "{message} Re-run with --force to confirm.",
    )))
}

fn is_git_workdir_clean(path: &Path) -> bool {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output();

    match output {
        Ok(output) => output.status.success() && output.stdout.is_empty(),
        Err(_) => false,
    }
}

fn install_module(url: &str, id: Option<String>) -> CmdResult<ModuleOutput> {
    let module_id = match id {
        Some(id) => slugify_module_id(&id)?,
        None => derive_module_id_from_url(url)?,
    };

    let module_dir = AppPaths::module(&module_id)?;
    if module_dir.exists() {
        return Err(homeboy_core::Error::other(format!(
            "Module '{module_id}' already exists",
        )));
    }

    AppPaths::ensure_directories()?;

    let status = Command::new("git")
        .args(["clone", url, module_dir.to_string_lossy().as_ref()])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| homeboy_core::Error::other(e.to_string()))?;

    if !status.success() {
        return Err(homeboy_core::Error::other("git clone failed".to_string()));
    }

    write_install_metadata(&module_id, url)?;

    let mut app_config = ConfigManager::load_app_config()?;
    let installed_modules = app_config
        .installed_modules
        .get_or_insert_with(Default::default);
    installed_modules
        .entry(module_id.clone())
        .and_modify(|existing| {
            if existing.source_url.is_none() {
                existing.source_url = Some(url.to_string());
            }
        })
        .or_insert_with(|| InstalledModuleConfig {
            settings: Default::default(),
            source_url: Some(url.to_string()),
        });
    ConfigManager::save_app_config(&app_config)?;

    // Auto-run setup if module defines a setup_command
    if let Some(module) = load_module(&module_id) {
        if module
            .runtime
            .as_ref()
            .is_some_and(|r| r.setup_command.is_some())
        {
            let _ = setup_module(&module_id)?;
        }
    }

    Ok((
        ModuleOutput::Install {
            module_id: module_id.clone(),
            url: url.to_string(),
            path: module_dir.to_string_lossy().to_string(),
        },
        0,
    ))
}

fn update_module(module_id: &str, force: bool) -> CmdResult<ModuleOutput> {
    let module_dir = AppPaths::module(module_id)?;
    if !module_dir.exists() {
        return Err(homeboy_core::Error::other(format!(
            "Module '{module_id}' not found",
        )));
    }

    if !is_git_workdir_clean(&module_dir) {
        confirm_dangerous_action(
            force,
            "Module has uncommitted changes; update may overwrite them.",
        )?;
    }

    let metadata = read_install_metadata(module_id)?;

    let status = Command::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(&module_dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| homeboy_core::Error::other(e.to_string()))?;

    if !status.success() {
        return Err(homeboy_core::Error::other("git pull failed".to_string()));
    }

    // Auto-run setup if module defines a setup_command
    if let Some(module) = load_module(module_id) {
        if module
            .runtime
            .as_ref()
            .is_some_and(|r| r.setup_command.is_some())
        {
            let _ = setup_module(module_id)?;
        }
    }

    Ok((
        ModuleOutput::Update {
            module_id: module_id.to_string(),
            url: metadata.source_url,
            path: module_dir.to_string_lossy().to_string(),
        },
        0,
    ))
}

fn uninstall_module(module_id: &str, force: bool) -> CmdResult<ModuleOutput> {
    let module_dir = AppPaths::module(module_id)?;
    if !module_dir.exists() {
        return Err(homeboy_core::Error::other(format!(
            "Module '{module_id}' not found",
        )));
    }

    confirm_dangerous_action(force, "This will permanently remove the module")?;

    fs::remove_dir_all(&module_dir).map_err(|err| {
        homeboy_core::Error::internal_io(
            err.to_string(),
            Some("remove module directory".to_string()),
        )
    })?;

    Ok((
        ModuleOutput::Uninstall {
            module_id: module_id.to_string(),
            path: module_dir.to_string_lossy().to_string(),
        },
        0,
    ))
}

fn setup_module(module_id: &str) -> CmdResult<ModuleOutput> {
    let module = load_module(module_id)
        .ok_or_else(|| homeboy_core::Error::other(format!("Module '{}' not found", module_id)))?;

    let runtime = match module.runtime.as_ref() {
        Some(r) => r,
        None => {
            return Ok((
                ModuleOutput::Setup {
                    module_id: module_id.to_string(),
                },
                0,
            ));
        }
    };

    let setup_command = match &runtime.setup_command {
        Some(cmd) => cmd,
        None => {
            return Ok((
                ModuleOutput::Setup {
                    module_id: module_id.to_string(),
                },
                0,
            ));
        }
    };

    let module_path = module
        .module_path
        .as_ref()
        .ok_or_else(|| homeboy_core::Error::other("module_path not set".to_string()))?;

    let entrypoint = runtime.entrypoint.clone().unwrap_or_default();
    let vars: Vec<(&str, &str)> = vec![
        ("modulePath", module_path.as_str()),
        ("entrypoint", entrypoint.as_str()),
    ];

    let command = template::render(setup_command, &vars);

    let exit_code = execute_local_command_interactive(&command, Some(module_path), None);

    if exit_code != 0 {
        return Err(homeboy_core::Error::other(format!(
            "Setup command failed with exit code {}",
            exit_code
        )));
    }

    Ok((
        ModuleOutput::Setup {
            module_id: module_id.to_string(),
        },
        0,
    ))
}

fn is_module_ready(module: &ModuleManifest) -> bool {
    let Some(runtime) = module.runtime.as_ref() else {
        // Modules without runtime (platform modules) are always ready
        return true;
    };

    // If module has a ready_check command, run it
    if let Some(ref ready_check) = runtime.ready_check {
        if let Some(ref module_path) = module.module_path {
            let entrypoint = runtime.entrypoint.clone().unwrap_or_default();
            let vars: Vec<(&str, &str)> = vec![
                ("modulePath", module_path.as_str()),
                ("entrypoint", entrypoint.as_str()),
            ];
            let command = template::render(ready_check, &vars);
            let exit_code = execute_local_command_interactive(&command, Some(module_path), None);
            return exit_code == 0;
        }
        return false;
    }

    // No ready_check defined - assume ready
    true
}

fn is_module_compatible(module: &ModuleManifest, project: Option<&ProjectConfiguration>) -> bool {
    let Some(project) = project else {
        return true;
    };

    let Some(ref requires) = module.requires else {
        return true;
    };

    // Check required modules
    for required_module in &requires.modules {
        if !project.has_module(required_module) {
            return false;
        }
    }

    // Check components
    for component in &requires.components {
        if !project.component_ids.contains(component) {
            return false;
        }
    }

    true
}

fn is_module_linked(module_id: &str) -> bool {
    AppPaths::module(module_id)
        .map(|p| p.is_symlink())
        .unwrap_or(false)
}

fn link_module(path: &str, id: Option<String>) -> CmdResult<ModuleOutput> {
    let source_path = Path::new(path);

    // Resolve to absolute path
    let source_path = if source_path.is_absolute() {
        source_path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| homeboy_core::Error::other(e.to_string()))?
            .join(source_path)
    };

    if !source_path.exists() {
        return Err(homeboy_core::Error::other(format!(
            "Source path does not exist: {}",
            source_path.display()
        )));
    }

    // Validate homeboy.json exists
    let manifest_path = source_path.join("homeboy.json");
    if !manifest_path.exists() {
        return Err(homeboy_core::Error::other(format!(
            "No homeboy.json found at {}",
            source_path.display()
        )));
    }

    // Read manifest to get module id if not provided
    let manifest_content = fs::read_to_string(&manifest_path).map_err(|e| {
        homeboy_core::Error::internal_io(e.to_string(), Some("read module manifest".to_string()))
    })?;
    let manifest: ModuleManifest = serde_json::from_str(&manifest_content).map_err(|e| {
        homeboy_core::Error::config_invalid_json(manifest_path.to_string_lossy().to_string(), e)
    })?;

    let module_id = match id {
        Some(id) => slugify_module_id(&id)?,
        None => manifest.id.clone(),
    };

    if module_id.is_empty() {
        return Err(homeboy_core::Error::other(
            "Module id is empty. Provide --id or ensure manifest has an id field.".to_string(),
        ));
    }

    let module_dir = AppPaths::module(&module_id)?;
    if module_dir.exists() {
        return Err(homeboy_core::Error::other(format!(
            "Module '{}' already exists at {}",
            module_id,
            module_dir.display()
        )));
    }

    AppPaths::ensure_directories()?;

    // Create symlink
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source_path, &module_dir).map_err(|e| {
        homeboy_core::Error::internal_io(e.to_string(), Some("create symlink".to_string()))
    })?;

    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&source_path, &module_dir).map_err(|e| {
        homeboy_core::Error::internal_io(e.to_string(), Some("create symlink".to_string()))
    })?;

    // Write install metadata with linked: true
    let metadata_path = module_dir.join(".install.json");
    let metadata = ModuleInstallMetadata {
        source_url: source_path.to_string_lossy().to_string(),
        linked: true,
    };
    let metadata_content = serde_json::to_string_pretty(&metadata).map_err(|e| {
        homeboy_core::Error::internal_json(
            e.to_string(),
            Some("serialize module install metadata".to_string()),
        )
    })?;
    fs::write(&metadata_path, metadata_content).map_err(|e| {
        homeboy_core::Error::internal_io(
            e.to_string(),
            Some("write module install metadata".to_string()),
        )
    })?;

    // Register in app config
    let mut app_config = ConfigManager::load_app_config()?;
    let installed_modules = app_config
        .installed_modules
        .get_or_insert_with(Default::default);
    installed_modules
        .entry(module_id.clone())
        .or_insert_with(|| InstalledModuleConfig {
            settings: Default::default(),
            source_url: Some(source_path.to_string_lossy().to_string()),
        });
    ConfigManager::save_app_config(&app_config)?;

    Ok((
        ModuleOutput::Link {
            module_id,
            source_path: source_path.to_string_lossy().to_string(),
            symlink_path: module_dir.to_string_lossy().to_string(),
        },
        0,
    ))
}

fn unlink_module(module_id: &str) -> CmdResult<ModuleOutput> {
    let module_dir = AppPaths::module(module_id)?;

    if !module_dir.exists() {
        return Err(homeboy_core::Error::other(format!(
            "Module '{}' not found",
            module_id
        )));
    }

    if !module_dir.is_symlink() {
        return Err(homeboy_core::Error::other(format!(
            "Module '{}' is not a symlink. Use `uninstall` to remove git-cloned modules.",
            module_id
        )));
    }

    // Remove the symlink (this does not delete the source directory)
    fs::remove_file(&module_dir).map_err(|e| {
        homeboy_core::Error::internal_io(e.to_string(), Some("remove symlink".to_string()))
    })?;

    // Remove from app config
    let mut app_config = ConfigManager::load_app_config()?;
    if let Some(ref mut installed_modules) = app_config.installed_modules {
        installed_modules.remove(module_id);
    }
    ConfigManager::save_app_config(&app_config)?;

    Ok((
        ModuleOutput::Unlink {
            module_id: module_id.to_string(),
            path: module_dir.to_string_lossy().to_string(),
        },
        0,
    ))
}

fn run_action(
    module_id: &str,
    action_id: &str,
    project_id: Option<String>,
    data: Option<String>,
) -> CmdResult<ModuleOutput> {
    let module = load_module(module_id)
        .ok_or_else(|| homeboy_core::Error::other(format!("Module '{}' not found", module_id)))?;

    // Find the action in the module manifest
    if module.actions.is_empty() {
        return Err(homeboy_core::Error::other(format!(
            "Module '{}' has no actions defined",
            module_id
        )));
    }

    let action = module
        .actions
        .iter()
        .find(|a| a.id == action_id)
        .ok_or_else(|| {
            homeboy_core::Error::other(format!(
                "Action '{}' not found in module '{}'",
                action_id, module_id
            ))
        })?;

    // Parse the selected data
    let selected: Vec<Value> = if let Some(data_str) = &data {
        serde_json::from_str(data_str).map_err(|e| {
            homeboy_core::Error::other(format!("Invalid JSON data: {}", e))
        })?
    } else {
        Vec::new()
    };

    // Handle based on action type
    match action.action_type.as_str() {
        "api" => {
            // API actions require a project
            let project_id = project_id.ok_or_else(|| {
                homeboy_core::Error::other("--project is required for API actions")
            })?;

            let project = ConfigManager::load_project(&project_id)?;
            let client = ApiClient::new(&project_id, &project.api)?;

            // Check auth if required
            if action.requires_auth.unwrap_or(false) && !client.is_authenticated() {
                return Err(homeboy_core::Error::other(
                    "Not authenticated. Run 'homeboy auth login --project <id>' first.",
                ));
            }

            // Build payload by interpolating templates
            let endpoint = action.endpoint.as_ref().ok_or_else(|| {
                homeboy_core::Error::other("API action missing 'endpoint'")
            })?;

            let method = action.method.as_deref().unwrap_or("POST");

            // Get module settings for interpolation
            let settings = get_module_settings(module_id, Some(&project_id), None)?;

            // Interpolate payload
            let payload = interpolate_action_payload(action, &selected, &settings)?;

            // Make the request
            let response = if method == "GET" {
                client.get(endpoint)?
            } else {
                client.post(endpoint, &payload)?
            };

            Ok((
                ModuleOutput::Action {
                    module_id: module_id.to_string(),
                    action_id: action_id.to_string(),
                    project_id: Some(project_id),
                    response,
                },
                0,
            ))
        }
        "builtin" => {
            let builtin = action.builtin.as_ref().ok_or_else(|| {
                homeboy_core::Error::other("Builtin action missing 'builtin' field")
            })?;

            let response = match builtin.as_str() {
                "copy-column" => {
                    let column = action.column.as_ref().ok_or_else(|| {
                        homeboy_core::Error::other("copy-column action missing 'column' field")
                    })?;

                    let values: Vec<String> = selected
                        .iter()
                        .filter_map(|row| {
                            row.get(column).and_then(|v| match v {
                                Value::String(s) => Some(s.clone()),
                                _ => Some(v.to_string()),
                            })
                        })
                        .collect();

                    let text = values.join("\n");

                    // Copy to clipboard
                    let mut clipboard = Clipboard::new().map_err(|e| {
                        homeboy_core::Error::other(format!("Failed to access clipboard: {}", e))
                    })?;
                    clipboard.set_text(&text).map_err(|e| {
                        homeboy_core::Error::other(format!("Failed to copy to clipboard: {}", e))
                    })?;

                    serde_json::json!({
                        "action": "copy-column",
                        "column": column,
                        "count": values.len(),
                        "copied": true
                    })
                }
                "export-csv" => {
                    // Generate CSV and output to stdout
                    let csv = generate_csv(&selected)?;
                    print!("{}", csv);

                    serde_json::json!({
                        "action": "export-csv",
                        "rows": selected.len()
                    })
                }
                "copy-json" => {
                    let json = serde_json::to_string_pretty(&selected).map_err(|e| {
                        homeboy_core::Error::other(format!("Failed to serialize JSON: {}", e))
                    })?;

                    // Copy to clipboard
                    let mut clipboard = Clipboard::new().map_err(|e| {
                        homeboy_core::Error::other(format!("Failed to access clipboard: {}", e))
                    })?;
                    clipboard.set_text(&json).map_err(|e| {
                        homeboy_core::Error::other(format!("Failed to copy to clipboard: {}", e))
                    })?;

                    serde_json::json!({
                        "action": "copy-json",
                        "count": selected.len(),
                        "copied": true
                    })
                }
                _ => {
                    return Err(homeboy_core::Error::other(format!(
                        "Unknown builtin action: {}",
                        builtin
                    )));
                }
            };

            Ok((
                ModuleOutput::Action {
                    module_id: module_id.to_string(),
                    action_id: action_id.to_string(),
                    project_id,
                    response,
                },
                0,
            ))
        }
        other => Err(homeboy_core::Error::other(format!(
            "Unknown action type: {}",
            other
        ))),
    }
}

fn get_module_settings(
    module_id: &str,
    project_id: Option<&str>,
    _component_id: Option<&str>,
) -> homeboy_core::Result<HashMap<String, Value>> {
    let mut settings = HashMap::new();

    // Load from app config
    let app_config = ConfigManager::load_app_config()?;
    if let Some(installed) = app_config.installed_modules.as_ref() {
        if let Some(module_config) = installed.get(module_id) {
            for (k, v) in &module_config.settings {
                settings.insert(k.clone(), v.clone());
            }
        }
    }

    // Load from project config (overrides app settings)
    if let Some(pid) = project_id {
        if let Ok(project) = ConfigManager::load_project(pid) {
            if let Some(scoped) = project.scoped_modules.as_ref() {
                if let Some(module_scope) = scoped.get(module_id) {
                    for (k, v) in &module_scope.settings {
                        settings.insert(k.clone(), v.clone());
                    }
                }
            }
        }
    }

    Ok(settings)
}

fn interpolate_action_payload(
    action: &homeboy_core::module::ActionConfig,
    selected: &[Value],
    settings: &HashMap<String, Value>,
) -> homeboy_core::Result<Value> {
    let payload_template = match &action.payload {
        Some(p) => p,
        None => return Ok(Value::Object(serde_json::Map::new())),
    };

    let mut result = serde_json::Map::new();

    for (key, value) in payload_template {
        let interpolated = interpolate_payload_value(value, selected, settings)?;
        result.insert(key.clone(), interpolated);
    }

    Ok(Value::Object(result))
}

fn interpolate_payload_value(
    value: &Value,
    selected: &[Value],
    settings: &HashMap<String, Value>,
) -> homeboy_core::Result<Value> {
    match value {
        Value::String(template) => {
            if template == "{{selected}}" {
                // Return selected rows as array
                Ok(Value::Array(selected.to_vec()))
            } else if template.starts_with("{{settings.") && template.ends_with("}}") {
                // Extract setting key
                let key = &template[11..template.len() - 2];
                Ok(settings
                    .get(key)
                    .cloned()
                    .unwrap_or(Value::String(String::new())))
            } else {
                Ok(Value::String(template.clone()))
            }
        }
        Value::Array(arr) => {
            let interpolated: Result<Vec<Value>, _> = arr
                .iter()
                .map(|v| interpolate_payload_value(v, selected, settings))
                .collect();
            Ok(Value::Array(interpolated?))
        }
        Value::Object(obj) => {
            let mut result = serde_json::Map::new();
            for (k, v) in obj {
                result.insert(k.clone(), interpolate_payload_value(v, selected, settings)?);
            }
            Ok(Value::Object(result))
        }
        _ => Ok(value.clone()),
    }
}

fn generate_csv(rows: &[Value]) -> homeboy_core::Result<String> {
    if rows.is_empty() {
        return Ok(String::new());
    }

    // Get headers from first row
    let headers: Vec<String> = match &rows[0] {
        Value::Object(obj) => obj.keys().cloned().collect(),
        _ => return Err(homeboy_core::Error::other("Expected array of objects")),
    };

    let mut csv = String::new();

    // Header row
    csv.push_str(&headers.join(","));
    csv.push('\n');

    // Data rows
    for row in rows {
        if let Value::Object(obj) = row {
            let values: Vec<String> = headers
                .iter()
                .map(|h| {
                    obj.get(h)
                        .map(|v| escape_csv_field(v))
                        .unwrap_or_default()
                })
                .collect();
            csv.push_str(&values.join(","));
            csv.push('\n');
        }
    }

    Ok(csv)
}

fn escape_csv_field(value: &Value) -> String {
    let s = match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        _ => value.to_string(),
    };

    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}
