use clap::{Command, CommandFactory, Parser, Subcommand};
use std::path::PathBuf;

use crate::commands::{
    api, audit, auth, bench, build, changelog, changes, component, config, daemon, db, deploy,
    deps, doctor, extension, file, fleet, git, issues, lint, logs, observe, project, refactor,
    release, report, review, rig, runs, self_cmd, server, ssh, stack, status, test, trace, triage,
    undo, upgrade, version,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "homeboy")]
#[command(version = VERSION)]
#[command(about = "CLI tool for development and deployment automation")]
pub struct Cli {
    /// Write structured JSON output to a file (in addition to stdout).
    /// The file contains command-specific JSON — no log text.
    #[arg(long, global = true, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Suppress resource policy warnings for intentionally hot commands.
    #[arg(long, global = true)]
    pub force_hot: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage project configuration
    #[command(visible_alias = "projects")]
    Project(project::ProjectArgs),
    /// SSH into a project server or configured server
    Ssh(ssh::SshArgs),
    /// Manage SSH server configurations
    #[command(visible_alias = "servers")]
    Server(server::ServerArgs),
    /// Run tests for a component
    Test(test::TestArgs),
    /// Run performance benchmarks for a component
    Bench(bench::BenchArgs),
    /// Capture black-box behavioral traces for a component
    Trace(trace::TraceArgs),
    /// Passively observe a running system and persist timeline evidence
    Observe(observe::ObserveArgs),
    /// Lint a component
    Lint(lint::LintArgs),
    /// Database operations
    Db(db::DbArgs),
    /// Manage component dependencies
    #[command(visible_alias = "dependencies")]
    Deps(deps::DepsArgs),
    /// Read-only local diagnostics for Homeboy-adjacent work
    Doctor(doctor::DoctorArgs),
    /// Remote file operations
    File(file::FileArgs),
    /// Manage fleets (groups of projects)
    #[command(visible_alias = "fleets")]
    Fleet(fleet::FleetArgs),
    /// Remote log viewing
    Logs(logs::LogsArgs),
    /// Read-only attention report for components, projects, fleets, and rigs
    Triage(triage::TriageArgs),
    /// Deploy components to remote server
    Deploy(deploy::DeployArgs),
    /// Manage standalone component configurations
    #[command(visible_alias = "components")]
    Component(component::ComponentArgs),
    /// Manage global Homeboy configuration
    Config(config::ConfigArgs),
    /// Run the local-only HTTP API daemon
    Daemon(daemon::DaemonArgs),
    /// Execute CLI-compatible extensions
    #[command(visible_alias = "extensions")]
    Extension(extension::ExtensionArgs),
    /// Actionable component status overview
    Status(status::StatusArgs),
    /// Display CLI documentation
    Docs(crate::commands::docs::DocsArgs),
    /// Changelog operations
    Changelog(changelog::ChangelogArgs),
    /// Git operations for components
    Git(git::GitArgs),
    /// Reconcile findings against an issue tracker
    Issues(issues::IssuesArgs),
    /// Version management for components
    Version(version::VersionArgs),
    /// Build a component
    Build(build::BuildArgs),
    /// Show changes since last version tag
    Changes(changes::ChangesArgs),
    /// Plan release workflows
    Release(release::ReleaseArgs),
    /// Render reports from Homeboy structured output artifacts
    Report(report::ReportArgs),
    /// Run scoped audit + lint + test umbrella against PR-style changes
    Review(review::ReviewArgs),
    /// Audit code conventions and detect architectural drift
    Audit(audit::AuditArgs),
    /// Structural refactoring (rename terms across codebase)
    Refactor(refactor::RefactorArgs),
    /// Manage local dev rigs (reproducible multi-component environments)
    #[command(visible_alias = "rigs")]
    Rig(rig::RigArgs),
    /// Inspect persisted observation runs and artifacts
    Runs(runs::RunsArgs),
    /// Inspect the active Homeboy binary and install signals
    #[command(name = "self")]
    SelfCmd(self_cmd::SelfArgs),
    /// Manage stacks (combined-fixes branches built from base + cherry-picked PRs)
    #[command(visible_alias = "stacks")]
    Stack(stack::StackArgs),
    /// Undo the last write operation (audit fix, refactor, etc.)
    Undo(undo::UndoArgs),
    /// Authenticate with a project's API
    Auth(auth::AuthArgs),
    /// Make API requests to a project
    Api(api::ApiArgs),
    /// Upgrade Homeboy to the latest version
    Upgrade(upgrade::UpgradeArgs),
    /// List available commands (alias for --help)
    List,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandResponseMode {
    Json,
    Raw(CommandRawOutputMode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRawOutputMode {
    InteractivePassthrough,
    Markdown,
    PlainText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutputArtifactPolicy {
    GenericEnvelope,
    ReviewStableArtifact,
    TraceJsonSummaryArtifact,
}

impl Commands {
    pub fn response_mode(&self, has_output_file: bool) -> CommandResponseMode {
        match self {
            Commands::Ssh(args) if args.subcommand.is_none() && args.command.is_empty() => {
                CommandResponseMode::Raw(CommandRawOutputMode::InteractivePassthrough)
            }
            Commands::Logs(args) if logs::is_interactive(args) => {
                CommandResponseMode::Raw(CommandRawOutputMode::InteractivePassthrough)
            }
            Commands::File(args) if file::is_raw_read(args) => {
                CommandResponseMode::Raw(CommandRawOutputMode::PlainText)
            }
            Commands::Docs(args) if crate::commands::docs::is_json_mode(args) => {
                CommandResponseMode::Json
            }
            Commands::Docs(_) => CommandResponseMode::Raw(CommandRawOutputMode::Markdown),
            Commands::Changelog(args) if changelog::is_show_markdown(args) => {
                CommandResponseMode::Raw(CommandRawOutputMode::Markdown)
            }
            Commands::Review(args) if review::is_markdown_mode(args) => {
                CommandResponseMode::Raw(CommandRawOutputMode::Markdown)
            }
            Commands::Trace(args) if trace::is_markdown_mode(args) => {
                CommandResponseMode::Raw(CommandRawOutputMode::Markdown)
            }
            Commands::Runs(args) if !has_output_file && args.is_markdown_mode() => {
                CommandResponseMode::Raw(CommandRawOutputMode::Markdown)
            }
            Commands::Report(args) if report::is_markdown_mode(args) => {
                CommandResponseMode::Raw(CommandRawOutputMode::Markdown)
            }
            Commands::List => CommandResponseMode::Raw(CommandRawOutputMode::Markdown),
            _ => CommandResponseMode::Json,
        }
    }

    pub fn output_artifact_policy(&self, has_output_file: bool) -> CommandOutputArtifactPolicy {
        match self {
            Commands::Review(_) => CommandOutputArtifactPolicy::ReviewStableArtifact,
            Commands::Trace(args) if has_output_file && args.json_summary => {
                CommandOutputArtifactPolicy::TraceJsonSummaryArtifact
            }
            _ => CommandOutputArtifactPolicy::GenericEnvelope,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSurface {
    pub commands: Vec<CommandSurfaceEntry>,
}

impl CommandSurface {
    pub fn contains_path(&self, path: &[&str]) -> bool {
        let Some((first, rest)) = path.split_first() else {
            return false;
        };

        let Some(entry) = self.commands.iter().find(|entry| entry.matches(first)) else {
            return false;
        };

        match rest {
            [] => true,
            [second] => entry.subcommands.iter().any(|sub| sub.matches(second)),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSurfaceEntry {
    pub name: String,
    pub visible_aliases: Vec<String>,
    pub subcommands: Vec<CommandSurfaceEntry>,
}

impl CommandSurfaceEntry {
    fn matches(&self, name: &str) -> bool {
        self.name == name || self.visible_aliases.iter().any(|alias| alias == name)
    }
}

pub fn current_command_surface() -> CommandSurface {
    command_surface_from(Cli::command())
}

pub fn command_surface_from(command: Command) -> CommandSurface {
    CommandSurface {
        commands: visible_subcommands(&command, 1),
    }
}

fn visible_subcommands(command: &Command, remaining_depth: usize) -> Vec<CommandSurfaceEntry> {
    command
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set())
        .map(|subcommand| CommandSurfaceEntry {
            name: subcommand.get_name().to_string(),
            visible_aliases: subcommand
                .get_visible_aliases()
                .map(str::to_string)
                .collect(),
            subcommands: if remaining_depth == 0 {
                Vec::new()
            } else {
                visible_subcommands(subcommand, remaining_depth - 1)
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed_command(args: &[&str]) -> Commands {
        Cli::try_parse_from(args)
            .expect("CLI args should parse")
            .command
    }

    #[test]
    fn test_current_command_surface() {
        let surface = current_command_surface();

        assert!(surface.contains_path(&["self"]));
        assert!(surface.contains_path(&["self", "status"]));
        assert!(surface.contains_path(&["doctor", "resources"]));
        assert!(surface.contains_path(&["observe"]));
    }

    #[test]
    fn test_command_surface_from() {
        let surface = command_surface_from(Cli::command());

        assert!(surface.contains_path(&["self"]));
        assert!(surface.contains_path(&["self", "status"]));
        assert!(surface.contains_path(&["doctor", "resources"]));
        assert!(surface.contains_path(&["observe"]));
    }

    #[test]
    fn test_contains_path() {
        let surface = current_command_surface();

        assert!(surface.contains_path(&["self"]));
        assert!(!surface.contains_path(&["self", "missing"]));
    }

    #[test]
    fn test_response_mode() {
        assert_eq!(
            parsed_command(&["homeboy", "status"]).response_mode(false),
            CommandResponseMode::Json
        );
        assert_eq!(
            parsed_command(&["homeboy", "review", "--report", "pr-comment"]).response_mode(false),
            CommandResponseMode::Raw(CommandRawOutputMode::Markdown)
        );
        assert_eq!(
            parsed_command(&["homeboy", "trace", "--report", "markdown"]).response_mode(false),
            CommandResponseMode::Raw(CommandRawOutputMode::Markdown)
        );
        assert_eq!(
            Commands::List.response_mode(false),
            CommandResponseMode::Raw(CommandRawOutputMode::Markdown)
        );
    }

    #[test]
    fn test_output_artifact_policy() {
        assert_eq!(
            parsed_command(&["homeboy", "status"]).output_artifact_policy(true),
            CommandOutputArtifactPolicy::GenericEnvelope
        );
        assert_eq!(
            parsed_command(&["homeboy", "review"]).output_artifact_policy(true),
            CommandOutputArtifactPolicy::ReviewStableArtifact
        );
        assert_eq!(
            parsed_command(&["homeboy", "trace", "--json-summary"]).output_artifact_policy(true),
            CommandOutputArtifactPolicy::TraceJsonSummaryArtifact
        );
        assert_eq!(
            parsed_command(&["homeboy", "trace", "--json-summary"]).output_artifact_policy(false),
            CommandOutputArtifactPolicy::GenericEnvelope
        );
    }
}
