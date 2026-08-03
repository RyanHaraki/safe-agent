# Architecture

The host user starts a trusted `safe-agent` supervisor. The supervisor resolves TOML policy, creates a fresh session directory, generates command shims and a macOS Seatbelt profile, starts a Unix control socket, and launches the requested agent as an untrusted child.

The child edits the live workspace. Its `HOME` and `TMPDIR` point into the disposable session directory, while the child environment is rebuilt from an allowlist. The `safe-agent` shim carries the session socket path explicitly, so runtime discovery does not require Safe Agent-specific environment variables.

The operating-system profile is the hard boundary. Skills, denial messages, and requests improve the workflow but are not security controls.

