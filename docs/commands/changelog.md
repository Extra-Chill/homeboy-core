# `homeboy changelog`

## Synopsis

```sh
homeboy changelog <COMMAND>
```

## Subcommands

### `show`

```sh
homeboy changelog show
```

Shows the embedded Homeboy CLI changelog documentation (from `docs/changelog.md`).

### `add`

```sh
homeboy changelog add <component_id> <message> [--project-id <id>]
```

Adds a changelog item to the configured "next" section in the component's changelog file.

Required configuration:

- Component config must include `changelogTargets` (first target used)
- A "next section" label must be configured via:
  - `component.json`: `changelogNextSectionLabel` / `changelogNextSectionAliases`, or
  - `project.json`: `changelogNextSectionLabel` / `changelogNextSectionAliases`, or
  - `config.json`: `defaultChangelogNextSectionLabel` / `defaultChangelogNextSectionAliases`

Resolution order is most-specific first: `component.json`  `project.json`  `config.json`.

## JSON output

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). The object below is the `data` payload.

`homeboy changelog` returns a tagged union:

- `command`: `show` | `add`

### JSON output (show)

```json
{
  "command": "show",
  "topicLabel": "changelog",
  "content": "<markdown content>"
}
```

### JSON output (add)

```json
{
  "command": "add",
  "componentId": "<component_id>",
  "projectId": "<id>|null",
  "changelogPath": "</absolute/or/resolved/path.md>",
  "nextSectionLabel": "<label>",
  "message": "<message>",
  "changed": true
}
```

## Errors

- `show`: errors if embedded docs do not contain `changelog`
- `add`: errors if `changelogTargets` or the next-section label is not configured

## Related

- [Docs command](docs.md)
- [Changelog content](../changelog.md)
