# Runs Command

Inspect persisted observation-store runs and artifacts.

## Synopsis

```bash
homeboy runs list [--kind bench|rig|trace] [--component <id>] [--rig <id>] [--status <status>] [--limit 20]
homeboy runs show <run-id>
homeboy runs artifacts <run-id>
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
