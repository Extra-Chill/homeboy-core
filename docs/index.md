# Homeboy CLI documentation

Homeboy is LLM-first, human-second (for LLMs and developers using LLMs). Humans can use the CLI and a desktop app exists as a convenient GUI, but the CLI is the source of truth and may evolve faster than the app.

## Configuration

Configuration and state live under the Homeboy data directory (`dirs::data_dir()/Homeboy/`):

- macOS: `~/Library/Application Support/Homeboy/`
- Linux: `~/.local/share/Homeboy/` (exact path varies by distribution)

Common directories:

- `config.json`
- `projects/`
- `servers/`
- `components/`
- `modules/`
- `module-sources/`
- `keys/`
- `backups/`
- `project-types/`

Additional (not created by default):

- `playwright-browsers/` (used via `PLAYWRIGHT_BROWSERS_PATH` for Python modules)
- `docs/` (reserved for cached/generated documentation)

## Documentation

- Commands: [Commands index](commands/commands-index.md)
- JSON output (global output envelope + NDJSON events): [JSON output contract](json-output/json-output-contract.md)
- Embedded docs behavior: [Embedded docs topic resolution](embedded-docs/embedded-docs-topic-resolution.md)
- Changelog: [Changelog](changelog.md)
- Commands: [Commands index](commands/commands-index.md)
- JSON output (global output envelope): [JSON output contract](json-output/json-output-contract.md)
- Embedded docs behavior: [Embedded docs topic resolution](embedded-docs/embedded-docs-topic-resolution.md)
- Changelog: [Changelog](changelog.md)
