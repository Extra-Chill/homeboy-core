# Homeboy CLI

LLM-first CLI for development and deployment automation. Designed for LLMs and developers using LLMs to iterate, debug, and ship fast.

Note: This should be considered experimental and is still early in development. Breaking changes may be frequent before 0.3.0

## Installation

### Homebrew
```bash
brew tap extra-chill/tap
brew install homeboy
```

This installs the **Homeboy CLI** (`homeboy`). It does not install the macOS desktop app.

### Cargo (requires Rust)
```bash
cargo install --path crates/homeboy
```

### Direct Download
Download from [GitHub Releases](https://github.com/Extra-Chill/homeboy-cli/releases).

## Commands

See:

- [Docs index](docs/index.md)
- [Commands index](docs/commands/commands-index.md)
- [JSON output contract](docs/json-output/json-output-contract.md)

## Agent instructions

Homeboy includes agent-oriented instruction files under `agent-instructions/`.

- `agent-instructions/skills/`: skills that can be added to LLM coding agents
- `agent-instructions/commands/`: reusable command recipes (slash-command style)

Recommended setup (optional): symlink the command recipes into your global agent commands directory so they are available everywhere:

```bash
ln -sf "/Users/chubes/Developer/Extra Chill Platform/homeboy/homeboy-cli/agent-instructions/commands/homeboy-init.md" \
  "/Users/chubes/.claude/commands/homeboy-init.md"

ln -sf "/Users/chubes/Developer/Extra Chill Platform/homeboy/homeboy-cli/agent-instructions/commands/version-bump.md" \
  "/Users/chubes/.claude/commands/version-bump.md"
```

These files are not used by Homeboy at runtime; they are meant for humans and coding agents.

## Usage

```bash
homeboy project list
homeboy project create "My Site" example.com wordpress --activate
homeboy project set <projectId> --domain example.com --server-id <serverId>
homeboy project repair <projectId>
homeboy project switch <projectId>
homeboy wp <projectId> core version
homeboy deploy <projectId> --dry-run --all
homeboy ssh <projectId>
homeboy docs [topic...]
```

## Configuration

Configuration and state live under the OS config directory (from `dirs::config_dir()`), under a `homeboy/` folder:

- **macOS**: `~/Library/Application Support/homeboy/`
- **Linux**: `$XDG_CONFIG_HOME/homeboy/` (fallback: `~/.config/homeboy/`)
- **Windows**: `%APPDATA%\\homeboy\\`

Common paths:

- `config.json`
- `projects/`
- `servers/`
- `components/`
- `modules/`
- `keys/`
- `backups/`
- `project-types/`

See [CLI documentation](docs/index.md) for details.

## SSH

Homeboy connects over SSH using server configuration stored under `homeboy/servers/`.

Key management commands (generate/import/use/unset/show) are documented in [server](docs/commands/server.md).

## License

MIT
