# `homeboy plugin`

Inspect plugins installed with Homeboy.

Plugins provide optional integrations (for example: adding extra CLI passthrough commands like `wp` or `pm2`, default pinned files/logs, and plugin-specific behaviors).

## Synopsis

```sh
homeboy plugin <COMMAND>
```

## Subcommands

### `list`

List all available plugins.

```sh
homeboy plugin list
```

### `show`

Show details for a single plugin by ID.

```sh
homeboy plugin show <pluginId>
```

## JSON output

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). The object below is the `data.payload` object.

### `list` payload

```json
{
  "command": "plugin.list",
  "plugins": [
    {
      "id": "wordpress",
      "name": "WordPress",
      "version": "0.1.0",
      "description": "...",
      "hasCli": true,
      "commands": ["wp"]
    }
  ]
}
```

### `show` payload

```json
{
  "command": "plugin.show",
  "pluginId": "wordpress",
  "plugin": {
    "id": "wordpress",
    "name": "WordPress",
    "version": "0.1.0",
    "description": "...",
    "author": "...",
    "icon": "...",
    "hasCli": true,
    "commands": ["wp"],
    "cliTool": "wp",
    "defaultPinnedFiles": [],
    "defaultPinnedLogs": [],
    "pluginPath": "..."
  }
}
```

Notes:

- `description`, `author`, `cliTool`, and `pluginPath` may be omitted.
- `plugins` is omitted on `show`.

## Related

- [commands index](commands-index.md)
- [wp](wp.md) (plugin-provided CLI tool example)
- [pm2](pm2.md) (plugin-provided CLI tool example)
