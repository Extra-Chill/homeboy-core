# `homeboy` root command

## Synopsis

```sh
homeboy [--json <spec>] <COMMAND>
```

## Description

`homeboy` is a CLI tool for development and deployment automation.

## Global flags

These are provided by clap:

- `--version` / `-V`: print version and exit
- `--help` / `-h`: print help and exit

Homeboy also defines:

- `--json <spec>`: JSON input spec override for a command.
  - Use `-` to read from stdin, `@file.json` to read from a file, or provide an inline JSON string.
  - `--json` is a global flag and should come before the subcommand (e.g. `homeboy --json @payload.json changelog add`).

## Subcommands

See the full list of supported subcommands in the [Commands index](../commands/commands-index.md).
