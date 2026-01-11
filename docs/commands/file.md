# `homeboy file`

## Synopsis

```sh
homeboy file <COMMAND>
```

## Subcommands

- `list <project_id> <path>`
- `read <project_id> <path>`
- `write <project_id> <path>` (reads content from stdin)
- `delete <project_id> <path> [--recursive]`
- `rename <project_id> <old_path> <new_path>`

## JSON output

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). `homeboy file` returns a `FileOutput` object as the `data` payload.

Fields:

- `command`: `file.list` | `file.read` | `file.write` | `file.delete` | `file.rename`
- `projectId`
- `basePath`: project base path if configured
- `path` / `oldPath` / `newPath`: resolved full remote paths
- `recursive`: present for delete
- `entries`: for `list` (parsed from `ls -la`)
- `content`: for `read`
- `bytesWritten`: for `write`
- `exitCode`, `success`

List entries (`entries[]`):

- `name`
- `path`
- `isDirectory`
- `size`
- `permissions` (permission bits excluding the leading file type)

## Exit code

This command returns `0` on success; failures are returned as errors.

## Related

- [logs](logs.md)
- [project](project.md)
