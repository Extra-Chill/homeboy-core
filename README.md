# Homeboy

Opinionated CLI for managing codebases at scale — versioning, refactoring, auditing, deploying.

## What It Does

Homeboy replaces scattered scripts, FTP clients, and manual SSH sessions with one tool:

- **Deploy anything** — Push plugins, themes, CLIs, and extensions to remote servers
- **Fleet management** — Group projects, detect shared components, deploy everywhere at once
- **Release pipelines** — Version bump, changelog, build, tag, publish — one command
- **Structural refactoring** — Rename terms across a codebase with case-variant awareness and collision detection
- **Code auditing** — Discover conventions from your codebase and flag drift automatically
- **Remote operations** — SSH, file management, database queries, log tailing
- **Structured output** — JSON for scripting and AI agents, human-readable for terminals

## How It Works

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  COMPONENT  │ ──▶ │   PROJECT   │ ──▶ │   SERVER    │
│  Plugin,    │     │  Site or    │     │  VPS, host, │
│  theme, CLI │     │  application│     │  cloud...   │
└─────────────┘     └─────────────┘     └─────────────┘
                          │
                    ┌─────┴─────┐
                    │   FLEET   │
                    │  Group of │
                    │  projects │
                    └───────────┘
```

**Components** are buildable/deployable units. **Projects** are deployment targets. **Servers** are machines. **Fleets** group projects for batch operations.

## Example Workflows

| Workflow | Commands |
|----------|----------|
| Deploy a plugin | `homeboy deploy my-site my-plugin` |
| Deploy to all sites | `homeboy deploy my-plugin --shared` |
| Fleet rollout | `homeboy deploy my-plugin --fleet production` |
| Release a new version | `homeboy release run my-plugin` |
| Rename a term | `homeboy refactor rename --from widget --to gadget -c my-plugin` |
| Rename exact string | `homeboy refactor rename --literal --from old-slug --to new-slug --path .` |
| Check what's outdated | `homeboy deploy my-site --check --outdated` |
| Audit docs vs code | `homeboy docs audit my-component` |
| Tail remote logs | `homeboy logs show my-site error.log --follow` |
| Query remote DB | `homeboy db query my-site "SELECT * FROM wp_posts LIMIT 5"` |

## Quick Start

```bash
# Discover your environment
homeboy init

# Set up infrastructure
homeboy server create my-vps --host 1.2.3.4 --user root
homeboy project create my-site example.com --server my-vps
homeboy component create my-plugin --local-path ./my-plugin --remote-path wp-content/plugins/my-plugin

# Link and deploy
homeboy project components add my-site my-plugin
homeboy deploy my-site my-plugin
```

## Core Commands

| Command | Purpose |
|---------|---------|
| `init` | Environment discovery and setup guidance |
| `deploy` | Push components to projects/fleets |
| `release` | Version bump → changelog → build → tag → publish |
| `refactor` | Structural refactoring (rename terms across codebase) |
| `version` | Semantic version management |
| `changelog` | Add entries, finalize releases |
| `git` | Status, commit, push, pull with component awareness |
| `ssh` | Managed SSH connections |
| `file` | Remote file operations (list, read, write, find, grep) |
| `db` | Database queries, search, tunneling |
| `logs` | Remote log viewing and searching |
| `fleet` | Group projects, coordinated operations |
| `docs` | Embedded documentation, codebase auditing |
| `extension` | Install and manage extensions |

Run `homeboy docs commands/commands-index` for the full reference.

## Refactoring

The `refactor rename` command finds and replaces terms across a codebase with automatic case-variant generation and word-boundary awareness.

```bash
# Dry run (default) — preview what would change
homeboy refactor rename --from widget --to gadget --path .

# Apply changes
homeboy refactor rename --from widget --to gadget --path . --write

# Literal mode — exact string match, no boundary detection
homeboy refactor rename --literal --from old-slug --to new-slug --path . --write
```

**Standard mode** generates case variants automatically:
- `widget` → `gadget` (lowercase)
- `Widget` → `Gadget` (PascalCase)
- `WIDGET` → `GADGET` (UPPER_CASE)
- `widgets` → `gadgets` (plural)
- Snake_case compounds like `load_widget` → `load_gadget`

**Literal mode** (`--literal`) matches the exact string with no boundary detection or case variants. Useful for compound renames like `datamachine-events` → `data-machine-events` where inserting characters breaks boundary rules.

Both modes include **collision detection** — warnings when a rename would create duplicate identifiers or overwrite existing files.

## For AI Agents

Homeboy is built for agentic workflows. Every command returns structured JSON, and embedded docs give agents full context without leaving the terminal.

**Agent Hooks:** Install [Agent Hooks](https://github.com/Extra-Chill/homeboy-extensions/tree/main/agent-hooks) to guide Claude Code or OpenCode to use Homeboy effectively.

```bash
homeboy docs list                      # Browse available topics
homeboy docs commands/deploy           # Deep dive on any command
homeboy docs scaffold my-component     # Analyze codebase for doc gaps
homeboy docs audit my-component        # Verify docs match code
```

## Extensions

Extensions add project-type support — WordPress, Node.js, Rust, and more. Browse all available extensions at [homeboy-extensions](https://github.com/Extra-Chill/homeboy-extensions).

| Extension | Purpose |
|-----------|---------|
| **wordpress** | WP-CLI integration, build, test, lint |
| **nodejs** | PM2 process management |
| **rust** | Cargo CLI integration |
| **github** | Issues, PRs, releases |
| **homebrew** | Tap publishing |
| **agent-hooks** | AI agent guardrails |

```bash
# Install an extension by name
homeboy extension install https://github.com/Extra-Chill/homeboy-extensions --id wordpress

# List installed extensions
homeboy extension list

# Use extension commands
homeboy wp my-site plugin list         # WordPress via extension
```

Extensions support **versioning** with constraint matching (`^1.0`, `>=2.0`, `~1.2`), **auto-update checks** on startup, and **language extractors** for fingerprinting project files.

## Configuration

All config lives in `~/.config/homeboy/`:

```
~/.config/homeboy/
├── projects/          # Project definitions
├── servers/           # Server connections
├── components/        # Component definitions
├── extensions/        # Installed extensions
├── keys/              # SSH keys
└── homeboy.json       # Global defaults
```

No repo-local config files. Everything is centralized.

## Installation

```bash
# Homebrew (macOS/Linux)
brew tap Extra-Chill/homebrew-tap
brew install homeboy

# From source (requires Rust toolchain)
git clone https://github.com/Extra-Chill/homeboy.git
cd homeboy && cargo install --path .
```

## Documentation

- `homeboy docs list` — Browse all embedded topics
- `homeboy docs commands/commands-index` — Full command reference
- [docs/](docs/) — Detailed documentation
- [homeboy-extensions](https://github.com/Extra-Chill/homeboy-extensions) — Public extensions

## License

MIT License
Created by Chris Huber
https://chubes.net
