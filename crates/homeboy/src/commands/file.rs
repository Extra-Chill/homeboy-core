use clap::{Args, Subcommand};
use serde::Serialize;
use std::io::{self, Read};
use homeboy_core::config::ConfigManager;
use homeboy_core::ssh::SshClient;
use homeboy_core::output::{print_success, print_error};

#[derive(Args)]
pub struct FileArgs {
    #[command(subcommand)]
    command: FileCommand,
}

#[derive(Subcommand)]
enum FileCommand {
    /// List directory contents
    List {
        /// Project ID
        project_id: String,
        /// Remote directory path
        path: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Read file content
    Read {
        /// Project ID
        project_id: String,
        /// Remote file path
        path: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Write content to file (from stdin)
    Write {
        /// Project ID
        project_id: String,
        /// Remote file path
        path: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a file or directory
    Delete {
        /// Project ID
        project_id: String,
        /// Remote path to delete
        path: String,
        /// Delete directories recursively
        #[arg(short, long)]
        recursive: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Rename or move a file
    Rename {
        /// Project ID
        project_id: String,
        /// Current path
        old_path: String,
        /// New path
        new_path: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub fn run(args: FileArgs) {
    match args.command {
        FileCommand::List { project_id, path, json } => list(&project_id, &path, json),
        FileCommand::Read { project_id, path, json } => read(&project_id, &path, json),
        FileCommand::Write { project_id, path, json } => write(&project_id, &path, json),
        FileCommand::Delete { project_id, path, recursive, json } => {
            delete(&project_id, &path, recursive, json)
        }
        FileCommand::Rename { project_id, old_path, new_path, json } => {
            rename(&project_id, &old_path, &new_path, json)
        }
    }
}

struct FileContext {
    client: SshClient,
    base_path: String,
}

fn build_context(project_id: &str, json: bool) -> Option<FileContext> {
    let project = match ConfigManager::load_project(project_id) {
        Ok(p) => p,
        Err(e) => {
            if json { print_error(e.code(), &e.to_string()); }
            else { eprintln!("Error: {}", e); }
            return None;
        }
    };

    let server_id = match &project.server_id {
        Some(id) => id,
        None => {
            let msg = format!("Server not configured for project '{}'", project_id);
            if json { print_error("SERVER_NOT_CONFIGURED", &msg); }
            else { eprintln!("Error: {}", msg); }
            return None;
        }
    };

    let server = match ConfigManager::load_server(server_id) {
        Ok(s) => s,
        Err(e) => {
            if json { print_error(e.code(), &e.to_string()); }
            else { eprintln!("Error: {}", e); }
            return None;
        }
    };

    let client = match SshClient::from_server(&server, server_id) {
        Ok(c) => c,
        Err(e) => {
            if json { print_error("SSH_ERROR", &e.to_string()); }
            else { eprintln!("Error: {}", e); }
            return None;
        }
    };

    Some(FileContext {
        client,
        base_path: project.base_path.unwrap_or_default(),
    })
}

fn resolve_path(path: &str, base_path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else if base_path.is_empty() {
        path.to_string()
    } else if base_path.ends_with('/') {
        format!("{}{}", base_path, path)
    } else {
        format!("{}/{}", base_path, path)
    }
}

fn list(project_id: &str, path: &str, json: bool) {
    let ctx = match build_context(project_id, json) {
        Some(c) => c,
        None => return,
    };

    let full_path = resolve_path(path, &ctx.base_path);
    let command = format!("ls -la '{}'", full_path);
    let output = ctx.client.execute(&command);

    if !output.success {
        if json {
            print_error("LIST_FAILED", &output.stderr);
        } else {
            eprintln!("Error: {}", output.stderr);
        }
        return;
    }

    if json {
        let entries = parse_ls_output(&output.stdout, &full_path);
        print_success(entries);
    } else {
        print!("{}", output.stdout);
    }
}

fn read(project_id: &str, path: &str, json: bool) {
    let ctx = match build_context(project_id, json) {
        Some(c) => c,
        None => return,
    };

    let full_path = resolve_path(path, &ctx.base_path);
    let command = format!("cat '{}'", full_path);
    let output = ctx.client.execute(&command);

    if !output.success {
        if json {
            print_error("READ_FAILED", &output.stderr);
        } else {
            eprintln!("Error: {}", output.stderr);
        }
        return;
    }

    if json {
        #[derive(Serialize)]
        struct FileContent {
            path: String,
            content: String,
        }

        print_success(FileContent {
            path: full_path,
            content: output.stdout,
        });
    } else {
        print!("{}", output.stdout);
    }
}

fn write(project_id: &str, path: &str, json: bool) {
    let ctx = match build_context(project_id, json) {
        Some(c) => c,
        None => return,
    };

    // Read content from stdin
    let mut content = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut content) {
        if json {
            print_error("STDIN_ERROR", &e.to_string());
        } else {
            eprintln!("Error reading stdin: {}", e);
        }
        return;
    }

    // Remove trailing newline if present
    if content.ends_with('\n') {
        content.pop();
    }

    let full_path = resolve_path(path, &ctx.base_path);
    let command = format!(
        "cat > '{}' << 'HOMEBOYEOF'\n{}\nHOMEBOYEOF",
        full_path, content
    );
    let output = ctx.client.execute(&command);

    if !output.success {
        if json {
            print_error("WRITE_FAILED", &output.stderr);
        } else {
            eprintln!("Error: {}", output.stderr);
        }
        return;
    }

    if json {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct WriteResult {
            path: String,
            bytes_written: usize,
        }

        print_success(WriteResult {
            path: full_path,
            bytes_written: content.len(),
        });
    } else {
        println!("Written {} bytes to {}", content.len(), full_path);
    }
}

fn delete(project_id: &str, path: &str, recursive: bool, json: bool) {
    let ctx = match build_context(project_id, json) {
        Some(c) => c,
        None => return,
    };

    let full_path = resolve_path(path, &ctx.base_path);
    let flags = if recursive { "-rf" } else { "-f" };
    let command = format!("rm {} '{}'", flags, full_path);
    let output = ctx.client.execute(&command);

    if !output.success {
        if json {
            print_error("DELETE_FAILED", &output.stderr);
        } else {
            eprintln!("Error: {}", output.stderr);
        }
        return;
    }

    if json {
        #[derive(Serialize)]
        struct DeleteResult {
            path: String,
        }

        print_success(DeleteResult { path: full_path });
    } else {
        println!("Deleted: {}", full_path);
    }
}

fn rename(project_id: &str, old_path: &str, new_path: &str, json: bool) {
    let ctx = match build_context(project_id, json) {
        Some(c) => c,
        None => return,
    };

    let full_old = resolve_path(old_path, &ctx.base_path);
    let full_new = resolve_path(new_path, &ctx.base_path);
    let command = format!("mv '{}' '{}'", full_old, full_new);
    let output = ctx.client.execute(&command);

    if !output.success {
        if json {
            print_error("RENAME_FAILED", &output.stderr);
        } else {
            eprintln!("Error: {}", output.stderr);
        }
        return;
    }

    if json {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct RenameResult {
            old_path: String,
            new_path: String,
        }

        print_success(RenameResult {
            old_path: full_old,
            new_path: full_new,
        });
    } else {
        println!("Renamed: {} → {}", full_old, full_new);
    }
}

#[derive(Serialize)]
struct FileEntry {
    name: String,
    path: String,
    #[serde(rename = "isDirectory")]
    is_directory: bool,
    size: Option<i64>,
    permissions: String,
}

fn parse_ls_output(output: &str, base_path: &str) -> Vec<FileEntry> {
    let mut entries = Vec::new();

    for line in output.lines() {
        if line.is_empty() || line.starts_with("total ") {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            continue;
        }

        let permissions = parts[0];
        let name = parts[8..].join(" ");

        if name == "." || name == ".." {
            continue;
        }

        let is_directory = permissions.starts_with('d');
        let size = parts[4].parse::<i64>().ok();

        let full_path = if base_path.ends_with('/') {
            format!("{}{}", base_path, name)
        } else {
            format!("{}/{}", base_path, name)
        };

        entries.push(FileEntry {
            name,
            path: full_path,
            is_directory,
            size,
            permissions: permissions[1..].to_string(),
        });
    }

    entries.sort_by(|a, b| {
        if a.is_directory != b.is_directory {
            return b.is_directory.cmp(&a.is_directory);
        }
        a.name.to_lowercase().cmp(&b.name.to_lowercase())
    });

    entries
}
