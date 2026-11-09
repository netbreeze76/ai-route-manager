#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || { echo "usage: $0 /path/to/AI Route Manager.app" >&2; exit 64; }
APP="$1"
[[ -d "$APP" ]] || { echo "app bundle not found: $APP" >&2; exit 66; }
RESOURCE_ROOT="$APP/Contents/Resources/resources/engine-macos"
[[ -d "$RESOURCE_ROOT" ]] || RESOURCE_ROOT="$APP/Contents/Resources/engine-macos"
[[ -x "$RESOURCE_ROOT/DynamicProxyEngine" ]] || { echo "bundled engine is missing or not executable" >&2; exit 67; }
[[ -x "$RESOURCE_ROOT/bin/mihomo" ]] || { echo "bundled mihomo is missing or not executable" >&2; exit 68; }
[[ -f "$RESOURCE_ROOT/MANIFEST.sha256" ]] || { echo "engine manifest is missing" >&2; exit 69; }
[[ ! -e "$RESOURCE_ROOT/DynamicProxyEngine.exe" ]] || { echo "Windows engine leaked into macOS resources" >&2; exit 72; }

while read -r expected relative; do
  [[ -n "$expected" && -n "$relative" ]] || continue
  actual="$(shasum -a 256 "$RESOURCE_ROOT/$relative" | awk '{print toupper($1)}')"
  [[ "$actual" == "$expected" ]] || { echo "manifest mismatch: $relative" >&2; exit 70; }
done < "$RESOURCE_ROOT/MANIFEST.sha256"

codesign --verify --deep --strict --verbose=2 "$APP"
codesign --verify --strict --verbose=2 "$RESOURCE_ROOT/DynamicProxyEngine"
codesign --verify --strict --verbose=2 "$RESOURCE_ROOT/bin/mihomo"
spctl --assess --type execute --verbose=2 "$APP" || {
  if [[ -n "${APPLE_SIGNING_IDENTITY:-}" && "$APPLE_SIGNING_IDENTITY" != "-" ]]; then
    echo "Gatekeeper assessment failed for a Developer ID build" >&2
    exit 71
  fi
  echo "GATEKEEPER_NOTE=ad-hoc local build is expected to require manual approval"
}
"$RESOURCE_ROOT/bin/mihomo" -v | head -n 1
echo "MACOS_BUNDLE_VERIFY_OK app=$APP"
