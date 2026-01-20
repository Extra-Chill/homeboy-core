# `homeboy api`

## Synopsis

```sh
homeboy api <project_id> <COMMAND>
```

## Description

Make HTTP requests to a project’s configured API.

This command uses the project’s API configuration (`projects/<project_id>.json`) and any stored authentication (see `homeboy auth`).

## Subcommands

### `get`

```sh
homeboy api <project_id> get <endpoint>
```

### `post`

```sh
homeboy api <project_id> post <endpoint> [--body <json>]
```

### `put`

```sh
homeboy api <project_id> put <endpoint> [--body <json>]
```

### `patch`

```sh
homeboy api <project_id> patch <endpoint> [--body <json>]
```

### `delete`

```sh
homeboy api <project_id> delete <endpoint>
```

## Notes

- `<endpoint>` is passed through as provided (example: `/wp/v2/posts`).
- `--body` is parsed as JSON. If parsing fails, the request is sent with `body: null`.
- If `--body` is omitted, `body` is `null`.

## Output

JSON output is wrapped in the global envelope. `data` is the `homeboy::api::ApiOutput` struct.

## Related

- [auth](auth.md)
- [project](project.md)
- [JSON output contract](../architecture/output-system.md)
