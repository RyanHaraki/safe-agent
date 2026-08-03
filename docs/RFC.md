# RFC: Safe Agent Implementation Plan

Status: Draft
Date: 2026-08-02

Related docs:

- `SPEC.md`
- `TEST_PLAN.md`
- `local-agent-sandbox-research.md`

## Summary

Safe Agent will be implemented as a trusted local supervisor that launches an untrusted coding agent inside a constrained macOS runtime.

The MVP is native macOS, Dockerless, live-repo, ephemeral-state, terminal-first, and agent-agnostic. It should work with Codex, Claude, Aider, Gemini, or any other CLI coding agent because it wraps the process from underneath instead of requiring an agent-specific plugin.

The first implementation should prioritize the actual environment boundary:

- Fake `HOME`.
- Scrubbed environment.
- Generated `sandbox-exec`/Seatbelt profile.
- Repo-scoped filesystem access.
- Denied `.env` and host credential paths.
- Supervisor-owned session directory.
- Terminal-native approvals.
- CLI-based capability requests through `safe-agent request`.
- Basic command shims.
- Keychain-backed one-command secret injection.
- Session audit summary.

Network proxying, policy signing, VM mode, GUI approvals, reusable caches, and hosted skill distribution are later phases.

## Current Decisions

1. Native macOS mode comes first.
2. Live repo editing is the default.
3. Quarantine mode is deferred.
4. Each session uses a fresh ephemeral home and temp directory.
5. Safe Agent-specific environment variables are not required.
6. Session discovery happens through a `safe-agent` shim in `PATH`.
7. Terminal-native approvals are the MVP approval surface.
8. Secrets are brokered by the supervisor and injected only into approved subprocesses.
9. Repo policy can request capabilities but cannot silently grant dangerous host access.
10. Enforcement must fail closed if the selected sandbox backend cannot start.

## Architecture

```txt
host terminal
  safe-agent supervisor
    loads policy
    creates session dir
    creates fake HOME
    creates command shims
    starts control socket
    starts approval loop
    generates sandbox profile
    launches child agent in PTY

  untrusted child process
    codex / claude / aider / gemini / shell
    cwd = target repo
    HOME = session home
    PATH = session shims + approved tools
    env = scrubbed allowlist
    filesystem = sandboxed
    network = ask-gated or mediated
```

The supervisor is the trusted control plane. The child process is untrusted, even if the agent is useful and usually well-behaved.

Security boundaries must not depend on agent cooperation. Skills, startup briefings, and denial messages improve behavior, but the OS sandbox and supervisor policy are the hard boundary.

## Major Components

### CLI Entrypoint

The main binary exposes:

```sh
safe-agent run <agent-command> [args...] [--workspace <path>]
safe-agent init
safe-agent status
safe-agent status --json
safe-agent request <capability> ...
safe-agent policy validate
safe-agent policy explain
safe-agent policy diff
safe-agent policy reload
safe-agent secrets add <NAME> [VALUE]
safe-agent secrets doctor
safe-agent summary
safe-agent skills install
```

Implementation notes:

- `safe-agent run` is the host-side supervisor command.
- `safe-agent status`, `request`, and `summary` must also work from inside the sandbox.
- Inside the sandbox, `safe-agent` resolves to a generated shim in the session `bin` directory.
- The shim calls the real binary with the current session's control socket path.
- Outside a session, `safe-agent status` returns a clear "not in a Safe Agent session" response.

The generated shim avoids Safe Agent-specific environment variables:

```sh
#!/bin/sh
exec /path/to/real/safe-agent --session-socket /tmp/safe-agent/sessions/<run-id>/control.sock "$@"
```

The socket path is not treated as a secret. Requests still require policy checks and user approval.

Command reference:

#### `safe-agent run`

Starts a sandboxed agent session.

```txt
safe-agent run [OPTIONS] -- <agent-command> [agent-args...]
safe-agent run [OPTIONS] <agent-command> [agent-args...]
```

Context:

- Host-side only.
- This is the command the user runs from a normal terminal.
- It starts the trusted supervisor and launches the untrusted child agent in a PTY.

Inputs:

- Agent command and args.
- Workspace path.
- Optional profile, policy file, backend, and network mode.

Side effects:

- Creates a fresh session directory under `/tmp/safe-agent/sessions/<run-id>/`.
- Creates session `home/`, `tmp/`, `bin/`, `logs/`, `policy/`, and `state/`.
- Generates the `safe-agent` shim and command shims.
- Builds a scrubbed child environment.
- Generates a Seatbelt profile.
- Opens the supervisor control socket.
- Captures a pre-run workspace checkpoint.
- Launches the child process.
- Cleans ephemeral state on exit unless debug flags preserve it.

Output:

