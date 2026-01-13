# Agent Instructions (homeboy-core)

> **Note**: For project-wide architecture and future refactor plans, see the root [`../CLAUDE.md`](../CLAUDE.md).

This crate embeds its CLI documentation from `docs/` into the `homeboy` binary.

Key references:

- Embedded docs topics map to paths under `docs/` without the `.md` extension (e.g. `docs/commands/deploy.md`  `commands/deploy`).
- Command output is machine-oriented and wrapped in a stable JSON envelope: [docs/json-output/json-output-contract.md](docs/json-output/json-output-contract.md).

When updating documentation, keep it concise and aligned with current implementation.
