# `homeboy docs`

## Synopsis

```sh
homeboy docs [TOPIC]...
homeboy docs list
```

`homeboy docs list` is the only built-in way to list available topics. There is no `--list` flag; `list` is a topic argument.

## Description

This command renders documentation topics from two sources:

1) Embedded core docs in the CLI binary
2) Installed module docs under `<config dir>/homeboy/modules/<moduleId>/docs/`

Topic arguments are treated as a free-form trailing list.

Note: the CLI strips a stray `--format <...>` pair from the trailing topic args before resolving the topic. `homeboy docs` does not define a `--format` option; this is defensive parsing to avoid global flags being interpreted as part of the topic.

Topic resolution is documented in: [Embedded docs topic resolution](../embedded-docs/embedded-docs-topic-resolution.md).

## Arguments

- `[TOPIC]...` (optional): documentation topic. This resolves to an embedded docs key (path under `docs/` without `.md`). Examples: `commands/deploy`, `commands/project`, `index`.

## Output

### Default (render topic)

`homeboy docs` prints the resolved markdown content to stdout.

### `list`

`homeboy docs list` prints the available topics as newline-delimited plain text (not JSON).

### JSON content mode

`homeboy docs` always runs in raw markdown mode. JSON output mode is not supported for this command.

## Errors

If the topic does not exist in embedded core docs or installed module docs, the command fails with a missing-key style error:

- `config_missing_key("docs.<topic>")`

## Related

- [Changelog command](changelog.md)
- [JSON output contract](../json-output/json-output-contract.md)
