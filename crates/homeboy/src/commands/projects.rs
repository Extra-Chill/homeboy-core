use clap::Args;
use serde::Serialize;
use homeboy_core::config::ConfigManager;
use homeboy_core::output::{print_success, print_error};

#[derive(Args)]
pub struct ProjectsArgs {
    /// Show only the active project ID
    #[arg(long)]
    current: bool,

    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Serialize)]
struct ProjectListItem {
    id: String,
    name: String,
    domain: String,
    #[serde(rename = "projectType")]
    project_type: String,
    active: bool,
}

#[derive(Serialize)]
struct ProjectsOutput {
    projects: Vec<ProjectListItem>,
    #[serde(rename = "activeProjectId")]
    active_project_id: Option<String>,
}

pub fn run(args: ProjectsArgs) {
    let app_config = match ConfigManager::load_app_config() {
        Ok(config) => config,
        Err(e) => {
            print_error(e.code(), &e.to_string());
            return;
        }
    };

    let active_id = app_config.active_project_id.clone();

    if args.current {
        match &active_id {
            Some(id) => {
                if args.json {
                    print_success(serde_json::json!({ "id": id }));
                } else {
                    println!("{}", id);
                }
            }
            None => {
                if args.json {
                    print_error("NO_ACTIVE_PROJECT", "No active project set");
                } else {
                    eprintln!("No active project set");
                }
            }
        }
        return;
    }

    let projects = match ConfigManager::list_projects() {
        Ok(p) => p,
        Err(e) => {
            print_error(e.code(), &e.to_string());
            return;
        }
    };

    let items: Vec<ProjectListItem> = projects
        .into_iter()
        .map(|p| ProjectListItem {
            active: active_id.as_ref().map_or(false, |a| a == &p.id),
            id: p.id,
            name: p.name,
            domain: p.domain,
            project_type: p.project_type,
        })
        .collect();

    if args.json {
        print_success(ProjectsOutput {
            projects: items,
            active_project_id: active_id,
        });
    } else {
        if items.is_empty() {
            println!("No projects configured");
            return;
        }

        println!("{:<3} {:<20} {:<30} {:<12}", "", "ID", "NAME", "TYPE");
        println!("{}", "-".repeat(70));
        for item in items {
            let marker = if item.active { "*" } else { "" };
            println!(
                "{:<3} {:<20} {:<30} {:<12}",
                marker, item.id, item.name, item.project_type
            );
        }
    }
}
