---
name: version-bump
description: Bump component version and update changelog via Homeboy.
version: 0.1.0
allowed-tools: Bash(homeboy *)
---

# Version bump

Use Homeboy to update the version and changelog together. Do not manually edit changelog files.

## Workflow

1. `homeboy component show <componentId>`
2. `homeboy version show <componentId>`
3. Decide bump interval based on code-level changes and git logs since previous version bump: `patch|minor|major`
4. Bump version and add changelog entries (repeat `--changelog-add` per item):

```sh
homeboy version bump <componentId> <patch|minor|major> \
  --changelog-add "<change 1>" \
  --changelog-add "<change 2>"
```
5. `homeboy build <componentId>`
6. `homeboy git commit <componentId> "Bump version to X.Y.Z"`
7. `homeboy git push <componentId>`

## Notes

- Ask the user if you should also use `homeboy git tag` and `homeboy git push <componentId> --tags` 
