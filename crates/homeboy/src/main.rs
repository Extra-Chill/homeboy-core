use clap::{Parser, Subcommand};
use serde_json;

#[derive(Debug, Clone, Copy)]
enum ResponseMode {
    Json,
    InteractivePassthrough,
}

mod commands;
mod docs;

use commands::{
    build, changelog, component, config, db, deploy, docs as docs_command, doctor, error, file,
    git, logs, module, pm2, project, server, ssh, version, wp,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "homeboy")]
#[command(version = VERSION)]
#[command(about = "CLI tool for development and deployment automation")]
struct Cli {
    /// JSON input spec override.
    ///
    /// Use "-" to read from stdin, "@file.json" to read from a file, or an inline JSON string.
    #[arg(long, global = true)]
    json: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage project configuration
    Project(project::ProjectArgs),
    /// SSH into a project server or configured server
    Ssh(ssh::SshArgs),
    /// Run WP-CLI commands on WordPress projects
    Wp(wp::WpArgs),
    /// Run PM2 commands on Node.js projects
    Pm2(pm2::Pm2Args),
    /// Manage SSH server configurations
    Server(server::ServerArgs),
    /// Database operations
    Db(db::DbArgs),
    /// Remote file operations
    File(file::FileArgs),
    /// Remote log viewing
    Logs(logs::LogsArgs),
    /// Deploy components to remote server
    Deploy(deploy::DeployArgs),
    /// Manage standalone component configurations
    Component(component::ComponentArgs),
    /// Manage global config.json
    Config(config::ConfigArgs),
    /// Execute CLI-compatible modules
    Module(module::ModuleArgs),
    /// Display CLI documentation
    Docs(docs_command::DocsArgs),
    /// Changelog operations
    Changelog(changelog::ChangelogArgs),
    /// Git operations for components
    Git(git::GitArgs),
    /// Version management for components
    Version(version::VersionArgs),
    /// Build a component
    Build(build::BuildArgs),
    /// Diagnose configuration problems
    Doctor(doctor::DoctorArgs),
    /// Error code registry and explanations
    Error(error::ErrorArgs),
}

fn response_mode(command: &Commands) -> ResponseMode {
    match command {
        Commands::Ssh(args) if args.command.is_none() => ResponseMode::InteractivePassthrough,
        Commands::Logs(args) if logs::is_interactive(args) => ResponseMode::InteractivePassthrough,
        _ => ResponseMode::Json,
    }
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    let mode = response_mode(&cli.command);

    match mode {
        ResponseMode::Json => {}
        ResponseMode::InteractivePassthrough => {
            if !homeboy_core::tty::require_tty_for_interactive() {
                let err = homeboy_core::Error::validation_invalid_argument(
                    "tty",
                    "This command requires an interactive TTY",
                    None,
                    None,
                );
                homeboy_core::output::print_result::<serde_json::Value>(Err(err));
                return std::process::ExitCode::from(exit_code_to_u8(2));
            }
        }
    }

    let (json_result, exit_code) = match cli.command {
        Commands::Project(args) => homeboy_core::output::map_cmd_result_to_json(
            project::run(args, cli.json.as_deref())
                .map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Ssh(args) => homeboy_core::output::map_cmd_result_to_json(
            ssh::run(args, cli.json.as_deref()).map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Wp(args) => homeboy_core::output::map_cmd_result_to_json(
            wp::run(args, cli.json.as_deref()).map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Pm2(args) => homeboy_core::output::map_cmd_result_to_json(
            pm2::run(args, cli.json.as_deref()).map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Server(args) => homeboy_core::output::map_cmd_result_to_json(
            server::run(args, cli.json.as_deref())
                .map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Db(args) => homeboy_core::output::map_cmd_result_to_json(
            db::run(args, cli.json.as_deref()).map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::File(args) => homeboy_core::output::map_cmd_result_to_json(
            file::run(args, cli.json.as_deref()).map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Logs(args) => homeboy_core::output::map_cmd_result_to_json(
            logs::run(args, cli.json.as_deref()).map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Deploy(args) => homeboy_core::output::map_cmd_result_to_json(
            deploy::run(args, cli.json.as_deref())
                .map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Component(args) => homeboy_core::output::map_cmd_result_to_json(
            component::run(args, cli.json.as_deref())
                .map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Config(args) => homeboy_core::output::map_cmd_result_to_json(
            config::run(args, cli.json.as_deref())
                .map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Module(args) => homeboy_core::output::map_cmd_result_to_json(
            module::run(args, cli.json.as_deref())
                .map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Docs(args) => homeboy_core::output::map_cmd_result_to_json(
            docs_command::run(args, cli.json.as_deref())
                .map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Changelog(args) => homeboy_core::output::map_cmd_result_to_json(
            changelog::run(args, cli.json.as_deref())
                .map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Git(args) => homeboy_core::output::map_cmd_result_to_json(
            git::run(args, cli.json.as_deref()).map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Version(args) => {
            homeboy_core::output::map_cmd_result_to_json(version::run(args, cli.json.as_deref()))
        }
        Commands::Build(args) => homeboy_core::output::map_cmd_result_to_json(
            build::run(args, cli.json.as_deref())
                .map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Doctor(args) => homeboy_core::output::map_cmd_result_to_json(
            doctor::run(args, cli.json.as_deref())
                .map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
        Commands::Error(args) => homeboy_core::output::map_cmd_result_to_json(
            error::run(args, cli.json.as_deref())
                .map(|(data, exit_code)| (data, vec![], exit_code)),
        ),
    };

    match mode {
        ResponseMode::Json => homeboy_core::output::print_json_result(json_result),
        ResponseMode::InteractivePassthrough => {}
    }

    std::process::ExitCode::from(exit_code_to_u8(exit_code))
}

fn exit_code_to_u8(code: i32) -> u8 {
    if code <= 0 {
        0
    } else if code >= 255 {
        255
    } else {
        code as u8
    }
}