- Prints the resolved workspace, profile, backend, and safety summary.
- Streams the child agent terminal session.
- Prints a session summary on exit.
- Returns the child process exit code unless Safe Agent itself fails first.

MVP options:

```txt
-w, --workspace <PATH>
    Project directory to expose as the workspace.
    Default: current working directory.

-p, --profile <NAME>
    Policy profile to use from repo or built-in config.
    Default: repo-coder.

--policy <PATH>
    Extra TOML policy file to load for this run.
    Default: <workspace>/.safe-agent/policy.toml if present.

--no-repo-policy
    Ignore <workspace>/.safe-agent/policy.toml for this run.

--backend <NAME>
    Enforcement backend.
    MVP value: macos-seatbelt.
    Future values: vm, none-for-debug.

--network <MODE>
    Initial network mode.
    MVP values: deny, ask.
    Default: profile-dependent.

--keep-logs
    Preserve durable audit logs after exit.

--keep-session
    Preserve the ephemeral session directory for debugging.
    This should print a warning because preserved session homes can contain scratch state.

--dry-run
    Resolve config and policy, print the session plan, and exit without launching a child.
```

Deferred options:

```txt
--quarantine
    Run in a copied/snapshot workspace instead of editing the live repo.

--strict
    Use a stricter backend/profile, eventually VM-backed.

--allow <CAPABILITY>
    Pre-grant a narrow capability for this session.

--deny <CAPABILITY>
    Add a session-level deny override.

--timeout <DURATION>
    Stop the session after a duration.

--approval-timeout <DURATION>
    Default-deny pending approval prompts after a timeout.

--json
    Emit machine-readable session start/end events.
```

Examples:

```sh
safe-agent run codex .
safe-agent run claude .
safe-agent run aider --workspace ~/projects/my-app
safe-agent run --workspace . -- codex --model gpt-5
safe-agent run -- zsh
```

Parsing rule:

- Use `--` before the agent command when the agent command has flags that could be confused with Safe Agent flags.

#### `safe-agent init`

Creates starter project policy.

```txt
safe-agent init [--workspace <PATH>] [--force]
```

Context:

- Host-side.
- Does not require an active session.

Inputs:

- Workspace path, defaulting to current working directory.

Side effects:

- Creates `<workspace>/.safe-agent/` if needed.
- Writes `<workspace>/.safe-agent/policy.toml` if it does not exist.
- With `--force`, may overwrite after confirmation.
- Never writes secret values.

Output:

- Shows the created file path.
- Summarizes the generated default profile.
- Prints next steps such as `safe-agent policy validate` and `safe-agent run codex .`.

Failure cases:

- Refuses to overwrite existing policy unless explicitly forced.
- Fails if workspace cannot be canonicalized.

#### `safe-agent status`

Shows whether the current process is inside a Safe Agent session.

```txt
safe-agent status [--json]
```

Context:

- Host-side and sandbox-side.
- Inside the sandbox, this is usually invoked through the generated `safe-agent` shim.
- Outside a session, it should work without error and report "not in a Safe Agent session."

Inside-session output:

- Session ID.
- Workspace.
- Profile.
- Backend.
- Network mode.
- Fake home path.
- Policy sources.
- Available request commands.
- Supervisor connection health.

Outside-session output:

```txt
Not in a Safe Agent session.
```

`--json` output:

- Same data in a stable machine-readable shape for agents, scripts, and future skills.
- Should include a `in_session: true | false` field.
- Should not require Safe Agent-specific environment variables.

#### `safe-agent request`

Requests a capability from the supervisor.

```txt
safe-agent request secret <NAME> --for "<COMMAND>"
safe-agent request network <HOST> [--port <PORT>] [--reason "<REASON>"]
safe-agent request filesystem-read <PATH> --reason "<REASON>"
safe-agent request filesystem-write <PATH> --reason "<REASON>"
safe-agent request command "<COMMAND>" --reason "<REASON>"
```

Context:

- Primarily sandbox-side.
- Can be used host-side for testing only when pointed at a session socket.

Inputs:

- Capability type.
- Resource name/path/host/command.
- Reason supplied by the agent.
- Optional requested scope, such as once or session.

Side effects:

- Sends a structured request to the supervisor over the control socket.
- May trigger a terminal approval prompt.
- May create a session-scoped approval or denial.
- Does not directly grant anything without supervisor policy evaluation.

Output when allowed:

```txt
safe-agent: request allowed

Capability: network
Resource: registry.npmjs.org
Scope: this session
```

Output when denied:

```txt
safe-agent: request denied

Capability: secret
Resource: DATABASE_URL
Reason: user denied
Alternative: continue without this secret, use a mock, or ask the user for another approach.
```

Failure cases:

