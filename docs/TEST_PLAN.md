# Safe Agent Test Plan

Status: Draft
Date: 2026-08-02

Related docs:

- `SPEC.md`
- `RFC.md`
- `local-agent-sandbox-research.md`

## Purpose

This document defines the behavior Safe Agent must prove before implementation is considered correct. Tests should be written against public seams, not private implementation details.

The first implementation should be test-first: add one failing test or probe for a behavior, implement the smallest slice to pass it, then continue.

## Test Seams

These are the public boundaries we should test.

### CLI Seam

Public interface:

```sh
safe-agent <command> [args...]
```

Use for tests that verify command parsing, user-visible output, exit codes, config paths, and command behavior.

Examples:

- `safe-agent init`
- `safe-agent status --json`
- `safe-agent policy validate`
- `safe-agent policy explain`
- `safe-agent run -- zsh -lc '<probe>'`

### Child Process Seam

Public interface:

```sh
safe-agent run -- <command>
```

Use for tests that verify what the child process can observe and do after Safe Agent launches it.

Examples:

- Child sees fake `HOME`.
- Child does not inherit host secrets.
- Child can edit allowed workspace files.
- Child cannot read protected files.

### Policy Seam

Public interface:

```sh
safe-agent policy validate
safe-agent policy explain ...
```

Use for tests that verify TOML config parsing, rule resolution, deny precedence, ask decisions, and explainable output.

### Supervisor Request Seam

Public interface:

```sh
safe-agent request <capability> ...
```

Use for tests that verify sandbox-side capability requests, approval decisions, denial messages, and default-deny behavior when the supervisor cannot be reached.

### Secret Management Seam

Public interface:

```sh
safe-agent secrets add <NAME> [VALUE]
safe-agent secrets doctor
safe-agent request secret <NAME> --for "<COMMAND>"
```

Use for tests that verify secret setup, secret mapping, one-command injection, redaction, and approval-gated agent-initiated secret writes.

### macOS Sandbox Probe Seam

Public interface:

```sh
safe-agent run -- zsh -lc '<probe-command>'
```

Use for end-to-end macOS behavior. These tests prove the actual OS boundary, not just policy logic.

## Test Tiers

### Unit Tests

Fast tests that do not invoke `sandbox-exec`.

Targets:

- CLI parsing.
- TOML schema validation.
- Policy resolution.
- Path canonicalization.
- Environment allowlist.
- Command classification.
- Redaction.

### Integration Tests

Tests that launch Safe Agent subprocesses but can use test doubles for Keychain and approvals.

Targets:

- Session directory creation.
- Generated shims.
- `safe-agent status --json`.
- Supervisor control socket.
- Approval request flow.
- Secret injection with fake backend.
- Audit summary.

### macOS Behavioral Probes

End-to-end tests that require macOS and `sandbox-exec`.

Targets:

- Filesystem enforcement.
- Environment scrubbing.
- Fake home.
- Network ask/deny behavior where feasible.
- Fail-closed sandbox launch.

These should be separately runnable because they depend on the host OS.

```sh
cargo test
cargo test --test macos_sandbox_probes -- --ignored
```

## MVP Test Matrix

### 1. CLI Initialization

Behavior:

```sh
safe-agent init
```

Expected:

- Creates `.safe-agent/policy.toml`.
- Uses TOML, not YAML.
- Does not write secret values.
- Refuses to overwrite existing policy without `--force`.
- Generated policy validates.

### 2. Status Outside Session

Behavior:

```sh
safe-agent status
safe-agent status --json
```

Expected:

- Human output says no Safe Agent session was detected.
- JSON output includes `in_session = false` or equivalent JSON boolean.
- Exit code is success.

### 3. Session Startup

Behavior:

```sh
safe-agent run -- zsh -lc 'pwd; echo "$HOME"; echo "$TMPDIR"; command -v safe-agent'
```

Expected:

