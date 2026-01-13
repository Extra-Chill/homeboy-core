# Homeboy CLI

Homeboy is a config-driven automation engine for development and deployment automation.

It standardizes common operations (project/component discovery, remote execution, deployments) and produces machine-readable output via a stable JSON envelope for most commands.

**Note:** This is still early in development. Breaking changes may occur between releases.

## Installation

See the monorepo-level documentation for installation options:

- [Root README](../README.md)
- [Homebrew tap](../homebrew-tap/README.md)

This README stays focused on the CLI codebase layout and links to the canonical CLI reference docs under `docs/`.
## Commands

See:

- [Docs index](docs/index.md)
- [Commands index](docs/commands/commands-index.md)
- [JSON output contract](docs/json-output/json-output-contract.md)

## Agent instructions

Homeboy includes agent-oriented instruction files under `agent-instructions/`.

- `agent-instructions/skills/`: skills that can be added to LLM coding agents
- `agent-instructions/commands/`: reusable command recipes (slash-command style)

These files are not used by Homeboy at runtime; they are meant for humans and coding agents.

## Usage

See [CLI documentation](docs/index.md) for the authoritative command list, topic docs (`homeboy docs ...`), and JSON contracts.

A few common entrypoints:

```bash
homeboy list
homeboy docs --list
homeboy docs commands/project
homeboy project list
homeboy module list
homeboy pm2 <projectId> status
homeboy wp <projectId> plugin list
homeboy ssh <projectId>   # interactive passthrough
homeboy logs show <projectId> <path> --follow   # interactive passthrough
```

## Configuration

Configuration and state live under the OS config directory (from `dirs::config_dir()`), under a `homeboy/` folder:

- **macOS**: `~/Library/Application Support/homeboy/`
- **Linux**: `$XDG_CONFIG_HOME/homeboy/` (fallback: `~/.config/homeboy/`)
- **Windows**: `%APPDATA%\\homeboy\\`

Common paths:

- `projects/<id>.json`, `servers/<id>.json`, and `components/<id>.json` under the Homeboy config root
- `modules/<moduleId>/homeboy.json` (module manifest)

(There is no separate global `homeboy.json` config file in the current CLI implementation.)
- `projects/`
- `servers/`
- `components/`
- `modules/`
- `keys/`
- `backups/`

See [CLI documentation](docs/index.md) for details.

## SSH

Homeboy connects over SSH using server configuration stored under `servers/` inside the OS config directory.

Key management commands (generate/import/use/unset/show) are documented in [server](docs/commands/server.md).

## License

MIT
