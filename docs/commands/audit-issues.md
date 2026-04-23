# `homeboy audit-issues`

## Synopsis

```sh
homeboy audit-issues sync <component-id|path> [--dry-run] [--strike-resolved]
```

## Description

Cross-reference a fresh audit run against **open GitHub issues** labelled `audit` + `audit:<kind>`. When an audit bucket `(component, kind)` matches an existing open issue, the command updates the issue body in-place instead of filing a duplicate.

Findings are stored inside a deterministic block in the issue body:

```
<!-- homeboy:findings -->
| File | Description | First seen | Resolved |
|---|---|---|---|
| path/foo.php:42 | Dead class_exists('WP_Ability') guard | 2026-04-20 |  |
| ~~path/bar.php:17~~ | ~~Dead function_exists guard~~ | 2026-04-15 | 2026-04-23 |
<!-- /homeboy:findings -->
```

The block round-trips across runs: unchanged rows are preserved, new findings are appended, and rows for findings that disappeared can be struck-through (stretch behavior, opt-in via `--strike-resolved`).

This command consumes the same audit pipeline as `homeboy audit` — it does not define new detectors.

## Arguments

- `<component-id|path>`: Component ID to audit, or a direct filesystem path.

## Options

- `--dry-run`: Print proposed new issue bodies to stdout. Does not call the GitHub API. Works without `GH_TOKEN`/`GITHUB_TOKEN` when only local inspection is needed.
- `--strike-resolved`: When a row in an existing issue has no matching finding in the new run, wrap it in `~~...~~` and set the `Resolved` column to today's date.

## Grouping

One GitHub issue per `(component, kind)` tuple. File paths are rows inside that issue, not part of the key. So a single `god_file` issue will list many files as table rows.

When multiple open issues match the same `(component, kind)` key, the command picks the **lowest-numbered** issue (oldest) to keep repeat runs deterministic.

## GitHub Integration

Reads the same env vars as `core::refactor::auto::guard`:

- `GITHUB_REPOSITORY` — `owner/name` slug.
- `GH_TOKEN` or `GITHUB_TOKEN` — bearer token with `issues: write` permission.

When either is unset (and `--dry-run` is not used), the command prints `[audit-issues] CI env not set — skipping sync` to stderr and exits `0`. This matches the soft-fail pattern the rest of the CI integration uses.

The HTTP client is `reqwest::blocking` with a 10s timeout and `application/vnd.github+json` accept header — same as the existing GitHub plumbing.

## Examples

```sh
# Dry-run: print proposed bodies, don't PATCH anything
homeboy audit-issues sync /path/to/data-machine --dry-run

# Real sync with strike-through for disappeared findings
GITHUB_REPOSITORY=Extra-Chill/data-machine \
GH_TOKEN=$(gh auth token) \
homeboy audit-issues sync data-machine --strike-resolved
```

## JSON Output

```json
{
  "success": true,
  "data": {
    "command": "audit-issues.sync",
    "component_id": "data-machine",
    "source_path": "/path/to/data-machine",
    "dry_run": false,
    "strike_resolved": true,
    "groups": [
      {
        "kind": "god_file",
        "finding_count": 12,
        "issue_number": 1342,
        "issue_title": "[audit] god_file in data-machine",
        "action": "updated"
      },
      {
        "kind": "broken_doc_reference",
        "finding_count": 3,
        "issue_number": null,
        "issue_title": null,
        "action": "no_match"
      }
    ],
    "updated": 1,
    "unmatched": 1
  }
}
```

When the CI env is not configured:

```json
{
  "success": true,
  "data": {
    "command": "audit-issues.skipped",
    "reason": "GITHUB_REPOSITORY and/or token env not set"
  }
}
```

## Exit Code

- `0`: Sync completed (or soft-skipped).
- `1`: Internal error (audit run itself failed, not individual PATCH failures).

Individual PATCH failures are reported per-group as `action: { "failed": { "reason": "..." } }` but do not flip the overall exit code — one flaky issue shouldn't mask the rest of the sync.

## Related

- [audit](audit.md) — the underlying audit pipeline.
- [JSON output contract](../json-output/json-output-contract.md).
