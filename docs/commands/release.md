# `homeboy release`

## Synopsis

```sh
homeboy release <COMMAND>
```

## Description

`homeboy release` plans release workflows based on the component-scoped `release` configuration.

## Subcommands

### `plan`

```sh
homeboy release plan <component_id> [--module <module_id>]
```

Generates an ordered release plan without executing any steps.

Notes:

- Release config is read from the component (`components/<id>.json`).
- If no release config exists for the component, the command errors and suggests adding one via `homeboy component set`.
- `--module` is optional and only used for module actions.

## JSON output

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). The object below is the `data` payload.

```json
{
  "command": "release.plan",
  "plan": {
    "component_id": "<component_id>",
    "enabled": true,
    "sources": {
      "module": true,
      "project": false,
      "component": true
    },
    "steps": [
      {
        "id": "build",
        "type": "build",
        "label": "Build",
        "needs": [],
        "config": {},
        "status": "ready",
        "missing": []
      }
    ],
    "warnings": [],
    "hints": []
  }
}
```

## Related

- [component](component.md)
- [module](module.md)
