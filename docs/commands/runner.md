# `homeboy runner`

## Synopsis

```sh
homeboy runner <COMMAND>
```

`runner` manages durable execution backends. It records where future Homeboy workflows can run, but it does not execute remote commands.

## Subcommands

### `add`

```sh
homeboy runner add <id> --workspace-root <path>
homeboy runner add <id> --server <server-id> --workspace-root <path>
homeboy runner add --json <spec>
```

Options:

- `--kind local|ssh`: explicit runner kind. Defaults to `ssh` when `--server` is set, otherwise `local`.
- `--server <server-id>`: existing `homeboy server` record for SSH runners.
- `--workspace-root <path>`: workspace root on the runner machine.
- `--homeboy-path <path>`: Homeboy binary path on the runner machine.
- `--daemon`: marks the runner as daemon-preferred for future commands.
- `--concurrency-limit <n>`: maximum concurrent workflows this runner should accept.
- `--artifact-policy <label>`: artifact policy label reserved for future execution commands.

### `list`

```sh
homeboy runner list
```

### `show`

```sh
homeboy runner show <id>
```

### `set`

```sh
homeboy runner set <id> --json <JSON>
homeboy runner set <id> workspace_root=/srv/homeboy
homeboy runner set <id> -- --concurrency_limit 4
```

Updates a runner by merging a JSON object into `runners/<id>.json`.

### `remove`

```sh
homeboy runner remove <id>
```

## Runner Shape

Runner records are stored as JSON config entities under `~/.config/homeboy/runners/`.

```json
{
  "id": "lab-local",
  "kind": "local",
  "server_id": null,
  "workspace_root": "/Users/chubes/Developer",
  "homeboy_path": "/usr/local/bin/homeboy",
  "daemon": false,
  "concurrency_limit": 2,
  "artifact_policy": "copy",
  "env": {},
  "resources": {}
}
```

Rules:

- `kind` is `local` or `ssh`.
- `ssh` runners require `server_id` to reference an existing `homeboy server` record.
- `concurrency_limit`, when set, must be greater than zero.
- `env` and `resources` are metadata maps for future `connect`, `doctor`, `exec`, and Desktop workflows.

## JSON Output

All command output is wrapped in the global JSON envelope described in the [JSON output contract](../architecture/output-system.md). The `data` payload uses the generic entity CRUD shape:

- `command`: action identifier such as `runner.add`, `runner.list`, `runner.show`, `runner.set`, or `runner.remove`
- `id`: present for single-runner actions
- `entity`: runner configuration for single-runner read/write actions
- `entities`: list for `list`
- `updated_fields`: list of updated field names for writes
- `deleted`: list of removed runner IDs
