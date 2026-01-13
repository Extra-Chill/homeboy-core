use clap::{Args, Subcommand};
use serde::Serialize;
use std::path::Path;
use std::process::{Command, Stdio};

use homeboy::module::{
    is_module_compatible, is_module_linked, is_module_ready, load_all_modules, load_module,
    module_path, run_setup,
};
use homeboy::project::{self, Project};

use crate::commands::CmdResult;

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
    /// Install a module from a git URL or local path
    Install {
        /// Git URL or local path to module directory
        source: String,
        /// Override module id
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
    /// Uninstall a module
    Uninstall {
        /// Module ID
        module_id: String,
        /// Force deletion for git-cloned modules (not needed for symlinked modules)
        #[arg(long)]
        force: bool,
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
        ModuleCommand::Install { source, id } => install_module(&source, id),
        ModuleCommand::Update { module_id, force } => update_module(&module_id, force),
        ModuleCommand::Uninstall { module_id, force } => uninstall_module(&module_id, force),
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
        source: String,
        path: String,
        linked: bool,
    },
    #[serde(rename = "module.update")]
    Update {
        module_id: String,
        url: String,
        path: String,
    },
    #[serde(rename = "module.uninstall")]
    Uninstall {
        module_id: String,
        path: String,
        was_linked: bool,
    },
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

    let project_config: Option<Project> = project
        .as_ref()
        .and_then(|id| project::load(id).ok());

    let entries: Vec<ModuleEntry> = modules
        .iter()
        .map(|module| {
            let ready = is_module_ready(module);
            let compatible = is_module_compatible(module, project_config.as_ref());
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
                configured: true,
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
    let result = homeboy::module::run_module(
        module_id,
        project.as_deref(),
        component.as_deref(),
        inputs,
        args,
    )?;

    Ok((
        ModuleOutput::Run {
            module_id: module_id.to_string(),
            project_id: result.project_id,
        },
        result.exit_code,
    ))
}

fn confirm_dangerous_action(force: bool, message: &str) -> homeboy::Result<()> {
    if force {
        return Ok(());
    }

    Err(homeboy::Error::other(format!(
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

fn install_module(source: &str, id: Option<String>) -> CmdResult<ModuleOutput> {
    let result = homeboy::module::install(source, id.as_deref())?;
    let linked = is_module_linked(&result.module_id);

    Ok((
        ModuleOutput::Install {
            module_id: result.module_id,
            source: result.url,
            path: result.path.to_string_lossy().to_string(),
            linked,
        },
        0,
    ))
}

fn update_module(module_id: &str, force: bool) -> CmdResult<ModuleOutput> {
    let module_dir = module_path(module_id);
    if !module_dir.exists() {
        return Err(homeboy::Error::other(format!(
            "Module '{module_id}' not found",
        )));
    }

    // Check if module is linked (symlink) - linked modules are managed externally
    if is_module_linked(module_id) {
        return Err(homeboy::Error::other(format!(
            "Module '{module_id}' is linked. Update the source directory directly.",
        )));
    }

    if !is_git_workdir_clean(&module_dir) {
        confirm_dangerous_action(
            force,
            "Module has uncommitted changes; update may overwrite them.",
        )?;
    }

    // Load module to get sourceUrl from manifest
    let module = load_module(module_id).ok_or_else(|| {
        homeboy::Error::other(format!(
            "Module '{module_id}' not found or invalid manifest"
        ))
    })?;

    let source_url = module.source_url.ok_or_else(|| {
        homeboy::Error::other(format!(
            "Module '{module_id}' has no sourceUrl. Reinstall with 'homeboy module install <url>'."
        ))
    })?;

    let status = Command::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(&module_dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| homeboy::Error::other(e.to_string()))?;

    if !status.success() {
        return Err(homeboy::Error::other("git pull failed".to_string()));
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
            url: source_url,
            path: module_dir.to_string_lossy().to_string(),
        },
        0,
    ))
}

fn uninstall_module(module_id: &str, force: bool) -> CmdResult<ModuleOutput> {
    let was_linked = is_module_linked(module_id);
    let path = homeboy::module::uninstall(module_id, force)?;

    Ok((
        ModuleOutput::Uninstall {
            module_id: module_id.to_string(),
            path: path.to_string_lossy().to_string(),
            was_linked,
        },
        0,
    ))
}

fn setup_module(module_id: &str) -> CmdResult<ModuleOutput> {
    let result = run_setup(module_id)?;

    Ok((
        ModuleOutput::Setup {
            module_id: module_id.to_string(),
        },
        result.exit_code,
    ))
}

fn run_action(
    module_id: &str,
    action_id: &str,
    project_id: Option<String>,
    data: Option<String>,
) -> CmdResult<ModuleOutput> {
    let response = homeboy::module::run_action(
        module_id,
        action_id,
        project_id.as_deref(),
        data.as_deref(),
    )?;

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

