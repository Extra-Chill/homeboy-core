# Architecture Overview

Homeboy is a development and deployment automation tool built in Rust with a config-driven architecture.

## Design Principles

### Single Source of Truth

Configuration is the authoritative source for all system behavior. No hard-coded project-specific logic. Everything is driven by JSON configuration files in the config directory.

### Local-First

Homeboy runs on your local machine and orchestrates remote operations. It does not run as a remote server service. All state is stored locally in your OS config directory.

### Module System

Extensibility through a module system that allows:
- Platform-specific behaviors (WordPress, Node.js, Rust)
- Custom CLI commands
- Release pipeline actions
- Documentation topics

### Configuration-Driven

All behavior is configurable via JSON:
- Projects, servers, components
- Module manifests
- Release pipelines
- Module settings per project/component

## Core Systems

### Configuration Management

**Location:** `src/core/config.rs`

Centralized configuration system that:
- Loads JSON configs from config directory
- Validates schemas
- Merges settings across scopes (project + component)
- Provides helpers for set/merge/remove operations

Config entities:
- **Projects**: Deployable environments
- **Servers**: SSH connection settings
- **Components**: Buildable/deployable units
- **Modules**: Extensible behaviors and tools

### Storage Layer

**Location:** `src/core/local_files.rs`

File-based storage for configurations:
- Reads/writes JSON files in config directory
- Handles atomic operations for safety
- Cross-platform paths (macOS, Linux, Windows)

**Future:** Storage abstraction trait for database backends (see `storage-system-decoupling-plan.md`)

### Template System

**Location:** `src/utils/template.rs`

Variable substitution in templates:
- Both `{var}` and `{{var}}` syntax supported
- Context-aware resolution (project, component, module variables)
- Used in: deploy commands, module runtime, platform behaviors

### Execution System

**Location:** `src/core/engine/executor.rs` and `src/core/engine/`

Executes shell commands with:
- Environment variable injection
- Working directory management
- Output capture
- Exit code handling

Supports:
- Local commands (builds, tests)
- Remote commands via SSH
- Module runtime execution
- Module actions (CLI or API)

### SSH Operations

**Location:** `src/core/ssh/` and `src/utils/shell.rs`

SSH client wrapper that:
- Manages SSH connections
- Handles keychain-stored passphrases
- Supports SSH agent forwarding
- Executes remote commands and file operations

### HTTP Client

**Location:** `src/core/http.rs`

HTTP client for API operations:
- Template-based URL construction
- Keychain-stored authentication
- JSON request/response handling
- Module action API integration

### Keychain Integration

**Location:** `src/core/keychain.rs`

Secure credential storage:
- macOS: Keychain Access
- Linux: libsecret/gnome-keyring
- Windows: Credential Manager

Secrets stored:
- API tokens (per project)
- Database passwords (per project)
- SSH key passphrases

### Git Operations

**Location:** `src/core/git/`

Git wrapper for:
- Status checking
- Committing changes
- Tagging releases
- Push/pull operations

### Version Management

**Location:** `src/core/version.rs`

Semantic versioning:
- Pattern-based version detection in files
- Version bump operations (patch, minor, major)
- Multi-target version detection

### Changelog Management

**Location:** `src/core/changelog/`

Changelog operations:
- Add entries
- Categorize changes (Feature, Fix, Breaking, Docs, Chore, Other)
- Finalize for release
- Extract for git commits

### Module System

**Location:** `src/core/module/mod.rs`

Module management:
- Install from git or local path
- Load manifests
- Resolve settings
- Execute runtime and actions
- Provide CLI commands and docs

### Release Pipeline

**Location:** `src/core/release/` (executor.rs, pipeline.rs, types.rs)

Local orchestration system:
- Define steps as configuration
- Dependency graph resolution
- Plan without execution
- Execute with error handling

Step types:
- Built-in: build, version_bump, git_commit, git_tag, git_push
- Module: module_run, module_action

### Fleet Management

**Location:** `src/core/fleet.rs`

Fleet management for cloud version management:
- Named groups of projects
- Shared component detection
- Fleet-wide operations (status, check, deploy)
- Coordinated deployments across multiple servers

