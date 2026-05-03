use clap::{Args, Subcommand};

use super::CmdResult;

pub mod resources;

#[derive(Args)]
pub struct DoctorArgs {
    #[command(subcommand)]
    pub command: DoctorCommand,
}

#[derive(Subcommand)]
pub enum DoctorCommand {
    /// Report current machine pressure and Homeboy-adjacent hot processes
    Resources(resources::ResourcesArgs),
}

pub fn run(args: DoctorArgs, _global: &super::GlobalArgs) -> CmdResult<resources::DoctorOutput> {
    match args.command {
        DoctorCommand::Resources(args) => resources::run(args),
    }
}
