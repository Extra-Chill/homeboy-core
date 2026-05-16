# `homeboy runner`

## Synopsis

```sh
homeboy runner <COMMAND>
```

`runner` manages durable execution backends. It records where Homeboy workflows can run and can execute explicit commands through a connected runner daemon or an opt-in SSH diagnostic path.

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

### `exec`

```sh
homeboy runner exec <runner-id> -- <command...>
homeboy runner exec <runner-id> --cwd /runner/workspace/project -- <command...>
homeboy runner exec <runner-id> --ssh --cwd /runner/workspace/project -- <command...>
```

`exec` submits the command to the connected runner daemon when `homeboy runner connect <runner-id>` has established a live loopback tunnel. If no daemon session is connected, local runners execute directly and SSH runners require explicit `--ssh`.

Path rules:

- SSH runners require `workspace_root` so local paths are not silently reused remotely.
- SSH `--cwd` must be an absolute path under the configured `workspace_root`.
- Omitting `--cwd` on an SSH runner uses the runner `workspace_root`.
- `--ssh` is an MVP/diagnostic fallback; daemon execution is preferred because it records job metadata and supports artifact-oriented workflows.

### `workspace sync`

```sh
homeboy runner workspace sync <runner-id> --path <local-worktree>
homeboy runner workspace sync <runner-id> --path <local-worktree> --mode snapshot
homeboy runner workspace sync <runner-id> --path <local-worktree> --mode git
```

`workspace sync` materializes a laptop worktree under the runner's configured `workspace_root` so Lab execution can run against an explicit remote path while Git operations and canonical edits stay local.

Modes:

- `snapshot` copies the current local tree, including dirty edits, through a tar stream.
- `git` requires a clean local tree, then clones or refreshes `remote.origin.url` on the runner and checks out local `HEAD` detached.

Safety rules:

- The remote path is deterministic and lives under `<workspace_root>/_lab_workspaces/`.
- Snapshot sync excludes dependency directories, build outputs, caches, `.git`, and common secret file patterns such as `.env*`, `*.pem`, and `*.key`.
- Output includes `local_path`, `remote_path`, `sync_mode`, `snapshot_identity`, and snapshot `files` / `bytes` when available.
- The runner workspace is execution-only; this command does not push branches, commit, or make the runner authoritative for source changes.

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

- `command`: action identifier such as `runner.add`, `runner.list`, `runner.show`, `runner.set`, `runner.remove`, `runner.exec`, or `runner.workspace.sync`
- `id`: present for single-runner actions
- `entity`: runner configuration for single-runner read/write actions
- `entities`: list for `list`
- `updated_fields`: list of updated field names for writes
- `deleted`: list of removed runner IDs
