# Troubleshooting

`safe-agent status --json` reports whether the current process is inside a session. A request outside a session is denied rather than creating authority.

If Seatbelt cannot launch, Safe Agent returns an error and does not fall back to an unconfined process. Use `--backend none-for-debug` only when developing the wrapper or its tests.

If a file is blocked, the child sees a normal command failure and can inspect the structured `safe-agent` denial. The intended recovery is to request a narrowly scoped capability or choose a workflow that does not need the protected resource.

