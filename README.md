# Safe Agent

Safe Agent is a macOS-first supervisor for running CLI coding agents with a constrained environment. It keeps live-repo editing, but gives the child process an ephemeral `HOME`, a scrubbed environment, repo-scoped Seatbelt access, protected policy and secret paths, and an approval/request protocol.

## Quick start

```sh
cargo build --release
./target/release/safe-agent init
./target/release/safe-agent run -- codex
```

Use `--backend none-for-debug` only for development of the wrapper itself. The default backend is `macos-seatbelt`, and it fails closed if the sandbox cannot launch.

The project policy lives at `.safe-agent/policy.toml`. User configuration and secret mappings live under `~/.config/safe-agent/`; secret values are stored in the macOS Keychain. Session homes are created under `/tmp`, are fresh for each run, and are removed by default.

## Development

```sh
cargo test
cargo fmt --check
```

The integration suite includes a real macOS Seatbelt probe. It verifies normal live-repo writes, `.env` protection, environment scrubbing, fake home behavior, requests, secret redaction, and durable summaries.

## Repository layout

- `crates/safe-agent-cli`: Rust CLI and supervisor
- `docs`: specification, RFC, research, and test plan
- `apps/docs`: documentation workspace placeholder for the future published docs site

This is local security tooling, not a claim of perfect malware containment. For high-risk repositories, use a VM-backed or quarantine workflow when those backends are added.

