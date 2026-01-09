use clap::{Args, Subcommand};
use homeboy_core::config::ConfigManager;
use homeboy_core::output::{print_success, print_error};

#[derive(Args)]
pub struct ProjectArgs {
    #[command(subcommand)]
    command: ProjectCommand,
}

#[derive(Subcommand)]
enum ProjectCommand {
    /// Show project configuration
    Show {
        /// Project ID (uses active project if not specified)
        project_id: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Switch active project
    Switch {
        /// Project ID to switch to
        project_id: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub fn run(args: ProjectArgs) {
    match args.command {
        ProjectCommand::Show { project_id, json } => show(project_id, json),
        ProjectCommand::Switch { project_id, json } => switch(&project_id, json),
    }
}

fn show(project_id: Option<String>, json: bool) {
    let project = match project_id {
        Some(id) => ConfigManager::load_project(&id),
        None => ConfigManager::get_active_project(),
    };

    match project {
        Ok(p) => {
            if json {
                print_success(&p);
            } else {
                println!("Project: {} ({})", p.name, p.id);
                println!("Type: {}", p.project_type);
                println!("Domain: {}", p.domain);
                if let Some(server_id) = &p.server_id {
                    println!("Server: {}", server_id);
                }
                if let Some(base_path) = &p.base_path {
                    println!("Base Path: {}", base_path);
                }
                if !p.sub_targets.is_empty() {
                    println!("\nSubtargets:");
                    for target in &p.sub_targets {
                        let marker = if target.is_default { " (default)" } else { "" };
                        println!("  - {} ({}){}", target.name, target.domain, marker);
                    }
                }
                if !p.component_ids.is_empty() {
                    println!("\nComponents: {}", p.component_ids.join(", "));
                }
            }
        }
        Err(e) => {
            if json {
                print_error(e.code(), &e.to_string());
            } else {
                eprintln!("Error: {}", e);
            }
        }
    }
}

fn switch(project_id: &str, json: bool) {
    match ConfigManager::set_active_project(project_id) {
        Ok(()) => {
            if json {
                print_success(serde_json::json!({
                    "message": format!("Switched to project: {}", project_id),
                    "projectId": project_id
                }));
            } else {
                println!("Switched to project: {}", project_id);
            }
        }
        Err(e) => {
            if json {
                print_error(e.code(), &e.to_string());
            } else {
                eprintln!("Error: {}", e);
            }
        }
    }
}