Config entities:
- **Fleets**: Named groups of projects for batch operations

### CLI Layer

**Location:** `src/commands/` and `src/main.rs`

Command-line interface built with `clap`:
- Command definitions and parsing
- Output mode selection (JSON, markdown, interactive)
- JSON envelope wrapping
- Response mapping

### Documentation System

**Location:** `src/docs/mod.rs` and `build.rs`

Embedded documentation:
- Markdown files embedded at compile time (`build.rs`)
- Runtime topic resolution
- Module-provided docs support

## Data Flow

### Deploy Command Flow

1. CLI parses `homeboy deploy <project> [components]`
2. Loads project configuration
3. Loads linked server configuration
4. Loads component configurations
5. Resolves deployment targets
6. For each component:
   - Detect version from local files
   - Detect version from remote files
   - Compare versions
   - If outdated or explicitly selected:
     - Execute build command
     - Upload artifact via SSH
     - Execute extract command
7. Return results in JSON envelope

### Module Execution Flow

1. CLI parses `homeboy module run <module> --project <project> --component <component>`
2. Load module manifest
3. Resolve project configuration (if provided)
4. Resolve component configuration (if provided)
5. Merge settings from project and component scopes
6. Build execution context:
   - Module metadata
   - Project context (domain, paths)
   - Component context (paths)
   - Merged settings
7. Set environment variables
8. Execute `runtime.run_command` with template resolution
9. Capture output and exit code
10. Return results in JSON envelope

### Release Pipeline Flow

1. CLI parses `homeboy release run <component>`
2. Load component configuration
3. Parse release pipeline steps
4. Validate step dependencies
5. Execute steps in order:
   - Wait for dependencies to complete
   - Execute step (build, module, git, etc.)
   - Stop on failure
6. Return results with status for each step

## Module Integration

### Module Manifest

Module manifest defines:
- Runtime configuration
- Platform behaviors (database, deployment, version patterns)
- CLI commands
- Actions (CLI or API)
- Release actions
- Documentation topics

### Module Loading

Modules are loaded from:
- Git-cloned directories in `~/.config/homeboy/modules/`
- Symlinked local directories
- Module manifest: `<module_id>/<module_id>.json`

### Module Execution

Two execution paths:
1. **Direct execution**: `homeboy module run <module_id>`
2. **Pipeline step**: `module.run` step in release pipeline

Both paths use the same execution context builder and template resolver.

## Error Handling

**Location:** `src/core/error/mod.rs`

Centralized error system:
- Error categories (validation, io, module, etc.)
- Error context (file path, component ID, etc.)
- Error messages for CLI output
- Error conversion for JSON envelope

## Output System

**Location:** `src/output/` (response.rs, mod.rs)

Output modes:
- **JSON**: Machine-readable, wrapped in stable envelope
- **Markdown**: Human-readable documentation
- **Interactive**: Passthrough for TTY commands (SSH, logs)

JSON envelope structure:
```json
{
  "success": true|false,
  "data": {},
  "error": {}
}
```

## Cross-Platform Considerations

### Paths

Homeboy handles path differences:
- **macOS/Linux**: Unix-style paths (`/home/user/`)
- **Windows**: Windows-style paths (`C:\Users\user\`)
- Path expansion: `~` is expanded to home directory

### Config Directory

Universal config directory:
- **macOS**: `~/.config/homeboy/`
- **Linux**: `~/.config/homeboy/`
- **Windows**: `%APPDATA%\homeboy\`

### Keychain

OS-specific credential storage:
- **macOS**: Keychain Access framework
- **Linux**: libsecret or gnome-keyring
- **Windows**: Windows Credential Manager API

## Related

- [API client system](../architecture/api-client.md) - HTTP client details
- [Keychain/secrets management](../architecture/keychain-secrets.md) - Credential storage
- [SSH key management](../architecture/ssh-key-management.md) - SSH operations
- [Release pipeline system](../architecture/release-pipeline.md) - Release orchestration
- [Execution context](../architecture/execution-context.md) - Runtime context building
- [Config directory structure](./config-directory.md) - File organization
