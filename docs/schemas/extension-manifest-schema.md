# Extension Manifest Schema

Extension manifests define extension metadata, runtime behavior, platform behaviors, and integration points. Stored as `<extension_id>/<extension_id>.json` in the extension directory.

## Schema

```json
{
  "name": "string",
  "id": "string",
  "version": "string",
  "description": "string",
  "runtime": {},
  "platform": {},
  "commands": {},
  "actions": {},
  "release_actions": {},
  "docs": [],
  "capabilities": [],
  "storage_backend": "string"
}
```

## Fields

### Required Fields

- **`name`** (string): Human-readable extension name
- **`id`** (string): Unique extension identifier (must match directory name)
- **`version`** (string): Extension version (semantic versioning)

### Optional Fields

- **`description`** (string): Extension description
- **`runtime`** (object): Executable extension runtime configuration
- **`platform`** (object): Platform behavior definitions
- **`commands`** (object): Additional CLI commands provided by extension
- **`actions`** (object): Action definitions for `homeboy extension action`
- **`release_actions`** (object): Release pipeline step definitions
- **`docs`** (array): Documentation topic paths
- **`capabilities`** (array): Capabilities provided by extension (e.g., `["storage"]`)
- **`storage_backend`** (string): Storage backend identifier for storage capability

## Runtime Configuration

Runtime configuration defines how executable extensions are executed.

```json
{
  "runtime": {
    "run_command": "string",
    "setup_command": "string",
    "ready_check": "string",
    "entrypoint": "string",
    "env": {}
  }
}
```

### Runtime Fields

- **`run_command`** (string): Shell command to execute the extension
  - Template variables: `{{extensionPath}}`, `{{entrypoint}}`, `{{args}}`, plus project context variables
  - Example: `"./venv/bin/python3 {{entrypoint}} {{args}}"`
- **`setup_command`** (string): Command to run during install/update (optional)
  - Example: `"python3 -m venv venv && ./venv/bin/pip install -r requirements.txt"`
- **`ready_check`** (string): Command to verify extension readiness (optional)
  - Exit code 0 = ready, non-zero = not ready
  - Example: `"test -f ./venv/bin/python3"`
- **`entrypoint`** (string): Extension entrypoint script (optional)
  - Example: `"main.py"`
- **`env`** (object): Environment variables to set during execution
  - Values can use template variables
  - Example: `{"MY_VAR": "{{extensionPath}}/data"}`

## Platform Configuration

Platform configuration defines database, deployment, and version detection behaviors.

```json
{
  "platform": {
    "database": {},
    "deployment": {},
    "version_patterns": []
  }
}
```

### Database Configuration

```json
{
  "platform": {
    "database": {
      "cli": {
        "connect": "string",
        "query": "string",
        "tables": "string",
        "describe": "string"
      },
      "defaults": {
        "host": "string",
        "port": number,
        "user": "string"
      }
    }
  }
}
```

#### Database Fields

- **`cli`** (object): Database CLI template commands
  - **`connect`** (string): Connection command template
    - Template variables: `{{db_host}}`, `{{db_port}}`, `{{db_name}}`, `{{db_user}}`
  - **`query`** (string): Query command template
    - Template variables: `{{query}}`, `{{db_host}}`, `{{db_name}}`, etc.
  - **`tables`** (string): List tables command template
  - **`describe`** (string): Describe table command template
- **`defaults`** (object): Default database connection values
  - **`host`** (string): Default host
  - **`port`** (number): Default port
  - **`user`** (string): Default user

### Deployment Configuration

```json
{
  "platform": {
    "deployment": {
      "override_command": "string",
      "override_extract_command": "string"
    }
  }
}
```

#### Deployment Fields

- **`override_command`** (string): Custom build command template
  - Template variables: `{{targetDir}}`, `{{siteRoot}}`, `{{domain}}`, `{{cliPath}}`, `{{allowRootFlag}}`
- **`override_extract_command`** (string): Custom extract command template
  - Template variables: `{{artifact}}`, `{{targetDir}}`, `{{stagingArtifact}}`

### Version Patterns

```json
{
  "platform": {
    "version_patterns": [
      {
        "file": "string",
        "pattern": "string"
      }
    ]
  }
}
```

#### Version Pattern Fields

- **`file`** (string): Path to version file (relative to component root)
- **`pattern`** (string): Regex pattern to extract version

## Commands Configuration

Extensions can register additional top-level CLI commands.

```json
{
  "commands": {
    "<command_name>": {
      "description": "string",
      "run_command": "string",
      "help": "string"
    }
  }
}
```

### Command Fields

- **`description`** (string): Command description for help text
- **`run_command`** (string): Execution template
  - Template variables: `{{args}}`, plus extension runtime variables
- **`help`** (string): Detailed help text (optional)

