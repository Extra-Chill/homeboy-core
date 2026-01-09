use clap::{Args, Subcommand};
use serde::Serialize;
use std::fs;
use std::process::Command;
use homeboy_core::config::{ConfigManager, ServerConfig, AppPaths};
use homeboy_core::output::{print_success, print_error};

#[derive(Args)]
pub struct ServerArgs {
    #[command(subcommand)]
    command: ServerCommand,
}

#[derive(Subcommand)]
enum ServerCommand {
    /// Register a new SSH server
    Create {
        /// Server display name
        name: String,
        /// SSH host
        #[arg(long)]
        host: String,
        /// SSH username
        #[arg(long)]
        user: String,
        /// SSH port (default: 22)
        #[arg(long, default_value = "22")]
        port: u16,
    },
    /// Display server configuration
    Show {
        /// Server ID
        server_id: String,
    },
    /// Modify server settings
    Set {
        /// Server ID
        server_id: String,
        /// Server display name
        #[arg(long)]
        name: Option<String>,
        /// SSH host
        #[arg(long)]
        host: Option<String>,
        /// SSH username
        #[arg(long)]
        user: Option<String>,
        /// SSH port
        #[arg(long)]
        port: Option<u16>,
    },
    /// Remove a server configuration
    Delete {
        /// Server ID
        server_id: String,
        /// Confirm deletion
        #[arg(long)]
        force: bool,
    },
    /// List all configured servers
    List,
    /// Manage SSH keys
    Key(KeyArgs),
}

#[derive(Args)]
pub struct KeyArgs {
    #[command(subcommand)]
    command: KeyCommand,
}

#[derive(Subcommand)]
enum KeyCommand {
    /// Generate a new SSH key pair
    Generate {
        /// Server ID
        server_id: String,
    },
    /// Display the public SSH key
    Show {
        /// Server ID
        server_id: String,
        /// Output raw key only (no JSON)
        #[arg(long)]
        raw: bool,
    },
    /// Import an existing SSH private key
    Import {
        /// Server ID
        server_id: String,
        /// Path to private key file
        private_key_path: String,
    },
}

pub fn run(args: ServerArgs) {
    match args.command {
        ServerCommand::Create { name, host, user, port } => create(&name, &host, &user, port),
        ServerCommand::Show { server_id } => show(&server_id),
        ServerCommand::Set { server_id, name, host, user, port } => {
            set(&server_id, name, host, user, port)
        }
        ServerCommand::Delete { server_id, force } => delete(&server_id, force),
        ServerCommand::List => list(),
        ServerCommand::Key(key_args) => run_key(key_args),
    }
}

fn run_key(args: KeyArgs) {
    match args.command {
        KeyCommand::Generate { server_id } => key_generate(&server_id),
        KeyCommand::Show { server_id, raw } => key_show(&server_id, raw),
        KeyCommand::Import { server_id, private_key_path } => key_import(&server_id, &private_key_path),
    }
}

fn create(name: &str, host: &str, user: &str, port: u16) {
    let id = ServerConfig::generate_id(host);

    if ConfigManager::load_server(&id).is_ok() {
        print_error("SERVER_EXISTS", &format!("Server '{}' already exists", id));
        return;
    }

    let server = ServerConfig {
        id: id.clone(),
        name: name.to_string(),
        host: host.to_string(),
        user: user.to_string(),
        port,
    };

    if let Err(e) = ConfigManager::save_server(&server) {
        print_error("SAVE_ERROR", &e.to_string());
        return;
    }

    #[derive(Serialize)]
    struct CreateResult {
        id: String,
        name: String,
        host: String,
        user: String,
        port: u16,
        note: String,
    }

    print_success(CreateResult {
        id: id.clone(),
        name: name.to_string(),
        host: host.to_string(),
        user: user.to_string(),
        port,
        note: format!("Run 'homeboy server key generate {}' to create SSH key", id),
    });
}

fn show(server_id: &str) {
    match ConfigManager::load_server(server_id) {
        Ok(server) => print_success(&server),
        Err(e) => print_error(e.code(), &e.to_string()),
    }
}

fn set(server_id: &str, name: Option<String>, host: Option<String>, user: Option<String>, port: Option<u16>) {
    let mut server = match ConfigManager::load_server(server_id) {
        Ok(s) => s,
        Err(e) => {
            print_error(e.code(), &e.to_string());
            return;
        }
    };

    let mut changes = Vec::new();

    if let Some(n) = name {
        server.name = n;
        changes.push("name");
    }
    if let Some(h) = host {
        server.host = h;
        changes.push("host");
    }
    if let Some(u) = user {
        server.user = u;
        changes.push("user");
    }
    if let Some(p) = port {
        server.port = p;
        changes.push("port");
    }

    if changes.is_empty() {
        print_error("NO_CHANGES", "No changes specified");
        return;
    }

    if let Err(e) = ConfigManager::save_server(&server) {
        print_error("SAVE_ERROR", &e.to_string());
        return;
    }

    #[derive(Serialize)]
    struct SetResult {
        id: String,
        updated: Vec<String>,
    }

    print_success(SetResult {
        id: server_id.to_string(),
        updated: changes.iter().map(|s| s.to_string()).collect(),
    });
}

