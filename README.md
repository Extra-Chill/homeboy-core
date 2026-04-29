# Homeboy

Homeboy is a component-aware automation CLI for local development, CI, releases, dev rigs, stacks, benchmarks, dependency maintenance, triage, and ops. It runs the same workflows from your terminal or from GitHub Actions, and it can write stable JSON output so agents and scripts can consume results without scraping logs.

Homeboy is Chris Huber's working automation tool. It is designed to make the repos he maintains easier to develop, review, release, deploy, and operate from one binary.

## Operating Modes

Homeboy is not just a CI robot. The same component model drives several workflows:

```text
                         ┌─────────────────────────────┐
                         │        homeboy.json          │
                         │ component id + extensions   │
                         └──────────────┬──────────────┘
                                        │
        ┌───────────────────────────────┼───────────────────────────────┐
        │                               │                               │
        v                               v                               v
┌────────────────┐              ┌────────────────┐              ┌────────────────┐
│ Local terminal │              │ GitHub Actions │              │ Agent/runtime  │
│ audit/lint/test│              │ checks/release │              │ JSON artifacts │
└───────┬────────┘              └───────┬────────┘              └───────┬────────┘
        │                               │                               │
        v                               v                               v
┌────────────────┐              ┌────────────────┐              ┌────────────────┐
│ Dev substrate  │              │ Release/deploy │              │ Attention loop │
│ rig/stack/bench│              │ tag/changelog  │              │ triage/report  │
└────────────────┘              └────────────────┘              └────────────────┘
```

## Product Pillars

- **Code quality and review** — `audit`, `lint`, `test`, `review`, and `refactor` find drift, run project checks, produce PR-ready summaries, and apply safe mechanical fixes.
- **Release and changelog** — `release`, `version`, `changelog`, and `changes` derive version bumps and release notes from conventional commits instead of hand-edited version files.
- **Local dev environments** — `rig` materializes reproducible multi-component setups, while `stack` manages combined-fixes branches built from a base branch plus declared PRs.
- **Benchmarks** — `bench` runs extension-provided performance scenarios with baselines, ratchets, regression gates, and rig-scoped comparisons.
- **Dependencies and attention** — `deps`, `triage`, `status`, `issues`, and `report` surface dependency drift, open work, findings, and structured review artifacts.
- **Fleet and ops** — `deploy`, `ssh`, `file`, `db`, `logs`, and `transfer` manage servers and deployed projects from the same component/project/fleet model.
- **Extensions and platforms** — extensions add platform-specific behavior such as `wp` for WordPress and `cargo` for Rust without changing the core command contract.

For exhaustive command docs, see [docs/commands/commands-index.md](docs/commands/commands-index.md) or run `homeboy docs list`.

## Quick Start

### 1. Add `homeboy.json`

Put a portable component config at the repo root:

```json
{
  "id": "my-project",
  "extensions": {
    "rust": {}
  }
}
```

Use the extension that matches the repo. For example, Rust projects can use `rust`, and WordPress projects can use `wordpress`.

### 2. Run local quality gates

Start locally before wiring CI:

```bash
homeboy audit
homeboy lint
homeboy test
homeboy review
```

The same commands can also target a component explicitly or use a checkout path:

```bash
homeboy review my-project --changed-since origin/main
homeboy lint my-project --path /path/to/worktree
```

### 3. Add CI when the local loop is useful

`Extra-Chill/homeboy-action@v2` installs Homeboy in GitHub Actions, sets up extensions, runs commands, posts PR comments, and can apply safe autofixes.

```yaml
name: Homeboy
on: [pull_request]

jobs:
  quality:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: Extra-Chill/homeboy-action@v2
        with:
          extension: rust
          commands: audit,lint,test,review
```

Release automation can be manual, scheduled, or CI-driven. Use the same action when you want release jobs to run in GitHub Actions:

```yaml
name: Release
on:
  workflow_dispatch:

jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: Extra-Chill/homeboy-action@v2
        with:
          extension: rust
          component: my-project
          commands: release
```

## Core Workflows

### Code Quality And Review

`homeboy audit` detects convention drift, duplication, dead code, test coverage gaps, stale docs, and structural risk. Baselines let existing debt stay visible while preventing new regressions.

`homeboy lint` delegates formatting and static analysis to the configured extension. `homeboy test` runs the component's test suite. `homeboy review` is the PR-shaped umbrella that runs scoped audit, lint, and test checks and can render a PR-comment report.

```bash
homeboy audit --changed-since origin/main
homeboy lint --changed-only
homeboy test
homeboy review --changed-since origin/main --report pr-comment
```

`homeboy refactor` handles structural mechanical edits such as naming changes. Safe rewrites can be applied automatically; higher-risk plans stay explicit.

### Release And Changelog

Homeboy's release path is driven by conventional commits:

- `fix:` commits produce patch releases.
- `feat:` commits produce minor releases.
- `BREAKING CHANGE` produces major releases.
- `docs:`, `test:`, `ci:`, and `chore:` commits do not require a release by default.

The release family keeps version targets, changelogs, tags, and release artifacts tied to the same plan:

```bash
homeboy changes
homeboy version
homeboy changelog
homeboy release
```

### Local Dev Rigs And Stacks

`homeboy rig` manages reproducible local environments: components, background services, symlinks, checks, git operations, builds, and patch steps. A rig can bring up a multi-repo setup, check its health, sync stacks, and tear it down.

