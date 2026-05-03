# `homeboy daemon`

Run and inspect the local-only Homeboy HTTP API daemon.

## Synopsis

```sh
homeboy daemon <COMMAND>
```

## Subcommands

- `start` — start the local daemon in the background
- `serve` — run the daemon in the foreground
- `stop` — stop the background daemon recorded in the state file
- `status` — show daemon state and selected local address

## Local HTTP API

The daemon binds to loopback only. `homeboy daemon start` writes the selected
address and PID to the daemon state file so headless clients can discover it via
`homeboy daemon status`.

Always treat the API as a local UI contract. It is not a hosted or remote
multi-user service.

### Built-in Endpoints

- `GET /health` — daemon health and Homeboy version
- `GET /version` — Homeboy version
- `GET /config/paths` — local Homeboy config paths

### Read-Only Contract Endpoints

These endpoints dispatch through Homeboy's transport-free read-only HTTP API
contract and return the same JSON envelope shape as other daemon responses.

- `GET /components`
- `GET /components/:id`
- `GET /components/:id/status`
- `GET /components/:id/changes`
- `GET /rigs`
- `GET /rigs/:id`
- `POST /rigs/:id/check`
- `GET /stacks`
- `GET /stacks/:id`
- `POST /stacks/:id/status`
- `GET /runs?kind=bench|audit&component=<id>&rig=<id>&status=<status>&limit=<n>`
- `GET /runs/:id`
- `GET /runs/:id/artifacts`
- `GET /audit/runs?component=<id>&rig=<id>&status=<status>&limit=<n>`
- `GET /bench/runs?component=<id>&rig=<id>&status=<status>&limit=<n>`

The run readers expose persisted observation-store evidence from previous
analysis runs. They do not start audit, lint, test, bench, rig, or stack work.

The analysis entry points `POST /audit`, `POST /lint`, `POST /test`, and
`POST /bench` are reserved by the contract, but intentionally return a job-model
blocker until the long-running job/event API lands.

Mutating operations such as deploy, release, rig up/down, stack apply, git
writes, and SSH execution are not exposed by this daemon slice.

## Related

- [self](self.md)
- [status](status.md)
