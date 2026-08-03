# Safe Agent Sandbox Spec

Status: Draft
Date: 2026-08-01

## Overview

Safe Agent is a local wrapper for coding agents such as Codex, Claude, Gemini, Aider, or similar terminal-based development agents.

The goal is to let a developer keep the normal coding loop while preventing the agent from receiving broad authority over the whole machine. The agent should be able to edit the intended project, run tests, start local workflows, and request extra capabilities when needed. It should not inherit the user's real environment, read secrets by default, access unrelated files, exfiltrate data over arbitrary network paths, or perform high-impact actions without explicit approval.

Supporting research lives in `local-agent-sandbox-research.md`. Implementation details and delivery phases live in `RFC.md`. The pre-implementation verification suite lives in `TEST_PLAN.md`.

## Problem

Developers often run coding agents in broad-permission modes because the workflow is convenient. In that mode, the agent can typically:

- Run arbitrary shell commands.
- Read and write outside the active project.
- Read `.env` files and inherited environment variables.
- Use authenticated CLIs such as `gh`, `aws`, `vercel`, `supabase`, `stripe`, `npm`, or `kubectl`.
- Access SSH keys, cloud config, browser profiles, shell history, package manager tokens, and other credentials.
- Send data to arbitrary network endpoints.
- Modify persistence paths such as shell startup files, Git hooks, LaunchAgents, package scripts, or agent config.

This creates risk from prompt injection, malicious dependencies, compromised docs, mistaken agent plans, and accidental destructive commands.

## Goals

- Scope agent filesystem access to the active project by default.
- Preserve the normal live development loop where edits appear in the user's existing dev server.
- Prevent ambient secrets and environment variables from being exposed to the agent process.
- Allow secrets to be used through explicit, auditable approvals.
- Block arbitrary network access by default.
- Provide clear denial messages so the agent can recover from blocked actions.
- Support team-shareable project policy without letting repo config silently grant dangerous host access.
- Provide stronger optional isolation for high-risk work.
- Work as a wrapper around multiple coding agents, not as a replacement agent.

## Non-Goals

- Perfect malware containment in native macOS mode.
- Full replacement for VM isolation in high-risk environments.
- Proving that installed third-party packages are safe.
- Letting repo policy alone grant access to host secrets, host home directories, or deployment authority.
- Supporting every CLI tool with rich mediation in the first version.
- Requiring Docker.

## Product Principles

1. The agent can ask. The supervisor decides.
2. Deny by default for secrets, host files, network, and high-impact commands.
3. Deny rules always win.
4. Repo policy can describe project needs, but cannot silently expand trust.
5. User-private policy controls local secrets and personal approvals.
6. The default workflow should edit the live repo to preserve developer ergonomics.
7. Stronger isolation should be available when safety matters more than immediacy.
8. Every approval should be scoped, logged, and revocable.
9. Session state should be ephemeral by default. The first version should not reuse agent homes across sessions.
10. Approval prompts should be clear enough for the user to decide quickly without reading implementation details.

## Architecture

Safe Agent uses a trusted supervisor / untrusted child architecture.

```txt
trusted supervisor
  safe-agent wrapper
  owns policy, approvals, secrets, sandbox setup, logs

untrusted child
  codex / claude / aider / gemini / shell commands
  edits code, runs allowed commands, requests capabilities
```

The supervisor is a small deterministic control layer. It creates the sandbox, resolves policy, clears environment variables, provides a fake home directory, mediates sensitive tools, brokers secrets, logs actions, and explains denials.

The child is the coding agent process. It is useful but not fully trusted because it may follow bad instructions from external content. The child gets the capabilities required for coding, not full host authority.

## Default User Experience

A developer starts in their project:

```sh
cd ~/Desktop/projects/my-app
npm run dev
```

In another terminal:

```sh
safe-agent run codex .
```

Safe Agent prints a policy summary:

```txt
Workspace: /Users/ryanharaki/Desktop/projects/my-app
Mode: live-repo
Profile: repo-coder

Allowed:
  read/write project files
  run build/test/lint commands
  use isolated HOME
  bind approved localhost dev ports

Blocked:
  host HOME
  .env files
  ambient env vars
  arbitrary network
  git push / deploy / publish
  shell persistence paths

Secrets:
  none exposed
```

