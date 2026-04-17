# GforceNode

Cross-platform node agent for Gforce on-prem infrastructure.

Runs on customer-owned machines (macOS, Linux, Windows) to enroll them as
Gforce infrastructure, report specs, send heartbeats, and execute dispatched
jobs from the Gforce control plane.

## Crates

- `node-core` — shared types, config, wire protocol
- `node-daemon` — long-running background service (heartbeat, control-plane WebSocket)
- `node-executor` — executes jobs dispatched by Gforce
- `node-cli` — user-facing `gforce-node` command (register, install, status)

## Install

```sh
curl -sSL https://gforce.nearminds.org/install.sh | TOKEN=<enrollment-token> sh
```

Windows support and MSI installer: in progress. See tracking issue.

## Relationship to Gforce

This agent is installed on customer-owned machines. It is a deliberately
separate codebase from [Gforce](https://github.com/nearminds/GForce) so that
customers can audit 100% of what runs in their environment.

The Gforce server side (enrollment, heartbeat ingestion, dispatch) lives in
the Gforce repo. This repo contains only the node agent.