- Fails clearly if not inside a session.
- Fails clearly if the capability type is unknown.
- Defaults to deny if the supervisor cannot be reached.

#### `safe-agent policy validate`

Validates policy and config files.

```txt
safe-agent policy validate [--workspace <PATH>] [--policy <PATH>] [--user-config <PATH>]
```

Context:

- Host-side.
- Does not require an active session.

Inputs:

- Built-in defaults.
- Repo policy at `<workspace>/.safe-agent/policy.toml`.
- User config at `~/.config/safe-agent/config.toml`.
- Secret mappings at `~/.config/safe-agent/secrets.toml`.
- Optional explicit policy path.

Side effects:

- None, except reading config files.

Output:

- Success summary when valid.
- Actionable errors with file path, line/field when available, and reason.
- Warnings for ignored repo grants that cannot grant authority by themselves.

Failure cases:

- Invalid TOML.
- Unknown fields.
- Unsupported policy version.
- Invalid path or capability pattern.

#### `safe-agent policy explain`

Explains how Safe Agent would decide a request.

```txt
safe-agent policy explain path <PATH> --action read|write
safe-agent policy explain network <HOST> [--port <PORT>]
safe-agent policy explain secret <NAME> --for "<COMMAND>"
safe-agent policy explain command "<COMMAND>"
```

Context:

- Host-side and sandbox-side.
- Debugging tool for users, agents, and policy authors.

Inputs:

- A hypothetical request.
- Current workspace and policy context.

Side effects:

- None.
- Does not request approval.
- Does not read or reveal secret values.

Output:

- Final decision: allow, ask, or deny.
- Request details.
- Matched built-in, repo, user, and session rules.
- Source files for matched rules.
- Explanation of why repo policy did or did not become a grant.
- Suggested alternative when denied.

Example:

```sh
safe-agent policy explain path .env --action read
safe-agent policy explain network registry.npmjs.org
safe-agent policy explain secret DATABASE_URL --for "npm test"
```

#### `safe-agent policy diff`

Shows policy changes that affect trust.

```txt
safe-agent policy diff [--workspace <PATH>] [--since-session <ID>]
```

Context:

- Host-side.
- Later phases may also compare against signed team policy.

Inputs:

- Current repo policy.
- Policy snapshot from active or most recent session.
- Future: trusted/signed baseline.

Side effects:

- None.

Output:

- Added, removed, or changed policy rules.
- Highlighted broadening changes such as new network hosts, new secret requests, new write paths, or changed command permissions.
- Clear warning when policy changed during an active session.

MVP behavior:

- Compare current repo policy against the snapshot captured at session start.

#### `safe-agent policy reload`

Validates and adopts changed policy during an active session.

```txt
safe-agent policy reload [--workspace <PATH>]
```

Context:

- Primarily sandbox-side or host-side while a session is active.
- Requires an active session.

Inputs:

- Current repo policy.
- Current user config.
- Session policy snapshot.

Side effects:

- Re-reads policy TOML.
- Validates the new effective policy.
- Shows a policy diff when repo policy changed.
- Asks the user to confirm broadening changes before adopting them.
- Updates the session policy snapshot if accepted.

Output:

- Validation result.
- Policy diff summary.
- Whether the live session policy was updated.

MVP behavior:

- Narrowing changes can be adopted automatically after validation.
- Broadening changes, such as allowing a previously denied network host, require user confirmation.
- If invoked by the agent inside the sandbox, the user must approve any broadening change before it affects the session.

#### `safe-agent secrets add`

Stores or updates a user-private secret source.

```txt
safe-agent secrets add <NAME> [VALUE] [--project <PATH>] [--backend keychain] [--stdin]
```

Context:

- Host-side by default.
- Sandbox-side only through the generated shim, and only as an approval-gated durable-state operation.
- If an agent invokes this command during a session, the supervisor must ask the user before storing or updating anything.

Inputs:

- Secret name.
- Optional secret value.
- If `VALUE` is omitted, prompt for the value using hidden input.
- If `--stdin` is provided, read the value from stdin.
- Project path, defaulting to current workspace.

Side effects:

- Stores the secret value in macOS Keychain under a Safe Agent namespace.
- Updates `~/.config/safe-agent/secrets.toml` with the source URI.
- Creates `~/.config/safe-agent/` if needed.
- If run from inside a Safe Agent session, records the approval and audit event.

Output:

- Confirms the secret name and source URI.
- Never prints the secret value.
- Warns when `VALUE` was passed as a positional argument because it may be captured by shell history or process listings.

Examples:

```sh
safe-agent secrets add DATABASE_URL
safe-agent secrets add OPENAI_API_KEY "sk-..."
printf '%s' "$STRIPE_SECRET_KEY" | safe-agent secrets add STRIPE_SECRET_KEY --stdin
```

