# `homeboy doctor`

## Synopsis

```sh
homeboy doctor scan [OPTIONS]
homeboy doctor cleanup [OPTIONS]
```

## Description

Scan Homeboy configuration files and report issues (errors, warnings, info), or safely clean up unknown top-level keys. This command always prints a JSON report to stdout.

## Commands

### `scan`

```sh
homeboy doctor scan [OPTIONS]
```

Options:

- `--scope <SCOPE>`: Scope of configuration to scan (default: `all`)
  - Allowed values: `all`, `app`, `projects`, `servers`, `components`, `modules`
- `--file <PATH>`: Scan a specific JSON file path instead of a scope
- `--fail-on <LEVEL>`: Exit non-zero when issues at this severity exist (default: `error`)
  - Allowed values: `error`, `warning`

### `cleanup`

```sh
homeboy doctor cleanup [OPTIONS]
```

Removes unknown top-level keys (the same keys reported as `UNKNOWN_KEYS`) from recognized Homeboy config JSON files. This does not attempt to fix broken references.

Options:

- `--scope <SCOPE>`: Scope of configuration to clean (default: `all`)
  - Allowed values: `all`, `app`, `projects`, `servers`, `components`, `modules`
- `--file <PATH>`: Clean up a specific config JSON file instead of a scope
  - Refuses to run if the path is not a recognized Homeboy config JSON kind
- `--dry-run`: Preview changes without writing files
- `--fail-on <LEVEL>`: Exit non-zero when issues at this severity exist after cleanup (default: `error`)
  - Allowed values: `error`, `warning`

## JSON output

Homeboy wraps command output in the global JSON envelope described in [JSON output contract](../json-output/json-output-contract.md).

On success:

- `doctor scan` returns a `DoctorReport` value.
- `doctor cleanup` returns `{ cleanup: DoctorCleanupReport, scan: DoctorReport }`.

Example `doctor scan` output:

```json
{
  "success": true,
  "data": {
    "command": "doctor.scan",
    "summary": {
      "filesScanned": 3,
      "issues": {
        "error": 1,
        "warning": 2,
        "info": 0
      }
    },
    "issues": [
      {
        "severity": "error",
        "code": "PROJECT.MISSING_SERVER",
        "message": "Project references unknown server id 'prod'",
        "file": "/path/to/projects/my-project.json",
        "pointer": "/serverId"
      }
    ]
  }
}
```

Example `doctor cleanup` output:

```json
{
  "success": true,
  "data": {
    "cleanup": {
      "command": "doctor.cleanup",
      "summary": {
        "filesConsidered": 2,
        "filesChanged": 1,
        "keysRemoved": 1,
        "filesSkipped": 0,
        "dryRun": false
      },
      "changes": [
        {
          "file": "/path/to/projects/my-project.json",
          "schema": "ProjectConfiguration",
          "removedKeys": ["id"]
        }
      ],
      "skipped": []
    },
    "scan": {
      "command": "doctor.scan",
      "summary": {
        "filesScanned": 2,
        "issues": {
          "error": 0,
          "warning": 0,
          "info": 0
        }
      },
      "issues": []
    }
  }
}
```

Notes:

- `severity` is lowercase: `error`, `warning`, `info`.
- `pointer` and `details` are optional and may be omitted.

## Exit codes

- `0`: no errors (and no warnings when `--fail-on warning` is used)
- `1`: errors found, or warnings found with `--fail-on warning`

Note: `--fail-on` controls warnings vs errors only; `info` never triggers a non-zero exit code.
