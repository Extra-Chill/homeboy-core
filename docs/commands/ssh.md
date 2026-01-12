# `homeboy ssh`

## Synopsis

```sh
# Non-interactive discovery (JSON output):
homeboy ssh list

# Connect (interactive when COMMAND is omitted):
homeboy ssh [OPTIONS] [ID] [COMMAND]
```

## Subcommands

### `list`

Lists configured SSH server targets. This is safe for CI/headless usage.

```sh
homeboy ssh list
```

## Arguments and flags

- `[ID]`: project ID or server ID (project wins when both exist). Optional when using `--project` or `--server`.
- `--project <PROJECT>`: force project resolution
- `--server <SERVER>`: force server resolution
- `[COMMAND]` (optional): command to execute (omit for interactive shell)

Note: clap shows `Usage: homeboy ssh [OPTIONS] [ID] [COMMAND] [COMMAND]`, but the helper text describes a single `[COMMAND]` argument.

## JSON output

### `ssh list`

> Note: output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). The object below is `data.payload`.

```json
{
  "action": "list",
  "servers": [
    {
      "id": "...",
      "name": "...",
      "host": "...",
      "user": "...",
      "port": 22,
      "identityFile": null
    }
  ]
}
```

### Connect (`homeboy ssh <id> [command]`)

The connect action uses an interactive SSH session and does not print the JSON envelope (it is treated as passthrough output).

When `command` is provided, it is passed to the remote shell via the interactive session.

## Exit code

Exit code matches the underlying SSH session/command exit code.

## Related

- [server](server.md)
