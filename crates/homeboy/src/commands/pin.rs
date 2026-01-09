use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use uuid::Uuid;
use homeboy_core::config::{ConfigManager, PinnedRemoteFile, PinnedRemoteLog};
use homeboy_core::output::{print_success, print_error};

#[derive(Args)]
pub struct PinArgs {
    #[command(subcommand)]
    command: PinCommand,
}

#[derive(Subcommand)]
enum PinCommand {
    /// List pinned items
    List {
        /// Project ID
        project_id: String,
        /// Item type: file or log
        #[arg(long, value_enum)]
        r#type: PinType,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Pin a file or log
    Add {
        /// Project ID
        project_id: String,
        /// Path to pin (relative to basePath or absolute)
        path: String,
        /// Item type: file or log
        #[arg(long, value_enum)]
        r#type: PinType,
        /// Optional display label
        #[arg(long)]
        label: Option<String>,
        /// Number of lines to tail (logs only, default: 100)
        #[arg(long, default_value = "100")]
        tail: u32,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Unpin a file or log
    Remove {
        /// Project ID
        project_id: String,
        /// Path to unpin
        path: String,
        /// Item type: file or log
        #[arg(long, value_enum)]
        r#type: PinType,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum PinType {
    File,
    Log,
}

pub fn run(args: PinArgs) {
    match args.command {
        PinCommand::List { project_id, r#type, json } => list(&project_id, r#type, json),
        PinCommand::Add { project_id, path, r#type, label, tail, json } => {
            add(&project_id, &path, r#type, label, tail, json)
        }
        PinCommand::Remove { project_id, path, r#type, json } => {
            remove(&project_id, &path, r#type, json)
        }
    }
}

fn list(project_id: &str, pin_type: PinType, json: bool) {
    let project = match ConfigManager::load_project(project_id) {
        Ok(p) => p,
        Err(e) => {
            if json {
                print_error(e.code(), &e.to_string());
            } else {
                eprintln!("Error: {}", e);
            }
            return;
        }
    };

    match pin_type {
        PinType::File => {
            if json {
                #[derive(Serialize)]
                #[serde(rename_all = "camelCase")]
                struct FileItem {
                    path: String,
                    label: Option<String>,
                    display_name: String,
                }

                let items: Vec<FileItem> = project
                    .remote_files
                    .pinned_files
                    .iter()
                    .map(|f| FileItem {
                        path: f.path.clone(),
                        label: f.label.clone(),
                        display_name: f.display_name().to_string(),
                    })
                    .collect();

                print_success(items);
            } else {
                let files = &project.remote_files.pinned_files;
                if files.is_empty() {
                    println!("No pinned files for project '{}'", project_id);
                } else {
                    println!("Pinned files for '{}':", project_id);
                    for file in files {
                        let label = file.label.as_ref().map(|l| format!(" ({})", l)).unwrap_or_default();
                        println!("  {}{}", file.path, label);
                    }
                }
            }
        }
        PinType::Log => {
            if json {
                #[derive(Serialize)]
                #[serde(rename_all = "camelCase")]
                struct LogItem {
                    path: String,
                    label: Option<String>,
                    display_name: String,
                    tail_lines: u32,
                }

                let items: Vec<LogItem> = project
                    .remote_logs
                    .pinned_logs
                    .iter()
                    .map(|l| LogItem {
                        path: l.path.clone(),
                        label: l.label.clone(),
                        display_name: l.display_name().to_string(),
                        tail_lines: l.tail_lines,
                    })
                    .collect();

                print_success(items);
            } else {
                let logs = &project.remote_logs.pinned_logs;
                if logs.is_empty() {
                    println!("No pinned logs for project '{}'", project_id);
                } else {
                    println!("Pinned logs for '{}':", project_id);
                    for log in logs {
                        let label = log.label.as_ref().map(|l| format!(" ({})", l)).unwrap_or_default();
                        println!("  {}{} [tail: {}]", log.path, label, log.tail_lines);
                    }
                }
            }
        }
    }
}

fn add(project_id: &str, path: &str, pin_type: PinType, label: Option<String>, tail: u32, json: bool) {
    let mut project = match ConfigManager::load_project(project_id) {
        Ok(p) => p,
        Err(e) => {
            if json {
                print_error(e.code(), &e.to_string());
            } else {
                eprintln!("Error: {}", e);
            }
            return;
        }
    };

    match pin_type {
        PinType::File => {
            if project.remote_files.pinned_files.iter().any(|f| f.path == path) {
                let msg = format!("File '{}' is already pinned", path);
                if json {
                    print_error("ALREADY_PINNED", &msg);
                } else {
                    eprintln!("Error: {}", msg);
                }
                return;
            }

            let pinned = PinnedRemoteFile {
                id: Uuid::new_v4(),
                path: path.to_string(),
                label,
            };
            project.remote_files.pinned_files.push(pinned);
        }
        PinType::Log => {
            if project.remote_logs.pinned_logs.iter().any(|l| l.path == path) {
                let msg = format!("Log '{}' is already pinned", path);
                if json {
                    print_error("ALREADY_PINNED", &msg);
                } else {
                    eprintln!("Error: {}", msg);
                }
                return;
            }

            let pinned = PinnedRemoteLog {
                id: Uuid::new_v4(),
                path: path.to_string(),
                label,
                tail_lines: tail,
            };
            project.remote_logs.pinned_logs.push(pinned);
        }
    }

    if let Err(e) = ConfigManager::save_project(&project) {
        if json {
            print_error(e.code(), &e.to_string());
        } else {
            eprintln!("Error: {}", e);
        }
        return;
    }

    if json {
        #[derive(Serialize)]
        struct AddResult {
            path: String,
            r#type: String,
        }

        print_success(AddResult {
            path: path.to_string(),
            r#type: match pin_type {
                PinType::File => "file",
                PinType::Log => "log",
            }
            .to_string(),
        });
    } else {
        let type_str = match pin_type {
            PinType::File => "file",
            PinType::Log => "log",
        };
        println!("Pinned {}: {}", type_str, path);
    }
}

fn remove(project_id: &str, path: &str, pin_type: PinType, json: bool) {
    let mut project = match ConfigManager::load_project(project_id) {
        Ok(p) => p,
        Err(e) => {
            if json {
                print_error(e.code(), &e.to_string());
            } else {
                eprintln!("Error: {}", e);
            }
            return;
        }
    };

    let removed = match pin_type {
        PinType::File => {
            let original_len = project.remote_files.pinned_files.len();
            project.remote_files.pinned_files.retain(|f| f.path != path);
            project.remote_files.pinned_files.len() < original_len
        }
        PinType::Log => {
            let original_len = project.remote_logs.pinned_logs.len();
            project.remote_logs.pinned_logs.retain(|l| l.path != path);
            project.remote_logs.pinned_logs.len() < original_len
        }
    };

    if !removed {
        let type_str = match pin_type {
            PinType::File => "File",
            PinType::Log => "Log",
        };
        let msg = format!("{} '{}' is not pinned", type_str, path);
        if json {
            print_error("NOT_PINNED", &msg);
        } else {
            eprintln!("Error: {}", msg);
        }
        return;
    }

    if let Err(e) = ConfigManager::save_project(&project) {
        if json {
            print_error(e.code(), &e.to_string());
        } else {
            eprintln!("Error: {}", e);
        }
        return;
    }

    if json {
        #[derive(Serialize)]
        struct RemoveResult {
            path: String,
            r#type: String,
        }

        print_success(RemoveResult {
            path: path.to_string(),
            r#type: match pin_type {
                PinType::File => "file",
                PinType::Log => "log",
            }
            .to_string(),
        });
    } else {
        let type_str = match pin_type {
            PinType::File => "file",
            PinType::Log => "log",
        };
        println!("Unpinned {}: {}", type_str, path);
    }
}
