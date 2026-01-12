# `homeboy component`

Manage standalone component configurations.

## Synopsis

```sh
homeboy component [OPTIONS] <COMMAND>
```

Global option:

- `--dry-run`: show what would happen without writing

## Subcommands

- `create`: Create a new component configuration
  - Usage: `homeboy component create [OPTIONS] [NAME]`
  - Options: `--json <JSON>`, `--skip-existing`, `--local-path <LOCAL_PATH>`, `--remote-path <REMOTE_PATH>`, `--build-artifact <BUILD_ARTIFACT>`, `--version-target <TARGET>` (repeatable), `--build-command <BUILD_COMMAND>`
- `show`: Display component configuration
  - Usage: `homeboy component show [OPTIONS] <ID>`
- `set`: Update component configuration fields
  - Usage: `homeboy component set [OPTIONS] <ID>`
  - Options: `--name <NAME>`, `--local-path <LOCAL_PATH>`, `--remote-path <REMOTE_PATH>`, `--build-artifact <BUILD_ARTIFACT>`, `--version-target <TARGET>` (repeatable, replaces list), `--build-command <BUILD_COMMAND>`
- `delete`: Delete a component configuration
  - Usage: `homeboy component delete [OPTIONS] <ID>`
  - Options: `--force` (skip confirmation)
- `list`: List all available components
  - Usage: `homeboy component list [OPTIONS]`

## JSON output

All `homeboy component` subcommands return JSON wrapped in the global envelope described in the [JSON output contract](../json-output/json-output-contract.md).

> Note: `homeboy component` does not include an `import` subcommand.

## Related

- [build](build.md)
- [deploy](deploy.md)
- [project](project.md)
- [JSON output contract](../json-output/json-output-contract.md)
