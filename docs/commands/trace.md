# homeboy trace

Capture black-box behavioral traces for a component. Trace runners write a JSON evidence envelope plus optional artifacts under the Homeboy run directory.

## Usage

```sh
homeboy trace <component> <scenario>
homeboy trace <component> list
homeboy trace <component> <scenario> --rig <rig-id>
homeboy trace <component> <scenario> --json-summary
```

## Extension Manifest

```json
{
  "trace": {
    "extension_script": "scripts/trace/trace-runner.sh"
  }
}
```

## Runner Environment

- `HOMEBOY_TRACE_RESULTS_FILE`
- `HOMEBOY_TRACE_SCENARIO`
- `HOMEBOY_TRACE_LIST_ONLY`
- `HOMEBOY_TRACE_ARTIFACT_DIR`
- `HOMEBOY_TRACE_RIG_ID` when `--rig` is used
- `HOMEBOY_TRACE_COMPONENT_PATH` when Homeboy resolves a path override
- `HOMEBOY_RUN_DIR`

## Results Envelope

```json
{
  "component_id": "studio",
  "scenario_id": "close-window-running-site",
  "status": "fail",
  "summary": "Window reopened after close",
  "timeline": [
    { "t_ms": 0, "source": "desktop", "event": "window.closed", "data": { "id": 1 } }
  ],
  "assertions": [
    { "id": "no-window-reopen", "status": "fail", "message": "Window reopened" }
  ],
  "artifacts": [
    { "label": "main log", "path": "artifacts/main.log" }
  ]
}
```

V1 statuses are `pass`, `fail`, and `error`.
