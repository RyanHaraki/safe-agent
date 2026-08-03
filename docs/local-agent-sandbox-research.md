# Non-Docker Local Coding-Agent Sandboxes on macOS

Date: 2026-08-02

Scope: non-Docker local mechanisms that could wrap Codex, Claude Code, or another terminal coding agent on macOS. The Ry Walker article was used only as a map; the findings below are based on primary or owner sources where available, plus local macOS 26.5 man pages for Apple-shipped CLIs.

## Executive Findings

The best native macOS wrapper design is layered, not one mechanism:

1. Use Seatbelt through `sandbox-exec` for the inner OS-enforced process boundary.
2. Run the wrapper/agent as a dedicated Standard macOS user with a separate home and least-privilege filesystem ownership.
3. Block direct egress in the Seatbelt profile, force HTTP(S)/SOCKS traffic through a localhost proxy, and add a PF UID rule as a system-level backstop.
4. Start the agent with a scrubbed environment and a fake session home; never pass broad secrets directly to the agent.
5. Put `git`, `gh`, `ssh`, `npm`, package managers, cloud CLIs, and deploy commands behind command mediation or child sandboxes.
6. Use a Lima/UTM/Apple Virtualization VM mode for high-risk repos, malware-ish build scripts, or full-auto work that needs stronger isolation than same-kernel Seatbelt.

`sandbox-exec` is deprecated, but it remains the only built-in, no-container, arbitrary-process sandbox on macOS. Apple recommends App Sandbox for apps, but App Sandbox is entitlement/code-signing oriented, not a clean replacement for wrapping arbitrary CLIs. The practical choice is either "use Seatbelt carefully" or "move the risky process into a VM."

## Alternatives

### 1. macOS Seatbelt / `sandbox-exec`

Mechanism: `sandbox-exec` runs a command inside a Seatbelt sandbox from a named profile, file profile, or inline profile string. The local macOS 26.5 man page says the command is deprecated and points developers to App Sandbox, but still documents `-f`, `-n`, `-p`, and `-D key=value`. `sandbox(7)` says new processes inherit the parent's sandbox, restrictions apply when acquiring resources, and open file descriptors obtained before restrictions may remain usable.

Primary/source evidence:

- Local primary source: `man sandbox-exec` on macOS 26.5: "execute within a sandbox (DEPRECATED)" and "enters a sandbox using a profile".
- Local primary source: `man sandbox` on macOS 26.5: sandbox inheritance and pre-opened file descriptor caveat.
- Apple App Sandbox docs describe the recommended app model: capabilities are removed by enabling App Sandbox, then selectively restored by entitlements added to a target's `.entitlements` file and code signature. They also list outgoing/incoming network entitlements and child-process inheritance entitlements: https://developer.apple.com/library/archive/documentation/Miscellaneous/Reference/EntitlementKeyReference/Chapters/EnablingAppSandbox.html
- OpenAI documents that Codex on macOS uses Seatbelt profiles, and that if a selected policy cannot be enforced Codex refuses to run the command rather than silently running unsandboxed: https://developers.openai.com/codex/permissions
- OpenAI's sandbox docs say macOS sandboxing works out of the box using the built-in Seatbelt framework: https://developers.openai.com/codex/sandboxing
- Claude Code's sandbox docs say the sandbox uses Seatbelt on macOS and that OS-level restrictions apply to Bash commands and child processes: https://code.claude.com/docs/en/sandboxing
- Anthropic Sandbox Runtime says it uses `sandbox-exec` on macOS, `bubblewrap` on Linux, and proxy-based network filtering: https://github.com/anthropic-experimental/sandbox-runtime

Strengths:

- Native and already installed on macOS.
- No daemon, image, or Docker dependency.
- Applies to child processes when the agent spawns shell commands.
- Proven enough that Codex, Claude Code, Anthropic Sandbox Runtime, Fence, and nono all use Seatbelt/`sandbox-exec` on macOS.
- Can enforce filesystem and network restrictions below the model and permission UI layer.

Weaknesses:

- Apple marks `sandbox-exec` deprecated.
- Seatbelt profile language is poorly documented for third-party CLI wrapper authors.
- It is same-user, same-kernel isolation, not a VM.
- Policies are easy to get wrong: readable home dotfiles, `~/Library`, Unix sockets, Mach services, TTY/PTTY behavior, shell startup files, and toolchain paths all need deliberate treatment.
- Restrictions apply when a resource is acquired, so the wrapper must close unneeded file descriptors before entering the sandbox.

Design implications:

