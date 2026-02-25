---
name: homeboy
description: "Use Homeboy CLI for version management, deployment, fleet operations, documentation tooling, database ops, module management, and remote file/log access."
compatibility: "Cross-platform Rust CLI. Works with any language/framework. Requires SSH for remote operations."
---

# Homeboy CLI

Development and deployment automation. Generic — works with any language/framework.

## Entity Hierarchy

```
Component  →  versioned, deployable unit (plugin, CLI, module)
Project    →  deployment target (site on a server, links to components)
Server     →  SSH connection config
Fleet      →  named group of projects (batch operations)
Module     →  installable extension (CLI scripts, actions, docs)
```

Storage: `~/.config/homeboy/{components,projects,servers,fleets}/<id>.json`

## Version + Release

```bash
homeboy changelog add <component> -t Added -m "Feature description"
homeboy version bump <component> minor   # 0.1.0 → 0.2.0
homeboy version bump <component> patch   # 0.1.0 → 0.1.1
homeboy release <component>              # interactive release workflow
homeboy changes <component>              # changes since last tag
```

Version targets are regex patterns — work on any file format. Components support `pre_version_bump_commands` and `post_version_bump_commands` hooks (fatal on failure).

## Deploy

```bash
homeboy deploy <project> <component>           # single project
homeboy deploy <project> <component> --check   # dry run
homeboy deploy <component> --fleet <name>      # deploy to fleet
homeboy deploy <component> --shared            # all projects using component
```

## Fleet

```bash
homeboy fleet create prod --projects site-a,site-b
homeboy fleet status prod           # versions across fleet (local)
homeboy fleet check prod            # drift detection (local vs remote)
homeboy fleet add prod site-c
homeboy fleet remove prod site-a
```

## Documentation

```bash
homeboy docs scaffold <component>   # analyze coverage (read-only)
homeboy docs audit <component>      # verify claims against codebase
homeboy docs generate --json '<spec>' | @spec.json | -   # bulk create
homeboy docs list                   # available topics
homeboy docs <topic>                # render topic
```

**Audit** extracts claims from docs and verifies against code. Statuses: `verified`, `broken`, `needs_verification`. Scans for undocumented features via `audit_feature_patterns` in module manifests.

**Generate** spec: `{"output_dir": "docs", "files": [{"path": "f.md", "content": "..."}]}`

**Workflow:** scaffold → learn (`docs documentation/generation`) → plan → generate → audit → maintain (`docs documentation/alignment`)

## Modules

```bash
homeboy module list [-p <project>]
homeboy module show <module>
homeboy module run <module> [-p <project>] [-c <component>] [-i key=val]
homeboy module setup <module>
homeboy module install <source> [--id <id>]   # git URL or local path
homeboy module update <module>
homeboy module uninstall <module>
homeboy module action <module> <action>
```

## Remote Operations

### Files

```bash
homeboy file <project> ls [path] | read <path> | write <path>
homeboy file <project> delete <path> | rename <from> <to>
homeboy file <project> find <pattern> | search <pattern> | download <path>
```

### Logs

```bash
homeboy logs <project>                # show pinned logs
homeboy logs <project> <log-id>       # specific log
homeboy logs <project> list | search <pattern> | clear <log-id>
```

### Database

```bash
homeboy db <project> tables | describe <table> | query <table>
homeboy db <project> search <table> <col> <val>
homeboy db <project> delete-row <table> <id> | drop-table <table>
homeboy db <project> tunnel           # SSH tunnel to DB
```

### SSH & Transfer

```bash
homeboy ssh <project>                 # interactive session
homeboy ssh <project> -- <command>    # run remote command
homeboy transfer <src-project> <dest-project> <path>
```

## Git, Build, Test

```bash
homeboy git status|commit|push|pull|tag <component>
homeboy build <component>
homeboy test <component>
homeboy lint <component>
```

## API Client

```bash
homeboy auth login|status|clear <project>
homeboy api get|post|put|patch|delete <project> <endpoint> [--json '<body>']
```

## Config

```bash
homeboy config show | set <pointer> <value> | unset <pointer> | reset | path
```

## Common Patterns

```bash
# Self-deploying CLI
homeboy deploy homeboy --shared

# Multi-site plugin update
homeboy changelog add my-plugin -t Fixed -m "Bug fix"
homeboy version bump my-plugin patch
homeboy deploy my-plugin --fleet production

# Drift check before deploy
homeboy fleet check production

# Docs audit cycle
homeboy docs scaffold my-component
homeboy docs audit my-component   # fix broken refs, re-audit
```

## JSON Output

All commands return structured JSON: `{"success": true, "data": {...}, "hints": [...]}`. Errors: `{"success": false, "error": "...", "hints": [...]}`.
