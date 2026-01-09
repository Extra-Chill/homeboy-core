use clap::{Args, Subcommand};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use homeboy_core::config::{ConfigManager, AppPaths, ProjectConfiguration};
use homeboy_core::module::{load_module, load_all_modules, ModuleManifest, RuntimeType};
use homeboy_core::output::{print_success, print_error};
use homeboy_core::template;

const SYSTEM_PYTHON_PATH: &str = "/opt/homebrew/bin/python3";

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
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Execute a module (Python, Shell, or CLI)
    Run {
        /// Module ID
        module_id: String,
        /// Project ID (defaults to active project)
        #[arg(short, long)]
        project: Option<String>,
        /// Input values as key=value pairs
        #[arg(short, long, value_parser = parse_key_val)]
        input: Vec<(String, String)>,
        /// Arguments to pass to the module (for CLI modules)
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Setup a Python module (create venv and install dependencies)
    Setup {
        /// Module ID
        module_id: String,
    },
}

fn parse_key_val(s: &str) -> Result<(String, String), String> {
    let pos = s.find('=').ok_or_else(|| format!("invalid KEY=value: no `=` found in `{s}`"))?;
    Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
}

pub fn run(args: ModuleArgs) {
    match args.command {
        ModuleCommand::List { project, json } => list(project, json),
        ModuleCommand::Run { module_id, project, input, args } => {
            run_module(&module_id, project, input, args)
        }
        ModuleCommand::Setup { module_id } => setup_module(&module_id),
    }
}

fn list(project: Option<String>, json: bool) {
    let modules = load_all_modules();

    if modules.is_empty() {
        if json {
            print_success::<Vec<()>>(vec![]);
        } else {
            println!("No modules installed.");
            println!("Modules are installed at: {}", AppPaths::modules().display());
        }
        return;
    }

    let project_config: Option<ProjectConfiguration> = project.as_ref().and_then(|id| {
        ConfigManager::load_project(id).ok()
    });

    if json {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ModuleEntry {
            id: String,
            name: String,
            version: String,
            description: String,
            runtime: String,
            compatible: bool,
            ready: bool,
        }

        let entries: Vec<ModuleEntry> = modules
            .iter()
            .map(|m| {
                let ready = is_module_ready(m);
                ModuleEntry {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    version: m.version.clone(),
                    description: m.description.lines().next().unwrap_or("").to_string(),
                    runtime: format!("{:?}", m.runtime.runtime_type).to_lowercase(),
                    compatible: is_module_compatible(m, project_config.as_ref()),
                    ready,
                }
            })
            .collect();

        print_success(entries);
    } else {
        println!("Available modules:\n");
        for m in &modules {
            let compatible = is_module_compatible(m, project_config.as_ref());
            let ready = is_module_ready(m);
            let compat_marker = if compatible { "✓" } else { "✗" };
            let ready_marker = if ready { "" } else { " (needs setup)" };
            let runtime = format!("{:?}", m.runtime.runtime_type).to_lowercase();

            println!("  {} {}{}", compat_marker, m.id, ready_marker);
            println!("    {} (v{})", m.name, m.version);
            println!("    Runtime: {}", runtime);
            if let Some(first_line) = m.description.lines().next() {
                println!("    {}", first_line);
            }
            println!();
        }

        if project_config.is_some() {
            println!("✓ = compatible with project, ✗ = not compatible");
        }
        println!("\nModules needing setup can be initialized with: homeboy module setup <id>");
    }
}

fn run_module(module_id: &str, project: Option<String>, inputs: Vec<(String, String)>, args: Vec<String>) {
    let module = match load_module(module_id) {
        Some(m) => m,
        None => {
            print_error("MODULE_NOT_FOUND", &format!(
                "Module '{}' not found. Use 'homeboy module list' to see available modules.",
                module_id
            ));
            return;
        }
    };

    // Convert inputs to HashMap for easier lookup
    let input_values: HashMap<String, String> = inputs.into_iter().collect();

    match module.runtime.runtime_type {
        RuntimeType::Python => run_python_module(&module, input_values),
        RuntimeType::Shell => run_shell_module(&module, input_values),
        RuntimeType::Cli => run_cli_module(&module, project, input_values, args),
    }
}

