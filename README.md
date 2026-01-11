# Homeboy CLI

LLM-first CLI for development and deployment automation. Designed for LLMs and developers using LLMs to iterate, debug, and ship fast.

**Note:** This should be considered experimental and is still early in development. Breaking changes may be frequent before 0.3.0

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

Configuration and state live in the Homeboy data directory (from `dirs::data_dir()`), under a `Homeboy/` folder:

- **macOS**: `~/Library/Application Support/Homeboy/`
- **Linux**: `~/.local/share/Homeboy/` (exact path varies by distribution)

Common paths:

- `config.json`
- `projects/`
- `servers/`
- `components/`
- `modules/`
- `module-sources/`
- `keys/`

See [CLI documentation](docs/index.md) for details.

## SSH

Homeboy connects over SSH using server configuration stored under `Homeboy/servers/`.

Key management commands (generate/import/use/unset/show) are documented in [server](docs/commands/server.md).

## License

MIT
