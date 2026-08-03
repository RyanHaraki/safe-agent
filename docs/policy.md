# Policy

Project policy is TOML at `.safe-agent/policy.toml`. User configuration is at `~/.config/safe-agent/config.toml`; secret mappings are at `~/.config/safe-agent/secrets.toml`.

The default network mode is `ask`. Explicit denies and dangerous private destinations win over allow and ask rules. Project policy can describe project needs, but it cannot silently grant host credentials or unrestricted access.

Useful commands:

```sh
safe-agent policy validate
safe-agent policy explain path .env --action read
safe-agent policy explain network registry.npmjs.org
safe-agent policy reload
```

