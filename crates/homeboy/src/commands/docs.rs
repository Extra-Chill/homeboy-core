use clap::Args;
use std::fs;
use std::io::{self, Write};
use std::process::{Command, Stdio};
use homeboy_core::config::AppPaths;

#[derive(Args)]
pub struct DocsArgs {
    /// Topic to filter (e.g., 'deploy', 'project set')
    #[arg(trailing_var_arg = true)]
    topic: Vec<String>,
}

pub fn run(args: DocsArgs) {
    let content = match load_documentation() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            eprintln!("Documentation available at: https://github.com/Extra-Chill/homeboy/blob/main/docs/CLI.md");
            return;
        }
    };

    if args.topic.is_empty() {
        display_with_pager(&content);
    } else {
        let search_topic = args.topic.join(" ");
        let filtered = filter_to_topic(&content, &search_topic);

        if filtered.is_empty() {
            eprintln!("No documentation found for '{}'.", search_topic);
            eprintln!("Available topics: projects, project, server, wp, pm2, db, deploy, ssh, module, component, pin, file, logs");
        } else {
            println!("{}", filtered);
        }
    }
}

fn load_documentation() -> Result<String, String> {
    // Try multiple locations
    let paths = [
        AppPaths::docs().join("CLI.md"),
        AppPaths::homeboy().join("docs/CLI.md"),
    ];

    for path in &paths {
        if let Ok(content) = fs::read_to_string(path) {
            return Ok(content);
        }
    }

    // Return embedded minimal documentation if no file found
    Ok(get_embedded_docs())
}

fn filter_to_topic(content: &str, topic: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::new();
    let mut capturing = false;
    let mut capture_depth = 0;
    let normalized_topic = topic.to_lowercase();

    for line in lines {
        let trimmed = line.trim();

        if trimmed.starts_with('#') {
            let depth = trimmed.chars().take_while(|&c| c == '#').count();
            let heading = trimmed
                .trim_start_matches('#')
                .trim()
                .to_lowercase();

            if heading == normalized_topic
                || heading.starts_with(&format!("{} ", normalized_topic))
                || heading.contains(&normalized_topic)
            {
                capturing = true;
                capture_depth = depth;
                result.push(line);
            } else if capturing && depth <= capture_depth {
                break;
            } else if capturing {
                result.push(line);
            }
        } else if capturing {
            result.push(line);
        }
    }

    result.join("\n").trim().to_string()
}

fn display_with_pager(content: &str) {
    // Check if stdout is a terminal
    if !atty::is(atty::Stream::Stdout) {
        println!("{}", content);
        return;
    }

    // Try to use less pager
    let less = Command::new("less")
        .arg("-R")
        .stdin(Stdio::piped())
        .spawn();

    match less {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(content.as_bytes());
            }
            let _ = child.wait();
        }
        Err(_) => {
            // Fallback to plain output
            println!("{}", content);
        }
    }
}

fn get_embedded_docs() -> String {
    r#"# Homeboy CLI Documentation

## Overview

Homeboy is a CLI tool for development and deployment automation. It manages projects, servers, and remote operations.

## Commands

### projects
List all configured projects.
```
homeboy projects [--current] [--json]
```

### project
Manage project configurations.
```
homeboy project show <id>
homeboy project switch <id>
homeboy project create <id> --name <name> --domain <domain> --type <type>
homeboy project set <id> [--name <name>] [--domain <domain>] [--server <server-id>]
homeboy project delete <id>
```

### server
Manage SSH server configurations.
```
homeboy server list [--json]
homeboy server show <id>
homeboy server create <id> --name <name> --host <host> --user <user>
homeboy server set <id> [--name <name>] [--host <host>] [--user <user>]
homeboy server delete <id>
homeboy server key generate <id>
homeboy server key show <id>
```

### ssh
SSH into project server or run commands.
```
homeboy ssh <project-id>                    # Interactive shell
homeboy ssh <project-id> <command>          # Run command
```

### wp
Run WP-CLI commands on WordPress projects.
```
homeboy wp <project-id> [subtarget] <args>
```
Example: `homeboy wp extrachill core version`

### pm2
Run PM2 commands on Node.js projects.
```
homeboy pm2 <project-id> [subtarget] <args>
```
Example: `homeboy pm2 myapp status`

### db
Database operations.
```
homeboy db <project-id> tables [--json]
homeboy db <project-id> describe <table> [--json]
homeboy db <project-id> query "<sql>" [--json]
homeboy db <project-id> tunnel [--port <port>]
homeboy db <project-id> delete-row <table> <id> --confirm
homeboy db <project-id> drop-table <table> --confirm
```

### deploy
Deploy components to remote server.
```
homeboy deploy <project-id> [component-ids...] [--all] [--build] [--dry-run] [--json]
```

### component
Manage standalone component configurations.
```
homeboy component list [--json]
homeboy component show <id>
homeboy component create <id> --name <name> --local-path <path> --remote-path <path> --build-artifact <path>
homeboy component set <id> [--name <name>] [--build-command <cmd>]
homeboy component delete <id> [--force]
homeboy component import '<json>' [--skip-existing]
```

### file
Remote file operations.
```
homeboy file list <project-id> <path> [--json]
homeboy file read <project-id> <path> [--json]
homeboy file write <project-id> <path> [--json]    # Reads content from stdin
homeboy file delete <project-id> <path> [-r] [--json]
homeboy file rename <project-id> <old-path> <new-path> [--json]
```

### logs
Remote log viewing.
```
homeboy logs list <project-id> [--json]
homeboy logs show <project-id> <path> [-n <lines>] [-f] [--json]
homeboy logs clear <project-id> <path> [--json]
```

### pin
Manage pinned files and logs.
```
homeboy pin list <project-id> --type file|log [--json]
homeboy pin add <project-id> <path> --type file|log [--label <label>] [--tail <lines>] [--json]
homeboy pin remove <project-id> <path> --type file|log [--json]
```

### module
Execute CLI-compatible modules.
```
homeboy module list [--project <id>] [--json]
homeboy module run <module-id> [--project <id>] [-i key=value...] [args...]
homeboy module setup <module-id>
```

### docs
Display CLI documentation.
```
homeboy docs [<topic>]
```
Example: `homeboy docs deploy`

## Configuration

Configuration is stored in:
- macOS: `~/Library/Application Support/Homeboy/`
- Linux: `~/.local/share/Homeboy/`

```
Homeboy/
├── config.json           # Active project ID
├── projects/             # Project configurations
├── servers/              # Server configurations
├── components/           # Component configurations
├── modules/              # Installed modules
├── keys/                 # SSH keys
└── playwright-browsers/  # Shared Playwright browsers
```

## More Information

Full documentation: https://github.com/Extra-Chill/homeboy
"#.to_string()
}
