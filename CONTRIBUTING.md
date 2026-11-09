# Contributing

## Development checks

```powershell
corepack enable
pnpm install --frozen-lockfile
pnpm check
pnpm build
.\scripts\test-rust-gnu.ps1
python -m unittest discover -s engine -p "test_*.py" -v
```

Changes to routing, credentials, updates, firewall rules or remote deployment
must include a regression test and preserve preview, explicit confirmation,
atomic commit and rollback behavior.

Do not commit subscription URLs, credentials, private keys, signing material,
diagnostic archives, generated installers or local runtime state.

By submitting a contribution, you agree that it is licensed under GPL-3.0-only.