The agent edits the actual repo, so the user's existing dev server sees file changes normally.

## Workspace Modes

### Live Repo Mode

Live repo mode is the default.

The agent edits the real working tree but is restricted to the project path and allowed subpaths. This preserves the normal developer loop:

```txt
agent edits file -> dev server reloads -> user sees change
```

Safe Agent captures a pre-run checkpoint and can summarize or revert agent changes after the session.

### Quarantine Mode

Quarantine mode is optional:

```sh
safe-agent run codex . --quarantine
```

The agent edits a temp copy, APFS clone, snapshot, overlay, or VM-mounted workspace. The user reviews and applies changes later.

This mode is safer for untrusted repos, risky dependency installs, malware-like build scripts, or fully autonomous work, but it adds friction because the user's existing dev server will not see changes unless sync machinery is enabled.

## Session State

The MVP uses ephemeral session state only.

Each run gets a fresh session directory:

```txt
/tmp/safe-agent/sessions/<run-id>/
  home/
  tmp/
  logs/
  policy/
```

The agent's `HOME` points to the session home, not the user's real home:

```txt
real HOME:
  /Users/example

agent HOME:
  /tmp/safe-agent/sessions/<run-id>/home
```

This fake home exists because normal development tools expect `HOME` to exist. Package managers, shells, Git, language servers, and agents often write caches, config, logs, or temporary state under home. Removing `HOME` entirely would break ordinary workflows.

The session home may contain only disposable state:

- Tool caches created during the session.
- Agent scratch files.
- Minimal generated config.
- Logs and command metadata.

The session home must not contain durable authority:

- API tokens.
- SSH keys.
- Cloud credentials.
- GitHub CLI auth.
- npm publish tokens.
- Keychain or 1Password access.
- Real shell startup files.

By default, Safe Agent deletes the session directory at the end of the run, after preserving audit logs if configured. Reusable homes and shared caches are out of scope for the MVP because persistent state can become a hidden authority or compromise channel.

Codex, Claude, and other agents therefore do not inherit an existing host
login automatically. A user who starts an agent in Safe Agent may need to
authenticate inside that session. The sandbox may allow a loopback-only login
callback listener on `localhost`, but it must not expose the host home,
credential files, or unrestricted external network to complete that flow.

Persistent agent credentials are a future opt-in feature. It must use a
brokered credential or session handoff mechanism rather than mounting the
host agent home into the sandbox.

## Capability Model

A capability is a scoped permission granted to the agent or one of its child commands.

```txt
subject: agent, shell command, npm, git, gh, curl, test runner
action: read, write, connect, bind, use_secret, publish, deploy
resource: path, host, port, secret name, API operation
scope: once, session, project
constraints: argv, host, path, method, output redaction
```

## Default Capability Policy

Allowed by default:

- Read project files, excluding protected paths.
- Write project files, excluding protected paths.
- Write temp/cache files inside the sandbox home and temp directory.
- Run common local commands such as tests, lint, build, typecheck, and format.
- Read package manifests, lockfiles, and docs in the repo.
- Start local dev servers on approved loopback ports.

Ask first:

- Network access to package registries, docs, GitHub, model APIs, or other external hosts.
- Installing or updating dependencies.
- Running database migrations.
- Using a secret for a specific command.
- Running authenticated CLIs such as `gh`, `aws`, `vercel`, `supabase`, `stripe`, `kubectl`, or `npm`.
- Git push, PR creation, deploys, publishing, database writes.
- Reading another local directory.
- Binding a public-facing port.

Disabled by default:

- Reading the user's real home directory.
- Reading `.env`, `.env.*`, SSH keys, cloud configs, npm tokens, Keychain files, browser profiles, shell history, or unrelated projects.
- Inheriting ambient environment variables.
- Direct arbitrary network access.
- Access to localhost/private network services unless approved.
- Writing shell startup files, Git hooks, LaunchAgents, cron-like paths, agent configs, package manager auth files, or files on `$PATH`.
- `sudo`, host app automation, Docker socket access, force push, package publishing, repo deletion, or cloud resource deletion.

