# Execution Context

Execution context provides runtime information to extensions during execution via environment variables and template variable resolution.

## Overview

When Homeboy executes extensions (via `homeboy extension run` or release pipeline steps), it builds an execution context containing:

- Extension metadata
- Project and component information (when available)
- Resolved settings
- Template variables
- Environment variables

## Environment Variables

Homeboy sets the following environment variables before executing extensions:

### Base Context Variables

- **`HOMEBOY_EXEC_CONTEXT_VERSION`**: Execution context protocol version (currently `"1"`)

### Extension Variables

- **`HOMEBOY_MODULE_ID`**: Extension identifier
- **`HOMEBOY_MODULE_PATH`**: Absolute path to extension directory

### Project Context (when project is specified)

- **`HOMEBOY_PROJECT_ID`**: Project identifier
- **`HOMEBOY_DOMAIN`**: Project domain
- **`HOMEBOY_SITE_PATH`**: Project site path (absolute)

### Component Context (when component is resolved)

- **`HOMEBOY_COMPONENT_ID`**: Component identifier
- **`HOMEBOY_COMPONENT_PATH`**: Absolute path to component directory

### Settings

- **`HOMEBOY_SETTINGS_JSON`**: Merged effective settings as JSON string

## Context Resolution Flow

Homeboy resolves execution context in the following order:

### 1. Extension Resolution

Extension is loaded from:
- Installed extensions directory (`~/.config/homeboy/extensions/<extension_id>/`)
- Or directly referenced via path (for development)

Extension metadata is extracted from `<extension_id>.json` manifest.

### 2. Project Resolution (optional)

If `--project` is specified:
- Project configuration is loaded (`projects/<project_id>.json`)
- Server configuration is loaded (via `server_id`)
- Database and API configuration is resolved

### 3. Component Resolution (optional)

If `--component` is specified:
- Component configuration is loaded (`components/<component_id>.json`)
- Component path is validated
- Component extension associations are identified

If `--component` is omitted:
- Homeboy attempts to resolve component from project's `component_ids`
- First matching component is used
- Ambiguity is resolved via user prompt or explicit specification

### 4. Settings Merge

Extension settings are merged from multiple scopes in order (later scopes override earlier ones):

1. **Project settings**: `projects/<project_id>.json` -> `extensions.<extension_id>.settings`
2. **Component settings**: `components/<component_id>.json` -> `extensions.<extension_id>.settings`

Merged settings are available as:
- Environment variable: `HOMEBOY_SETTINGS_JSON`
- Template variable: `{{settings.<key>}}`

### 5. Template Variable Resolution

Template variables are resolved from:
- Execution context variables
- Extension manifest `runtime.env` definitions
- Extension `platform` configuration
- CLI input parameters

## Template Variables Available in Execution

### Standard Variables

Available in most contexts:
- **`{{projectId}}`**: Project ID
- **`{{domain}}`**: Project domain
- **`{{sitePath}}`**: Site root path
- **`{{cliPath}}`**: CLI executable path

### Extension Runtime Variables

Available in `runtime.run_command`:
- **`{{extensionPath}}`**: Extension installation path
- **`{{entrypoint}}`**: Extension entrypoint file
- **`{{args}}`**: Command-line arguments

### Project Context Variables

Available when project is resolved:
- **`{{db_host}}`**: Database host
- **`{{db_port}}`**: Database port
- **`{{db_name}}`**: Database name
- **`{{db_user}}`**: Database user
- **`{{db_password}}`**: Database password (from keychain)

### Special Variables

Available in specific contexts:
- **`{{selected}}`**: Selected result rows (from `--data` flag)
- **`{{settings.<key>}}`**: Extension settings value
- **`{{payload.<key>}}`**: Action payload data
- **`{{release.<key>}}`**: Release configuration data

## CLI Command Resolution

When extension provides top-level CLI commands, execution context is resolved similarly to `homeboy extension run`.

### Extension Command Execution

```bash
homeboy wp <project_id> plugin list
```

Context resolution:
1. Extension is loaded (wordpress)
2. Project is resolved (`<project_id>`)
3. Component is resolved (if component specified or project has single component)
4. Settings are merged
5. Environment variables are set
6. Command is executed with template resolution

## Extension Execution vs Release Pipeline Execution

Both `homeboy extension run` and `extension.run` pipeline steps share the same execution context behavior:

- Same template variable resolution
- Same settings merge logic
- Same environment variable setting
- Same CLI output contract

This ensures consistent behavior regardless of how extensions are invoked.

## Example Contexts

### Simple Extension Execution

```bash
homeboy extension run rust --component mycomponent
```

Environment variables:
```bash
HOMEBOY_EXEC_CONTEXT_VERSION=1
HOMEBOY_MODULE_ID=rust
HOMEBOY_MODULE_PATH=/home/user/.config/homeboy/extensions/rust
HOMEBOY_COMPONENT_ID=mycomponent
HOMEBOY_COMPONENT_PATH=/home/user/dev/mycomponent
HOMEBOY_SETTINGS_JSON={}
```

### Full Context with Project

```bash
homeboy extension run wordpress --project mysite --component mytheme
```

Environment variables:
```bash
HOMEBOY_EXEC_CONTEXT_VERSION=1
HOMEBOY_MODULE_ID=wordpress
HOMEBOY_MODULE_PATH=/home/user/.config/homeboy/extensions/wordpress
HOMEBOY_PROJECT_ID=mysite
HOMEBOY_DOMAIN=mysite.com
HOMEBOY_SITE_PATH=/var/www/mysite
HOMEBOY_COMPONENT_ID=mytheme
HOMEBOY_COMPONENT_PATH=/home/user/dev/mytheme
HOMEBOY_SETTINGS_JSON={"php_version":"8.1"}
```

Template variables in `run_command`:
- `{{extensionPath}}` → `/home/user/.config/homeboy/extensions/wordpress`
- `{{entrypoint}}` → `main.py` (from manifest)
- `{{args}}` → CLI arguments passed to extension
- `{{projectId}}` → `mysite`
- `{{domain}}` → `mysite.com`
- `{{settings.php_version}}` → `8.1`

## Extension Environment Variables

Extensions can define additional environment variables in their manifest:

```json
{
  "runtime": {
    "run_command": "python3 {{entrypoint}} {{args}}",
    "env": {
      "PYTHON_PATH": "{{extensionPath}}/lib",
      "CACHE_DIR": "{{extensionPath}}/cache"
    }
  }
}
```

These are set alongside Homeboy's standard environment variables.

## Context Limits and Validation

### Validation Rules

- **Required context**: Some commands require project or component context
- **Ambiguity resolution**: Multiple components in project require explicit `--component`
- **Path validation**: Component paths must exist and be directories
- **Extension validation**: Extension must be installed or specified via path

### Error Conditions

- **Extension not found**: Extension ID not in extensions directory
- **Project not found**: Project ID not in projects directory
- **Component not found**: Component ID not in components directory
- **Context missing**: Command requires project/component but none provided

## Related

- [Extension command](../commands/extension.md) - Extension execution
- [Extension manifest schema](../schemas/extension-manifest-schema.md) - Runtime configuration
- [Template variables](../templates.md) - Template variable reference
