# `homeboy deploy`

## Synopsis

```sh
homeboy deploy <project_id> [<component_ids...>] [-c|--component <id>]... [--all] [--outdated] [--check] [--dry-run] [--json '<spec>']
# If no component IDs are provided, you must use --all, --outdated, or --check.
```

## Arguments and flags

- `project_id`: project ID
- `<component_ids...>` (optional): component IDs to deploy (positional, trailing)

Options:

- `-c`, `--component`: component ID to deploy (can be repeated, alternative to positional)
- `--all`: deploy all configured components
- `--outdated`: deploy only outdated components
  - Determined from the first version target for each component.
- `--check`: check component status without building or deploying
  - Shows all components for the project with version comparison status.
  - Combines with `--outdated` or component IDs to filter results.
- `--dry-run`: preview what would be deployed without executing (no build, no upload)
- `--json`: JSON input spec for bulk operations (`{"component_ids": ["component-id", ...]}`)

Bulk JSON input uses `component_ids` (snake_case):

```json
{ "component_ids": ["component-a", "component-b"] }
```

Positional and flag component IDs can be mixed; both are merged into the deployment list.

If no component IDs are provided and neither `--all` nor `--outdated` is set, Homeboy returns an error. If `--outdated` finds no outdated components, Homeboy returns an error.

## JSON output

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../json-output/json-output-contract.md). The object below is `data`.

```json
{
  "command": "deploy.run",
  "project_id": "<project_id>",
  "all": false,
  "outdated": false,
  "check": false,
  "dry_run": false,
  "results": [
    {
      "id": "<component_id>",
      "name": "<name>",
      "status": "deployed|failed|skipped|planned|checked",
      "deploy_reason": "explicitly_selected|all_selected|version_mismatch|unknown_local_version|unknown_remote_version",
      "component_status": "up_to_date|needs_update|behind_remote|unknown",
      "local_version": "<v>|null",
      "remote_version": "<v>|null",
      "error": "<string>|null",
      "artifact_path": "<path>|null",
      "remote_path": "<path>|null",
      "build_command": "<cmd>|null",
      "build_exit_code": "<int>|null",
      "deploy_exit_code": "<int>|null",
      "release_state": {
        "commits_since_version": 5,
        "has_uncommitted_changes": false,
        "baseline_ref": "v0.9.15"
      }
    }
  ],
  "summary": { "succeeded": 0, "failed": 0, "skipped": 0 }
}
```

Notes:

- `deploy_reason` is omitted when not applicable.
- `component_status` is only present when using `--check` or `--check --dry-run`.
- `artifact_path` is the component build artifact path as configured; it may be relative but must include a filename.

Note: `build_exit_code`/`deploy_exit_code` are numbers when present (not strings).

### Component status values

When using `--check`, each component result includes a `component_status` field:

- `up_to_date`: local and remote versions match
- `needs_update`: local version ahead of remote (needs deployment)
- `behind_remote`: remote version ahead of local (local is behind)
- `unknown`: cannot determine status (missing version information)

### Release state

When using `--check`, each component result includes a `release_state` field that tracks unreleased changes:

- `commits_since_version`: number of commits since the last version tag
- `has_uncommitted_changes`: whether there are uncommitted changes in the working directory
- `baseline_ref`: the tag or commit hash used as baseline for comparison

This helps identify components where `component_status` is `up_to_date` but work has been done since the last version bump (commits_since_version > 0), indicating a version bump may be needed before deployment.

Exit code is `0` when `summary.failed == 0`, otherwise `1`.

## Exit code

- `0` when all selected component deploys succeed.
- `1` when any component deploy fails.

## Preview Before Deploying

Use `--dry-run` to see what would be deployed without executing:

```sh
homeboy deploy myproject --outdated --dry-run
```

## Check Component Status

Use `--check` to view version status for all components without building or deploying:

```sh
# Check all components for a project
homeboy deploy myproject --check

# Check only outdated components
homeboy deploy myproject --check --outdated

# Check specific components
homeboy deploy myproject --check component-a component-b
```

To see detailed git changes (commits, diffs) before deploying, use the `changes` command:

```sh
# Show changes for all project components
homeboy changes --project myproject

# Show changes with git diffs included
homeboy changes --project myproject --git-diffs
```

## Related

- [build](build.md)
- [changes](changes.md)
- [component](component.md)
