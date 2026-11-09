#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT"

MODE="${1:---local-test}"
[[ $# -le 1 ]] || { echo "usage: $0 [--local-test|--release]" >&2; exit 64; }
case "$MODE" in
  --local-test|--release) ;;
  *) echo "usage: $0 [--local-test|--release]" >&2; exit 64 ;;
esac

[[ "$(uname -s)" == "Darwin" ]] || { echo "build-macos.sh must run on macOS" >&2; exit 2; }
for command in xcodebuild xcrun rustup pnpm cargo plutil python3; do
  command -v "$command" >/dev/null || { echo "missing command: $command" >&2; exit 3; }
done
xcodebuild -version

MACOS_CONFIG="$PROJECT/src-tauri/tauri.macos.conf.json"
[[ -f "$MACOS_CONFIG" ]] || { echo "missing platform config: $MACOS_CONFIG" >&2; exit 7; }
python3 - "$MACOS_CONFIG" <<'PY'
import json, sys
config = json.load(open(sys.argv[1], encoding="utf-8"))
resources = config.get("bundle", {}).get("resources")
assert resources == ["resources/engine-macos/**/*"], resources
macos = config["bundle"]["macOS"]
assert macos.get("minimumSystemVersion") == "11.0"
assert macos.get("hardenedRuntime") is True
print("MACOS_PLATFORM_CONFIG_OK=resources/engine-macos/**/*")
PY

BUILD_CONFIG=""
BUILD_CONFIG_DIR=""
cleanup_build_config() {
  if [[ -n "$BUILD_CONFIG" && -f "$BUILD_CONFIG" ]]; then
    rm -f -- "$BUILD_CONFIG"
  fi
  if [[ -n "$BUILD_CONFIG_DIR" && -d "$BUILD_CONFIG_DIR" ]]; then
    rmdir -- "$BUILD_CONFIG_DIR"
  fi
}
trap cleanup_build_config EXIT
TAURI_CONFIG_ARGS=()

if [[ "$MODE" == "--local-test" ]]; then
  unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID APPLE_API_ISSUER APPLE_API_KEY APPLE_API_KEY_PATH
  export APPLE_SIGNING_IDENTITY="-"
  NOTARIZATION_MODE="ad-hoc-local-test"
  BUILD_CONFIG_DIR="$(mktemp -d "${TMPDIR:-/tmp}/ai-route-manager-local-test.XXXXXX")"
  BUILD_CONFIG="$BUILD_CONFIG_DIR/tauri.local-test.conf.json"
  printf '%s\n' '{"bundle":{"createUpdaterArtifacts":false}}' > "$BUILD_CONFIG"
  TAURI_CONFIG_ARGS=(--config "$BUILD_CONFIG")
else
  : "${TAURI_SIGNING_PRIVATE_KEY:?Release builds require the existing updater private key}"
  : "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:?Release builds require the updater private key password}"
  : "${APPLE_SIGNING_IDENTITY:?Release builds require a Developer ID Application identity}"
  [[ "$APPLE_SIGNING_IDENTITY" != "-" ]] || { echo "release mode rejects ad-hoc signing" >&2; exit 9; }
  if [[ -n "${APPLE_ID:-}" || -n "${APPLE_PASSWORD:-}" || -n "${APPLE_TEAM_ID:-}" ]]; then
    : "${APPLE_ID:?Developer ID builds require APPLE_ID or App Store Connect API credentials}"
    : "${APPLE_PASSWORD:?Developer ID builds require APPLE_PASSWORD}"
    : "${APPLE_TEAM_ID:?Developer ID builds require APPLE_TEAM_ID}"
    NOTARIZATION_MODE="apple-id"
  elif [[ -n "${APPLE_API_ISSUER:-}" || -n "${APPLE_API_KEY:-}" || -n "${APPLE_API_KEY_PATH:-}" ]]; then
    : "${APPLE_API_ISSUER:?App Store Connect notarization requires APPLE_API_ISSUER}"
    : "${APPLE_API_KEY:?App Store Connect notarization requires APPLE_API_KEY}"
    : "${APPLE_API_KEY_PATH:?App Store Connect notarization requires APPLE_API_KEY_PATH}"
    [[ -f "$APPLE_API_KEY_PATH" ]] || { echo "APPLE_API_KEY_PATH does not exist" >&2; exit 8; }
    NOTARIZATION_MODE="app-store-connect"
  else
    echo "release mode requires complete Apple ID or App Store Connect notarization credentials" >&2
    exit 9
  fi
