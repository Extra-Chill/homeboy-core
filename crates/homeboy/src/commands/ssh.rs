use clap::Args;
use homeboy_core::config::ConfigManager;
use homeboy_core::ssh::SshClient;

#[derive(Args)]
pub struct SshArgs {
    /// Project ID
    pub project_id: String,

    /// Command to execute (omit for interactive shell)
    pub command: Option<String>,
}

pub fn run(args: SshArgs) {
    let project = match ConfigManager::load_project(&args.project_id) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let server_id = match &project.server_id {
        Some(id) => id,
        None => {
            eprintln!("Error: Server not configured for project '{}'", args.project_id);
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

    if !server.is_valid() {
        eprintln!("Error: Server '{}' is not properly configured", server_id);
        std::process::exit(1);
    }

    let client = match SshClient::from_server(&server, server_id) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let exit_code = client.execute_interactive(args.command.as_deref());
    std::process::exit(exit_code);
}
