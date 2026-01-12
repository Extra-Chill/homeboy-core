# `homeboy docs`

## Synopsis

```sh
homeboy docs [--list] [<topic>...]
```

## Description

By default, this command prints raw markdown to stdout.

Use `--list` to return a JSON list of available topics.

- Topic arguments are treated as a free-form trailing list.
- The resolved key must exist in embedded docs; otherwise the command errors.

Topic resolution is documented in: [Embedded docs topic resolution](../embedded-docs/embedded-docs-topic-resolution.md).

## Arguments

- `<topic>...` (optional): documentation topic. This must resolve to an embedded docs key (path under `docs/` without `.md`). Examples: `commands/deploy`, `commands/project`, `index`.

## Options

- `--list`: return available embedded keys and exit (JSON output).

## JSON output (success)

This section applies only when `--list` is not used.

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). The object below is the `data` payload.

```json
{
  "mode": "content",
  "topic": "<original topic as a single space-joined string>",
  "topicLabel": "<resolver topic label>",
  "resolvedKey": "<resolved embedded key (e.g. commands/deploy)>",
  "segments": ["<normalized>", "<segments>"],
  "slug": "<last segment>",
  "content": "<markdown content>",
  "availableTopics": ["<embedded key>", "<embedded key>"]
}
```

### Fields

- `mode`: response mode (`content` or `list`).
- `topic`: raw user input joined by spaces.
- `topicLabel`: label returned by the resolver.
- `resolvedKey`: resolved embedded key.
- `segments`: normalized key segments (lowercased; spaces/tabs become `-`).
- `slug`: last segment of `segments` (defaults to `index` when empty).
- `content`: embedded markdown content.
- `availableTopics`: list of available embedded keys (sorted).

## JSON output (list topics)

```json
{
  "mode": "list",
  "availableTopics": ["<embedded key>", "<embedded key>"]
}
```

## Errors

If resolved content is empty, the command returns an error message:

- `No documentation found for '<topic>' (available: <available_topics>)`

`<available_topics>` is a newline-separated list included in the error string.

## Related

- [Changelog command](changelog.md)
- [JSON output contract](../json-output/json-output-contract.md)
