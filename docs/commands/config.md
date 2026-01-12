# `homeboy config`

Manage the global Homeboy config file (`homeboy.json`).

- Path: `dirs::config_dir()/homeboy/homeboy.json` (see `homeboy config path`)
- This does not edit project/component config files.

## Commands

### `homeboy config path`

Print the resolved path to the global `homeboy.json`.

### `homeboy config show`

Show the current global config. If the file does not exist yet, Homeboy returns the default struct (mostly empty/`null` values).

### `homeboy config keys`

List the known keys that `homeboy config set`/`unset` supports.

### `homeboy config set <key> <value>`

Set a known key.

- String keys: pass the string directly
- String array keys: pass a comma-separated list (whitespace is trimmed)

Examples:

- `homeboy config set defaultChangelogNextSectionLabel Unreleased`
- `homeboy config set defaultChangelogNextSectionAliases "Unreleased,[Unreleased]"`

### `homeboy config unset <key>`

Unset (remove) a known key.

Example:

- `homeboy config unset defaultChangelogNextSectionAliases`

### `homeboy config set-json <pointer> <value> [--allow-unknown]`

Synopsis:

```sh
homeboy config set-json <pointer> <value> [--allow-unknown]
```

Note: `set-json` is an escape hatch; `homeboy config set` only supports known keys.

Escape hatch for setting a raw JSON value at a JSON pointer.


- If `<pointer>` is not in the known-key registry, you must pass `--allow-unknown`.
- `<value>` must be valid JSON (e.g. `"hello"`, `123`, `true`, `[]`, `{}`).

Examples:

- `homeboy config set-json /defaultChangelogNextSectionAliases '["Unreleased","Next"]'`
- `homeboy config set-json /someNewKey '{"a":1}' --allow-unknown`