fi
echo "MACOS_BUILD_MODE=$MODE"
echo "MACOS_SIGNING_MODE=$NOTARIZATION_MODE"

case "$(uname -m)" in
  arm64) RUST_TARGET="aarch64-apple-darwin" ;;
  x86_64) RUST_TARGET="x86_64-apple-darwin" ;;
  *) echo "unsupported macOS architecture: $(uname -m)" >&2; exit 4 ;;
esac

bash "$SCRIPT_DIR/build-engine-macos.sh"
rustup target add "$RUST_TARGET"
pnpm install --frozen-lockfile
pnpm check
cargo test --manifest-path src-tauri/Cargo.toml --lib
pnpm tauri build --target "$RUST_TARGET" --bundles dmg "${TAURI_CONFIG_ARGS[@]}"

APP_DIR="$PROJECT/src-tauri/target/$RUST_TARGET/release/bundle/macos"
APP="$(find "$APP_DIR" -maxdepth 1 -type d -name '*.app' -print -quit)"
[[ -n "$APP" && -d "$APP" ]] || { echo "macOS app bundle was not created" >&2; exit 5; }
bash "$SCRIPT_DIR/verify-macos-bundle.sh" "$APP"

DMG_DIR="$PROJECT/src-tauri/target/$RUST_TARGET/release/bundle/dmg"
DMG="$(find "$DMG_DIR" -maxdepth 1 -type f -name '*.dmg' -print -quit)"
[[ -n "$DMG" && -f "$DMG" ]] || { echo "macOS DMG was not created" >&2; exit 6; }

if [[ "$MODE" == "--release" ]]; then
  xcrun stapler validate "$APP"
  xcrun stapler validate "$DMG"
  echo "MACOS_NOTARIZATION_STAPLE_OK=1"
fi

VERSION="$(python3 -c 'import json; print(json.load(open("package.json", encoding="utf-8"))["version"])')"
if [[ "$MODE" == "--release" ]]; then
  RELEASE_KIND="macos"
else
  RELEASE_KIND="macos-local-test"
fi
RELEASE_DIR="$PROJECT/release/$VERSION/$RELEASE_KIND/$RUST_TARGET"
STAGING_DIR="$PROJECT/release/$VERSION/.$RELEASE_KIND-$RUST_TARGET-$$"
mkdir -p "$STAGING_DIR"
cp -p "$DMG" "$STAGING_DIR/"
if [[ "$MODE" == "--release" ]]; then
  while IFS= read -r artifact; do
    cp -p "$artifact" "$STAGING_DIR/"
  done < <(find "$APP_DIR" -maxdepth 1 -type f \( -name '*.app.tar.gz' -o -name '*.app.tar.gz.sig' \) -print | LC_ALL=C sort)
  [[ -n "$(find "$STAGING_DIR" -maxdepth 1 -type f -name '*.app.tar.gz.sig' -print -quit)" ]] || { echo "release mode did not create updater signature" >&2; exit 12; }
fi
[[ -f "$STAGING_DIR/$(basename "$DMG")" ]] || { echo "release staging lost DMG" >&2; exit 10; }
find "$STAGING_DIR" -maxdepth 1 -type f ! -name 'SHA256SUMS.txt' -print \
  | LC_ALL=C sort \
  | while IFS= read -r artifact; do
      hash="$(shasum -a 256 "$artifact" | awk '{print toupper($1)}')"
      printf '%s  %s\n' "$hash" "$(basename "$artifact")"
    done > "$STAGING_DIR/SHA256SUMS.txt"
[[ ! -e "$RELEASE_DIR" ]] || { echo "release directory already exists: $RELEASE_DIR" >&2; exit 11; }
mkdir -p "$(dirname "$RELEASE_DIR")"
mv "$STAGING_DIR" "$RELEASE_DIR"

shasum -a 256 "$DMG"
echo "MACOS_RELEASE_DIR=$RELEASE_DIR"
echo "MACOS_BUILD_OK target=$RUST_TARGET dmg=$DMG signing=$NOTARIZATION_MODE mode=$MODE"
