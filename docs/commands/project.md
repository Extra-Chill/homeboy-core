# `homeboy project`

## Synopsis

```sh
homeboy project <COMMAND>
```

## Subcommands

### `list`

```sh
homeboy project list [--current]
```

- `--current`: return only the active project ID.

JSON output (`list`):

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). The object below is the `data` payload.

```json
{
  "command": "project.list",
  "activeProjectId": "<id>|null",
  "projects": [
    {
      "id": "<id>",
      "name": "<name>",
      "domain": "<domain>",
      "projectType": "<type>",
      "active": true
    }
  ]
}
```

JSON output (`--current`):

```json
{
  "command": "project.current",
  "activeProjectId": "<id>|null",
  "projects": null
}
```

### `show`

```sh
homeboy project show [project_id]
```

- `project_id` (optional): if omitted, uses the active project.

JSON output:

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). The object below is the `data` payload.

```json
{
  "command": "project.show",
  "projectId": "<id>",
  "project": { }
}
```

`project` is the serialized `ProjectConfiguration`.

### `switch`

```sh
homeboy project switch <project_id>
```

JSON output:

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). The object below is the `data` payload.

```json
{
  "command": "project.switch",
  "projectId": "<id>",
  "project": { }
}
```

## Related

- [JSON output contract](../json-output/json-output-contract.md)
