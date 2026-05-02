# homeboy trace

Capture black-box behavioral traces for a component. Trace runners write a JSON evidence envelope plus optional artifacts under the Homeboy run directory.

## Usage

```sh
homeboy trace <component> <scenario>
homeboy trace <component> list
homeboy trace <component> <scenario> --rig <rig-id>
homeboy trace <component> <scenario> --json-summary
homeboy trace <component> <scenario> --span submit_to_cli:ui.submit:cli.start
homeboy trace <component> <scenario> --phase submit:ui.submit --phase cli:cli.start --phase ready:server.ready
homeboy trace <component> <scenario> --rig <rig-id> --phase-preset create-site
homeboy trace <component> <scenario> --repeat 5 --aggregate spans --schedule interleaved
homeboy trace compare before.json after.json --focus-span phase.wp_boot_start_to_wp_boot_ready
homeboy trace compare-variant --rig studio --scenario studio-app-create-site --repeat 5 --overlay overlays/change.patch --output-dir .homeboy/experiments/change
homeboy trace <component> <scenario> --report=markdown
homeboy trace <component> <scenario> --baseline
homeboy trace <component> <scenario> --ratchet
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
  "span_definitions": [
    { "id": "close_to_assertion", "from": "desktop.window.closed", "to": "assertion.checked" }
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

## Spans

Spans are generic intervals over timeline keys. A timeline key is `source.event`, using the event's `source` and `event` fields.

Runners can emit `span_definitions`, or callers can pass repeatable `--span id:from:to` flags. Homeboy writes computed results back into the command output as `span_results`:

```json
{
  "span_results": [
    {
      "id": "submit_to_cli",
      "from": "ui.create_site.submit_clicked",
      "to": "cli.validating_site_configuration",
      "status": "ok",
      "duration_ms": 1065,
      "from_t_ms": 120,
      "to_t_ms": 1185
    }
  ]
}
```

If an endpoint is missing, Homeboy emits a skipped result with `missing` keys instead of panicking.

When a timeline contains repeated events with the same key, Homeboy resolves the span to the nearest valid `from`/`to` pair where the `to` event occurs at or after the `from` event. This keeps simple `source.event` span definitions stable for common lifecycle events that naturally repeat.

## Phases

Use repeatable `--phase [label:]source.event` flags to provide an ordered milestone chain. Homeboy expands the chain into adjacent span results plus a `phase.total` span from the first milestone to the last milestone:

```sh
homeboy trace studio create-site \
  --phase submit:ui.create_site.submit_clicked \
  --phase cli:studio_server_child.run_cli.before \
  --phase ready:playground.run_cli.ready \
  --report=markdown
