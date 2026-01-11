# `homeboy ssh`

## Synopsis

```sh
homeboy ssh <id> [command]
# or:
homeboy ssh --project <project_id> [command]
homeboy ssh --server <server_id> [command]
```

## Arguments and flags

- `id`: a project ID or server ID (the CLI resolves which one you mean)
- `--project <project_id>`: force project resolution
- `--server <server_id>`: force server resolution
- `command` (optional): if provided, executes a single command; otherwise opens an interactive SSH session.

## JSON output

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). The object below is the `data` payload.

```json
{
  "resolved_type": "project|server",
  "project_id": "<id>|null",
  "server_id": "<id>",
  "command": "<string>|null"
}
```

## Exit code

Exit code matches the underlying SSH session/command exit code.

## Related

- [server](server.md)