### Localhost Callbacks

The Seatbelt profile allows child processes to bind and connect to
`localhost:*` for OAuth/device-login callbacks and local development tools.
This exception does not allow external network egress, access to private LAN
addresses, or access to the user's host home.

## Denied Access UX

Denied access must be enforced below the agent, but explained above the raw OS error.

For raw process access, the OS sandbox blocks the action:

```sh
cat ~/.ssh/id_rsa
```

The command may see:

```txt
Operation not permitted
```

Safe Agent should add a structured denial message when possible:

```txt
safe-agent: access denied

Action: read
Path: /Users/example/.ssh/id_rsa
Reason: outside workspace and matches protected secret path
Policy: host_home.read = deny

You can request access with:
  safe-agent request filesystem-read /Users/example/.ssh/id_rsa
```

For repo-local secret files:

```txt
safe-agent: access denied

Action: read
Path: /project/.env
Reason: .env files are secret-bearing and not readable by agents
Alternative:
  request a named secret instead:
  safe-agent request secret DATABASE_URL --for "npm test"
```

Denial messages should not reveal sensitive file contents or over-confirm details about protected host paths.

## Approval UX

When the agent requests a capability, the supervisor asks the user to approve or deny. The agent should not be able to grant its own requests.

For the MVP, approvals should be terminal-native. Safe Agent already owns the parent terminal and launches the agent in a child PTY, so the supervisor can pause the child command, show an approval prompt, and resume or deny the action.

Example network approval:

```txt
safe-agent approval required

Request: network access
Command: npm install
Destination: registry.npmjs.org
Reason: install dependencies
Policy: network.registry.npmjs.org = ask

[a] allow once  [s] allow session  [d] deny  [?] details
```

Example secret approval:

```txt
safe-agent approval required

Request: secret DATABASE_URL
Command: npm test
Exposure: injected into this subprocess only
Network during command: blocked
Output redaction: enabled
Policy: secrets.DATABASE_URL = ask

[a] allow once  [s] allow session  [d] deny  [?] details
```

Example high-impact command approval:

```txt
safe-agent approval required

Request: GitHub write
Command: gh pr create
Operation: create pull request
Repository: owner/my-app
Policy: github.create_pr = ask

[a] allow once  [s] allow session  [d] deny  [?] details
```

Approval decisions should support:

- Allow once.
- Allow for this session.
- Deny once.
- Deny for this session.
- Show details.
- Open policy explanation.

Approvals should be scoped to the smallest useful unit. For example, approving `DATABASE_URL` for `npm test` should not expose it to the agent shell, other commands, or the full session unless the user explicitly chooses that wider scope.

When the user denies a request, the agent receives a recoverable message:

```txt
safe-agent: request denied

Request: secret DATABASE_URL
Reason: user denied
Alternative: continue without this secret, use a mock database, or ask the user for a different test strategy.
```

Future versions can add GUI approvals through macOS notifications, a menu bar app, or a local browser UI. The terminal prompt is the MVP because it is agent-agnostic and works with any CLI-based coding agent.

## Secret Model

Secrets live outside the sandbox. The agent can request secret-dependent actions, but it does not receive broad read access to secret stores.

Supported secret backends should include:

- macOS Keychain.
- 1Password CLI.
- Environment injection from a trusted supervisor process.
- Future: cloud secret managers.

The preferred mode is scoped injection:

```txt
Agent requests DATABASE_URL

Command: npm test
Exposure: this subprocess only
Network: blocked
Output: redacted

Allow once / allow session / deny
```

The supervisor retrieves the secret and injects it only into the approved child command.

The agent process itself should not start with `DATABASE_URL`, `OPENAI_API_KEY`, or other secrets in its ambient environment.

Users can add or update local secrets with:

```sh
safe-agent secrets add <NAME> [VALUE]
```

If `VALUE` is omitted, Safe Agent should prompt for it using hidden input. Passing `VALUE` directly is supported for scripting, but Safe Agent should warn that command-line arguments may be captured by shell history or process listings.

If an agent invokes `safe-agent secrets add` from inside a Safe Agent session, the operation must be approval-gated because it creates durable user-private secret state. The agent cannot add or overwrite secrets without user approval.