Failure cases:

- Refuses invalid secret names.
- Fails if Keychain is unavailable or the item cannot be written.
- Defaults to deny if invoked by an agent and the user does not approve.
- Refuses to overwrite an existing secret unless the user confirms or an explicit overwrite flag is added in a later phase.

#### `safe-agent secrets doctor`

Checks local secret setup without revealing values.

```txt
safe-agent secrets doctor [--workspace <PATH>]
```

Context:

- Host-side.
- Does not require an active session.

Inputs:

- Repo policy secret declarations.
- User-private `secrets.toml`.
- Configured secret backend metadata.

Side effects:

- None by default.
- May trigger normal Keychain access checks, but must not print values.

Output:

- Secrets declared by repo policy.
- Whether each has a local mapping.
- Whether each mapped backend item is reachable.
- Backend health.
- Missing or stale mappings.

#### `safe-agent summary`

Shows session audit information.

```txt
safe-agent summary [--session <ID>] [--json]
```

Context:

- Host-side and sandbox-side.
- During a session, shows live session state.
- After a session, shows the most recent session by default.

Inputs:

- Session ID or default recent session pointer.

Side effects:

- None.

Output:

- Files changed since session start.
- Commands mediated by shims.
- Approvals granted.
- Denials.
- Secret names used, never values.
- Policy hash and policy-change warnings.
- Note that live-repo mode may include concurrent user edits.

#### `safe-agent skills install`

Installs optional agent instructions.

```txt
safe-agent skills install [--workspace <PATH>] [--source <SOURCE>]
```

Context:

- Host-side.
- Deferred from MVP.

Inputs:

- Workspace.
- Future source such as `npx skills` or Vercel Skills.

Side effects:

- Downloads or updates a repo-local Safe Agent skill/instruction pack.
- Does not change enforcement policy.
- Does not grant permissions.

Output:

- Installed instruction files.
- Which agents are likely to consume them.
- Reminder that skills are convenience only, not a security boundary.

### Supervisor

The supervisor owns:

- Session lifecycle.
- Policy loading and resolution.
- Sandbox profile generation.
- PTY child process management.
- Approval prompts.
- Secret access.
- Audit logs.
- Control socket RPC.
- Exit cleanup.

The supervisor must keep running for the lifetime of the agent session.

Expected run flow:

```txt
1. Parse CLI args.
2. Resolve and canonicalize workspace.
3. Load built-in defaults.
4. Load repo policy if present.
5. Load user policy if present.
6. Resolve effective policy.
7. Capture pre-run workspace checkpoint.
8. Create session directory.
9. Create fake HOME, TMPDIR, logs, policy snapshot, and shims.
10. Start supervisor control socket.
11. Generate Seatbelt profile.
12. Launch child agent in sandboxed PTY.
13. Proxy terminal IO between user and child.
14. Handle shim requests and approvals while child runs.
15. On exit, write session summary and clean ephemeral state.
```

### Session Directory

Each run creates a fresh directory:

```txt
/tmp/safe-agent/sessions/<run-id>/
  bin/
  home/
  tmp/
  logs/
  policy/
  state/
  control.sock
  seatbelt.sb
  session.json
```

Permissions:

- Session root should be `0700`.
- `home/`, `tmp/`, and selected log paths are writable by the child.
- `bin/`, `policy/`, `seatbelt.sb`, and supervisor-owned metadata should be readable but not writable by the child.
- `control.sock` accepts requests from the child, but all sensitive operations remain policy/approval gated.

Cleanup:

- Ephemeral session state is deleted after the session by default.
- Audit summaries may be copied to a durable user-controlled log path if configured.
- The MVP does not reuse homes or shared caches.

### Environment Builder

The child process is launched with an explicit allowlist.

Allowed by default:

```txt
HOME=<session>/home
TMPDIR=<session>/tmp
PATH=<session>/bin:<approved-system-paths>
TERM=<host TERM or xterm-256color>
LANG=<safe locale>
SHELL=/bin/zsh or /bin/sh
```

Denied by default:

- User shell variables.
- API keys.
- Cloud credentials.
- Package manager tokens.
- `SSH_AUTH_SOCK`.
- `GPG_AGENT_INFO`.
- `DYLD_*`.
- Anything not explicitly allowlisted.

The environment builder should be small, deterministic, and covered by snapshot tests. It should never inherit the host environment wholesale.

### macOS Sandbox Backend

The native backend uses `sandbox-exec` with a generated Seatbelt profile.

Backend responsibilities:

