# Component Hooks System

Homeboy provides lifecycle hooks for components that execute shell commands at specific points during version management and release operations.

## Overview

Hooks allow components to run custom commands at defined points in the version and release lifecycle. Each hook type has specific execution semantics and failure behavior.

## Hook Types

### `pre_version_bump_commands`

Commands that run **before** version targets are updated.

**Execution context:**
- Working directory: Component's `local_path`
- Runs: Before version files are modified
- Order: Sequential (each command must complete before the next starts)

**Failure behavior:** **Fatal** - Non-zero exit code stops the version bump operation immediately.

**Use cases:**
- Build artifacts that include version information (e.g., `cargo build --release` updates Cargo.lock)
- Generate files that need to be staged alongside version changes
- Run validation checks before committing to a version bump

**Example:**
```json
{
  "pre_version_bump_commands": [
    "cargo build --release",
    "npm run generate-schema"
  ]
}
```

### `post_version_bump_commands`

Commands that run **after** version files have been updated but before any git operations.

**Execution context:**
- Working directory: Component's `local_path`
- Runs: After all version targets have been modified
- Order: Sequential

**Failure behavior:** **Fatal** - Non-zero exit code stops the version bump, leaving version files updated but uncommitted.

**Use cases:**
- Stage additional files that changed due to version bump (e.g., `git add Cargo.lock`)
- Run post-bump validation or linting
- Update dependent files that reference the version

**Example:**
```json
{
  "post_version_bump_commands": [
    "git add Cargo.lock",
    "npm run format"
  ]
}
```

### `post_release_commands`

Commands that run **after** the release pipeline completes (all publish steps finished).

**Execution context:**
- Working directory: Component's `local_path`
- Runs: After all release pipeline steps complete successfully
- Order: Sequential

**Failure behavior:** **Non-fatal** - Failures are logged as warnings but don't fail the release.

**Use cases:**
- Send notifications
- Trigger dependent deployments
- Cleanup temporary files
- Update external tracking systems

**Example:**
```json
{
  "post_release_commands": [
    "curl -X POST https://hooks.example.com/release-complete",
    "rm -rf tmp/"
  ]
}
```

## Execution Details

### Working Directory

All hooks execute in the component's `local_path` directory via `sh -c`. The working directory is set before command execution.

### Environment Variables

Hooks have access to standard shell environment. Module-specific environment variables (like `HOMEBOY_MODULE_PATH`) are NOT set for hooks.

### Command Format

Each command is a string executed via `sh -c`. Multi-command sequences should use shell operators:

```json
{
  "post_version_bump_commands": [
    "npm run lint && npm run test"
  ]
}
```

### Error Handling

For fatal hooks (`pre_version_bump_commands`, `post_version_bump_commands`):
- Non-zero exit code stops the operation
- `stderr` output is included in the error message
- No automatic rollback of previous steps

For non-fatal hooks (`post_release_commands`):
- Non-zero exit code logs a warning
- Pipeline continues and reports success
- All hook results are captured in release output

## Hook vs Release Pipeline Steps

| Feature | Hooks | Release Steps |
|---------|-------|---------------|
| Configuration | Component-level arrays | Release pipeline `steps` array |
| Dependencies | None (sequential) | `needs` field for DAG ordering |
| Failure handling | Fixed (fatal or non-fatal) | Configurable per step |
| Execution point | Fixed lifecycle points | Custom ordering |
| Use case | Simple shell commands | Complex orchestration |

**When to use hooks:** Simple, component-specific commands that always run at the same lifecycle point.

**When to use release steps:** Complex orchestration with dependencies, module integration, or custom failure handling.

## Configuration

Hooks are configured in the component JSON file:

```json
{
  "id": "my-component",
  "local_path": "/path/to/component",
  "pre_version_bump_commands": ["command1", "command2"],
  "post_version_bump_commands": ["command3"],
  "post_release_commands": ["command4"]
}
```

Set hooks via CLI:
```bash
homeboy component set my-component --json '{
  "pre_version_bump_commands": ["cargo build --release"],
  "post_version_bump_commands": ["git add Cargo.lock"]
}'
```

## Related

- [Component schema](../schemas/component-schema.md) - Full component configuration reference
- [Release pipeline](release-pipeline.md) - Configurable release orchestration
- [Version command](../commands/version.md) - Version bump operations
