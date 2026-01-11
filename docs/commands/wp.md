# `homeboy wp`

## Synopsis

```sh
homeboy wp <project_id> [--local] <args...>
```

## Arguments and flags

- `project_id`: project ID
- `--local`: execute locally instead of on the remote server
- `<args...>`: WP-CLI command and arguments (trailing var args; hyphen values allowed)

## JSON output

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). The object below is the `data` payload.

```json
{
  "project_id": "<id>",
  "local": false,
  "args": ["plugin", "list"],
  "target_domain": "<domain>",
  "command": "<rendered command string>",
  "stdout": "<stdout>",
  "stderr": "<stderr>",
  "exit_code": 0
}
```

Notes:

- The command errors if no args are provided.
- For projects with subtargets, the first arg may be a subtarget identifier; `target_domain` reflects the resolved domain.

## Exit code

Exit code matches the executed WP-CLI command.

## Related

- [pm2](pm2.md)
- [db](db.md)
