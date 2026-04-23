# `homeboy version`

## Synopsis

```sh
homeboy version <COMMAND>
```

## Subcommands

### `show`

```sh
homeboy version show [<component_id>]
```

Shows the current version for the specified component, or the Homeboy binary version if omitted.

### `bump`

```sh
homeboy version bump <component_id> <patch|minor|major>
```

Alias for [`homeboy release`](release.md). Bumps version, finalizes changelog, commits, tags, and optionally pushes.

Flags (same as `release`):

- `--dry-run`: Preview without making changes
- `--no-tag`: Skip git tag creation
- `--no-push`: Skip pushing to remote
- `--no-commit`: Fail if uncommitted changes exist (strict mode)
- `--commit-message <MESSAGE>`: Custom pre-release commit message

### `set`

```sh
homeboy version set [<component_id>] <new_version>
```

`set` writes the version targets directly without incrementing and does not finalize the changelog.

## Description

`homeboy version bump`:

Alias for `homeboy release`. Delegates to the release command for full release pipeline execution. See [release](release.md) for details.

`homeboy version set`:

- Writes the new version directly to targets without touching the changelog.

Changelog entries are generated automatically from conventional-prefixed commits (`feat:` / `fix:` / etc.) at release time — no hand-curation needed.

Recommended release workflow:

- Land work as scoped conventional-prefixed commits (`feat:`, `fix:`, etc.) first.
- Use `homeboy changes <component_id>` to review everything since the last tag.
- Run `homeboy version bump <component_id> <patch|minor|major>` when the only remaining local changes are release metadata.
- Homeboy generates the changelog section from the commit trail and finalizes it as `## [X.Y.Z] - YYYY-MM-DD` in one shot.

Arguments:

- `[<component_id>]`: component ID (optional, shows Homeboy binary version when omitted)
- `<patch|minor|major>`: version bump type

## JSON output

> Note: all command output is wrapped in the global JSON envelope described in the [JSON output contract](../architecture/output-system.md). `homeboy version` returns a `VersionOutput` object as the `data` payload.

`homeboy version show` data payload:

- `command`: `version.show`
- `component_id`
- `version` (detected current version)
- `targets`: array of `{ file, pattern, full_path, match_count }`

`homeboy version bump` data payload:

- `command`: `version.bump`
- `component_id`
- `bump_type`: patch, minor, or major
- `dry_run`: boolean
- `no_tag`: boolean
- `no_push`: boolean
- `no_commit`: boolean
- `commit_message` (omitted if not specified)
- `plan` (present when `--dry-run`): release plan object
- `run` (present when not `--dry-run`): release run result object

See [release](release.md) for full plan and run object schemas.

`homeboy version set` data payload:

- `command`: `version.set`
- `component_id`
- `old_version`
- `new_version`
- `targets`: array of `{ file, pattern, full_path, match_count }`

Errors:

- `bump` errors follow the same validation as `release`. See [release](release.md) for error conditions.

## Exit code

- `show`: `0` on success; errors if the version cannot be parsed.
- `bump`: `0` on success.
- `set`: `0` on success.

## Notes

- Components must have `version_targets` configured (non-empty). Homeboy uses the first target as the primary version source.
- Each `version_targets[]` entry has `file` and optional `pattern`. When `pattern` is omitted, Homeboy checks extension-provided version patterns for that file type; if none are provided, the command errors.

### Changelog Requirements

`version bump` requires:
1. A changelog file to exist
2. The `changelog_target` to be configured on the component

**Setup:**
```sh
homeboy component set <id> --changelog-target "CHANGELOG.md"
```

To bypass changelog finalization entirely, use `version set` instead of `version bump`.

### Auto-Generation from Commits

`version bump` generates changelog entries from commits since the last tag. There is no manual entry path — all entries come from commits.

Requirements:

1. All changes are **committed** (uncommitted changes don't contribute entries).
2. At least one commit has an entry-producing prefix (see table below).

| Commit prefix | Changelog section |
|---------------|------------------|
| `feat:`       | Added            |
| `fix:`        | Fixed            |
| `BREAKING` / `!:` | Changed      |
| Other (non-conventional) | Changed |
| `docs:`, `chore:` | **Skipped**  |

**Important:** If all commits are `docs:` or `chore:`, no entries get generated and the release errors out. Commit at least one user-facing change with a `feat:`/`fix:`/conventional prefix before releasing.

## Related Workflows

After bumping, push and optionally tag:

```sh
homeboy git push <component_id>
homeboy git tag <component_id>
```

## Rollback Procedure

If you accidentally bump a version:

### 1. Revert local changes
```sh
git checkout -- CHANGELOG.md Cargo.toml package.json  # your version files
```

### 2. Delete local tag (if created)
```sh
git tag -d v0.X.Y
```

### 3. Delete remote tag (if pushed)
```sh
git push origin --delete v0.X.Y
```

### 4. Force push (if committed and pushed)
```sh
git reset --hard HEAD~1
git push --force-with-lease
```

**Prevention:** Always use `--dry-run` first:
```sh
homeboy version bump <component_id> patch --dry-run
```

## Related

- [build](build.md)
- [component](component.md)
- [git](git.md)