- `pwd` is the workspace.
- `HOME` points to the session home.
- `TMPDIR` points to the session temp dir.
- `safe-agent` resolves to the generated session shim.
- Session startup fails closed if the sandbox profile cannot be generated.

### 4. Environment Scrubbing

Setup:

```sh
export SHOULD_NOT_LEAK=secret-value
```

Behavior:

```sh
safe-agent run -- zsh -lc 'printenv'
```

Expected:

- Output does not contain `SHOULD_NOT_LEAK`.
- Output does not contain common credential variables.
- Output contains only allowlisted runtime variables.
- `SSH_AUTH_SOCK`, `GPG_AGENT_INFO`, and `DYLD_*` are absent.

### 5. Live Repo Write

Behavior:

```sh
safe-agent run -- zsh -lc 'printf ok > safe-agent-probe.txt'
```

Expected:

- File is created in the real workspace.
- Existing dev-server-style file watching would observe the change.
- Session summary reports the changed file.

### 6. Repo `.env` Read Denied

Setup:

```sh
printf 'TOP_SECRET=1\n' > .env
```

Behavior:

```sh
safe-agent run -- zsh -lc 'cat .env'
```

Expected:

- Command fails.
- Secret value is not printed.
- Denial is logged.
- If a friendly denial message is available, it suggests `safe-agent request secret <NAME> --for "<COMMAND>"`.

### 7. Host Home Read Denied

Behavior:

```sh
safe-agent run -- zsh -lc 'cat "$REAL_HOME/.ssh/id_rsa"'
```

Implementation note:

- Test harness should pass the real home path through a non-secret test mechanism or use a known harmless file under the real home. Do not depend on an actual SSH key existing.

Expected:

- Command fails.
- File content is not printed.
- Denial is logged.

### 8. Persistence Path Write Denied

Behavior:

```sh
safe-agent run -- zsh -lc 'mkdir -p .git/hooks && printf x > .git/hooks/pre-commit'
```

Expected:

- Write fails.
- Hook file is not created or modified.
- Denial is logged.

### 9. Repo Policy Write Denied

Behavior:

```sh
safe-agent run -- zsh -lc 'printf "\n[network]\ndefault = \"allow\"\n" > .safe-agent/policy.toml'
```

Expected:

- Write fails or is approval-gated.
- Policy file is not silently modified by the agent.
- Denial explains that `.safe-agent/policy.toml` is protected because it controls sandbox authority.
- User-private config remains inaccessible.

### 10. Policy Validate

Behavior:

```sh
safe-agent policy validate
```

Expected:

- Valid TOML policy succeeds.
- Invalid TOML fails with file path and useful error.
- Unknown fields fail or warn according to schema strictness.
- YAML policy is ignored or rejected as unsupported.

### 11. Policy Explain

Behavior:

```sh
safe-agent policy explain path .env --action read
safe-agent policy explain network registry.npmjs.org
safe-agent policy explain secret DATABASE_URL --for "npm test"
```

Expected:

- `.env` read explains `deny`.
- Network explains `ask` by default.
- Secret explains `ask` if repo declares it and local mapping exists.
- Output includes matched rules and source files.
- Output never prints secret values.

### 12. Network Ask Default

Behavior:

```sh
safe-agent policy explain network example.com
```

Expected:

- Decision is `ask` under default policy.
- TOML can configure a destination to `deny`.
- TOML can configure a destination to `ask`.
- Dangerous ranges explain `deny` by default.

Behavior:

```sh
safe-agent policy explain network 169.254.169.254
safe-agent policy explain network 192.168.0.10
```

Expected:

- Decision is `deny`.

### 13. Network Deny, User TOML Change, Reload, Retry

Behavior:

Start with repo policy denying a host:

```toml
[network]
default = "ask"
deny = ["example.com"]
```

Inside a session:

```sh
safe-agent request network example.com --reason "test denied network"
```

Expected:

- Request is denied.
- Denial explains the matched TOML rule.
- No network grant is created.

