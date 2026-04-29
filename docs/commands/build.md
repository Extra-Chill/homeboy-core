# `homeboy build`

## Synopsis

```sh
homeboy build <component_id>
homeboy build <component_id> --path /path/to/workspace/clone
homeboy build --json '<spec>'
homeboy build <project_id> --all
```

## Description

Resolves a build command from the component's linked extension and runs it in the component's `local_path`.

Builds are extension-owned. A component links to one build-capable extension, and Homeboy asks that extension for the command to run. One-off shell commands that do not belong in a reusable extension should live in a rig `command` step instead.

## Path Override

Use `--path` to run the build against a different directory than the configured `local_path`:

```sh
homeboy build data-machine --path /var/lib/datamachine/workspace/data-machine
```

This is useful for:
- **AI agent workflows** — agents working in workspace clones
- **CI/CD** — running builds on a fresh checkout
- **Multi-branch development** — testing different branches without swapping the installed plugin

The override is transient — it does not modify the stored component config.

Requires exactly one linked extension with build support. Component-level `build_command` is not supported as configuration; the `build_command` field in command output is the command Homeboy resolved from the linked extension for this run.

Useful remediation paths when a component is not buildable:

- Link a build-capable extension: `homeboy component set <id> --extension <extension_id>`
- Inspect installed extensions: `homeboy extension list`
- Use a rig `command` step for local or private build workflows that do not belong in an extension.

## Pre-Build Validation

If a component's extension defines a `pre_build_script` in its build configuration, that script runs before the build. If the pre-build script exits with a non-zero code, the build fails.

For WordPress components, this runs PHP syntax validation to catch errors before building.

Example extension configuration:
```json
{
  "build": {
    "script_names": ["build.sh"],
    "extension_script": "scripts/build.sh",
    "pre_build_script": "scripts/validate-build.sh"
  }
}
```

## JSON output

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../architecture/output-system.md). The object below is the `data` payload.

### Single

```json
{
  "command": "build.run",
  "component_id": "<component_id>",
  "build_command": "<extension-resolved command string>",
  "stdout": "<stdout>",
  "stderr": "<stderr>",
  "success": true
}
```

`stdout` and `stderr` are omitted when empty. `build_command` is diagnostic output from extension resolution, not a component-level config field.

### Bulk (`--json`)

```json
{
  "action": "build",
  "results": [
    {
      "id": "<component_id>",
      "command": "build.run",
      "component_id": "<component_id>",
      "build_command": "<extension-resolved command string>",
      "stdout": "<stdout>",
      "stderr": "<stderr>",
      "success": true
    }
  ],
  "summary": { "total": 1, "succeeded": 1, "failed": 0 }
}
```

Bulk JSON input accepts either a JSON array or an object with `component_ids`:

```json
{ "component_ids": ["component-a", "component-b"] }
```

## Exit code

- Single mode: exit code matches the underlying build process exit code.
- Bulk mode (`--json`): `0` if all builds succeed; `1` if any build fails.

## Related

- [component](component.md)
- [deploy](deploy.md)
- [lint](lint.md)
- [test](test.md)
