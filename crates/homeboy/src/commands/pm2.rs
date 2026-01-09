use clap::Args;
use std::collections::HashMap;
use homeboy_core::config::{ConfigManager, ProjectTypeManager, ProjectConfiguration};
use homeboy_core::ssh::{SshClient, execute_local_command};
use homeboy_core::template::{render_map, TemplateVars};

#[derive(Args)]
pub struct Pm2Args {
    /// Project ID
    pub project_id: String,

    /// Execute locally instead of on remote server
    #[arg(long)]
    pub local: bool,

    /// PM2 command and arguments
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

pub fn run(args: Pm2Args) {
    if args.args.is_empty() {
        eprintln!("Error: No command provided");
        eprintln!("Usage: homeboy pm2 <project> [--local] <command...>");
        std::process::exit(1);
    }

    let project = match ConfigManager::load_project(&args.project_id) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let type_def = ProjectTypeManager::resolve(&project.project_type);

    let cli_config = match &type_def.cli {
        Some(c) => c,
        None => {
            eprintln!("Error: Project type '{}' does not support CLI", type_def.display_name);
            std::process::exit(1);
        }
    };

    if cli_config.tool != "pm2" {
        eprintln!(
            "Error: Project '{}' is a {} project (uses '{}', not 'pm2')",
            args.project_id, type_def.display_name, cli_config.tool
        );
        std::process::exit(1);
    }

    if args.local {
        execute_local(&project, cli_config, &args.args);
    } else {
        execute_remote(&project, cli_config, &args.args);
    }
}

fn execute_remote(
    project: &ProjectConfiguration,
    cli_config: &homeboy_core::config::CliConfig,
    args: &[String],
) {
    let server_id = match &project.server_id {
        Some(id) => id,
        None => {
            eprintln!("Error: Server not configured for project '{}'", project.id);
            std::process::exit(1);
        }
    };

    let server = match ConfigManager::load_server(server_id) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let base_path = match &project.base_path {
        Some(p) if !p.is_empty() => p,
        _ => {
            eprintln!("Error: Remote base path not configured for project '{}'", project.id);
            std::process::exit(1);
        }
    };

    let client = match SshClient::from_server(&server, server_id) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let mut variables = HashMap::new();
    variables.insert(TemplateVars::PROJECT_ID.to_string(), project.id.clone());
    variables.insert(TemplateVars::DOMAIN.to_string(), project.domain.clone());
    variables.insert(TemplateVars::ARGS.to_string(), args.join(" "));
    variables.insert(TemplateVars::SITE_PATH.to_string(), base_path.clone());
    variables.insert(
        TemplateVars::CLI_PATH.to_string(),
        cli_config.default_cli_path.clone().unwrap_or_else(|| cli_config.tool.clone()),
    );

    let remote_command = render_map(&cli_config.command_template, &variables);
    let output = client.execute(&remote_command);

    print!("{}", output.stdout);
    if !output.stderr.is_empty() {
        eprint!("{}", output.stderr);
    }

    if !output.success {
        std::process::exit(output.exit_code);
    }
}

fn execute_local(
    project: &ProjectConfiguration,
    cli_config: &homeboy_core::config::CliConfig,
    args: &[String],
) {
    if !project.local_cli.is_configured() {
        eprintln!("Error: Local CLI not configured for project '{}'", project.id);
        eprintln!("Configure 'Local Site Path' in Homeboy.app Settings.");
        std::process::exit(1);
    }

    let cli_path = project
        .local_cli
        .cli_path
        .clone()
        .or_else(|| cli_config.default_cli_path.clone())
        .unwrap_or_else(|| cli_config.tool.clone());

    let mut variables = HashMap::new();
    variables.insert(TemplateVars::PROJECT_ID.to_string(), project.id.clone());
    variables.insert(TemplateVars::DOMAIN.to_string(), project.local_cli.domain.clone());
    variables.insert(TemplateVars::ARGS.to_string(), args.join(" "));
    variables.insert(TemplateVars::SITE_PATH.to_string(), project.local_cli.site_path.clone());
    variables.insert(TemplateVars::CLI_PATH.to_string(), cli_path);

    let local_command = render_map(&cli_config.command_template, &variables);
    let output = execute_local_command(&local_command);

    print!("{}", output.stdout);
    if !output.stderr.is_empty() {
        eprint!("{}", output.stderr);
    }

    if !output.success {
        std::process::exit(output.exit_code);
    }
}