Then the user edits `.safe-agent/policy.toml` outside the agent session:

```toml
[network]
default = "ask"
ask = ["example.com"]
```

Then the user or agent runs:

```sh
safe-agent policy reload
safe-agent request network example.com --reason "test allowed after policy change"
```

Expected:

- `policy reload` validates the changed TOML.
- `policy reload` shows a policy diff.
- Because this is a broadening change, the supervisor requires user approval before adopting it in the live session.
- After approval, the session policy updates.
- The second network request is ask-gated or allowed according to the new policy.
- The audit log records the original denial, policy reload, user approval, and later request.

Security expectation:

- Editing TOML alone does not silently change a running session's authority.
- Invalid TOML is rejected and the previous session policy remains active.
- The agent cannot silently edit `.safe-agent/policy.toml`.
- If an agent somehow triggers `policy reload`, broadening changes still require user approval.

### 14. Request Outside Session

Behavior:

```sh
safe-agent request network registry.npmjs.org --reason "install dependencies"
```

Expected:

- Fails clearly because there is no active session.
- Does not attempt to create a session.
- Does not grant anything.

### 15. Request Inside Session

Behavior:

```sh
safe-agent run -- zsh -lc 'safe-agent request network registry.npmjs.org --reason "install dependencies"'
```

Expected:

- Request reaches supervisor.
- Policy is evaluated.
- In noninteractive test mode, pending approvals default to deny unless an explicit test approval fixture is provided.
- Denial is recoverable and human-readable.

### 16. Secret Add Outside Session

Behavior:

```sh
safe-agent secrets add TEST_SECRET value --backend test
```

Expected:

- Stores secret in test backend.
- Updates user-private `secrets.toml` in test config home.
- Does not print the value.
- Warns that positional values may leak through shell history/process listings.

### 17. Agent-Initiated Secret Add Requires Approval

Behavior:

```sh
safe-agent run -- zsh -lc 'safe-agent secrets add TEST_SECRET value --backend test'
```

Expected:

- Supervisor treats this as durable state mutation.
- User approval is required.
- In unattended tests, default decision is deny.
- No secret is stored when denied.

### 18. One-Command Secret Injection

Setup:

- Test secret backend contains `DATABASE_URL=test-value`.
- Repo policy allows `DATABASE_URL` for `print-db-url-test-command`.

Behavior:

```sh
safe-agent run -- zsh -lc 'safe-agent request secret DATABASE_URL --for "print-db-url-test-command"'
```

Expected:

- With approval fixture, approved command receives `DATABASE_URL`.
- Agent shell does not receive `DATABASE_URL`.
- Unrelated commands do not receive `DATABASE_URL`.
- Captured output redacts the secret value.

### 19. Summary

Behavior:

```sh
safe-agent summary
```

Expected:

- Shows files changed since session start.
- Shows approvals and denials.
- Shows secret names used, never values.
- States that live-repo mode may include concurrent user edits.

## Test Harness Requirements

The test harness should support:

- Temporary workspaces.
- Temporary user config home.
- Test secret backend.
- Noninteractive approval fixtures.
- Real macOS sandbox probes behind an ignored/host-gated test target.
- Golden output tests for policy explanations and denial messages.

Suggested environment variables for tests only:

```txt
SAFE_AGENT_TEST_CONFIG_HOME=<temp-dir>
SAFE_AGENT_TEST_SECRET_BACKEND=memory
SAFE_AGENT_TEST_APPROVALS=<fixture-path>
```

These are test harness controls, not runtime session discovery variables for agents.

## First Red Tests

The first implementation cycle should start with these failing tests:

1. `status_outside_session_reports_not_in_session`
2. `init_writes_toml_policy`
3. `run_uses_fake_home_and_scrubbed_env`
4. `run_allows_workspace_write`
5. `run_denies_repo_env_read`
6. `run_denies_real_home_read`

These tests prove the minimum environment boundary before we invest in secrets, approvals, or docs.
