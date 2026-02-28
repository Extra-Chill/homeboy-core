# Homeboy

Development and deployment automation CLI built in Rust — manage projects, servers, and fleets from the terminal.

## What It Does

Homeboy replaces scattered scripts, FTP clients, and manual SSH sessions with one tool:

- **Deploy anything** — Push plugins, themes, CLIs, and extensions to remote servers
- **Fleet management** — Group projects, detect shared components, deploy everywhere at once
- **Release pipelines** — Version bump, changelog, build, tag, publish — one command
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
| Deploy across fleet | `homeboy deploy my-plugin --fleet fleet-servers` |
| Release a new version | `homeboy release run my-plugin` |
| Check what's outdated | `homeboy deploy my-site --check --outdated` |
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
| `version` | Semantic version management |
| `changelog` | Add entries, finalize releases |
| `git` | Status, commit, push, pull with component awareness |
| `ssh` | Managed SSH connections |
| `file` | Remote file operations (list, read, write, find, grep) |
| `db` | Database queries, search, tunneling |
| `logs` | Remote log viewing and searching |
| `fleet` | Group projects, coordinated operations |
| `docs` | Embedded documentation, codebase auditing |
| `extension` | Install and manage extension extensions |

Run `homeboy docs commands/commands-index` for the full reference.

## For AI Agents

Homeboy is built for agentic workflows. Every command returns structured JSON, and embedded docs give agents full context without leaving the terminal.

**OpenClaw Skill:** Install `skills/homeboy/` for AI agents using OpenClaw.

**Agent Hooks:** Use [Agent Hooks](https://github.com/Extra-Chill/homeboy-extensions/tree/main/agent-hooks) to guide Claude Code or OpenCode to use Homeboy effectively.

```bash
homeboy docs list                      # Browse available topics
homeboy docs commands/deploy           # Deep dive on any command
homeboy docs scaffold my-component     # Analyze codebase for doc gaps
homeboy docs audit my-component        # Verify docs match code
```

## Extensions

Extensions extend Homeboy with project-type support — WordPress, Node.js, Rust, and more. Browse all available extensions at [homeboy-extensions](https://github.com/Extra-Chill/homeboy-extensions).

| Extension | Purpose |
|--------|---------|
| **wordpress** | WP-CLI integration, build, test, lint |
| **nodejs** | PM2 process management |
| **rust** | Cargo CLI integration |
| **github** | Issues, PRs, releases |
| **homebrew** | Tap publishing |
| **agent-hooks** | AI agent guardrails |

Install from the [homeboy-extensions](https://github.com/Extra-Chill/homeboy-extensions) monorepo:

```bash
# Install a extension by name
homeboy extension install https://github.com/Extra-Chill/homeboy-extensions --id wordpress

# List installed extensions
homeboy extension list

# Use a extension's commands
homeboy wp my-site plugin list         # WordPress via extension
```

Homeboy auto-detects monorepo layout — just pass `--id` with the extension name. For single-extension repos, `--id` is optional.

## Configuration

All config lives in `~/.config/homeboy/`:

```
~/.config/homeboy/
├── projects/          # Project definitions
├── servers/           # Server connections
├── components/        # Component definitions
├── extensions/           # Installed extensions
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
- [Homeboy Desktop](../homeboy-desktop/) — Native macOS app with a visual dashboard (early development)

## License

MIT License
Created by Chris Huber
https://chubes.net