For API keys, a stronger mode is credential proxying:

- The agent receives a fake token, sentinel token, or rewritten base URL.
- Requests go through the supervisor proxy.
- The supervisor injects the real credential upstream.
- The supervisor can enforce host/path/method policy and redact logs.

## Environment Model

The agent starts with a scrubbed environment.

Default allowed environment:

- `HOME` pointing to a fake sandbox home.
- `TMPDIR` pointing to sandbox temp.
- `PATH` pointing to approved tool shims and system tools.
- `TERM`, `LANG`, and other minimal terminal/runtime values.

Default denied environment:

- User shell variables.
- API keys.
- Cloud credentials.
- Package manager tokens.
- `SSH_AUTH_SOCK`.
- `GPG_AGENT_INFO`.
- Injection-prone variables such as `DYLD_*`.
- Any variable not explicitly allowlisted.

## Network Model

Network is ask-gated by default.

The native macOS mode should block direct egress from the agent and force allowed traffic through a supervisor-controlled proxy. Hostname allowlists alone are not enough, because allowed hosts can still be exfiltration channels.

Default behavior:

- External network requests prompt the user before proceeding.
- Approved requests are scoped to host, port, protocol, command, and duration where possible.
- The repo and user TOML config can change network policy for specific destinations.
- Dangerous destinations remain denied by default unless explicitly configured in a trusted/user layer.

Default denied destinations:

- Private network ranges.
- Cloud metadata addresses such as `169.254.169.254`.
- Unix sockets and service sockets outside approved paths.
- Public inbound ports.

Ask-gated destinations:

- External HTTPS hosts.
- Package registries.
- GitHub/API hosts.
- Documentation sites.
- Localhost services, including dev servers and local databases.

Approved network grants should be scoped by:

- Host.
- Port.
- Protocol.
- Path or API operation where possible.
- Command.
- Duration.

Example approval:

```txt
Agent requests network access

Command: npm install
Destination: registry.npmjs.org
Reason: install project dependencies

Allow once / allow session / deny
```

## Command Mediation

Some commands need special treatment because they have high side effects or can use credentials without displaying them.

Priority tools to mediate:

- `git`
- `gh`
- `npm`, `pnpm`, `yarn`
- `pip`, `uv`, `poetry`
- `curl`, `wget`
- `ssh`, `scp`, `rsync`
- `aws`, `gcloud`, `az`
- `vercel`, `supabase`, `stripe`, `kubectl`
- `docker`

Mediation options:

- Block by default.
- Allow read-only operations.
- Ask for high-impact operations.
- Rewrite commands through shims.
- Run specific subcommands in child sandboxes.
- Proxy API operations.
- Redact output.
- Log command, argv, working directory, policy decision, and result.

## Team Configuration

Safe Agent supports layered config:

```txt
built-in defaults
  conservative, lowest trust

org/team policy
  trusted, optionally signed or installed locally

user policy
  private preferences, secret mappings, approval memory

repo policy
  shared project needs, committed to Git

session approvals
  temporary grants, never committed
```

The final permission set is the intersection of these layers plus explicit approvals. Deny rules always win.

Repo policy can request capabilities but cannot silently grant dangerous host access.

## Configuration Files

Safe Agent uses separate repo-shared and user-private configuration files.

Repo-shared config lives in the project:

```txt
<project>/.safe-agent/policy.toml
```

This file can be committed. It describes what the project needs: requested commands, protected paths, network hosts, secret names, and project-specific policy.

The agent may read repo policy but must not silently edit it. In live repo mode, `.safe-agent/policy.toml` is a protected path: user edits are allowed, but agent writes are denied or approval-gated because policy changes can broaden sandbox authority.

User-private config lives outside the project:

```txt
~/.config/safe-agent/config.toml
~/.config/safe-agent/secrets.toml
```

These files should not be committed. They describe the user's local preferences, approval memory, and secret source mappings.

On macOS, a future version may support:

```txt
~/Library/Application Support/Safe Agent/config.toml
```

The MVP should start with `~/.config/safe-agent/` because it is simple, CLI-friendly, and easy to inspect.