## Actions Configuration

Actions define executable operations accessible via `homeboy extension action`.

```json
{
  "actions": {
    "<action_id>": {
      "type": "cli|api",
      "description": "string",
      "config": {}
    }
  }
}
```

### Action Fields

- **`type`** (string): `"cli"` or `"api"`
- **`description`** (string): Action description
- **`config`** (object): Action-specific configuration

#### CLI Action

```json
{
  "actions": {
    "sync": {
      "type": "cli",
      "description": "Sync data",
      "config": {
        "command": "sync --output {{format}}"
      }
    }
  }
}
```

#### API Action

```json
{
  "actions": {
    "create_release": {
      "type": "api",
      "description": "Create GitHub release",
      "config": {
        "method": "POST",
        "path": "/repos/{owner}/{repo}/releases",
        "template": {
          "tag_name": "{{release.tag}}",
          "name": "{{release.name}}",
          "body": "{{release.notes}}"
        }
      }
    }
  }
}
```

## Release Actions Configuration

Release actions define steps for release pipelines.

```json
{
  "release_actions": {
    "<step_type>": {
      "type": "extension.run|extension.action",
      "config": {}
    }
  }
}
```

### Release Action Types

- **`extension.run`**: Execute extension runtime command
- **`extension.action`**: Execute extension action

#### Example

```json
{
  "release_actions": {
    "publish": {
      "type": "extension.run",
      "config": {
        "extension": "github",
        "inputs": [
          {"id": "create_release", "value": "true"}
        ]
      }
    }
  }
}
```

## Hooks Configuration

Extensions can declare lifecycle hooks that run at named events. Extension hooks execute before component hooks, providing platform-level behavior.

```json
{
  "hooks": {
    "pre:version:bump": ["cargo generate-lockfile"],
    "post:deploy": [
      "wp cache flush --path={{base_path}} --allow-root 2>/dev/null || true"
    ]
  }
}
```

### Hooks Fields

- **`hooks`** (object): Map of event names to command arrays
  - Keys: event name (e.g., `pre:version:bump`, `post:version:bump`, `post:release`, `post:deploy`)
  - Values: array of shell command strings

Most hooks execute locally in the component's directory. `post:deploy` hooks execute **remotely via SSH** with template variable expansion:

| Variable | Description |
|----------|-------------|
| `{{component_id}}` | The component ID |
| `{{install_dir}}` | Remote install directory (base_path + remote_path) |
| `{{base_path}}` | Project base path on the remote server |

See [hooks architecture](../architecture/hooks.md) for details on execution order and failure modes.

## Documentation Configuration

Extensions can provide embedded documentation.

```json
{
  "docs": [
    "overview.md",
    "commands/wp-cli.md"
  ]
}
```

Documentation files live in the extension's `docs/` directory. Topics resolve to `homeboy docs <extension_id>/<topic>`.

## Capabilities and Storage Backend

```json
{
  "capabilities": ["storage"],
  "storage_backend": "filesystem"
}
```

- **`capabilities`**: Array of capability strings (e.g., `["storage"]`)
- **`storage_backend`**: Storage backend identifier when providing storage capability

## Complete Example

```json
{
  "name": "WordPress",
  "id": "wordpress",
  "version": "1.0.0",
  "description": "WordPress platform integration with WP-CLI",
  "runtime": {
    "run_command": "wp {{args}}",
    "setup_command": "curl -O https://raw.githubusercontent.com/wp-cli/builds/gh-pages/phar/wp-cli.phar && chmod +x wp-cli.phar && sudo mv wp-cli.phar /usr/local/bin/wp",
    "ready_check": "wp --version"
  },
  "platform": {
    "database": {
      "cli": {
        "connect": "wp db cli",
        "query": "wp db query \"{{query}}\"",
        "tables": "wp db tables",
        "describe": "wp db describe {{table}}"
      },
      "defaults": {
        "host": "localhost",
        "port": 3306,
        "user": "root"
      }
    },
    "version_patterns": [
      {
        "file": "style.css",
        "pattern": "Version:\\s*([\\d.]+)"
      }
    ]
  },
  "commands": {
    "wp": {
      "description": "Run WP-CLI commands",
      "run_command": "wp {{args}}",
      "help": "Execute WP-CLI commands in the project context"
    }
  },
  "docs": [
    "overview.md",
    "commands/wp-cli.md"
  ]
}
```

## Storage Location

Extension manifests are stored in the extension directory:
- Git extensions: `~/.config/homeboy/extensions/<extension_id>/<extension_id>.json`
- Symlinked extensions: `<source_path>/<extension_id>.json`

## Related

- [Extension command](../commands/extension.md) - Manage extension installation and execution
- [Template variables](../templates.md) - Variable reference for templates
