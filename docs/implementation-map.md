# Implementation Map

| Area | Code | Tests | Linear scope |
| --- | --- | --- | --- |
| CLI and session lifecycle | `crates/safe-agent-cli/src/cli.rs`, `session.rs` | `tests/cli.rs` | SAFE-4, SAFE-5, SAFE-6 |
| macOS enforcement | `session.rs` Seatbelt profile | Seatbelt integration probe | SAFE-3, SAFE-7, SAFE-8, SAFE-9 |
| TOML policy | `config.rs`, `policy.rs` | init, explain, reload tests | SAFE-10, SAFE-11, SAFE-12 |
| Requests and approvals | `session.rs` Unix socket server | request/reload tests | SAFE-13, SAFE-14 |
| Command mediation | generated session shims | session integration tests | SAFE-15 |
| Secrets | `secrets.rs`, supervisor execution path | test backend and redaction test | SAFE-16, SAFE-17, SAFE-18 |
| Audit | `audit.rs` | summary and changed-file tests | SAFE-19 |
| Network | policy decisions and Seatbelt default | policy and reload tests | SAFE-20 |
| Quarantine | disposable session workspace | quarantine integration test | SAFE-26 |
| Documentation and skills | `docs/`, `apps/docs/`, skill installer | docs check | SAFE-22, SAFE-23, SAFE-24, SAFE-29 |
| Future isolation | design spikes | host-specific probes | SAFE-21, SAFE-25, SAFE-27 |

