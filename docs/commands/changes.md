# `homeboy changes`

## Synopsis

```sh
homeboy changes <componentId> [--since <tag>] [--include-diff]
homeboy changes --cwd [--include-diff]
homeboy changes --json <spec> [--include-diff]
homeboy changes --project <projectId> [--include-diff]
```

## Description

Show changes since the latest git tag for one component, multiple components (bulk JSON), all components attached to a project, or the current working directory.

This command reports:

- commits since the last tag (or a user-provided tag via `--since`)
- uncommitted changes in the working tree
- optionally, unified diffs for uncommitted changes

## Options

- `--cwd`: use current working directory (ad-hoc mode, no component registration required)
- `--since <tag>`: tag name to compare against (single-component mode)
- `--include-diff`: include a unified diff of uncommitted changes
- `--json <spec>`: bulk mode input
  - `<spec>` supports `-` (stdin), `@file.json`, or an inline JSON string
- `--project <projectId>`: show changes for all components attached to a project

## JSON output

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). `homeboy changes` returns either a single `ChangesOutput` or a bulk `BulkChangesOutput` as `data`.

### Single-component output

```json
{
  "action": "changes",
  "componentId": "<componentId>",
  "path": "<local path>",
  "since": "<tag>|null",
  "commits": [
    {
      "hash": "<sha>",
      "subject": "<subject>",
      "author": "<author>",
      "date": "<date>"
    }
  ],
  "uncommitted": {
    "hasChanges": true,
    "staged": ["..."],
    "unstaged": ["..."],
    "untracked": ["..."]
  },
  "diff": "<diff>|null"
}
```

### Bulk output (`--json` or `--project`)

```json
{
  "action": "changes",
  "results": [ { "componentId": "..." } ],
  "summary": {
    "total": 2,
    "withCommits": 1,
    "withUncommitted": 1,
    "clean": 0,
    "failed": 0
  }
}
```

(Exact `commits[]` and `summary` fields are defined by the CLI output structs.)

## Exit code

- `0` when the command runs successfully.

> Note: the per-component outputs include `success` plus optional `warning`/`error` fields. Bulk/project modes return a summary but do not currently change the process exit code when some components fail.

## Related

- [git](git.md)
- [version](version.md)