- Generate a fresh deny-by-default profile per session.
- Set a fake `HOME` inside the writable session directory and deny access to the real home, especially `~/.ssh`, `~/.aws`, `~/.config`, `~/.gnupg`, shell startup files, Keychains, browser profiles, and IDE config.
- Allow writes only to the working copy and session temp.
- Deny direct networking and only allow loopback to a wrapper-owned proxy port.
- Sanitize `DYLD_*` and other injection-prone environment variables before exec.
- Treat Seatbelt profile generation as security-critical code with golden tests that prove file reads/writes and network access are actually denied.

### 2. Separate macOS User and Filesystem Permissions

Mechanism: create a dedicated Standard user such as `agent-runner`, keep its home separate, and grant access only to a copied worktree or group/ACL-scoped project directory.

Primary/source evidence:

- Apple Users & Groups docs say Standard users can install apps and change their own settings, but cannot add users or change other users' settings: https://support.apple.com/guide/mac-help/change-users-groups-settings-mtusr001/mac
- Apple file permission docs describe per-user and per-group privileges: Read & Write, Read only, Write only, and No Access: https://support.apple.com/guide/mac-help/change-permissions-for-files-folders-or-disks-mchlp1203/mac

Strengths:

- Simple, understandable boundary.
- Separates default home directories, shell config, app state, and keychains.
- Gives PF a stable UID/GID target for network rules.
- Reduces blast radius even if the Seatbelt profile accidentally allows broader reads within the agent user's own home.

Weaknesses:

- POSIX permissions are not a complete sandbox. World-readable files remain readable, and anything deliberately granted to the user is available.
- Toolchains, Homebrew, language caches, TCC prompts, and GUI access become more operationally awkward.
- If you reuse the same user across sessions, state and compromise can persist.

Design implications:

- Prefer a dedicated non-admin user plus per-session workspace under that user's home.
- Avoid mounting the developer's real home into the agent account.
- Use group/ACL grants only for the current project, or copy/clone the repo into the agent account and sync changes back.
- Pair this with Seatbelt and PF. Do not rely on user separation alone for secret protection.

### 3. `chroot` and chroot-like Tools

Mechanism: native `chroot` changes a process's filesystem root. PRoot/fakechroot-like tools emulate root or bind mounts in user space, mostly on Linux.

Primary/source evidence:

- Apple `chroot(2)` says `chroot()` changes the root directory, is restricted to the superuser, and warns that root processes can escape: https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/chroot.2.html
- Local macOS 26.5 `man chroot` documents `-u`, `-g`, and `-G`, but still requires root-level setup in normal use.
- PRoot says it is "chroot, mount --bind, and binfmt_misc without privilege/setup for Linux" and relies on Linux `ptrace`: https://proot-me.github.io/

Strengths:

- Useful for filesystem layout tricks.
- Can reduce accidental writes outside a tree if combined with dropping privileges.

Weaknesses:

- Native `chroot` is not a strong security boundary on macOS.
- Requires elevated setup.
- Does not mediate network, syscalls, Mach services, or secrets.
- PRoot is Linux-oriented and ptrace-based, not a realistic macOS wrapper foundation.

Design implications:

- Do not use `chroot` as the primary sandbox.
- It can be used inside a Linux VM or Nix build, but Seatbelt or a VM should provide the actual boundary.

### 4. Nix

Mechanism: Nix can run builds in a sandboxed environment with controlled filesystem visibility. On Linux it additionally uses private PID, mount, network, IPC, and UTS namespaces for builds.

Primary/source evidence:

- Nix reference manual: sandboxed builds see only declared dependencies in the Nix store, the temporary build directory, and configured sandbox paths; Linux builds get private namespaces. It says sandboxing works on Linux and macOS, requires Nix run as root, and defaults to true on Linux but false on other platforms: https://nix.dev/manual/nix/2.23/command-ref/conf-file.html?highlight=sandbox

Strengths:

- Excellent for reproducible toolchains and avoiding undeclared build inputs.
- Good way to pin the wrapper's dependencies, agent helper CLIs, and optional VM/proxy tooling.
- Nix build users can separate build execution from the invoking user.

Weaknesses:

- Nix sandboxing is build-focused, not a general interactive terminal-agent sandbox.
- Darwin defaults and compatibility differ from Linux.
- Network policy is not a general per-agent egress allowlist.

Design implications:

- Use Nix to define the wrapper runtime and toolchain, not as the whole sandbox.
- Use Nix builds for specific build/test actions where reproducibility matters.
- Still wrap interactive agents with Seatbelt/user/PF/proxy, or run them in a VM.

### 5. bubblewrap and nsjail on macOS