```bash
homeboy rig install https://github.com/chubes4/homeboy-rigs.git//packages/studio
homeboy rig up studio
homeboy rig check studio
homeboy rig sync studio
homeboy rig down studio
```

`homeboy stack` manages combined-fixes branches built from a base branch plus declared PRs. It can inspect, apply, rebase, sync, diff, and push the target branch.

```bash
homeboy stack status studio-combined
homeboy stack sync studio-combined
homeboy stack push studio-combined
```

### Benchmarks

`homeboy bench` makes benchmarks a first-class quality gate. Extensions provide the benchmark runner; Homeboy owns baseline storage, regression policies, ratchets, and rig-scoped comparisons.

```bash
homeboy bench my-project --baseline
homeboy bench my-project --ratchet
homeboy bench my-project --rig studio
```

### Dependencies And Attention

`homeboy deps` inspects and updates component dependencies. `homeboy triage` gathers read-only attention reports across components, projects, fleets, rigs, or the whole workspace.

```bash
homeboy deps status my-project
homeboy deps update vendor/package my-project --to '^1.2'
homeboy status my-project
homeboy triage workspace --mine --failing-checks
```

`homeboy issues` reconciles findings against an issue tracker, and `homeboy report` renders structured output artifacts such as review results.

### Fleet And Ops

Homeboy models deployed work as components attached to projects, projects attached to servers, and projects grouped into fleets.

```text
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ component   │────>│ project     │────>│ server      │
│ plugin/CLI  │     │ site/app    │     │ VPS/host    │
└─────────────┘     └──────┬──────┘     └─────────────┘
                           │
                           v
                    ┌─────────────┐
                    │ fleet       │
                    │ group       │
                    └─────────────┘
```

That model backs ops commands:

```bash
homeboy deploy my-project
homeboy ssh production
homeboy logs production
homeboy db query production "select 1"
homeboy file read production /path/to/file
homeboy transfer production staging /path/to/file
```

## Command Surface Map

This is a map, not a generated reference. Use `homeboy <command> --help` or [docs/commands/commands-index.md](docs/commands/commands-index.md) for exact options.

| Area | Commands |
|------|----------|
| Code quality / review | `audit`, `lint`, `test`, `review`, `refactor`, `build`, `validate` |
| Release / changelog | `release`, `version`, `changelog`, `changes` |
| Local dev environments | `rig`, `stack` |
| Benchmarks | `bench` |
| Dependencies / attention | `deps`, `triage`, `status`, `issues`, `report` |
| Fleet / ops | `deploy`, `ssh`, `file`, `db`, `logs`, `transfer`, `server`, `project`, `component`, `fleet` |
| Extensions / platform verbs | `extension`, `wp`, `cargo` |
| Meta | `config`, `docs`, `daemon`, `self`, `undo`, `auth`, `api`, `upgrade`, `list` |

## JSON Output Contract

Most Homeboy commands can write machine-readable output with `--output <path>` while still printing human output to stdout.

```bash
homeboy review --changed-since origin/main --output homeboy-ci-results/review.json
homeboy bench my-project --output homeboy-ci-results/bench.json
homeboy triage workspace --output homeboy-ci-results/triage.json
```

The output file contains command-specific JSON only, without log text. That makes Homeboy suitable for CI jobs, scheduled automation, and AI agents that need stable artifacts instead of terminal scraping.

## Extensions

Extensions add platform-specific behavior while keeping the Homeboy command model consistent.

| Extension | What it provides |
|-----------|------------------|
| `rust` | Cargo integration, Rust build/test/lint/release behavior, and crates.io artifacts. |
| `wordpress` | WP-CLI integration, WordPress-aware build/test/lint/release behavior. |
| `nodejs` | Node/PM2 process management. |
| `github` | GitHub release and issue/PR integration. |
| `homebrew` | Homebrew tap publishing. |
| `swift` | Swift testing for macOS/iOS projects. |

```bash
homeboy extension install https://github.com/Extra-Chill/homeboy-extensions --id rust
homeboy extension list
```

Installed extensions can expose their own top-level verbs, such as `homeboy wp` and `homeboy cargo`.

## Configuration

Global config lives in `~/.config/homeboy/`. Portable component config lives in `homeboy.json` at the repository root.

```text
~/.config/homeboy/
├── homeboy.json       # global defaults
├── components/        # registered component definitions
├── projects/          # project definitions and server bindings
├── servers/           # server connections
├── fleets/            # project groups
├── rigs/              # local dev rig specs
├── stacks/            # stack specs
├── extensions/        # installed extensions
└── keys/              # SSH keys
```

The portable repo-level `homeboy.json` is enough for local commands and CI. Global component/project/server/fleet config is for reusable local and ops workflows.

## Installation

```bash
# Homebrew (macOS/Linux)
brew tap Extra-Chill/homebrew-tap
brew install homeboy

# Cargo
cargo install homeboy

# From source
git clone https://github.com/Extra-Chill/homeboy.git
cd homeboy && cargo install --path .
```

## Documentation

Command reference and deeper docs are checked into this repo and embedded in the binary.

```bash
homeboy docs list
homeboy docs commands/commands-index
homeboy docs code-factory
homeboy docs schemas/component-schema
```

Start with [docs/commands/commands-index.md](docs/commands/commands-index.md) for the command index.

## License

MIT License — Created by [Chris Huber](https://chubes.net)
