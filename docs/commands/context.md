# `homeboy context`

## Synopsis

```sh
homeboy context
```

## Description

Prints a JSON payload describing the current working directory context:

- current directory (`cwd`)
- detected git root (if any)
- whether the directory matches any configured component local paths

## JSON output

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). The object below is `data`.

```json
{
  "command": "context",
  "cwd": "/absolute/path",
  "gitRoot": "/absolute/git/root",
  "managed": true,
  "matchedComponents": ["component_id"],
  "suggestion": "..."
}
```

Payload shape:

```json
{ "command": "context", "cwd": "...", "gitRoot": "...", "managed": true, "matchedComponents": [], "suggestion": null }
```

### Fields

- `command` (string): `context`
- `cwd` (string): current working directory
- `gitRoot` (string|null): `git rev-parse --show-toplevel` when available
- `managed` (bool): `true` when `matchedComponents` is non-empty
- `matchedComponents` (string[]): component IDs whose `localPath` matches `cwd` (exact match or ancestor)
- `suggestion` (string|null): guidance when `managed` is `false`

## Related

- [init](init.md)
- [component](component.md)
- [project](project.md)
