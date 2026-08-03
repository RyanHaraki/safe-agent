# Secrets

Secret values are not placed in `.env` files visible to the child. Set one from the host with:

```sh
safe-agent secrets add DATABASE_URL
printf '%s' "$DATABASE_URL" | safe-agent secrets add DATABASE_URL --stdin
```

On macOS, values are stored in Keychain. During tests, `SAFE_AGENT_TEST_SECRET_BACKEND=memory` selects the test backend. A repo declares allowed names and exact commands in TOML. An agent requests one command with `safe-agent request secret NAME --for "command"`; Safe Agent approves, injects the value into that subprocess only, and redacts the value from captured output.

