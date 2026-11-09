# DynamicProxyEngine

`dynamic_proxy_manager.py` is the source used to build the bundled
`DynamicProxyEngine` sidecar. It refreshes user-supplied Clash-compatible
subscriptions, builds a Mihomo configuration and performs guarded A/B cutovers.

## Routing invariants

- AI destinations use only an explicitly approved fixed-region exit and fail
  closed when that exit is unavailable.
- General foreign and large-transfer traffic never falls back to the fixed AI
  egress.
- Mainland China and configured account-sensitive services remain direct.
- Candidate cores are validated before the stable local front door changes.
- Runtime configuration and credentials are not committed to the repository.

## Test

```powershell
python -m unittest discover -s engine -p "test_*.py" -v
```

## Build

Windows:

```powershell
.\scripts\build-engine.ps1
```

macOS:

```bash
./scripts/build-engine-macos.sh
```

The scripts package this source with PyInstaller and regenerate
`MANIFEST.sha256`. Mihomo and routing-data provenance is documented in
[`../THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md).