TOML is the canonical config format. It is stricter and easier to review than YAML for security-sensitive policy, and it avoids YAML-specific ambiguity such as anchors, aliases, and surprising scalar coercion.

## Repo Policy Example

```toml
# .safe-agent/policy.toml
version = 1
profile = "node-web-app"

[workspace]
read = ["."]
write = ["."]
deny_read = [".env", ".env.*"]
deny_write = [".safe-agent/policy.toml", ".git/hooks", ".github/workflows", ".env", ".env.*"]

[commands]
allow = ["npm test", "npm run build", "npm run lint"]
ask = ["npm install", "npm run db:migrate"]
deny = ["sudo", "git push --force", "npm publish"]

[network]
default = "ask"
ask = ["registry.npmjs.org", "api.github.com"]
deny = ["169.254.169.254", "127.0.0.1:*", "192.168.0.0/16"]

[secrets.DATABASE_URL]
purpose = "test database"
mode = "env_once"
allowed_commands = ["npm test"]

[secrets.OPENAI_API_KEY]
purpose = "eval runs"
mode = "proxy"
allowed_hosts = ["api.openai.com"]
allowed_commands = ["npm run evals"]

[git]
allow_read = true
ask = ["create_pr"]
deny = ["force_push", "delete_branch", "merge_pr"]
```

## User-Private Secret Mapping Example

```toml
# ~/.config/safe-agent/secrets.toml

[projects."/Users/example/projects/my-app"]
DATABASE_URL = "keychain://safe-agent/my-app/DATABASE_URL"
OPENAI_API_KEY = "op://Development/OpenAI/api-key"
```

The repo says what the project may request. The user config says where the user's private values live.

## Policy Trust and Signing

Repo policies are useful but not inherently trusted. A malicious branch or prompt-injected agent could edit policy to request broader access.

Safe Agent should:

- Warn when repo policy changes.
- Show policy diffs before using changed policy.
- Allow teams to sign trusted policy files.
- Treat unsigned or changed repo policy as request-only.
- Never let repo policy alone grant host home, secret, deploy, publish, or broad network access.

## Audit and Recovery

Safe Agent should capture a session checkpoint before starting:

- Git status.
- Existing tracked diffs.
- Existing untracked files.
- Policy hash.
- Current workspace root.

During the session, log:

- Commands.
- Denials.
- Network requests.
- Secret requests.
- User approvals.
- Files touched by the agent when observable.
- Final diff against the pre-run checkpoint.

End-of-session summary:

```txt
Safe Agent session complete

Files changed:
  src/App.tsx
  src/App.test.ts

Approvals granted:
  network registry.npmjs.org for npm install
  DATABASE_URL for npm test

Denied:
  read .env
  connect 192.168.1.10:5432

Actions:
  view diff
  revert agent changes
  keep changes
```

## Enforcement Backends

### Native macOS Backend

The first native backend should use:

- Seatbelt through `sandbox-exec`.
- Fake `HOME`.
- Scrubbed environment.
- Generated per-session sandbox profile.
- Supervisor proxy for network.
- Command shims for sensitive tools.
- Optional dedicated macOS user as defense in depth.

Seatbelt is practical but should be treated as a backend, not the whole architecture. Apple marks `sandbox-exec` deprecated and custom SBPL is not a stable third-party product API.

### Strict Backend

Strict mode should support a lightweight VM backend:

- Lima.
- UTM.
- Apple Virtualization or Apple's container tooling where available.

Inside the VM, Safe Agent can use Linux-native isolation such as bubblewrap, seccomp, or nsjail.

Strict mode is for high-risk repos, fully autonomous runs, malware-like build scripts, and users who prefer stronger isolation over low-friction live editing.

## CLI Shape

Initial commands:

```sh
safe-agent run codex .
safe-agent run claude .
safe-agent run aider .

safe-agent init
safe-agent policy validate
safe-agent policy explain
safe-agent policy diff
safe-agent policy reload
safe-agent secrets add <NAME> [VALUE]
safe-agent secrets doctor
safe-agent skills install
safe-agent summary
```

Capability request commands exposed inside the sandbox:

```sh
safe-agent request network registry.npmjs.org --reason "install dependencies"
safe-agent request secret <NAME> --for "<COMMAND>"
safe-agent request filesystem-read ../shared-lib --reason "inspect local dependency"
```

## Agent Instructions and Skills

Safe Agent should protect the host even when the child agent has no Safe Agent-specific knowledge. The hard boundary comes from the supervisor, sandbox, scrubbed environment, secret broker, network controls, and command mediation.

However, the agent experience improves if the agent knows how to request capabilities instead of repeatedly hitting denials. Safe Agent should expose that contract through the `safe-agent` CLI, not through Safe Agent-specific environment variables.

The `safe-agent` command inside the sandbox should be a supervisor-aware shim placed early in `PATH`. It should discover the active session from session metadata adjacent to the shim, from the supervisor socket path baked into the shim, or from another sandbox-internal discovery file. It should not require `SAFE_AGENT_*` environment variables.

The agent can inspect the session:

```sh
safe-agent status
safe-agent status --json
safe-agent help
```

Outside a Safe Agent session, `safe-agent status` should return a clear "not in a Safe Agent session" result. Inside a session, it should know the workspace, profile, policy, request mechanism, and supervisor connection from the session context created by the wrapper.

At startup, Safe Agent should provide a short briefing through the adapter when possible:

```txt
You are running inside Safe Agent.

- Work only inside the project workspace.
- Do not read .env files or host credentials.
- Request secrets with: safe-agent request secret <NAME> --for "<COMMAND>"
- Request network with: safe-agent request network <HOST> --reason "<REASON>"
- Inspect the current sandbox with: safe-agent status --json
- If access is denied, read the denial message and choose an allowed alternative.
```

For agents that support skills or repo-local instruction packs, Safe Agent can offer an optional install command:

```sh
safe-agent skills install
```

The intended future shape is to publish a Safe Agent skill through the relevant skills distribution path, such as an `npx skills` flow or Vercel Skills if that becomes the preferred ecosystem path. The command would download or update a repo-local instruction pack that teaches compatible agents the Safe Agent session contract.

This is a convenience layer only. A malicious or unaware agent can ignore the skill, so all security guarantees must continue to come from enforcement below the agent process.

## MVP Scope

The MVP should support:

- macOS native mode.
- Live repo editing.
- Fake `HOME`.
- Fresh ephemeral session directory per run.
- Scrubbed env.
- Basic Seatbelt profile generation.
- Deny reads of host home and repo `.env` files.
- Deny writes to repo persistence paths.
- Ask-gated network by default, configurable through TOML policy.
- Basic command allow/ask/deny policy.
- macOS Keychain-backed secret lookup.
- One-command secret injection with redaction.
- Repo `.safe-agent/policy.toml`.
- User-private secret mapping.
- Terminal-native approval prompts.
- Session audit summary.

MVP can defer:

- Full TLS-inspecting proxy.
- Signed team policy.
- VM backend.
- Rich per-API GitHub mediation.
- Cross-platform support.
- GUI approval UI.
- Perfect file-touch attribution.
- Reusable agent homes and shared package caches.
- Hosted Safe Agent skill distribution through `npx skills` or Vercel Skills.

## Security Requirements

- The agent must not read `.env` by default.
- The agent must not inherit the user's broad environment.
- The agent must not access the real home directory by default.
- The agent must not access Keychain or 1Password directly.
- The agent must not get raw secrets for a full session unless the user explicitly chooses that risk.
- The agent must not perform network egress except through approved policy.
- The agent must not write persistence paths by default.
- The sandbox must fail closed if the selected enforcement backend cannot start.
- Approvals must be scoped and visible.
- Denials must be explainable but not leak sensitive contents.

## Open Questions

- Should the first implementation be native macOS only, or should the policy model be built cross-platform from day one?
- How should Safe Agent distinguish agent-touched changes from concurrent user edits in live repo mode?
- Should command mediation be shell-shim based, PTY-supervisor based, or both?
- How much secret proxying is required before exposing this to real API keys?
- Should repo policy signing be part of v1 or v2?
- Should a dedicated macOS user be required, recommended, or optional?
- Should reusable safe caches be introduced after MVP, and if so, which cache types are low-risk enough to persist?
