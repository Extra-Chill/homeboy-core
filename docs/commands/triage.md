# `homeboy triage`

Produce an attention report for components, projects, fleets, rigs, or the full configured workspace.
GitHub access is read-only; each run also records a local observation in the
Homeboy SQLite database so later runs can report the previous triage time.

## Synopsis

```sh
homeboy triage [OPTIONS] [COMMAND]
```

When no command is provided, `homeboy triage` defaults to `homeboy triage workspace`.

## Subcommands

- `component` — triage one registered component, or any checkout via `--path`
- `project` — triage every component attached to a project
- `fleet` — triage unique components used across a fleet
- `rig` — triage components declared in a local rig spec
- `workspace` — triage every configured project, rig, and registered component once per repo

## Useful filters

- `--issues` / `--prs` — restrict which GitHub item types are included
- `--mine` — show work assigned to or authored by the authenticated GitHub user
- `--assigned <USER>` — restrict to one assignee
- `--label <LABEL>` — restrict to one label; repeatable
- `--needs-review` — restrict PRs to review-required items
- `--failing-checks` — restrict PRs to failing-check items
- `--drilldown` — include compact failing check names and URLs

## Output Signals

Surfaced issues include comment activity when GitHub returns it:

- `comments_count`
- `last_comment_at`

Surfaced pull requests include the same comment activity plus review activity:

- `comments_count`
- `reviews_count`
- `last_comment_at`
- `last_review_at`

Each successful observation adds an `observation` block to the JSON output with
the local `run_id`, recorded `item_count`, SQLite `store_path`, and
`previous_run_at` when the same triage target was observed before. Triage item
snapshots are stored in the `triage_items` table and linked to the existing
`runs` table.

## `--path` (component)

`homeboy triage component --path <CHECKOUT>` skips the registry entirely and
resolves the GitHub remote directly from the checkout's `origin`. Useful for:

- unregistered checkouts (CI runners, ad-hoc clones, worktrees)
- repos whose registry record is broken or stale (e.g. a leftover worktree
  pinned as `local_path`, or a non-URL `remote_url`) — the escape hatch lets
  you triage the checkout without first reconciling the registry
- one-off triage from a directory you do not want to register

The `COMPONENT_ID` positional becomes optional when `--path` is given. When both
are supplied, they must agree: if a registry record exists for `COMPONENT_ID`
and its `local_path` does not canonicalize to `<CHECKOUT>`, the command errors
clearly rather than silently picking one side.

The checkout must exist and be a git repository, and `git remote get-url origin`
must return a parseable GitHub URL — otherwise the command surfaces the same
`remote_url_is_not_github` reason as the registry-driven path.

## Examples

```sh
homeboy triage
homeboy triage --mine --drilldown
homeboy triage component homeboy --failing-checks --drilldown
homeboy triage component --path /Users/me/Developer/homeboy
homeboy triage component homeboy --path ./homeboy --failing-checks
```

## Related

- [status](status.md)
- [issues](issues.md)
- [review](review.md)