```

The example above produces span rows for `phase.submit_to_cli`, `phase.cli_to_ready`, and `phase.total`. Existing `--span` definitions still work and can be mixed with phase milestones.

Phase spans keep the same ordering semantics as normal spans: a phase interval is only `ok` when the later milestone occurs at or after the previous milestone. If both phase milestones exist but the later milestone was first observed before the previous milestone, Homeboy reports the span as skipped with a non-monotonic phase-chain diagnostic instead of treating the out-of-order interval as successful. Markdown reports include that diagnostic in the span status column so asynchronous readiness events are easier to distinguish from missing events.

Rigs and rig-owned trace workloads can declare reusable phase presets. Use `--phase-preset <name>` to expand a named preset from the selected rig/workload into the same adjacent phase spans:

```jsonc
{
  "trace_workloads": {
    "nodejs": [
      {
        "path": "${package.root}/trace/create-site.trace.mjs",
        "trace_default_phase_preset": "create-site",
        "trace_phase_presets": {
          "create-site": [
            "submit:ui.create_site.submit_clicked",
            "cli:studio_server_child.run_cli.before",
            "ready:playground.run_cli.ready"
          ]
        }
      }
    ]
  }
}
```

When `--repeat <N> --aggregate spans` is used with `--rig` and no explicit `--phase`, `--phase-preset`, or `--span` flags, Homeboy applies the workload's `trace_default_phase_preset`. A preset named `default` is also recognized when no explicit default pointer is present.

## Repeat And Aggregate

Use `--repeat <N> --aggregate spans` to run the same trace scenario multiple times and summarize span timings across runs. The aggregate output includes each run's preserved `trace.json` artifact path plus per-span `min_ms`, `median_ms`, `avg_ms`, percentile fields (`p75_ms`, `p90_ms`, `p95_ms`) when enough samples are available, `max_ms`, and `failures` counts.

```sh
homeboy trace studio studio-app-create-site --repeat 5 --aggregate spans
```

Each repeat uses a fresh Homeboy run directory, so completed run data is preserved even when a later repeat fails.

Use `--schedule grouped` or `--schedule interleaved` to record the intended run order in the aggregate manifest. The current single-scenario repeat runner records one `run` group; the planner is shared with future baseline/variant runners so paired experiments can use grouped order (`baseline...variant...`) or interleaved order (`baseline, variant, baseline, variant`).

Use repeatable `--focus-span <span-id>` to add a focused span section while keeping the full span table in the JSON and Markdown report.

## Guardrails

Rig-pinned aggregate traces can run post-trace guardrails after timing artifacts are captured. Guardrails reuse rig `check` probes, so command and HTTP checks are supported with the same fields as pipeline checks. Declare them at the rig level, on a trace workload, or on a named trace variant:

```jsonc
{
  "trace_guardrails": [
    { "label": "app health", "http": "http://127.0.0.1:3000/health", "expect_status": 200 }
  ],
  "trace_workloads": {
    "nodejs": [
      {
        "path": "${package.root}/trace/create-site.trace.mjs",
        "trace_guardrails": [
          { "label": "site still lists", "command": "npm run smoke:list-sites" }
        ]
      }
    ]
  },
  "trace_variants": {
    "fast-install": {
      "overlay": "overlays/fast-install.patch",
      "trace_guardrails": [
        { "label": "install behavior", "command": "npm run smoke:install" }
      ]
    }
  }
}
```

Guardrail failures mark the aggregate or experiment result as failed, but Homeboy still writes the timing artifacts, span summaries, and compare JSON. Compare outputs include before/after guardrail results alongside span deltas so a faster run cannot hide a behavior regression.

## Compare Aggregates

Use `trace compare` to compare two aggregate span JSON outputs. The comparison reports each span's before/after median and average, absolute deltas, and percentage deltas. Spans are sorted by absolute median delta descending so the largest changes are first; spans that only exist in one file are included with unavailable deltas after comparable spans. Markdown reports bold non-zero absolute deltas to make regressions and improvements easier to scan.

```sh
homeboy trace compare before.json after.json
homeboy trace compare before.json after.json --focus-span phase.wp_boot_start_to_wp_boot_ready --report=markdown
```

Focused compare spans are evaluated independently from the full span table. When a focused span's median slowdown exceeds both `--regression-threshold` and `--regression-min-delta-ms`, or its failure count increases, `trace compare` returns a failing exit code and records `focus_status`, `focus_regression_count`, and `focus_failure_count` in JSON output. All compared spans remain present in `spans`.

## Compare Variant Experiments

Use `trace compare-variant` to run a baseline aggregate, run the same trace with one or more overlays, compare the aggregate span outputs, and keep the evidence in one directory:

```sh
homeboy trace compare-variant \
  --rig studio \
  --scenario studio-app-create-site \
  --phase-preset wordpress-boot-steps \
  --repeat 5 \
  --overlay overlays/fresh-install-mode.patch \
  --overlay overlays/disable-install-mail.patch \
  --output-dir .homeboy/experiments/fast-install
```

The bundle contains `baseline.json`, `variant.json`, `compare.json`, and `summary.md`. The summary includes component SHAs from rig state when available plus the files touched by each variant overlay.

## Markdown Reports

Use `--report=markdown` to render a skim-friendly report from the same trace run. The report includes status, span table, assertions, artifacts, and timeline events.

## Span Baselines

Trace spans can use the same lifecycle flags as other baseline-aware commands:

- `--baseline` stores the current span durations in `homeboy.json` under `baselines.trace`.
- `--ratchet` updates the stored baseline when spans improve.
- `--ignore-baseline` skips comparison.
- `--regression-threshold=<PCT>` controls the allowed duration slowdown. Default is `5`.
- `--regression-min-delta-ms=<MS>` controls the minimum absolute slowdown before a regression can fail. Default is `50`.

Both regression thresholds must trip before Homeboy fails the run. For example, `9ms -> 15ms` exceeds the default percentage threshold but stays below the default `50ms` minimum delta, so it does not fail as a trace baseline regression.

Rig-pinned traces store separate baselines under `trace.rig.<rig-id>` so bare and rig-owned traces do not collide.
