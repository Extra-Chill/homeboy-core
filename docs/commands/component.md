# `homeboy component`

## Synopsis

```sh
homeboy component <COMMAND>
```

## Subcommands

- `create <name> --local-path <path> --remote-path <path> --build-artifact <path> [--version-target <file|file::pattern>]... [--build-command <cmd>] [--is-network]`
- `import <json> [--skip-existing]` (legacy bulk import; newer configs use `create --json`)
- `show <componentId>`
- `set <componentId> [--name <name>] [--local-path <path>] [--remote-path <path>] [--build-artifact <path>] [--version-target <file|file::pattern>]... [--build-command <cmd>] [--is-network] [--not-network]`
- `delete <componentId> --force`
- `list`

## JSON output

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). `homeboy component` returns a `ComponentOutput` object as the `data` payload.

`ComponentOutput`:

- `action`: `create` | `import` | `show` | `set` | `delete` | `list`
- `componentId` (present for single-component operations)
- `component`: present for `create`, `show`, `set`
- `components`: present for `list`
- `updatedFields`: list of field names updated by `set`
- `created`, `skipped`, `errors`: status lists (for `import`)
- `success` (boolean): overall status flag

## Exit code

- `import` returns exit code `1` if any errors occur while importing.
- Other subcommands return `0` on success.

## Related

- [deploy](deploy.md)
- [build](build.md)
- [version](version.md)