fn delete(server_id: &str, force: bool) {
    if !force {
        print_error("CONFIRM_REQUIRED", "Use --force to confirm deletion");
        return;
    }

    if ConfigManager::load_server(server_id).is_err() {
        print_error("SERVER_NOT_FOUND", &format!("Server '{}' not found", server_id));
        return;
    }

    // Check if any project uses this server
    if let Ok(projects) = ConfigManager::list_projects() {
        for project in projects {
            if project.server_id.as_deref() == Some(server_id) {
                print_error(
                    "SERVER_IN_USE",
                    &format!("Server is used by project '{}'. Update or delete the project first.", project.id),
                );
                return;
            }
        }
    }

    if let Err(e) = ConfigManager::delete_server(server_id) {
        print_error("DELETE_ERROR", &e.to_string());
        return;
    }

    #[derive(Serialize)]
    struct DeleteResult {
        deleted: String,
    }

    print_success(DeleteResult {
        deleted: server_id.to_string(),
    });
}

fn list() {
    match ConfigManager::list_servers() {
        Ok(servers) => {
            #[derive(Serialize)]
            struct ListResult {
                servers: Vec<ServerConfig>,
            }
            print_success(ListResult { servers });
        }
        Err(e) => print_error(e.code(), &e.to_string()),
    }
}

fn key_generate(server_id: &str) {
    if ConfigManager::load_server(server_id).is_err() {
        print_error("SERVER_NOT_FOUND", &format!("Server '{}' not found", server_id));
        return;
    }

    let key_path = AppPaths::key(server_id);
    let key_path_str = key_path.to_string_lossy();

    // Ensure keys directory exists
    if let Some(parent) = key_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    // Remove existing key if present
    let _ = fs::remove_file(&key_path);
    let _ = fs::remove_file(format!("{}.pub", key_path_str));

    // Generate new key pair
    let status = Command::new("ssh-keygen")
        .args([
            "-t", "rsa",
            "-b", "4096",
            "-f", &key_path_str,
            "-N", "",  // Empty passphrase
            "-C", &format!("homeboy-{}", server_id),
        ])
        .output();

    match status {
        Ok(output) if output.status.success() => {
            // Read the public key
            let pub_key_path = format!("{}.pub", key_path_str);
            match fs::read_to_string(&pub_key_path) {
                Ok(public_key) => {
                    #[derive(Serialize)]
                    #[serde(rename_all = "camelCase")]
                    struct KeyResult {
                        server_id: String,
                        public_key: String,
                        note: String,
                    }

                    print_success(KeyResult {
                        server_id: server_id.to_string(),
                        public_key: public_key.trim().to_string(),
                        note: "Add this public key to ~/.ssh/authorized_keys on the server".to_string(),
                    });
                }
                Err(e) => print_error("READ_ERROR", &format!("Failed to read public key: {}", e)),
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            print_error("KEYGEN_ERROR", &format!("ssh-keygen failed: {}", stderr));
        }
        Err(e) => print_error("KEYGEN_ERROR", &format!("Failed to run ssh-keygen: {}", e)),
    }
}

fn key_show(server_id: &str, raw: bool) {
    let key_path = AppPaths::key(server_id);
    let pub_key_path = format!("{}.pub", key_path.to_string_lossy());

    match fs::read_to_string(&pub_key_path) {
        Ok(public_key) => {
            if raw {
                println!("{}", public_key.trim());
            } else {
                #[derive(Serialize)]
                #[serde(rename_all = "camelCase")]
                struct KeyShowResult {
                    server_id: String,
                    public_key: String,
                }

                print_success(KeyShowResult {
                    server_id: server_id.to_string(),
                    public_key: public_key.trim().to_string(),
                });
            }
        }
        Err(_) => {
            print_error("KEY_NOT_FOUND", &format!("No SSH key configured for server '{}'", server_id));
        }
    }
}

fn key_import(server_id: &str, private_key_path: &str) {
    if ConfigManager::load_server(server_id).is_err() {
        print_error("SERVER_NOT_FOUND", &format!("Server '{}' not found", server_id));
        return;
    }

    // Expand tilde
    let expanded_path = shellexpand::tilde(private_key_path).to_string();

    // Read private key
    let private_key = match fs::read_to_string(&expanded_path) {
        Ok(k) => k,
        Err(e) => {
            print_error("READ_ERROR", &format!("Failed to read key file: {}", e));
            return;
        }
    };

    // Validate it looks like an SSH key
    if !private_key.contains("-----BEGIN") || !private_key.contains("PRIVATE KEY-----") {
        print_error("INVALID_KEY", "File doesn't appear to be a valid SSH private key");
        return;
    }

    // Derive public key using ssh-keygen
    let output = Command::new("ssh-keygen")
        .args(["-y", "-f", &expanded_path])
        .output();

    let public_key = match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        _ => {
            print_error("KEYGEN_ERROR", "Failed to derive public key from private key");
            return;
        }
    };

    // Write keys to Homeboy's key directory
    let key_path = AppPaths::key(server_id);
    let key_path_str = key_path.to_string_lossy();

    if let Some(parent) = key_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Err(e) = fs::write(&key_path, &private_key) {
        print_error("WRITE_ERROR", &format!("Failed to write private key: {}", e));
        return;
    }

    // Set permissions on private key (readable only by owner)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600));
    }

    if let Err(e) = fs::write(format!("{}.pub", key_path_str), &public_key) {
        print_error("WRITE_ERROR", &format!("Failed to write public key: {}", e));
        return;
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ImportResult {
        server_id: String,
        imported: String,
        public_key: String,
    }

    print_success(ImportResult {
        server_id: server_id.to_string(),
        imported: expanded_path,
        public_key,
    });
}
