#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT="$(cd "$SCRIPT_DIR/.." && pwd)"
SOURCE="$PROJECT/engine/dynamic_proxy_manager.py"
TEST_SOURCE="$PROJECT/engine/test_dynamic_proxy_manager.py"
COMMON_RESOURCES="$PROJECT/src-tauri/resources/engine/resources"
STAGING="$PROJECT/src-tauri/resources/engine-macos"
WORK="$PROJECT/work/macos-engine"
VENV="$WORK/venv"
CACHE="$WORK/cache"
BUILD="$WORK/build"
DIST="$WORK/dist"
MIHOMO_VERSION="1.19.30"

[[ "$(uname -s)" == "Darwin" ]] || { echo "build-engine-macos.sh must run on macOS" >&2; exit 2; }
for command in python3 curl gzip shasum codesign; do
  command -v "$command" >/dev/null || { echo "missing command: $command" >&2; exit 3; }
done
[[ -f "$SOURCE" && -f "$TEST_SOURCE" ]] || { echo "dynamic proxy manager source is missing" >&2; exit 4; }

case "$(uname -m)" in
  arm64)
    ARCH="arm64"
    ASSET="mihomo-darwin-arm64-v${MIHOMO_VERSION}.gz"
    ASSET_SHA256="2c7f3a7904fa1cee291e124123e630e7b1ebd13765dd9bf26c0a28432004d9f4"
    ;;
  x86_64)
    ARCH="x86_64"
    ASSET="mihomo-darwin-amd64-compatible-v${MIHOMO_VERSION}.gz"
    ASSET_SHA256="6e75de0732e8afabe413ff7c235e8f16226ce136672371c60787cbf9607402c5"
    ;;
  *) echo "unsupported macOS architecture: $(uname -m)" >&2; exit 5 ;;
esac

mkdir -p "$WORK" "$CACHE" "$BUILD" "$DIST" "$STAGING/bin" "$STAGING/resources"
if [[ ! -x "$VENV/bin/python" ]]; then
  python3 -m venv "$VENV"
fi
"$VENV/bin/python" -m pip install --disable-pip-version-check --requirement "$SCRIPT_DIR/requirements-engine.txt"
"$VENV/bin/python" -m PyInstaller \
  --noconfirm --clean --onefile \
  --name DynamicProxyEngine \
  --distpath "$DIST" --workpath "$BUILD" \
  --specpath "$WORK" \
  "$SOURCE"

ENGINE="$DIST/DynamicProxyEngine"
[[ -x "$ENGINE" ]] || { echo "PyInstaller did not create $ENGINE" >&2; exit 6; }
install -m 0755 "$ENGINE" "$STAGING/DynamicProxyEngine"

ASSET_PATH="$CACHE/$ASSET"
if [[ -f "$ASSET_PATH" ]]; then
  current_sha="$(shasum -a 256 "$ASSET_PATH" | awk '{print $1}')"
  [[ "$current_sha" == "$ASSET_SHA256" ]] || rm -f "$ASSET_PATH"
fi
if [[ ! -f "$ASSET_PATH" ]]; then
  curl --fail --location --retry 3 --output "$ASSET_PATH" \
    "https://github.com/MetaCubeX/mihomo/releases/download/v${MIHOMO_VERSION}/${ASSET}"
fi
actual_sha="$(shasum -a 256 "$ASSET_PATH" | awk '{print $1}')"
[[ "$actual_sha" == "$ASSET_SHA256" ]] || { echo "mihomo checksum mismatch: $actual_sha" >&2; exit 7; }
gzip -dc "$ASSET_PATH" > "$STAGING/bin/mihomo"
chmod 0755 "$STAGING/bin/mihomo"

for resource in ASN.mmdb GEOIP.dat GEOIP.metadb GEOSITE.dat; do
  [[ -f "$COMMON_RESOURCES/$resource" ]] || { echo "missing geodata: $resource" >&2; exit 8; }
  install -m 0644 "$COMMON_RESOURCES/$resource" "$STAGING/resources/$resource"
done

SIGN_IDENTITY="${APPLE_SIGNING_IDENTITY:--}"
if [[ "$SIGN_IDENTITY" == "-" ]]; then
  codesign --force --timestamp=none --sign - "$STAGING/bin/mihomo"
  codesign --force --timestamp=none --sign - "$STAGING/DynamicProxyEngine"
else
  codesign --force --timestamp --options runtime --sign "$SIGN_IDENTITY" "$STAGING/bin/mihomo"
  codesign --force --timestamp --options runtime --sign "$SIGN_IDENTITY" "$STAGING/DynamicProxyEngine"
fi

"$VENV/bin/python" -m unittest discover -s "$(dirname "$TEST_SOURCE")" -p 'test_*.py' -q
"$STAGING/bin/mihomo" -v | head -n 1
SMOKE_HOME="$WORK/smoke-home-$ARCH"
mkdir -p "$SMOKE_HOME"
"$STAGING/DynamicProxyEngine" status --home "$SMOKE_HOME" > "$WORK/status-$ARCH.json"
"$VENV/bin/python" -c 'import json,sys; data=json.load(open(sys.argv[1], encoding="utf-8")); assert data["manager_running"] is False' "$WORK/status-$ARCH.json"

MANIFEST="$STAGING/MANIFEST.sha256"
: > "$MANIFEST"
while IFS= read -r file; do
  relative="${file#"$STAGING/"}"
  hash="$(shasum -a 256 "$file" | awk '{print toupper($1)}')"
  printf '%s  %s\n' "$hash" "$relative" >> "$MANIFEST"
done < <(find "$STAGING" -type f ! -name 'MANIFEST.sha256' -print | LC_ALL=C sort)

echo "MAC_ENGINE_BUILD_OK arch=$ARCH mihomo=$MIHOMO_VERSION"
echo "MAC_ENGINE_STAGED=$STAGING"
