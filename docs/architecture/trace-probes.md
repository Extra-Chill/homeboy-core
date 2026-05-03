# Trace Probes

Trace probes are passive observation helpers that run beside a trace workload and emit events into the existing trace timeline shape:

```json
{ "t_ms": 12, "source": "log.tail", "event": "log.match", "data": { "line": "..." } }
```

## Contract

- Probes never append to `HOMEBOY_TRACE_RESULTS_FILE` directly.
- Homeboy starts probes before invoking the trace runner and stops them after the runner exits.
- Probe events are collected internally, sorted by `t_ms`, and merged into the runner's final `timeline` before span processing.
- Runner-owned events and probe-owned events share the same envelope, so existing span definitions can target either source.

## Rig Workload Configuration

Probes are declared on detailed `trace_workloads` entries:

```jsonc
{
  "trace_workloads": {
    "nodejs": [
      {
        "path": "${package.root}/trace/create-site.trace.mjs",
        "trace_probes": [
          { "type": "log.tail", "path": "${components.app.path}/app.log", "grep": "invalid_grant" },
          { "type": "process.snapshot", "pattern": "opencode.*serve", "interval_ms": 250 }
        ]
      }
    ]
  }
}
```

Probe values support the same `${components.<id>.path}` and `${package.root}` expansion as rig workload paths.

## v1 Probes

### `log.tail`

Inputs:

- `path`: log file to tail.
- `grep` or `match`: optional regex.

Events:

- `log.line`: emitted for each new line appended after the probe starts.
- `log.match`: emitted when the optional regex matches a line.

Data includes `path`, `line`, and `pattern` for match events.

### `process.snapshot`

Inputs:

- `pattern`: regex matched against process command lines.
- `interval_ms`: optional polling interval, default `1000`.

Events:

- `proc.list`: full matching process snapshot for each poll.
- `proc.spawn`: matching PID appeared since the previous snapshot.
- `proc.exit`: matching PID disappeared since the previous snapshot.

Data includes `pattern` and `processes` for list events, and `pid` plus `command` for delta events.

## Deferred

This substrate intentionally does not implement fanotify, PID-of-writer attribution, systemd integration, port owner detection, or the full six-probe inventory from issue #2163. Those can layer onto the same collection and merge contract later.