- Deny default where practical.
- Allow read/write access to the workspace except protected paths.
- Allow read/write access to session `home/` and `tmp/`.
- Allow read/execute access to required system binaries, libraries, shells, and developer tools.
- Deny reads of host home and credential paths.
- Deny reads of repo `.env` and `.env.*`.
- Deny writes to persistence paths.
- Deny or approval-gate writes to repo policy at `.safe-agent/policy.toml`.
- Deny or tightly mediate network access.
- Fail closed if profile generation or sandbox launch fails.

Protected path examples:

```txt
<workspace>/.env
<workspace>/.env.*
<workspace>/.safe-agent/policy.toml
<workspace>/.git/hooks/**
<workspace>/.safe-agent/trusted/**
<real-home>/**
<real-home>/.ssh/**
<real-home>/.aws/**
<real-home>/.config/**
<real-home>/Library/Keychains/**
<real-home>/Library/Application Support/**
```

The exact Seatbelt profile syntax should be treated as security-critical implementation detail. The first backend phase should build behavioral probes before relying on the profile.

Required probe cases:

- Can read/write normal workspace files.
- Cannot read workspace `.env`.
- Cannot read real home files.
- Cannot write `.safe-agent/policy.toml`.
- Cannot write `.git/hooks`.
- Cannot write shell startup files.
- Cannot connect to arbitrary network.
- Can execute approved tools.
- Sandbox launch fails closed when profile is invalid.

### Policy Engine

Policy inputs:

```txt
built-in defaults
org/team policy
user policy
repo policy
session approvals
```

Resolution rules:

- Deny beats ask.
- Ask beats allow until approved.
- Repo policy can request capabilities.
- Repo policy cannot silently grant host home, raw secrets, deploy, publish, broad network, or auth CLI access.
- User policy maps private secrets and can narrow grants.
- Session approvals are temporary and never committed.

Config file locations:

```txt
repo policy:
  <project>/.safe-agent/policy.toml

user config:
  ~/.config/safe-agent/config.toml

user secret mappings:
  ~/.config/safe-agent/secrets.toml
```

`~/.config/safe-agent/` is the MVP path. A later macOS-native path such as `~/Library/Application Support/Safe Agent/` can be added as an alias or migration target, but the first implementation should keep CLI config easy to inspect and edit.

TOML is the canonical config format. YAML support is out of scope for the MVP because this is security-sensitive policy and TOML is stricter, easier to review, and has fewer parser surprises.

Effective policy should be explainable:

```sh
safe-agent policy explain network registry.npmjs.org
safe-agent policy explain secret DATABASE_URL --for "npm test"
safe-agent policy explain path .env --action read
```

Policy data model:

```txt
CapabilityRequest
  subject
  action
  resource
  command
  cwd
  reason
  requested_scope

PolicyDecision
  allow | ask | deny
  reason
  matched_rules
  maximum_scope
  redaction_rules
```

### Approval System

The approval system is terminal-native in the MVP.

Flow:

```txt
1. Child invokes shim or mediated command.
2. Shim sends request to supervisor over control socket.
3. Supervisor evaluates policy.
4. If allow, supervisor returns grant.
5. If deny, supervisor returns recoverable denial.
6. If ask, supervisor pauses request and prompts user in parent terminal.
7. User chooses allow once, allow session, deny once, deny session, or details.
8. Supervisor logs decision.
9. Shim proceeds or returns denial to child.
```

Prompt requirements:

- Show request type.
- Show command.
- Show resource.
- Show policy reason.
- Show exposure/risk summary.
- Offer details.
- Default to deny on EOF, timeout, or malformed input.

Example:

```txt
safe-agent approval required

Request: secret DATABASE_URL
Command: npm test
Exposure: injected into this subprocess only
Network during command: blocked
Output redaction: enabled

[a] allow once  [s] allow session  [d] deny  [?] details
```

### Secret Broker

The MVP supports macOS Keychain through the supervisor.

User setup:

```sh
safe-agent secrets add <NAME> [VALUE]
```

This stores a secret under a Safe Agent service namespace, scoped by project or user config.

Repo policy may declare:

```toml
[secrets.DATABASE_URL]
purpose = "test database"
mode = "env_once"
allowed_commands = ["npm test"]
```

User-private config maps secret names to sources:

```toml
[projects."/Users/example/projects/my-app"]
DATABASE_URL = "keychain://safe-agent/my-app/DATABASE_URL"
```

One-command injection flow:

```txt
1. Agent requests secret NAME for command C.
2. Supervisor checks repo policy, user mapping, and session approvals.
3. Supervisor asks user if needed.
4. Supervisor retrieves secret from Keychain.
5. Supervisor runs command C with NAME in the subprocess environment.
6. Supervisor redacts known secret values from captured output.
7. Secret is not added to the agent's ambient environment.
```

For MVP, secret injection should be limited to exact or normalized command matches. Wider patterns require careful policy design and should be deferred.

