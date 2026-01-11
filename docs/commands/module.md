# `homeboy module`

## Synopsis

```sh
homeboy module <COMMAND>
```

## Subcommands

### `list`

```sh
homeboy module list [--project <project_id>]
```

### `run`

```sh
homeboy module run <module_id> [--project <project_id>] [--input <key=value>...] [<args...>]
```

- `--input` repeats; each value must be in `KEY=value` form.
- Trailing `<args...>` are passed to CLI-type modules.

### `setup`

```sh
homeboy module setup <module_id>
```

### `install`

```sh
homeboy module install <git_url> [--id <module_id>]
```

Installs a module by cloning it into `Homeboy/modules/<module_id>/` and writing `.install.json` so it can be updated later.

### `update`

```sh
homeboy module update <module_id> [--force]
```

Updates a module by running `git pull --ff-only` in the module directory. If the module has uncommitted changes, `--force` is required.

### `uninstall`

```sh
homeboy module uninstall <module_id> [--force]
```

Uninstalls a module by deleting its directory. `--force` is required (no interactive prompts).

## JSON output

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). `homeboy module` returns a `ModuleOutput` object as the `data` payload.

`ModuleOutput`:

- `command`: `module.list` | `module.run` | `module.setup` | `module.install` | `module.update` | `module.uninstall`
- `projectId` (only used for `module.list` filter)
- `moduleId`
- `modules`: list output for `module.list`
- `runtimeType`: `python` | `shell` | `cli` (for `run` and `setup`)
- `installed`: `{ url, path }` for `module.install`
- `updated`: `{ url, path }` for `module.update`
- `uninstalled`: `{ path }` for `module.uninstall`

Module entry (`modules[]`):

- `id`, `name`, `version`, `description`
- `runtime` (runtime type as lowercase string)
- `compatible` (with optional `--project`)
- `ready` (module readiness)

## Exit code

- `module.run`: exit code of the executed module.
- `module.setup`: `0` on success; when the module runtime is not python, it returns `0` without setup.

## Related

- [project](project.md)
