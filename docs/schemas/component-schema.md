# Component Schema

Component configuration defines buildable and deployable units stored in `components/<id>.json`.

## Schema

```json
{
  "id": "string",
  "name": "string",
  "local_path": "string",
  "remote_path": "string",
  "build_artifact": "string",
  "build_command": "string",
  "extract_command": "string",
  "version_targets": [
    {
      "file": "string",
      "pattern": "string"
    }
  ],
  "changelog_target": "string",
  "modules": {},
  "release": {}
}
```

## Fields

### Required Fields

- **`id`** (string): Unique component identifier, derived from `local_path` directory name (lowercased)
- **`local_path`** (string): Absolute path to local source directory, `~` is expanded
- **`remote_path`** (string): Remote path relative to project `base_path`
- **`build_artifact`** (string): Build artifact path relative to `local_path`, must include filename
- **`build_command`** (string): Shell command to execute in `local_path` during builds

### Optional Fields

- **`name`** (string): Human-readable component name, defaults to `id`
- **`extract_command`** (string): Command to execute after artifact upload, runs inside target directory
  - Supports template variables: `{artifact}`, `{targetDir}`
- **`version_targets`** (array): List of version detection patterns
  - **`file`** (string): Path to file containing version (relative to `local_path`)
  - **`pattern`** (string): Regex pattern to extract version (first capture group)
- **`changelog_target`** (string): Path to changelog file (relative to `local_path`)
- **`modules`** (object): Module-specific settings
  - Keys are module IDs (e.g., `"wordpress"`, `"rust"`)
  - Values are module setting objects
- **`release`** (object): Component-scoped release configuration
  - **`enabled`** (boolean): Whether release pipeline is enabled
  - **`steps`** (array): Release step definitions
  - **`settings`** (object): Release pipeline settings

## Example

```json
{
  "id": "extrachill-api",
  "name": "Extra Chill API",
  "local_path": "/Users/dev/extrachill-api",
  "remote_path": "wp-content/plugins/extrachill-api",
  "build_artifact": "build/extrachill-api.zip",
  "build_command": "npm run build",
  "extract_command": "unzip -o {{artifact}} && rm {{artifact}}",
  "version_targets": [
    {
      "file": "composer.json",
      "pattern": "\"version\":\\s*\"([^\"]+)\""
    }
  ],
  "changelog_target": "CHANGELOG.md",
  "modules": {
    "wordpress": {
      "settings": {
        "php_version": "8.1"
      }
    }
  },
  "release": {
    "enabled": true,
    "steps": [
      {
        "id": "test",
        "type": "module.run",
        "label": "Run Tests",
        "config": {
          "module": "rust"
        }
      }
    ]
  }
}
```

## Version Target Format

Version targets use regex to extract semantic versions from files. The pattern must include a capture group for the version string.

Common patterns:
- Composer: `\"version\":\\s*\"([^\"]+)\"`
- Cargo: `^version\\s*=\\s*\"([^\"]+)\"`
- Package.json: `\"version\":\\s*\"([^\"]+)\"`
- WordPress plugin header: `Version:\\s*([\\d.]+)`

## Extract Command Context

The `extract_command` runs inside the target directory after artifact upload. The working directory is:
- `project.base_path + component.remote_path`

Available template variables:
- **`{artifact}`** - The uploaded artifact filename only
- **`{targetDir}`** - Full target directory path

## Storage Location

Components are stored as individual JSON files under the OS config directory:
- **macOS/Linux**: `~/.config/homeboy/components/<id>.json`
- **Windows**: `%APPDATA%\homeboy\components\<id>.json`

## Related

- [Component command](../commands/component.md) - Manage component configuration
- [Project schema](project-schema.md) - How components link to projects
- [Module manifest schema](module-manifest-schema.md) - Module configuration structure
