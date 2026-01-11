# Homeboy CLI documentation

Homeboy is LLM-first, human-second (for LLMs and developers using LLMs). Humans can use the CLI and a desktop app exists as a convenient GUI, but the CLI is the source of truth and may evolve faster than the app.

## Configuration

Configuration and state live under the OS config directory (`dirs::config_dir()/homeboy/`):

- macOS: `~/Library/Application Support/homeboy/`
- Linux: `$XDG_CONFIG_HOME/homeboy/` (fallback: `~/.config/homeboy/`)
- Windows: `%APPDATA%\\homeboy\\`

Common directories:

- `config.json`
- `projects/`
- `servers/`
- `components/`
- `modules/`
- `keys/`
- `backups/`
- `project-types/`

Additional (not created by default):

- `playwright-browsers/` (used when running Python Playwright modules; see `PLAYWRIGHT_BROWSERS_PATH`)
- `docs/` (reserved for cached/generated documentation)

## Documentation

- Commands: [Commands index](commands/commands-index.md)
- JSON output contract: [JSON output contract](json-output/json-output-contract.md)
- Embedded docs behavior: [Embedded docs topic resolution](embedded-docs/embedded-docs-topic-resolution.md)
- Changelog: [Changelog](changelog.md)