### Command Shims

The session `bin/` directory appears first in `PATH`.

MVP shims:

- `safe-agent`
- `git`
- `gh`
- `npm`
- `pnpm`
- `yarn`
- `curl`
- `wget`

Shim behavior:

- Parse command enough to classify the operation.
- Ask supervisor for a policy decision.
- Execute the real binary only when allowed.
- Return structured denial when blocked.
- Log command metadata.

Initial command mediation should be pragmatic:

- `git status`, `git diff`, `git log` are allowed.
- `git push`, `git push --force`, branch deletion, and remote mutation are ask or deny.
- `gh` read operations are ask or allowed by policy; write operations ask.
- Package installs ask.
- Package publish denies by default.
- `curl` and `wget` require network approval.

Shims are not the only security boundary. Agents may call absolute binary paths or spawn interpreters. The OS sandbox, scrubbed environment, and network policy must still prevent high-impact bypasses.

### Network Control

Network control is phased.

MVP baseline:

- Ask-gate external network by default.
- Make the default network mode configurable in TOML.
- Hard-deny private ranges, metadata IPs, unknown Unix sockets, and public inbound ports by default.
- Route approved network requests through mediated tools where possible.
- Log all shim-mediated network requests.

Example TOML:

```toml
[network]
default = "ask"
ask = ["registry.npmjs.org", "api.github.com"]
deny = ["169.254.169.254", "127.0.0.1:*", "192.168.0.0/16"]
```

Supported MVP modes:

```txt
ask
  Prompt before first use of an external destination.

deny
  Block network unless a narrower allow/ask rule exists.
```

Phase-after-MVP:

- Supervisor HTTP(S) proxy.
- Optional local CA for TLS-aware inspection where users explicitly opt in.
- Host/path/method policy.
- Private range and metadata IP blocking.
- Per-command network grants.

Seatbelt's hostname-level network controls are limited. The RFC assumes hostname/path policy eventually lives in the supervisor proxy, not entirely in the OS sandbox.

### Audit and Recovery

Before launch:

- Record Git status.
- Record existing tracked diff hash.
- Record untracked file list.
- Record policy hashes.
- Record workspace root.

During session:

- Log command requests.
- Log approvals.
- Log denials.
- Log secret request metadata, not secret values.
- Log shim-mediated network requests.

After session:

```sh
safe-agent summary
```

Outputs:

- Files changed since pre-run checkpoint.
- Commands run through shims.
- Approvals granted.
- Denials.
- Secret names used.
- Policy changes observed.

The MVP should not claim perfect attribution between user edits and agent edits in live repo mode. It should report changes since session start and clearly label that concurrent user edits may be included.

## Suggested Tech Stack

Recommended implementation: Rust single binary.

Reasons:

- Good fit for security-sensitive local tooling.
- Single distributable binary.
- Strong CLI, process, PTY, Unix socket, and serialization ecosystem.
- No Node dependency install required just to start the sandbox.
- Works well with generated shims and low-level process control.

Likely crates:

- `clap` for CLI parsing.
- `serde`, `serde_json`, and `toml` for config.
- `portable-pty` or `nix` for PTY/session management.
- `tokio` for supervisor control socket and concurrent IO.
- `tracing` for audit logs.
- `uuid` or timestamp-based IDs for session IDs.
- `camino` for UTF-8 path handling if desired.

Alternative: TypeScript/Node CLI.

Node would be faster to prototype and easier to distribute through npm, but it is less attractive for the supervisor core because the tool is explicitly security-sensitive and should not require a large runtime dependency chain to launch.

Compromise:

- Rust for `safe-agent` core.
- Optional npm package later that installs/downloads the binary and hosts skills.

## Repository Layout

Safe Agent should be a monorepo because the project will need both the CLI/runtime implementation and user-facing documentation.

Initial repository layout:

```txt
safe-agent/
  SPEC.md
  RFC.md
  local-agent-sandbox-research.md
  README.md
  Cargo.toml                 # Rust workspace
  package.json               # optional docs/tooling workspace
  crates/
    safe-agent-cli/
      Cargo.toml
      src/
        main.rs
        cli.rs
        supervisor/
          mod.rs
          session.rs
          pty.rs
          approvals.rs
          audit.rs
        policy/
          mod.rs
          model.rs
          loader.rs
          resolve.rs
          explain.rs
        sandbox/
          mod.rs
          macos_seatbelt.rs
          profile.rs
          probes.rs
        secrets/
          mod.rs
          keychain.rs
          redact.rs
        shims/
          mod.rs
          generator.rs
          classify.rs
        config/
          defaults.toml
      tests/
        policy_resolution.rs
        env_builder.rs
        path_policy.rs
        seatbelt_probes.rs
  apps/
    docs/
      package.json
      src/
      content/
  examples/
    node-web-app/
    python-cli/
  fixtures/
    policies/
    workspaces/
  docs/
    architecture.md
    policy.md
    secrets.md
    troubleshooting.md
```