fn run_python_module(module: &ModuleManifest, input_values: HashMap<String, String>) {
    let module_path = module.module_path.as_ref().expect("module_path not set");
    let venv_path = format!("{}/venv", module_path);
    let venv_python = format!("{}/bin/python3", venv_path);

    // Determine Python executable
    let python_path = if Path::new(&venv_python).exists() {
        &venv_python
    } else if Path::new(SYSTEM_PYTHON_PATH).exists() {
        eprintln!("Warning: Using system Python. Run 'homeboy module setup {}' for isolated environment.", module.id);
        SYSTEM_PYTHON_PATH
    } else {
        print_error("PYTHON_NOT_FOUND", "Python not found. Install Python or run module setup.");
        return;
    };

    // Build entrypoint path
    let entrypoint = match &module.runtime.entrypoint {
        Some(e) => format!("{}/{}", module_path, e),
        None => {
            print_error("NO_ENTRYPOINT", "Module has no entrypoint defined");
            return;
        }
    };

    // Build arguments from module inputs
    let mut arguments = vec![entrypoint];
    for input in &module.inputs {
        if let Some(value) = input_values.get(&input.id) {
            if !value.is_empty() {
                arguments.push(input.arg.clone());
                arguments.push(value.clone());
            }
        }
    }

    // Set environment for Playwright
    let playwright_path = AppPaths::playwright_browsers().to_string_lossy().to_string();

    eprintln!("$ {} {}", python_path, arguments.join(" "));
    eprintln!();

    let status = Command::new(python_path)
        .args(&arguments)
        .env("PLAYWRIGHT_BROWSERS_PATH", &playwright_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    if let Ok(s) = status {
        if !s.success() {
            std::process::exit(s.code().unwrap_or(1));
        }
    }
}

fn run_shell_module(module: &ModuleManifest, input_values: HashMap<String, String>) {
    let module_path = module.module_path.as_ref().expect("module_path not set");

    // Build entrypoint path
    let entrypoint = match &module.runtime.entrypoint {
        Some(e) => format!("{}/{}", module_path, e),
        None => {
            print_error("NO_ENTRYPOINT", "Module has no entrypoint defined");
            return;
        }
    };

    // Build arguments from module inputs
    let mut arguments = vec![entrypoint];
    for input in &module.inputs {
        if let Some(value) = input_values.get(&input.id) {
            if !value.is_empty() {
                arguments.push(input.arg.clone());
                arguments.push(value.clone());
            }
        }
    }

    eprintln!("$ /bin/bash {}", arguments.join(" "));
    eprintln!();

    let status = Command::new("/bin/bash")
        .args(&arguments)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    if let Ok(s) = status {
        if !s.success() {
            std::process::exit(s.code().unwrap_or(1));
        }
    }
}

fn run_cli_module(
    module: &ModuleManifest,
    project: Option<String>,
    input_values: HashMap<String, String>,
    extra_args: Vec<String>,
) {
    let project_id = match project.or_else(|| {
        ConfigManager::load_app_config().ok().and_then(|c| c.active_project_id)
    }) {
        Some(id) => id,
        None => {
            print_error("NO_PROJECT", "No project specified and no active project set");
            return;
        }
    };

    let project_config = match ConfigManager::load_project(&project_id) {
        Ok(p) => p,
        Err(e) => {
            print_error(e.code(), &e.to_string());
            return;
        }
    };

    // Validate local CLI is configured
    if !project_config.local_cli.is_configured() {
        print_error(
            "LOCAL_CLI_NOT_CONFIGURED",
            &format!("Local CLI not configured for project '{}'. Configure 'Local Site Path' in Homeboy.app Settings.", project_id),
        );
        return;
    }

    // Build module args
    let mut module_args = Vec::new();
    if let Some(ref template_args) = module.runtime.args {
        module_args.push(template_args.clone());
    }
    for input in &module.inputs {
        if let Some(value) = input_values.get(&input.id) {
            if !value.is_empty() {
                module_args.push(format!("{}={}", input.arg, value));
            }
        }
    }
    module_args.extend(extra_args);
    let args_str = module_args.join(" ");

    // Build template variables
    let local_domain = if project_config.local_cli.domain.is_empty() {
        "localhost".to_string()
    } else {
        project_config.local_cli.domain.clone()
    };
    let cli_path = project_config.local_cli.cli_path.clone().unwrap_or_else(|| "wp".to_string());

    // Build command template (for WordPress CLI modules)
    let command_template = "{{cliPath}} --path={{sitePath}} {{args}}";
    let vars = [
        ("projectId", project_config.id.as_str()),
        ("domain", &local_domain),
        ("sitePath", &project_config.local_cli.site_path),
        ("cliPath", &cli_path),
        ("args", &args_str),
    ];

    let command = template::render(command_template, &vars);

    eprintln!("$ {}", command);
    eprintln!();

    let status = Command::new("sh")
        .args(["-c", &command])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    if let Ok(s) = status {
        if !s.success() {
            std::process::exit(s.code().unwrap_or(1));
        }
    }
}

fn setup_module(module_id: &str) {
    let module = match load_module(module_id) {
        Some(m) => m,
        None => {
            print_error("MODULE_NOT_FOUND", &format!("Module '{}' not found", module_id));
            return;
        }
    };

    if module.runtime.runtime_type != RuntimeType::Python {
        println!("Module '{}' is a {:?} module and doesn't require setup.", module_id, module.runtime.runtime_type);
        return;
    }

    let module_path = module.module_path.as_ref().expect("module_path not set");
    let venv_path = format!("{}/venv", module_path);

    println!("Setting up module: {}", module.name);
    println!();

    // Step 1: Create venv
    println!("Creating virtual environment...");
    let venv_status = Command::new(SYSTEM_PYTHON_PATH)
        .args(["-m", "venv", "--copies", &venv_path])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    if let Ok(s) = venv_status {
        if !s.success() {
            print_error("VENV_FAILED", "Failed to create virtual environment");
            return;
        }
    } else {
        print_error("VENV_FAILED", "Failed to run Python venv command");
        return;
    }

    // Step 2: Install dependencies
    if let Some(ref deps) = module.runtime.dependencies {
        if !deps.is_empty() {
            println!();
            println!("Installing dependencies...");

            let pip_path = format!("{}/bin/pip", venv_path);
            let mut pip_args = vec!["install".to_string()];
            pip_args.extend(deps.clone());

            let pip_status = Command::new(&pip_path)
                .args(&pip_args)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status();

            if let Ok(s) = pip_status {
                if !s.success() {
                    print_error("PIP_FAILED", "Failed to install dependencies");
                    return;
                }
            } else {
                print_error("PIP_FAILED", "Failed to run pip command");
                return;
            }
        }
    }

    // Step 3: Install Playwright browsers if needed
    if let Some(ref browsers) = module.runtime.playwright_browsers {
        if !browsers.is_empty() {
            println!();
            println!("Installing Playwright browsers...");

            let venv_python = format!("{}/bin/python3", venv_path);
            let playwright_path = AppPaths::playwright_browsers().to_string_lossy().to_string();

            let mut pw_args = vec!["-m".to_string(), "playwright".to_string(), "install".to_string()];
            pw_args.extend(browsers.clone());

            let pw_status = Command::new(&venv_python)
                .args(&pw_args)
                .env("PLAYWRIGHT_BROWSERS_PATH", &playwright_path)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status();

            if let Ok(s) = pw_status {
                if !s.success() {
                    print_error("PLAYWRIGHT_FAILED", "Failed to install Playwright browsers");
                    return;
                }
            } else {
                print_error("PLAYWRIGHT_FAILED", "Failed to run Playwright install command");
                return;
            }
        }
    }

    println!();
    println!("Setup complete! Run with: homeboy module run {}", module_id);
}

fn is_module_ready(module: &ModuleManifest) -> bool {
    match module.runtime.runtime_type {
        RuntimeType::Python => {
            // Python modules need venv to be ready
            if let Some(ref path) = module.module_path {
                let venv_python = format!("{}/venv/bin/python3", path);
                Path::new(&venv_python).exists()
            } else {
                false
            }
        }
        RuntimeType::Shell | RuntimeType::Cli => true,
    }
}

fn is_module_compatible(module: &ModuleManifest, project: Option<&ProjectConfiguration>) -> bool {
    let Some(project) = project else {
        return true;
    };

    let Some(ref requires) = module.requires else {
        return true;
    };

    // Check project type
    if let Some(ref required_type) = requires.project_type {
        if *required_type != project.project_type {
            return false;
        }
    }

    // Check components
    if let Some(ref required_components) = requires.components {
        for component in required_components {
            if !project.component_ids.contains(component) {
                return false;
            }
        }
    }

    // For CLI modules, check local CLI is configured
    if module.runtime.runtime_type == RuntimeType::Cli && !project.local_cli.is_configured() {
        return false;
    }

    true
}
