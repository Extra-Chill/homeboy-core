use clap::{Args, Subcommand};
use serde::Serialize;
use homeboy_core::config::{ConfigManager, ComponentConfiguration};
use homeboy_core::output::{print_success, print_error};

#[derive(Args)]
pub struct ComponentArgs {
    #[command(subcommand)]
    command: ComponentCommand,
}

#[derive(Subcommand)]
enum ComponentCommand {
    /// Create a new component configuration
    Create {
        /// Component ID (used for referencing)
        id: String,
        /// Display name
        #[arg(long)]
        name: String,
        /// Absolute path to local source directory
        #[arg(long)]
        local_path: String,
        /// Remote path relative to project basePath
        #[arg(long)]
        remote_path: String,
        /// Build artifact path relative to localPath
        #[arg(long)]
        build_artifact: String,
        /// Version file path relative to localPath
        #[arg(long)]
        version_file: Option<String>,
        /// Regex pattern for version extraction
        #[arg(long)]
        version_pattern: Option<String>,
        /// Build command to run in localPath
        #[arg(long)]
        build_command: Option<String>,
        /// WordPress multisite network-activated plugin
        #[arg(long)]
        is_network: bool,
    },
    /// Bulk create components from JSON
    Import {
        /// JSON array of component objects
        json: String,
        /// Skip components that already exist
        #[arg(long)]
        skip_existing: bool,
    },
    /// Display component configuration as JSON
    Show {
        /// Component ID
        id: String,
    },
    /// Update component configuration fields
    Set {
        /// Component ID
        id: String,
        /// Update display name
        #[arg(long)]
        name: Option<String>,
        /// Update local path
        #[arg(long)]
        local_path: Option<String>,
        /// Update remote path
        #[arg(long)]
        remote_path: Option<String>,
        /// Update build artifact path
        #[arg(long)]
        build_artifact: Option<String>,
        /// Update version file path
        #[arg(long)]
        version_file: Option<String>,
        /// Update version pattern regex
        #[arg(long)]
        version_pattern: Option<String>,
        /// Update build command
        #[arg(long)]
        build_command: Option<String>,
        /// Set as network-activated plugin
        #[arg(long)]
        is_network: bool,
        /// Clear network activation flag
        #[arg(long)]
        not_network: bool,
    },
    /// Delete a component configuration
    Delete {
        /// Component ID
        id: String,
        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },
    /// List all available components
    List {
        /// Output as JSON array
        #[arg(long)]
        json: bool,
    },
}

pub fn run(args: ComponentArgs) {
    match args.command {
        ComponentCommand::Create {
            id,
            name,
            local_path,
            remote_path,
            build_artifact,
            version_file,
            version_pattern,
            build_command,
            is_network,
        } => create(
            &id,
            &name,
            &local_path,
            &remote_path,
            &build_artifact,
            version_file,
            version_pattern,
            build_command,
            is_network,
        ),
        ComponentCommand::Import { json, skip_existing } => import(&json, skip_existing),
        ComponentCommand::Show { id } => show(&id),
        ComponentCommand::Set {
            id,
            name,
            local_path,
            remote_path,
            build_artifact,
            version_file,
            version_pattern,
            build_command,
            is_network,
            not_network,
        } => set(
            &id,
            name,
            local_path,
            remote_path,
            build_artifact,
            version_file,
            version_pattern,
            build_command,
            is_network,
            not_network,
        ),
        ComponentCommand::Delete { id, force } => delete(&id, force),
        ComponentCommand::List { json } => list(json),
    }
}

