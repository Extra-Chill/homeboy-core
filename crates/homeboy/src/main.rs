use clap::{ArgMatches, Command, CommandFactory, FromArgMatches, Parser, Subcommand};

use commands::GlobalArgs;

#[derive(Debug, Clone, Copy)]
enum ResponseMode {
    Json,
    Raw(RawOutputMode),
}

#[derive(Debug, Clone, Copy)]
enum RawOutputMode {
    InteractivePassthrough,
    Markdown,
}

mod commands;
mod docs;

use commands::{
    build, changelog, cli, component, config, context, db, deploy, doctor, error, file, git, init,
    logs, module, plugin, project, server, ssh, version,
};
use homeboy_core::plugin::load_all_plugins;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "homeboy")]
#[command(version = VERSION)]
#[command(about = "CLI tool for development and deployment automation")]
struct Cli {
    /// Dry-run: show what would happen without writing.
    #[arg(long, global = true)]
    dry_run: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage project configuration
    Project(project::ProjectArgs),
    /// SSH into a project server or configured server
    Ssh(ssh::SshArgs),
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
    /// Show context for current working directory
    Context(context::ContextArgs),
    /// Execute CLI-compatible modules
    Module(module::ModuleArgs),
    /// Manage platform plugins
    Plugin(plugin::PluginArgs),
    /// Initialize a repo for use with Homeboy
    Init(init::InitArgs),
    /// Display CLI documentation
    Docs(crate::commands::docs::DocsArgs),
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
    /// List available commands (alias for --help)
    List,
}

fn response_mode(command: &Commands) -> ResponseMode {
    match command {
        Commands::Ssh(args) if args.subcommand.is_none() && args.command.is_none() => {
            ResponseMode::Raw(RawOutputMode::InteractivePassthrough)
        }
        Commands::Logs(args) if logs::is_interactive(args) => {
            ResponseMode::Raw(RawOutputMode::InteractivePassthrough)
        }
        Commands::Docs(args) if !args.list => ResponseMode::Raw(RawOutputMode::Markdown),
        Commands::Init(_) => ResponseMode::Raw(RawOutputMode::Markdown),
        Commands::Changelog(args) if changelog::is_show_markdown(args) => {
            ResponseMode::Raw(RawOutputMode::Markdown)
        }
        Commands::List => ResponseMode::Raw(RawOutputMode::Markdown),
        _ => ResponseMode::Json,
    }
}

struct PluginCommand {
    tool: String,
    project_id: String,
    local: bool,
    args: Vec<String>,
}

struct PluginCliInfo {
    tool: String,
    display_name: String,
    plugin_name: String,
}

fn collect_plugin_cli_info() -> Vec<PluginCliInfo> {
    load_all_plugins()
        .into_iter()
        .filter_map(|p| {
            p.cli.map(|cli| PluginCliInfo {
                tool: cli.tool,
                display_name: cli.display_name,
                plugin_name: p.name,
            })
        })
        .collect()
}

fn build_augmented_command(plugin_info: &[PluginCliInfo]) -> Command {
    let mut cmd = Cli::command();

    for info in plugin_info {
        let tool_name: &'static str = Box::leak(info.tool.clone().into_boxed_str());
        cmd = cmd.subcommand(
            Command::new(tool_name)
                .about(format!("Run {} commands via {}", info.display_name, info.plugin_name))
                .arg(
                    clap::Arg::new("project_id")
                        .help("Project ID")
                        .required(true)
                        .index(1),
                )
                .arg(
                    clap::Arg::new("local")
                        .long("local")
                        .help("Execute locally instead of on remote server")
                        .action(clap::ArgAction::SetTrue),
                )
                .arg(
                    clap::Arg::new("args")
                        .help("Command arguments")
                        .index(2)
                        .num_args(0..)
                        .allow_hyphen_values(true),
                )
                .trailing_var_arg(true),
        );
    }

    cmd
}

fn try_parse_plugin_command(matches: &ArgMatches, plugin_info: &[PluginCliInfo]) -> Option<PluginCommand> {
    let (tool, sub_matches) = matches.subcommand()?;

    if !plugin_info.iter().any(|p| p.tool == tool) {
        return None;
    }

    let sub_matches = sub_matches;
    let project_id = sub_matches.get_one::<String>("project_id")?.clone();
    let local = sub_matches.get_flag("local");
    let args: Vec<String> = sub_matches
        .get_many::<String>("args")
        .map(|vals| vals.cloned().collect())
        .unwrap_or_default();

    Some(PluginCommand {
        tool: tool.to_string(),
        project_id,
        local,
        args,
    })
}

fn main() -> std::process::ExitCode {
    let plugin_info = collect_plugin_cli_info();
    let cmd = build_augmented_command(&plugin_info);
    let matches = cmd.get_matches();

    let global = GlobalArgs {
        dry_run: matches.get_flag("dry_run"),
    };

    if let Some(plugin_cmd) = try_parse_plugin_command(&matches, &plugin_info) {
        let result = cli::run(
            &plugin_cmd.tool,
            &plugin_cmd.project_id,
            plugin_cmd.local,
            plugin_cmd.args,
            &global,
        );

        let (json_result, exit_code) = homeboy_core::output::map_cmd_result_to_json(
            result.map(|(data, exit_code)| (data, vec![], exit_code)),
        );
        homeboy_core::output::print_json_result(json_result);
        return std::process::ExitCode::from(exit_code_to_u8(exit_code));
    }

    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(e) => {
            e.exit();
        }
    };

    let mode = response_mode(&cli.command);

    match mode {
        ResponseMode::Json => {}
        ResponseMode::Raw(RawOutputMode::InteractivePassthrough) => {
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
        ResponseMode::Raw(RawOutputMode::Markdown) => {}
    }

    if matches!(cli.command, Commands::List) {
        let mut cmd = build_augmented_command(&plugin_info);
        cmd.print_help().expect("Failed to print help");
        println!();
        return std::process::ExitCode::SUCCESS;
    }

    if let ResponseMode::Raw(RawOutputMode::Markdown) = mode {
        let markdown_result = commands::run_markdown(cli.command, &global);

        match markdown_result {
            Ok((content, exit_code)) => {
                print!("{}", content);
                return std::process::ExitCode::from(exit_code_to_u8(exit_code));
            }
            Err(err) => {
                homeboy_core::output::print_result::<serde_json::Value>(Err(err));
                return std::process::ExitCode::from(exit_code_to_u8(1));
            }
        }
    }

    let (json_result, exit_code) = commands::run_json(cli.command, &global);

    match mode {
        ResponseMode::Json => homeboy_core::output::print_json_result(json_result),
        ResponseMode::Raw(RawOutputMode::InteractivePassthrough) => {}
        ResponseMode::Raw(RawOutputMode::Markdown) => {}
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
