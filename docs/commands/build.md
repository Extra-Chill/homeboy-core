# `homeboy build`

## Synopsis

```sh
homeboy build <componentId>
```

## Description

Runs a build command for the component (either the component’s configured `build_command`, or an auto-detected default) in the component’s `local_path`.

## JSON output

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). The object below is the `data` payload.

```json
{
  "command": "build",
  "componentId": "<id>",
  "buildCommand": "<command string>",
  "stdout": "<stdout>",
  "stderr": "<stderr>",
  "success": true
}
```

## Exit code

Exit code matches the build command process exit code.

## Related

- [component](component.md)
- [deploy](deploy.md)
