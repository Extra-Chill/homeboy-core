use clap::{Args, Subcommand, ValueEnum};
use homeboy_core::doctor;

use super::CmdResult;

#[derive(Args)]
pub struct DoctorArgs {
    #[command(subcommand)]
    command: DoctorCommand,
}

#[derive(Subcommand)]
enum DoctorCommand {
    /// Scan Homeboy configuration and report issues
    Scan(DoctorScanArgs),

    /// Safely remove unknown top-level keys from config JSON
    Cleanup(DoctorCleanupArgs),
}

#[derive(Args)]
struct DoctorScanArgs {
    #[arg(long, value_enum, default_value_t = ScopeArg::All)]
    scope: ScopeArg,

    #[arg(long)]
    file: Option<String>,

    #[arg(long, value_enum, default_value_t = FailOnArg::Error)]
    fail_on: FailOnArg,
}

#[derive(Args)]
struct DoctorCleanupArgs {
    #[arg(long, value_enum, default_value_t = ScopeArg::All)]
    scope: ScopeArg,

    #[arg(long)]
    file: Option<String>,

    #[arg(long)]
    dry_run: bool,

    #[arg(long, value_enum, default_value_t = FailOnArg::Error)]
    fail_on: FailOnArg,
}

#[derive(Clone, Copy, ValueEnum)]
enum ScopeArg {
    All,
    App,
    Projects,
    Servers,
    Components,
    Modules,
}

#[derive(Clone, Copy, ValueEnum)]
enum FailOnArg {
    Error,
    Warning,
}

pub fn run(args: DoctorArgs, _global: &crate::commands::GlobalArgs) -> CmdResult<doctor::DoctorResult> {
    let input = match args.command {
        DoctorCommand::Scan(scan) => build_scan_json(&scan),
        DoctorCommand::Cleanup(cleanup) => build_cleanup_json(&cleanup),
    };
    doctor::run(&input)
}

fn build_scan_json(args: &DoctorScanArgs) -> String {
    serde_json::json!({
        "scan": {
            "scope": scope_str(args.scope),
            "file": args.file,
            "failOn": fail_on_str(args.fail_on),
        }
    }).to_string()
}

fn build_cleanup_json(args: &DoctorCleanupArgs) -> String {
    serde_json::json!({
        "cleanup": {
            "scope": scope_str(args.scope),
            "file": args.file,
            "dryRun": args.dry_run,
            "failOn": fail_on_str(args.fail_on),
        }
    }).to_string()
}

fn scope_str(scope: ScopeArg) -> &'static str {
    match scope {
        ScopeArg::All => "all",
        ScopeArg::App => "app",
        ScopeArg::Projects => "projects",
        ScopeArg::Servers => "servers",
        ScopeArg::Components => "components",
        ScopeArg::Modules => "modules",
    }
}

fn fail_on_str(fail_on: FailOnArg) -> &'static str {
    match fail_on {
        FailOnArg::Error => "error",
        FailOnArg::Warning => "warning",
    }
}