fn create(
    id: &str,
    name: &str,
    local_path: &str,
    remote_path: &str,
    build_artifact: &str,
    version_file: Option<String>,
    version_pattern: Option<String>,
    build_command: Option<String>,
    is_network: bool,
) {
    if ConfigManager::load_component(id).is_ok() {
        print_error("COMPONENT_EXISTS", &format!("Component '{}' already exists", id));
        return;
    }

    let expanded_path = shellexpand::tilde(local_path).to_string();

    let mut component = ComponentConfiguration::new(
        id.to_string(),
        name.to_string(),
        expanded_path.clone(),
        remote_path.to_string(),
        build_artifact.to_string(),
    );
    component.version_file = version_file;
    component.version_pattern = version_pattern;
    component.build_command = build_command;
    component.is_network = if is_network { Some(true) } else { None };

    if let Err(e) = ConfigManager::save_component(&component) {
        print_error(e.code(), &e.to_string());
        return;
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateResult {
        id: String,
        name: String,
        local_path: String,
        remote_path: String,
        build_artifact: String,
    }

    print_success(CreateResult {
        id: id.to_string(),
        name: name.to_string(),
        local_path: expanded_path,
        remote_path: remote_path.to_string(),
        build_artifact: build_artifact.to_string(),
    });
}

fn import(json_str: &str, skip_existing: bool) {
    let components: Vec<ComponentConfiguration> = match serde_json::from_str(json_str) {
        Ok(c) => c,
        Err(e) => {
            print_error("JSON_PARSE_ERROR", &format!("Failed to parse JSON - {}", e));
            return;
        }
    };

    if components.is_empty() {
        print_error("NO_COMPONENTS", "No components in JSON array");
        return;
    }

    let mut created: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for mut component in components {
        component.local_path = shellexpand::tilde(&component.local_path).to_string();

        if ConfigManager::load_component(&component.id).is_ok() {
            if skip_existing {
                skipped.push(component.id.clone());
                continue;
            } else {
                errors.push(format!("{}: already exists", component.id));
                continue;
            }
        }

        if let Err(e) = ConfigManager::save_component(&component) {
            errors.push(format!("{}: {}", component.id, e));
        } else {
            created.push(component.id.clone());
        }
    }

    #[derive(Serialize)]
    struct ImportResult {
        success: bool,
        created: Vec<String>,
        skipped: Vec<String>,
        errors: Vec<String>,
    }

    let result = ImportResult {
        success: errors.is_empty(),
        created,
        skipped,
        errors,
    };

    if result.success {
        print_success(result);
    } else {
        let json = serde_json::to_string_pretty(&result).unwrap();
        println!("{}", json);
    }
}

fn show(id: &str) {
    let component = match ConfigManager::load_component(id) {
        Ok(c) => c,
        Err(e) => {
            print_error(e.code(), &e.to_string());
            return;
        }
    };

    print_success(component);
}

fn set(
    id: &str,
    name: Option<String>,
    local_path: Option<String>,
    remote_path: Option<String>,
    build_artifact: Option<String>,
    version_file: Option<String>,
    version_pattern: Option<String>,
    build_command: Option<String>,
    is_network: bool,
    not_network: bool,
) {
    let mut component = match ConfigManager::load_component(id) {
        Ok(c) => c,
        Err(e) => {
            print_error(e.code(), &e.to_string());
            return;
        }
    };

    let mut updated: Vec<String> = Vec::new();

    if let Some(v) = name {
        component.name = v;
        updated.push("name".to_string());
    }
    if let Some(v) = local_path {
        component.local_path = shellexpand::tilde(&v).to_string();
        updated.push("localPath".to_string());
    }
    if let Some(v) = remote_path {
        component.remote_path = v;
        updated.push("remotePath".to_string());
    }
    if let Some(v) = build_artifact {
        component.build_artifact = v;
        updated.push("buildArtifact".to_string());
    }
    if let Some(v) = version_file {
        component.version_file = Some(v);
        updated.push("versionFile".to_string());
    }
    if let Some(v) = version_pattern {
        component.version_pattern = Some(v);
        updated.push("versionPattern".to_string());
    }
    if let Some(v) = build_command {
        component.build_command = Some(v);
        updated.push("buildCommand".to_string());
    }
    if is_network {
        component.is_network = Some(true);
        updated.push("isNetwork".to_string());
    }
    if not_network {
        component.is_network = None;
        updated.push("isNetwork".to_string());
    }

    if updated.is_empty() {
        print_error("NO_FIELDS", "No fields specified to update");
        return;
    }

    if let Err(e) = ConfigManager::save_component(&component) {
        print_error(e.code(), &e.to_string());
        return;
    }

    #[derive(Serialize)]
    struct SetResult {
        id: String,
        updated: Vec<String>,
    }

    print_success(SetResult {
        id: id.to_string(),
        updated,
    });
}

fn delete(id: &str, force: bool) {
    if ConfigManager::load_component(id).is_err() {
        print_error("COMPONENT_NOT_FOUND", &format!("Component '{}' not found", id));
        return;
    }

    if !force {
        let projects = ConfigManager::list_projects().unwrap_or_default();
        let using: Vec<String> = projects
            .iter()
            .filter(|p| p.component_ids.contains(&id.to_string()))
            .map(|p| p.id.clone())
            .collect();

        if !using.is_empty() {
            print_error(
                "COMPONENT_IN_USE",
                &format!(
                    "Component '{}' is used by projects: {}. Use --force to delete anyway.",
                    id,
                    using.join(", ")
                ),
            );
            return;
        }
    }

    if let Err(e) = ConfigManager::delete_component(id) {
        print_error(e.code(), &e.to_string());
        return;
    }

    #[derive(Serialize)]
    struct DeleteResult {
        deleted: String,
    }

    print_success(DeleteResult {
        deleted: id.to_string(),
    });
}

fn list(json: bool) {
    let components = match ConfigManager::list_components() {
        Ok(c) => c,
        Err(e) => {
            if json {
                print_error(e.code(), &e.to_string());
            } else {
                eprintln!("Error: {}", e);
            }
            return;
        }
    };

    if json {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ComponentEntry {
            id: String,
            name: String,
            local_path: String,
            remote_path: String,
            build_artifact: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            build_command: Option<String>,
        }

        let entries: Vec<ComponentEntry> = components
            .into_iter()
            .map(|c| ComponentEntry {
                id: c.id,
                name: c.name,
                local_path: c.local_path,
                remote_path: c.remote_path,
                build_artifact: c.build_artifact,
                build_command: c.build_command,
            })
            .collect();

        print_success(entries);
    } else {
        if components.is_empty() {
            println!("No components configured.");
            println!("Create one with: homeboy component create <id> --name <name> --local-path <path> --remote-path <path> --build-artifact <path>");
        } else {
            println!("Components:");
            for c in components {
                println!("  {}", c.id);
                println!("    Name: {}", c.name);
                println!("    Local: {}", c.local_path);
                println!("    Remote: {}", c.remote_path);
                if let Some(bc) = c.build_command {
                    println!("    Build: {}", bc);
                }
            }
        }
    }
}
