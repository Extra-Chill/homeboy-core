# Homeboy CLI

LLM-first CLI for development and deployment automation. Designed for LLMs and developers using LLMs to iterate, debug, and ship fast.

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
homeboy project switch <project-id>
homeboy wp <project-id> core version
homeboy deploy <project-id> --dry-run --all
homeboy ssh <project-id>
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
- `keys/`

See [CLI documentation](docs/index.md) for details.

## SSH

Homeboy connects over SSH using server configuration stored under `Homeboy/servers/`.

Key management commands (generate/import/use/unset/show) are documented in [server](docs/commands/server.md).

## License

MIT