Mechanism: both are Linux process-isolation tools built around Linux kernel primitives.

Primary/source evidence:

- bubblewrap says it uses Linux user namespaces to build the sandbox: https://github.com/containers/bubblewrap
- Homebrew's formula API describes bubblewrap as an "Unprivileged sandboxing tool for Linux" and provides bottles for `arm64_linux` and `x86_64_linux`, not macOS: https://formulae.brew.sh/api/formula/bubblewrap.json
- nsjail describes itself as a lightweight process isolation tool using Linux namespaces, cgroups, rlimits, and seccomp-bpf filters: https://github.com/google/nsjail
- Homebrew's formula API returned 404 for `nsjail` during this research, so it is not available as a standard Homebrew core formula at that endpoint.

Strengths:

- Stronger Linux process isolation model than macOS can offer in-process: namespaces, cgroups, and seccomp are useful for filesystem, process, network, and syscall limits.
- Good fit inside a Linux VM such as Lima.

Weaknesses:

- Not macOS-native. A wrapper cannot depend on bubblewrap/nsjail directly on the macOS host.
- To use them from macOS, you are really choosing a VM layer first.

Design implications:

- Do not design the macOS host wrapper around bubblewrap or nsjail.
- Offer a Linux-VM backend where the agent runs inside Lima and then uses bubblewrap/nsjail inside the guest.

### 6. VMs: Apple Virtualization Framework, Lima, and UTM

Mechanism: run the agent in a Linux or macOS VM without Docker. Use shared folders, SSH, or a sync bridge to expose the working copy.

Primary/source evidence:

- Apple's Virtualization framework creates and runs macOS and Linux virtual machines: https://developer.apple.com/documentation/virtualization
- Apple says `VZVirtualMachine` emulates a complete hardware machine of the same architecture as the underlying Mac: https://developer.apple.com/documentation/virtualization/vzvirtualmachine
- Lima launches Linux virtual machines with automatic file sharing and port forwarding; its docs say the original containerd focus expanded to non-container applications too: https://lima-vm.io/docs/
- Lima FAQ says macOS uses Virtualization.framework by default, supports QEMU, and has `plain` mode to ignore mounts, port forwarding, and containerd: https://lima-vm.io/docs/faq/
- UTM says it uses Apple's Hypervisor virtualization framework to run ARM64 operating systems on Apple Silicon at near-native speeds, and supports QEMU-based emulation/virtualization: https://mac.getutm.app/
- UTM docs say its Apple Virtualization backend supports only virtualization and is less mature than QEMU, but is the way to run macOS virtualized on Apple Silicon: https://docs.getutm.app/settings-apple/settings-apple/
- Local Homebrew metadata during this research showed Lima available for macOS bottles and UTM available as a cask.

Strengths:

- Stronger boundary than Seatbelt because the agent runs in a guest OS.
- Lets you use Linux-native primitives such as bubblewrap/nsjail/seccomp/Landlock.
- Easier to snapshot, discard, and rebuild.
- Avoids exposing the host home if mounts are kept narrow.

Weaknesses:

- More startup and provisioning overhead.
- File sharing, port forwarding, SSH keys, clipboard, and credential bridges can weaken the boundary.
- A Linux VM cannot directly run macOS-only build/test steps.
- A macOS guest is heavier and has licensing, setup, and automation complexity.

Design implications:

- Provide a "strict VM mode" for untrusted repos and long-running full-auto work.
- With Lima, prefer `plain` mode or explicit minimal mounts; do not mount `~` writable.
- Share only the project worktree and a scratch directory.
- Disable automatic port forwarding unless required.
- Use snapshots/ephemeral disks and throw away the VM after risky sessions.

### 7. Network Restriction

Mechanisms:

- Seatbelt profile denies direct network operations and permits only loopback to a wrapper proxy.
- A proxy outside the sandbox resolves DNS and enforces host/domain/method/path policy.
- PF can block or allow traffic by UID/GID as a host-level backstop when the agent runs as a dedicated user.
- App Sandbox network entitlements exist, but they are coarse app entitlements, not per-command domain policy.

Primary/source evidence:

- Claude Code docs say network access is controlled through a proxy outside the sandbox, no domains are pre-allowed by default, strict allowlists can deny instead of prompt, and restrictions apply to scripts/programs/subprocesses spawned by commands: https://code.claude.com/docs/en/sandboxing
- Anthropic Sandbox Runtime says it uses proxy-based network filtering: https://github.com/anthropic-experimental/sandbox-runtime
- OpenBSD `pf.conf(5)` documents `user`/`group` rules, notes only TCP/UDP packets can be associated with users, and gives examples blocking outbound TCP then allowing selected users: https://man.openbsd.org/pf.conf.5
- Local macOS 26.5 `man pf.conf` contains equivalent `user`/`group` rule semantics and examples.
- Apple App Sandbox entitlements for network are outgoing/incoming socket capabilities, not domain allowlists: https://developer.apple.com/library/archive/documentation/Miscellaneous/Reference/EntitlementKeyReference/Chapters/EnablingAppSandbox.html

Strengths:

- Proxy allowlists give domain-level policy and good audit logs.
- PF UID rules catch direct TCP/UDP attempts that bypass proxy environment variables.
- Seatbelt blocks network at the process boundary before proxy policy is even relevant.

Weaknesses:

- Domain allowlists are not the same as data-loss prevention; an allowed host can still receive secrets.
- If DNS is allowed directly, DNS can become an exfiltration path.
- PF is IP/port/UID oriented, not HTTP-domain semantic.
- TLS inspection requires terminating TLS in the proxy, which creates certificate and trust-management complexity.

Design implications:

- Default to no direct network. Permit only loopback to the wrapper proxy.
- Resolve DNS in the proxy, not in the sandbox.
- Set `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, and conservative `NO_PROXY`, but do not rely on env vars alone.
- Add PF rules for the dedicated agent UID: block outbound TCP/UDP except to localhost proxy ports and any explicitly required local services.
- Consider TLS termination only for brokered credentials where the user/admin explicitly opts in.

### 8. Environment and Secret Brokering

Mechanism: secrets stay outside the sandbox in Keychain/1Password/a wrapper process. The agent receives no secret, a sentinel, or a short-lived scoped token. A trusted broker injects credentials only into approved requests or mediated commands.

Primary/source evidence:

- Apple Platform Security says Keychain stores passwords, keys, and login tokens securely; keychain items use AES-256-GCM keys, and `securityd` determines which items each process or app can access: https://support.apple.com/guide/security/keychain-data-protection-secb0694df1a/web
- The local macOS `security(1)` man page describes the CLI as an interface to administer keychains, keys, certificates, and the Security framework.
- 1Password CLI `op run` loads secrets and runs a command in a subprocess with secrets available as environment variables only for that process duration: https://www.1password.dev/cli/secrets-environment-variables
- Claude Code sandbox docs describe credential `mask` mode: commands see a per-session sentinel and the proxy substitutes the real credential for approved hosts: https://code.claude.com/docs/en/sandboxing
- nono's README describes a credential proxy where tools like `gh` receive a GitHub token through a proxy and endpoint policy can restrict methods/paths: https://github.com/nolabs-ai/nono

Strengths:

- Removes broad `.env`, cloud credential files, SSH keys, and package tokens from the agent's readable filesystem.
- Allows per-service, per-host, and possibly per-route policy.
- Works across Codex/Claude if implemented in the wrapper rather than inside one agent.

Weaknesses:

- Plain environment-variable injection is still visible to the process and subprocesses.
- Credential proxies must carefully scope destination hosts and paths.
- TLS termination for request-body credential substitution is operationally sensitive.
- Keychain access prompts and ACLs can be awkward in headless sessions.

Design implications:

- Start the agent with `env -i` and an explicit env allowlist.
- Remove `SSH_AUTH_SOCK`, cloud SDK env vars, `GITHUB_TOKEN`, `NPM_TOKEN`, and all `*_TOKEN`/`*_KEY` vars unless brokered.
- Prefer brokered credentials over environment injection.
- For `gh`, `npm`, `pip`, `aws`, `gcloud`, `kubectl`, and `ssh`, use child-command policies with separate credentials and network limits.
- Use short-lived, least-scope tokens. Store long-lived secrets only in Keychain/1Password or a hardware-backed manager.

### 9. Command Mediation

Mechanism: a wrapper decides whether an attempted command is allowed before running it, and high-risk tools are run through controlled shims or child sandboxes.

Primary/source evidence:

- OpenAI Codex docs: in Auto mode, Codex can work in the workspace and asks for approval outside the workspace or for commands requiring network access: https://developers.openai.com/codex/agent-approvals-security
- Claude Code permissions docs warn that Read/Edit deny rules apply to built-in file tools and some recognized Bash file commands, but do not apply to arbitrary subprocesses that read/write files indirectly; OS sandboxing is needed for that: https://code.claude.com/docs/en/permissions
- Fence architecture docs include command and SSH policy, shell-chain parsing, default deny rules, runtime executable deny rules, and platform enforcement via macOS Seatbelt or Linux bubblewrap/Landlock/seccomp: https://fencesandbox.com/docs/reference/architecture
- nono describes delegated tools launched with separate filesystem, network, and credential policies outside the agent's control: https://github.com/nolabs-ai/nono

Strengths:

- Catches workflow-level actions that OS file/network sandboxes cannot express cleanly: `git push`, `gh repo delete`, `npm publish`, `kubectl apply`, `terraform apply`, `rm -rf`.
- Gives good UX: ask/deny/allow decisions can be human-readable.
- Can mediate SSH and API clients at a semantic level.

Weaknesses:

- String matching shell commands is fragile.
- If the agent can run arbitrary interpreters, it can reproduce many effects without the original command name.
- On macOS there is no direct seccomp user-notification equivalent for argv-aware child-process approval.

Design implications:

- Treat command mediation as policy UX and defense-in-depth, not the primary boundary.
- Avoid `sh -c` where possible; execute structured argv directly.
- Put shims earlier in `PATH` for `git`, `gh`, `ssh`, package managers, deploy tools, and cloud CLIs.
- Run mediated tools in narrower child policies with their own filesystem/network/credential grants.
- Keep agent-native permissions enabled, but assume prompt-injection can trick the agent into allowed commands.

## Recommended Wrapper Architecture

### Native macOS default mode

Use for normal local coding work where the agent needs fast feedback and host toolchains.

1. Provision or reuse a non-admin `agent-runner` Standard user.
2. Create a per-session workspace under the agent user's home or a controlled project copy.
3. Launch the agent with a scrubbed environment, fake `HOME`, session `TMPDIR`, no inherited secret env vars, and no inherited SSH agent socket.
4. Enter a generated Seatbelt profile that:
   - allows read access to required system/toolchain paths,
   - allows read/write only to workspace and temp,
   - denies reads to secret paths and the real user home,
   - denies direct network except wrapper-owned loopback proxy sockets,
   - permits only required Unix sockets, Mach lookups, TTY/PTTY behavior, and process exec/fork.
5. Run an egress proxy outside the sandbox. It should resolve DNS, log destinations, enforce domain allowlists, and optionally enforce method/path rules for credential-bearing APIs.
6. Add a PF anchor keyed to the agent UID/GID to block outbound TCP/UDP except the local proxy and explicitly allowed local services.
7. Put high-risk tools behind shims or a command broker; child tools get narrower policies than the agent session.
8. Store secrets in Keychain/1Password. The broker injects them only into allowed outbound requests or mediated child commands.
9. Log the resolved policy, executed commands, network destinations, denied accesses, and credential-broker decisions.

### Strict VM mode

Use for unknown repos, destructive migrations, install scripts, malware-like build steps, or long autonomous work.

1. Start an ephemeral Lima VM, UTM VM, or custom Apple Virtualization VM.
2. Mount only the worktree and scratch directory, preferably read-only until the agent needs a write phase.
3. Disable automatic port forwarding and broad host mounts; Lima `plain` mode is a useful baseline.
4. Run the agent inside the guest, then optionally use Linux-native bubblewrap/nsjail inside the VM.
5. Broker credentials from the host through a narrow localhost/SSH channel, not through mounted secret files.
6. Snapshot before the run and discard/revert after review.

## Short Ranking

1. Best native wrapper foundation: Seatbelt/`sandbox-exec` plus dedicated user plus proxy/PF plus secret broker.
2. Best strong-isolation option without Docker: Lima/UTM/Apple Virtualization VM with minimal mounts.
3. Best dependency/reproducibility layer: Nix, but not as the main interactive sandbox.
4. Best Linux-only process tools: bubblewrap/nsjail, but only inside a Linux VM on macOS.
5. Not recommended as security boundary: `chroot`, fakechroot, PRoot on host macOS.
6. Useful but insufficient alone: command mediation and agent permission prompts.

## Open Implementation Questions

- How much host toolchain compatibility must native mode preserve? Seatbelt policies become much harder if Homebrew, language caches, and IDE helpers all need access.
- Should default read access be "read-mostly with explicit secret denies" or "default-deny read with explicit system/toolchain allows"? The latter is safer but will break more tools.
- Is domain-level egress enough, or do GitHub/npm/cloud-provider credentials require method/path-level policy?
- Should the wrapper manage a persistent agent user or create per-session users? Per-session users are cleaner but operationally heavier.
- Do we need macOS GUI automation? If yes, VM mode may be the cleaner boundary because TCC, Accessibility, and Screen Recording permissions complicate native sandboxing.

