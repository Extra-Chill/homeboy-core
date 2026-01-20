# Execution Context

Execution context provides runtime information to modules during execution via environment variables and template variable resolution.

## Overview

When Homeboy executes modules (via `homeboy module run` or release pipeline steps), it builds an execution context containing:

- Module metadata
- Project and component information (when available)
- Resolved settings
- Template variables
- Environment variables

## Environment Variables

Homeboy sets the following environment variables before executing modules:

### Base Context Variables

- **`HOMEBOY_EXEC_CONTEXT_VERSION`**: Execution context protocol version (currently `"1"`)

### Module Variables

- **`HOMEBOY_MODULE_ID`**: Module identifier
- **`HOMEBOY_MODULE_PATH`**: Absolute path to module directory

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

### 1. Module Resolution

Module is loaded from:
- Installed modules directory (`~/.config/homeboy/modules/<module_id>/`)
- Or directly referenced via path (for development)

Module metadata is extracted from `<module_id>.json` manifest.

### 2. Project Resolution (optional)

If `--project` is specified:
- Project configuration is loaded (`projects/<project_id>.json`)
- Server configuration is loaded (via `server_id`)
- Database and API configuration is resolved

### 3. Component Resolution (optional)

If `--component` is specified:
- Component configuration is loaded (`components/<component_id>.json`)
- Component path is validated
- Component module associations are identified

If `--component` is omitted:
- Homeboy attempts to resolve component from project's `component_ids`
- First matching component is used
- Ambiguity is resolved via user prompt or explicit specification

### 4. Settings Merge

Module settings are merged from multiple scopes in order (later scopes override earlier ones):

1. **Project settings**: `projects/<project_id>.json` -> `modules.<module_id>.settings`
2. **Component settings**: `components/<component_id>.json` -> `modules.<module_id>.settings`

Merged settings are available as:
- Environment variable: `HOMEBOY_SETTINGS_JSON`
- Template variable: `{{settings.<key>}}`

### 5. Template Variable Resolution

Template variables are resolved from:
- Execution context variables
- Module manifest `runtime.env` definitions
- Module `platform` configuration
- CLI input parameters

## Template Variables Available in Execution

### Standard Variables

Available in most contexts:
- **`{{projectId}}`**: Project ID
- **`{{domain}}`**: Project domain
- **`{{sitePath}}`**: Site root path
- **`{{cliPath}}`**: CLI executable path

### Module Runtime Variables

Available in `runtime.run_command`:
- **`{{modulePath}}`**: Module installation path
- **`{{entrypoint}}`**: Module entrypoint file
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
- **`{{settings.<key>}}`**: Module settings value
- **`{{payload.<key>}}`**: Action payload data
- **`{{release.<key>}}`**: Release configuration data

## CLI Command Resolution

When module provides top-level CLI commands, execution context is resolved similarly to `homeboy module run`.

### Module Command Execution

```bash
homeboy wp <project_id> plugin list
```

Context resolution:
1. Module is loaded (wordpress)
2. Project is resolved (`<project_id>`)
3. Component is resolved (if component specified or project has single component)
4. Settings are merged
5. Environment variables are set
6. Command is executed with template resolution

## Module Execution vs Release Pipeline Execution

Both `homeboy module run` and `module.run` pipeline steps share the same execution context behavior:

- Same template variable resolution
- Same settings merge logic
- Same environment variable setting
- Same CLI output contract

This ensures consistent behavior regardless of how modules are invoked.

## Example Contexts

### Simple Module Execution

```bash
homeboy module run rust --component mycomponent
```

Environment variables:
```bash
HOMEBOY_EXEC_CONTEXT_VERSION=1
HOMEBOY_MODULE_ID=rust
HOMEBOY_MODULE_PATH=/home/user/.config/homeboy/modules/rust
HOMEBOY_COMPONENT_ID=mycomponent
HOMEBOY_COMPONENT_PATH=/home/user/dev/mycomponent
HOMEBOY_SETTINGS_JSON={}
```

### Full Context with Project

```bash
homeboy module run wordpress --project mysite --component mytheme
```

Environment variables:
```bash
HOMEBOY_EXEC_CONTEXT_VERSION=1
HOMEBOY_MODULE_ID=wordpress
HOMEBOY_MODULE_PATH=/home/user/.config/homeboy/modules/wordpress
HOMEBOY_PROJECT_ID=mysite
HOMEBOY_DOMAIN=mysite.com
HOMEBOY_SITE_PATH=/var/www/mysite
HOMEBOY_COMPONENT_ID=mytheme
HOMEBOY_COMPONENT_PATH=/home/user/dev/mytheme
HOMEBOY_SETTINGS_JSON={"php_version":"8.1"}
```

Template variables in `run_command`:
- `{{modulePath}}` → `/home/user/.config/homeboy/modules/wordpress`
- `{{entrypoint}}` → `main.py` (from manifest)
- `{{args}}` → CLI arguments passed to module
- `{{projectId}}` → `mysite`
- `{{domain}}` → `mysite.com`
- `{{settings.php_version}}` → `8.1`

## Module Environment Variables

Modules can define additional environment variables in their manifest:

```json
{
  "runtime": {
    "run_command": "python3 {{entrypoint}} {{args}}",
    "env": {
      "PYTHON_PATH": "{{modulePath}}/lib",
      "CACHE_DIR": "{{modulePath}}/cache"
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
- **Module validation**: Module must be installed or specified via path

### Error Conditions

- **Module not found**: Module ID not in modules directory
- **Project not found**: Project ID not in projects directory
- **Component not found**: Component ID not in components directory
- **Context missing**: Command requires project/component but none provided

## Related

- [Module command](../commands/module.md) - Module execution
- [Module manifest schema](../schemas/module-manifest-schema.md) - Runtime configuration
- [Template variables](../templates.md) - Template variable reference
