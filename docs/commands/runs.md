# Runs Command

Inspect persisted observation-store runs and artifacts.

## Synopsis

```bash
homeboy runs list [--kind bench|rig|trace] [--component <id>] [--rig <id>] [--status <status>] [--limit 20]
homeboy runs show <run-id>
homeboy runs artifacts <run-id>
homeboy runs export --run <run-id> --output <dir>
homeboy runs export --since <duration> --output <dir>
homeboy runs import <dir>
```

## Description

`homeboy runs` is a read-only query surface over Homeboy's local observation store. Producers such as `bench`, `rig`, and `trace` write run and artifact records; this command lets humans and agents inspect that evidence without opening SQLite directly.

The JSON output includes stable run fields: run id, kind, status, timestamps, component id, rig id, git SHA, command, cwd, metadata, and artifact records where relevant.

## Related Readers

```bash
homeboy bench history <component> [--scenario <id>] [--rig <id>] [--limit 20]
homeboy bench compare --from-run <run-id> --to-run <run-id>
homeboy rig runs <id> [--limit 20]
```

These commands are thin read-only wrappers over the same observation-store records.

## Portable Bundles

`homeboy runs export` writes an inspectable directory bundle for moving observation evidence between machines without copying raw SQLite:

```text
homeboy-observations/
  manifest.json
  runs.json
  artifacts.json
  trace_spans.json
```

The v1 bundle is metadata-only: artifact records are exported, but artifact file bytes are not copied. Zip output is intentionally out of scope for v1; pass a directory path to `--output`.

`homeboy runs import` is idempotent. Existing identical records are accepted, while conflicting records with the same primary key fail clearly.
