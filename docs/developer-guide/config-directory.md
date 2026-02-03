# Config Directory Structure

> **Important:** Homeboy uses centralized configuration only. There is no repo-local config file (no `.homeboy.toml` or `.homeboy/` directory). All configuration lives in `~/.config/homeboy/`.

Homeboy stores all configuration in a universal directory location across operating systems.

## Location

Homeboy configuration lives under:

### macOS

```
~/.config/homeboy/
```

### Linux

```
~/.config/homeboy/
```

### Windows

```
%APPDATA%\homeboy\
```

Typically: `C:\Users\<username>\AppData\Roaming\homeboy\`

## Directory Structure

```
~/.config/homeboy/
├── homeboy/
│   └── homeboy.json           # Global app configuration
├── projects/
│   ├── <project_id>.json       # Project configurations
│   └── ...
├── servers/
│   ├── <server_id>.json       # Server configurations
│   └── ...
├── components/
│   ├── <component_id>.json    # Component configurations
│   └── ...
├── modules/
│   ├── <module_id>/
│   │   ├── <module_id>.json   # Module manifest
│   │   ├── docs/             # Module documentation
│   │   └── ...              # Module files
│   └── ...
├── keys/                     # SSH private keys (optional)
│   ├── <key_name>
│   └── ...
└── backups/                  # Configuration backups (optional)
    └── ...
```

## File Details

### Global App Configuration

**File:** `homeboy/homeboy.json`

Contains global Homeboy settings. Created automatically on first run with defaults.

```json
{
  "storage": "builtin-filesystem",
  "installedModules": []
}
```

### Project Configurations

**Directory:** `projects/`

Each project is a separate JSON file named after the project ID.

**Example:** `projects/extrachill.json`

```json
{
  "id": "extrachill",
  "name": "Extra Chill",
  "domain": "extrachill.com",
  "server_id": "production",
  "component_ids": ["theme", "api"]
}
```

### Server Configurations

**Directory:** `servers/`

Each server is a separate JSON file named after the server ID.

**Example:** `servers/production.json`

```json
{
  "id": "production",
  "name": "Production Server",
  "host": "example.com",
  "user": "deploy"
}
```

### Component Configurations

**Directory:** `components/`

Each component is a separate JSON file named after the component ID.

**Example:** `components/theme.json`

```json
{
  "id": "theme",
  "local_path": "/home/dev/theme",
  "remote_path": "wp-content/themes/theme"
}
```

### Module Directory

**Directory:** `modules/`

Each module is a subdirectory containing:
- Module manifest: `<module_id>/<module_id>.json`
- Module documentation: `<module_id>/docs/`
- Module files: `<module_id>/` (executables, scripts, etc.)

**Example:** `modules/wordpress/wordpress.json`

Modules are installed via:
- Git clone (remote modules)
- Symlink (local development modules)

### Keys Directory

**Directory:** `keys/`

Stores SSH private keys managed by Homeboy (optional). Keys can be referenced via relative paths in server configurations.

**Example:** `keys/production_key`

### Backups Directory

**Directory:** `backups/`

Configuration backups created by Homeboy (optional). Created before destructive operations.

## File Operations

Homeboy does not write to directories outside the config directory:
- **No repo-local config files**: Configuration is centralized
- **No .homeboy directories**: Avoids repo contamination
- **Cross-repo compatibility**: Multiple repos can reference the same configurations

## Auto-creation

Directories are created automatically when needed:
- `homeboy/`: First run
- `projects/`: First project created
- `servers/`: First server created
- `components/`: First component created
- `modules/<module_id>/`: Module installed
- `keys/`: Key referenced in server config
- `backups/`: Backup created

## Manual Configuration Editing

While Homeboy provides CLI commands for most operations, configurations can be edited manually:

### Editing Tips

1. **Use JSON validators**: Ensure valid JSON syntax
2. **Backup first**: Copy file before editing
3. **Reload changes**: Some changes require command restart
4. **Reference schemas**: See schema documentation for field definitions

### Schema References

- [Component schema](../schemas/component-schema.md)
- [Project schema](../schemas/project-schema.md)
- [Server schema](../schemas/server-schema.md)
- [Module manifest schema](../schemas/module-manifest-schema.md)

## Migration and Backups

### Backup Strategy

Homeboy creates backups before:
- Deleting configurations
- Major schema updates (optional)
- Bulk import operations

Backups are stored in `backups/` with timestamps.

### Export Configurations

Export all configurations to archive:

```bash
tar czf homeboy-config-backup.tar.gz ~/.config/homeboy/
```

### Import Configurations

Restore from backup:

```bash
tar xzf homeboy-config-backup.tar.gz -C ~/.config/
```

## Security Permissions

### Directory Permissions

Config directories should be restricted to user only:

```bash
chmod 700 ~/.config/homeboy
chmod 700 ~/.config/homeboy/keys
```

### File Permissions

Configuration files should be readable only by user:

```bash
chmod 600 ~/.config/homeboy/projects/*.json
chmod 600 ~/.config/homeboy/servers/*.json
chmod 600 ~/.config/homeboy/components/*.json
```

### SSH Keys

SSH private keys must be restricted:

```bash
chmod 600 ~/.config/homeboy/keys/*
```

## Troubleshooting

### Permission Denied Errors

If Homeboy reports permission errors:

```bash
# Fix permissions
chmod 700 ~/.config/homeboy
chmod 600 ~/.config/homeboy/projects/*.json
chmod 600 ~/.config/homeboy/servers/*.json
chmod 600 ~/.config/homeboy/components/*.json
```

### Directory Not Found

If Homeboy cannot find config directory:

1. Verify config directory location for your platform
2. Create directory manually: `mkdir -p ~/.config/homeboy`
3. Run `homeboy init` to initialize

### Corrupt Configuration

If configuration file is invalid:

1. Restore from backup in `backups/`
2. Or delete corrupt file and recreate via CLI commands

## Related

- [Init command](../commands/init.md) - Initialize Homeboy
- [Config command](../commands/config.md) - Manage global configuration
- [Project command](../commands/project.md) - Manage project configurations
- [Server command](../commands/server.md) - Manage server configurations
- [Component command](../commands/component.md) - Manage component configurations
- [Module command](../commands/module.md) - Manage module installations
