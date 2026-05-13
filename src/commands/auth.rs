use clap::{Args, Subcommand};
use serde::Serialize;
use std::collections::HashMap;

use homeboy::server::auth::{
    self, AuthStatus, GetResult, LoginResult, LogoutResult, RemoveResult, SetResult,
};

use super::{CmdResult, GlobalArgs};
use crate::commands::utils::tty::{prompt, prompt_password};

#[derive(Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommand,
}

#[derive(Subcommand)]
enum AuthCommand {
    /// Authenticate with a project's API
    Login {
        /// Project ID
        #[arg(long)]
        project: String,

        /// Username or email
        #[arg(long)]
        identifier: Option<String>,

        /// Password (or read from stdin)
        #[arg(long)]
        password: Option<String>,
    },

    /// Store a project API variable in the OS keychain
    Set {
        /// Project ID
        #[arg(long)]
        project: String,

        /// Variable name
        variable: String,

        /// Secret value (or read from stdin)
        value: Option<String>,
    },

    /// Read a project API variable from the OS keychain
    Get {
        /// Project ID
        #[arg(long)]
        project: String,

        /// Variable name
        variable: String,

        /// Return a redacted marker instead of the secret value
        #[arg(long)]
        redacted: bool,
    },

    /// Remove a project API variable from the OS keychain
    Remove {
        /// Project ID
        #[arg(long)]
        project: String,

        /// Variable name
        variable: String,
    },

    /// Clear stored authentication for a project
    Logout {
        /// Project ID
        #[arg(long)]
        project: String,
    },

    /// Show authentication status for a project
    Status {
        /// Project ID
        #[arg(long)]
        project: String,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum AuthOutput {
    Login(LoginResult),
    Set(SetResult),
    Get(GetResult),
    Remove(RemoveResult),
    Logout(LogoutResult),
    Status(AuthStatus),
}

pub fn run(args: AuthArgs, _global: &GlobalArgs) -> CmdResult<AuthOutput> {
    match args.command {
        AuthCommand::Login {
            project,
            identifier,
            password,
        } => run_login(&project, identifier, password),
        AuthCommand::Set {
            project,
            variable,
            value,
        } => run_set(&project, &variable, value),
        AuthCommand::Get {
            project,
            variable,
            redacted,
        } => run_get(&project, &variable, redacted),
        AuthCommand::Remove { project, variable } => run_remove(&project, &variable),
        AuthCommand::Logout { project } => run_logout(&project),
        AuthCommand::Status { project } => run_status(&project),
    }
}

fn run_login(
    project_id: &str,
    identifier: Option<String>,
    password: Option<String>,
) -> CmdResult<AuthOutput> {
    let identifier = match identifier {
        Some(id) => id,
        None => prompt("Username/Email: ")?,
    };

    let password = match password {
        Some(pw) => pw,
        None => prompt_password("Password: ")?,
    };

    let mut credentials = HashMap::new();
    credentials.insert("identifier".to_string(), identifier);
    credentials.insert("password".to_string(), password);

    let result = auth::login(project_id, credentials)?;
    Ok((AuthOutput::Login(result), 0))
}

fn run_set(project_id: &str, variable: &str, value: Option<String>) -> CmdResult<AuthOutput> {
    let value = match value {
        Some(value) => value,
        None => prompt_password("Value: ")?,
    };

    let result = auth::set(project_id, variable, &value)?;
    Ok((AuthOutput::Set(result), 0))
}

fn run_get(project_id: &str, variable: &str, redacted: bool) -> CmdResult<AuthOutput> {
    let result = auth::get(project_id, variable, redacted)?;
    Ok((AuthOutput::Get(result), 0))
}

fn run_remove(project_id: &str, variable: &str) -> CmdResult<AuthOutput> {
    let result = auth::remove(project_id, variable)?;
    Ok((AuthOutput::Remove(result), 0))
}

fn run_logout(project_id: &str) -> CmdResult<AuthOutput> {
    let result = auth::logout(project_id)?;
    Ok((AuthOutput::Logout(result), 0))
}

fn run_status(project_id: &str) -> CmdResult<AuthOutput> {
    let result = auth::status(project_id)?;
    Ok((AuthOutput::Status(result), 0))
}
