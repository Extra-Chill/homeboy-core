# `homeboy docs`

## Synopsis

```sh
homeboy docs [OPTIONS] [TOPIC]...
```

## Description

This command renders documentation topics from two sources:

1) Embedded core docs in the CLI binary
2) Installed module docs under `<config dir>/homeboy/modules/<moduleId>/docs/`

Topic arguments are treated as a free-form trailing list.

Note: the CLI strips a stray `--format <...>` pair from the trailing topic args before resolving the topic. `homeboy docs` does not define a `--format` option; this is defensive parsing to avoid global flags being interpreted as part of the topic.

Topic resolution is documented in: [Embedded docs topic resolution](../embedded-docs/embedded-docs-topic-resolution.md).

## Arguments

- `[TOPIC]...` (optional): documentation topic. This resolves to an embedded docs key (path under `docs/` without `.md`). Examples: `commands/deploy`, `commands/project`, `index`.

## Options

- `--list`: list available topics and exit

## Output

### Default (render topic)

`homeboy docs` prints the resolved markdown content to stdout.

### `--list`

When `--list` is used, output is JSON.

> Note: all JSON output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). The object below is the top-level `data` value.

```json
{
  "mode": "list",
  "available_topics": ["index", "commands/deploy"]
}
```

### JSON content mode

When rendering a topic in JSON mode (for example: `homeboy docs commands/deploy`), the `data` payload includes resolved metadata and `content`.

Example (formatted as plain text since embedded docs are Rust raw strings):

    {
      "mode": "content",
      "topic": "commands/deploy",
      "topic_label": "commands/deploy",
      "resolved_key": "commands/deploy",
      "segments": ["commands", "deploy"],
      "slug": "deploy",
      "content": "(markdown omitted)",
      "source": "core",
      "available_topics": ["index", "commands/deploy"]
    }

Note: embedded docs are compiled into Rust raw strings, so a specific quote-plus-hash byte sequence cannot appear in any embedded doc.

## Errors

If the topic does not exist in embedded core docs or installed module docs, the command fails with a missing-key style error:

- `config_missing_key("docs.<topic>")`

## Related

- [Changelog command](changelog.md)
- [JSON output contract](../json-output/json-output-contract.md)
