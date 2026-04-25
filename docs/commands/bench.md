# Bench Command

Run performance benchmarks for a Homeboy component and surface regression
deltas against a stored baseline.

## Synopsis

```bash
homeboy bench <component> [options] [-- <runner-args>]
```

## Description

The `bench` command invokes the extension's bench runner, which measures
one or more scenarios over N iterations and emits a structured JSON
results file. Homeboy parses the results, compares declared numeric
metrics against a saved baseline, and returns a structured report plus
an exit code suitable for CI gates.

`bench` is a sibling of `test`, `lint`, and `build` under homeboy's
extension capability model. The runner contract, manifest shape, and
baseline primitive (`homeboy.json` → `baselines.bench`) are shared with
the other capabilities.

## Arguments

- `<component>`: Component to benchmark. Auto-detected from the current
  working directory if omitted. The component must have a linked
  extension that declares a `bench` capability.

## Options

- `--iterations <N>`: Iterations per scenario (default `10`). Forwarded
  to the runner via `$HOMEBOY_BENCH_ITERATIONS`. Extensions may clamp.
- `--baseline`: Save the current run as the new baseline under
  `homeboy.json` → `baselines.bench`.
- `--ignore-baseline`: Run without comparing to any saved baseline.
- `--ratchet`: When scenarios improve, auto-update the saved baseline so
  the improvement "sticks". Ignored when the run regresses.
- `--regression-threshold <PERCENT>`: Legacy p95 regression tolerance
  (default `5.0`) used when the runner does not declare `metric_policies`.
  A p95 scenario regresses when its current `p95_ms` exceeds
  `baseline.p95_ms * (1 + threshold/100)`.
- `--setting <key=value>`: Override component settings (may be repeated).
- `--path <PATH>`: Override the component's `local_path` for this run.
- `--json-summary`: Include a compact machine-readable summary in the
  JSON output envelope (for CI wrappers).

Arguments after `--` are passed verbatim to the extension's bench runner
script (e.g., `--filter=scenario_id` for selective execution).

## Examples

```bash
# Benchmark a component with defaults (10 iterations, 5% regression threshold)
homeboy bench my-component

# 50 iterations, stricter 2% regression threshold
homeboy bench my-component --iterations 50 --regression-threshold 2.0

# Save a new baseline
homeboy bench my-component --baseline

# Run with auto-ratchet on improvement
homeboy bench my-component --ratchet

# Select a single scenario via passthrough args
homeboy bench my-component -- --filter=hot_path
```

## Baseline Ratchet Semantics

The bench baseline is a list of per-scenario snapshots stored in
`homeboy.json` under the `baselines.bench` key. Each snapshot records
`{ id, metrics }` plus the iteration count at capture time.

On every run without `--baseline` or `--ignore-baseline`:

1. Each current scenario is matched against the baseline by `id`.
2. If the runner declares `metric_policies`, only those metrics are
   compared. Each policy declares whether lower or higher values are
   better and optional percent/absolute tolerances.
3. If the runner omits `metric_policies`, Homeboy keeps the historical
   default: compare `p95_ms` as lower-is-better with the CLI threshold.
4. A scenario improves when any compared metric moves in the better
   direction.
5. Scenarios present in one run but not the other are flagged as
   `new_scenario_ids` / `removed_scenario_ids`. Neither state triggers
   a regression by itself — they're informational.
6. If any scenario regressed, the command exits `1` regardless of the
   runner's own exit code.
7. If any scenario improved and `--ratchet` is set, the baseline is
   overwritten with the current snapshot.

p95 remains the default for legacy latency benchmarks because it is less
sensitive than mean to one-off GC pauses but more sensitive than p99 to
genuine regressions. Runners that care about non-latency signals should
declare `metric_policies` instead.

## Runner Contract

The extension's bench script must:

1. Read `$HOMEBOY_BENCH_ITERATIONS` to determine iteration count.
2. Write its JSON output to `$HOMEBOY_BENCH_RESULTS_FILE`.
3. Exit with a non-zero status only on runner-level failure (script
   error, workload crash) — regressions are homeboy's domain.

### JSON output schema

```json
{
  "component_id": "string",
  "iterations": 10,
  "metric_policies": {
    "error_rate": {
      "direction": "lower_is_better",
      "regression_threshold_absolute": 0.01
    },
    "requests_per_second": {
      "direction": "higher_is_better",
      "regression_threshold_percent": 5.0
    }
  },
  "scenarios": [
    {
      "id": "scenario_slug",
      "file": "tests/bench/some-workload.ext",
      "iterations": 10,
      "metrics": {
        "mean_ms": 120.3,
        "p50_ms": 118.0,
        "p95_ms": 145.0,
        "p99_ms": 160.0,
        "min_ms": 110.0,
        "max_ms": 172.0,
        "error_rate": 0.0,
        "requests_per_second": 180.5,
        "status_500_count": 0
      },
      "memory": { "peak_bytes": 41943040 }
    }
  ]
}
```

- Top-level keys are strict — unknown top-level fields are rejected to
  keep the contract honest.
- `metrics` is an arbitrary map of numeric values. Homeboy core does not
  attach domain meaning to metric names.
- `metric_policies` is optional. If omitted, Homeboy compares `p95_ms`
  using the legacy lower-is-better latency policy.
- Policy `direction` accepts `lower_is_better` / `lower` and
  `higher_is_better` / `higher`.
- Policy thresholds are optional. `regression_threshold_percent` compares
  relative movement; `regression_threshold_absolute` compares raw numeric
  movement. If both are present, a metric must exceed both tolerances to
  regress.
- Scenario-level unknown keys are **tolerated**, so extensions can emit
  additional metadata (tags, environment info, warmup counts) without
  breaking parsing.
- `memory` is optional. Extensions that can't measure peak memory omit it.
- `file` is optional but recommended for diagnostics.

### Environment variables injected

Bench scripts receive the standard runner contract plus bench-specific
variables:

- `HOMEBOY_BENCH_RESULTS_FILE` — where to write JSON output.
- `HOMEBOY_BENCH_ITERATIONS` — iteration count to use.
- `HOMEBOY_RUN_DIR` — per-run directory (shared with test/lint/build).
- `HOMEBOY_EXTENSION_ID`, `HOMEBOY_COMPONENT_ID`, `HOMEBOY_COMPONENT_PATH`,
  and the usual execution-context vars.
- `HOMEBOY_SETTINGS_JSON` — component settings as JSON.

## Component Requirements

For a component to be benchmarkable, it must have:

- A linked extension whose manifest declares a `bench` capability.
- A bench-runner script provided by the extension.

Extension manifest:

```json
{
  "bench": {
    "extension_script": "scripts/bench/bench-runner.sh"
  }
}
```

## Exit Codes

- `0` — All scenarios passed, no regressions detected (or no baseline
  exists yet).
- `1` — At least one scenario regressed beyond the threshold, or the
  runner itself failed.
- Other non-zero — Runner exit code passthrough (extension-specific).

## Related

- [test](./test.md) — Test sibling capability; bench mirrors its flag
  conventions for `--baseline`, `--ignore-baseline`, and `--ratchet`.
- [lint](./lint.md) — Lint sibling capability.
- [build](./build.md) — Build sibling capability.
