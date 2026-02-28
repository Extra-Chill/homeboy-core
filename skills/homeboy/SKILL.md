---
name: homeboy
description: "Use Homeboy CLI for version management, deployment, fleet operations, documentation tooling, database ops, extension management, code auditing, and remote file/log access."
compatibility: "Cross-platform Rust CLI. Works with any language/framework. Requires SSH for remote operations."
---

# Homeboy CLI

Development and deployment automation. Generic — works with any language/framework.

## Entity Hierarchy

```
Component  →  versioned, deployable unit (plugin, CLI, extension)
Project    →  deployment target (site on a server, links to components)
Server     →  SSH connection config
Fleet      →  named group of projects (batch operations)
Extension     →  installable extension (CLI scripts, actions, docs)
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

## Status + Audit + Cleanup

```bash
# Status — actionable component overview
homeboy status                      # components in current directory context
homeboy status --all                # all components
homeboy status --uncommitted        # components with uncommitted changes
homeboy status --needs-bump         # components needing version bump
homeboy status --ready              # components ready to deploy
homeboy status --docs-only          # components with docs-only changes

# Audit — detect architectural drift and convention violations
homeboy audit <component>           # full audit (conventions + findings)
homeboy audit <component> --conventions   # only show discovered conventions
homeboy audit <component> --fix           # generate fix stubs (dry run)
homeboy audit <component> --fix --write   # apply fixes to disk
homeboy audit <component> --baseline      # save current state as baseline

# Cleanup — identify config drift and stale state
homeboy cleanup                     # all components
homeboy cleanup <component>         # specific component
homeboy cleanup --severity error    # only errors
homeboy cleanup --category extensions  # only extension issues

# Init — read-only repo context (creates no state)
homeboy init                        # current context
homeboy init --all                  # all entities
```

**Audit** discovers conventions per directory (naming, imports, methods, registrations), identifies conforming files and outliers, and reports findings with confidence scores. Statuses: `clean`, `drift`, `fragmented`. Run before any structural change.

## Documentation

```bash
homeboy docs scaffold <component>   # analyze coverage (read-only)
homeboy docs audit <component>      # verify claims against codebase
homeboy docs generate --json '<spec>' | @spec.json | -   # bulk create
homeboy docs list                   # available topics
homeboy docs <topic>                # render topic
```

**Audit** extracts claims from docs and verifies against code. Statuses: `verified`, `broken`, `needs_verification`. Scans for undocumented features via `audit_feature_patterns` in extension manifests.

**Generate** spec: `{"output_dir": "docs", "files": [{"path": "f.md", "content": "..."}]}`

**Workflow:** scaffold → learn (`docs documentation/generation`) → plan → generate → audit → maintain (`docs documentation/alignment`)

## Extensions

```bash
homeboy extension list [-p <project>]
homeboy extension show <extension>
homeboy extension run <extension> [-p <project>] [-c <component>] [-i key=val]
homeboy extension setup <extension>
homeboy extension install <source> [--id <id>]   # git URL or local path
homeboy extension update <extension>
homeboy extension uninstall <extension>
homeboy extension action <extension> <action>
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

## Remote Tool Bridges

Run project-specific tools over SSH without logging in:

```bash
# WordPress CLI
homeboy wp <project> plugin list
homeboy wp <project> option get blogname
homeboy wp <project>:<subtarget> datamachine pipelines list  # multisite

# PM2 process manager
homeboy pm2 <project> list
homeboy pm2 <project> restart all

# OpenClaw agent management
homeboy openclaw <agent> gateway status
homeboy openclaw <agent> config get
homeboy openclaw <agent> cron list
```

## API Client

```bash
# Authentication
homeboy auth login <project>
homeboy auth status <project>
homeboy auth logout <project>

# REST requests (requires auth)
homeboy api <project> get <endpoint>
homeboy api <project> post <endpoint> --json '<body>'
homeboy api <project> put|patch|delete <endpoint> [--json '<body>']
```

## Git, Build, Test

```bash
homeboy git status|commit|push|pull|tag <component>
homeboy build <component>
homeboy test <component>
homeboy lint <component>
homeboy lint <component> --fix
```

## Config + Upgrade

```bash
homeboy config show | set <pointer> <value> | unset <pointer> | reset | path

# Self-upgrade
homeboy upgrade                     # upgrade to latest
homeboy upgrade --check             # check without installing
homeboy upgrade --force             # force even if at latest
homeboy upgrade --no-restart        # skip restart
homeboy upgrade --method cargo      # override install method
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

# Convention audit cycle
homeboy audit my-component              # discover drift
homeboy audit my-component --fix        # preview fixes
homeboy audit my-component --fix --write  # apply fixes
homeboy audit my-component --baseline   # save clean state

# Docs audit cycle
homeboy docs scaffold my-component
homeboy docs audit my-component   # fix broken refs, re-audit
```

## JSON Output

All commands return structured JSON: `{"success": true, "data": {...}, "hints": [...]}`. Errors: `{"success": false, "error": "...", "hints": [...]}`.
