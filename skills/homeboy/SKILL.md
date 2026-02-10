---
name: homeboy
description: Use Homeboy CLI for version management, deployment, and fleet operations. Invoke for component versioning (bump, changelog), deploying to remote servers via SSH, managing fleets of projects, and any task involving homeboy commands.
---

# Homeboy CLI

Development and deployment automation. Generic — works with any language/framework.

## Entity Hierarchy

```
Component  →  versioned, deployable unit (plugin, CLI, module)
    ↓
Project    →  deployment target (site on a server, links to components)
    ↓
Server     →  SSH connection config
    
Fleet      →  named group of projects (for batch operations)
```

**Storage:** `~/.config/homeboy/{components,projects,servers,fleets}/<id>.json`

## Core Workflows

### Version + Release

```bash
# Add changelog entries (repeatable -m for multiple)
homeboy changelog add <component> -t Added -m "Feature description"

# Bump version (updates files, finalizes changelog, commits, tags, pushes)
homeboy version bump <component> minor   # 0.1.0 → 0.2.0
homeboy version bump <component> patch   # 0.1.0 → 0.1.1
```

Version targets are regex patterns — work on any file (Cargo.toml, package.json, PHP headers).

### Deploy

```bash
# Single project
homeboy deploy <project> <component>

# Check what would deploy (no build/upload)
homeboy deploy <project> <component> --check

# Deploy to fleet
homeboy deploy <component> --fleet <fleet-name>

# Deploy to ALL projects using component (auto-detect)
homeboy deploy <component> --shared
```

### Fleet Management

```bash
homeboy fleet create prod --projects site-a,site-b
homeboy fleet status prod      # versions across fleet
homeboy fleet check prod       # drift detection (remote vs local)
homeboy fleet add prod site-c
homeboy fleet remove prod site-a
```

## Component Config Example

```json
{
  "local_path": "/path/to/repo",
  "remote_path": "/usr/local/bin",
  "build_command": "cargo build --release",
  "build_artifact": "target/release/myapp",
  "version_targets": [
    { "file": "Cargo.toml", "pattern": "^version = \"(\\d+\\.\\d+\\.\\d+)\"" }
  ],
  "changelog_target": "CHANGELOG.md"
}
```

## Project Config Example

```json
{
  "name": "My Site",
  "domain": "example.com",
  "server_id": "prod-server",
  "base_path": "/var/www/site",
  "component_ids": ["my-plugin", "my-theme"]
}
```

## Quick Reference

| Task | Command |
|------|---------|
| List components | `homeboy component list` |
| Show component | `homeboy component show <id>` |
| List projects | `homeboy project list` |
| Show project | `homeboy project show <id>` |
| See shared components | `homeboy component shared` |
| SSH to project server | `homeboy ssh <project>` |
| View remote logs | `homeboy logs <project> [log-id]` |
| Remote file ops | `homeboy file <project> <subcommand>` |

## Common Patterns

**Self-deploying CLI tools:**
```bash
# Add homeboy as component with build_command + build_artifact
# Deploy to all servers that use it
homeboy deploy homeboy --shared
```

**Multi-site plugin update:**
```bash
homeboy changelog add my-plugin -t Fixed -m "Bug fix"
homeboy version bump my-plugin patch
homeboy deploy my-plugin --fleet production
```

**Check drift before deploy:**
```bash
homeboy fleet check production --outdated
# Shows which projects have older versions
```

## Docs

Full documentation at repo: `/tmp/homeboy/docs/`
- `docs/commands/` — Command reference
- `docs/schemas/` — Config schemas
- `docs/architecture/` — System design