This layout is directional. The MVP can start flatter and split modules as behavior stabilizes.

## Phased Plan

### Phase 0: Enforcement Spike

Goal: prove the native macOS sandbox can enforce the minimum safety boundary.

Build:

- Minimal script or Rust prototype that launches `/bin/zsh` under `sandbox-exec`.
- Generated fake home.
- Scrubbed env.
- Simple Seatbelt profile.
- Probe commands.

Acceptance criteria:

- Can read/write a normal workspace file.
- Cannot read workspace `.env`.
- Cannot read real home files.
- Cannot write `.git/hooks`.
- Cannot inherit host API keys.
- Cannot use `SSH_AUTH_SOCK`.
- Invalid sandbox profile fails closed.

Exit decision:

- If Seatbelt cannot support the minimum boundary reliably, pivot native MVP to a stronger backend or make VM mode the first implementation.

### Phase 1: CLI and Session Runtime

Goal: build the agent-agnostic process wrapper without advanced policy.

Build:

- `safe-agent run -- <command>`.
- Workspace canonicalization.
- Session directory creation.
- Fake `HOME` and `TMPDIR`.
- Env allowlist.
- PTY launch and terminal passthrough.
- Generated `safe-agent` shim.
- `safe-agent status` and `safe-agent status --json`.
- Basic session cleanup.

Acceptance criteria:

- Can launch `zsh`, `codex`, `claude`, or `aider` as a child process.
- Child sees fake home.
- Child does not see host env.
- `safe-agent status --json` works inside session without `SAFE_AGENT_*` env vars.
- Startup target is under 2 seconds for native live-repo mode after binary is installed.

### Phase 2: Filesystem Sandbox MVP

Goal: enforce repo-scoped filesystem access.

Build:

- `macos_seatbelt` backend.
- Profile generator from workspace/session paths.
- Protected path denies.
- System/tool read allowlist.
- Sandbox launch integration.
- Behavioral probe test suite.

Acceptance criteria:

- Child can edit live repo files.
- Existing dev server sees changes.
- Child cannot read host home.
- Child cannot read `.env` or `.env.*`.
- Child cannot write `.git/hooks`.
- Child can run normal shell/tooling commands needed for simple tests.
- Sandbox errors are logged.

### Phase 3: Policy Engine and Config

Goal: make capability decisions configurable and explainable.

Build:

- Built-in default policy.
- Repo `.safe-agent/policy.toml`.
- User-private config path.
- Policy schema validation.
- Resolution engine.
- Deny/ask/allow decisions.
- `safe-agent policy validate`.
- `safe-agent policy explain`.
- `safe-agent init`.

Acceptance criteria:

- Invalid repo policy fails with actionable errors.
- Deny beats ask and allow.
- Repo policy cannot grant host home or raw secret access by itself.
- Policy explanations show matched rules.
- Project can declare command, path, network, and secret requests.

### Phase 4: Command Shims and Terminal Approvals

Goal: mediate common high-impact tools and ask the user for scoped authority.

Build:

- Supervisor control socket RPC.
- Generated command shims.
- Request/approval data model.
- Terminal approval prompt.
- Session approval memory.
- Recoverable denial messages.
- Initial classifiers for `git`, `gh`, package managers, `curl`, and `wget`.

Acceptance criteria:

- `safe-agent request network <host>` prompts the user.
- Denied requests return useful messages to the child.
- Allow-once grants apply only to that request.
- Allow-session grants expire at session end.
- `git push` is blocked or ask-gated.
- `npm publish` is denied by default.
- Approval logs contain no secret values.

### Phase 5: Secret Broker MVP

Goal: use secrets without exposing `.env` or ambient environment variables.

Build:

- `safe-agent secrets add <NAME> [VALUE]`.
- macOS Keychain storage backend.
- User-private secret mapping.
- `safe-agent request secret <NAME> --for "<COMMAND>"`.
- One-command env injection.
- Secret output redaction.
- `safe-agent secrets doctor`.

Acceptance criteria:

- Agent cannot read `.env`.
- Agent cannot print secret through ambient `printenv`.
- Approved command receives the secret.
- Other commands do not receive the secret.
- Redaction covers stdout/stderr captured by the supervisor path.
- Denied secret request is recoverable.

### Phase 6: Audit and Recovery

Goal: make sessions reviewable and debuggable.

Build:

- Pre-run checkpoint.
- Session log format.
- `safe-agent summary`.
- Diff summary against session start.
- Approval/denial summary.
- Policy hash logging.

