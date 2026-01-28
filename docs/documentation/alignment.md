# Documentation Alignment

Instructions for keeping existing `.md` documentation synchronized with current codebase implementation.

## Scope

Alignment covers documentation that describes current implementation only. Planning documents (describing future work, architectural plans, etc.) are excluded.

This includes:
- CLAUDE.md / AGENTS.md files
- README.md files
- `/docs` directory contents
- API documentation in `.md` format
- Any other `.md` files in the codebase

## Core Rules

### Never Create New Directory Structures
Work within existing documentation structures only. You may create missing `.md` files within existing directories to fill gaps, but never create new top-level documentation directories.

### Never Modify Code
Documentation alignment is read-only with respect to code files. If you detect code issues, note them but do not modify code.

### Minimal Intervention
Only update what needs correction. Preserve accurate existing content. Do not rewrite documentation that is already correct.

## Workflow

### 1. Run Audit
```sh
homeboy docs audit <component>
```

This extracts claims from documentation (file paths, directory paths, code examples) and verifies them against the codebase.

### 2. Review Priority Docs
If the output includes `changes_context.priority_docs`, these docs reference recently changed files and should be reviewed first.

### 3. Execute Broken Tasks
For each task with `status: "broken"`:
- The `action` field tells you exactly what to do
- Usually: file/directory was moved or deleted, update the reference

### 4. Verify Code Examples
For tasks with `status: "needs_verification"`:
- Read the referenced code
- Verify the example matches current implementation
- Update if the API has changed

### 5. Re-run Audit
```sh
homeboy docs audit <component>
```
Confirm `broken` count is 0. Some `needs_verification` items are acceptable if code examples haven't changed.

## Forbidden Content

Never generate these during alignment:
- Installation guides or setup instructions
- Getting started tutorials
- Troubleshooting sections
- Configuration walkthroughs
- Generic workflow examples
- Version history or changelog content
- Marketing copy

## Forbidden Actions

- Never use `git checkout`, `git reset`, or any command that reverts code
- Never modify non-`.md` files
- Never revert or undo code changes
- Ignore uncommitted code changes - they represent active development

## Gap-Filling Within Existing Structures

When existing directories have coverage gaps:

1. **Analyze Structure**: Examine existing directory organization and naming patterns
2. **Identify Gaps**: Find codebase features without corresponding documentation
3. **Create Files**: Add missing `.md` files within existing directory structure
4. **Follow Patterns**: Match existing naming conventions and organizational hierarchy

**Allowed**: Creating `.md` files within existing `/docs` directory
**Forbidden**: Creating new top-level documentation directories

## Quality Gates

Before completion, verify:
- All documented features exist in current codebase
- All outdated information is corrected or removed
- Present-tense language throughout
- Cross-references are accurate and functional
- No new directory structures were created
