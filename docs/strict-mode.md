# Strict Isolation Mode

Strict mode is a backend selection contract, not a silent fallback. `macos-seatbelt` is the current default. A future `vm` backend must either launch inside a Virtualization.framework/Lima-style VM or fail closed with an actionable message.

The VM backend must provide:

- no host home or credential mounts;
- an explicit workspace sharing mode;
- supervisor-controlled network egress;
- ephemeral VM state by default;
- a documented port-forwarding model;
- the same request, approval, secret, and audit protocol as native mode.

Until those guarantees are implemented, requesting an unsupported backend is rejected rather than downgraded to native or unconfined execution.