Acceptance criteria:

- User can see files changed since session start.
- User can see approvals and denials.
- Logs do not contain secret values.
- Summary clearly states that concurrent user edits may be included in live-repo mode.

### Phase 7: Network Hardening

Goal: move from tool-level network mediation toward supervisor-controlled egress.

Build:

- Supervisor local proxy.
- Seatbelt profile that blocks direct egress and allows only proxy loopback where practical.
- Network request policy by host/port/protocol.
- Private network and metadata IP denies.
- Per-command network grants.

Acceptance criteria:

- Direct `curl https://example.com` is denied unless approved.
- Approved network request is logged.
- Private network and metadata IP access is denied by default.
- Package install flow can be approved without opening arbitrary network for the whole session.

### Phase 8: Agent Instructions and Skills

Goal: improve agent cooperation without depending on it for safety.

Build:

- Startup briefing for generic adapter.
- Agent-specific startup adapters where practical.
- `safe-agent skills install` placeholder.
- Repo-local instruction pack format.

Acceptance criteria:

- Generic agents can discover `safe-agent help` and `safe-agent status`.
- Denial messages point to the correct request command.
- Skills/instructions are documented as convenience, not enforcement.

### Phase 9: Team Trust and Strict Mode

Goal: support broader team adoption and high-risk workflows.

Build:

- Policy signing and verification.
- Org/team policy install.
- VM backend spike.
- Quarantine workspace mode.
- Optional GUI approval UI.

Acceptance criteria:

- Changed repo policy is detected and explained.
- Signed policy can be verified.
- VM backend can run a basic coding-agent session.
- Quarantine mode can produce an applyable diff.

## Test Strategy

Test at the highest boundary that can prove behavior.

Unit tests:

- Policy resolution.
- Path canonicalization.
- Env allowlist.
- Command classification.
- Secret redaction.
- Config validation.

Integration tests:

- Launch child process with fake home.
- `safe-agent status --json` through generated shim.
- Approval request over control socket.
- Keychain mock backend.
- Command shim allow/deny behavior.

macOS behavioral tests:

- Seatbelt denies protected files.
- Seatbelt allows normal project edits.
- Network direct egress is blocked in high-safety mode.
- Invalid profile fails closed.

Manual smoke tests:

- Run `safe-agent run -- zsh`.
- Run `safe-agent run codex .`.
- Run `safe-agent run claude .`.
- Run a sample Node app with external dev server watching live edits.
- Try to read `.env`, host home files, SSH keys, and shell history.
- Try `npm test` with and without an approved secret.

## Security Review Checklist

Before any MVP release:

- Verify no host env is inherited by default.
- Verify fake home is used by the child.
- Verify protected repo files are denied.
- Verify real home paths are denied.
- Verify session shims are not writable by the child.
- Verify sandbox failure does not fall back to unsandboxed execution.
- Verify approval defaults to deny.
- Verify logs redact secrets.
- Verify repo policy cannot grant dangerous host access alone.
- Verify all path checks canonicalize symlinks and relative paths.

## Key Risks

### Seatbelt Profile Fragility

Apple marks `sandbox-exec` deprecated and custom SBPL is not a stable public product API.

Mitigation:

- Keep the backend abstract.
- Build behavioral probes.
- Fail closed.
- Add VM backend later.

### Shim Bypass

An agent may call absolute binary paths or interpreters to bypass shims.

Mitigation:

- Treat shims as UX/policy mediation, not the hard boundary.
- Enforce filesystem, env, and network limits below shims.
- Block direct network where practical.

### Live Repo Attribution

In live repo mode, the user may edit files while the agent is running.

Mitigation:

- Report "changes since session start" rather than claiming perfect attribution.
- Defer perfect file-touch attribution.

### Secret Leakage Through Approved Commands

If a command receives a secret, it can intentionally print or exfiltrate it.

Mitigation:

- Scope secrets to exact commands.
- Redact captured output.
- Pair secrets with network restrictions.
- Prefer proxy mode for API keys in later phases.

### Developer Tool Compatibility

Strict filesystem/network rules can break normal toolchains.

Mitigation:

- Start with a practical built-in profile.
- Add `safe-agent policy explain`.
- Keep denied actions recoverable.
- Build with real coding-agent smoke tests.

## Open Questions

- Which exact language/runtime should be used for the first prototype: Rust single binary or faster TypeScript prototype?
- Which command shims are required before the tool is useful with Codex/Claude/Aider?
- Should Keychain storage be project-scoped by absolute path, repo identifier, or explicit user namespace?
- How should policy handle symlinked workspaces and monorepos?
- Should dedicated macOS user support be part of MVP or a documented hardening mode?
- What is the minimum useful redaction guarantee when commands run outside supervisor-captured IO?
