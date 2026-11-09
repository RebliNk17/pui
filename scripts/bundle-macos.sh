#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != Darwin ]]; then
  echo "The .app packager runs on macOS only." >&2
  exit 1
fi
profile=release
cargo_flags=(--release --locked)
if [[ "${1:-}" == --debug ]]; then
  profile=debug
  cargo_flags=(--locked)
fi
cargo build "${cargo_flags[@]}"
app="$PWD/dist/Pui.app"
resources="$app/Contents/Resources"
iconset="$PWD/target/pui.iconset"
mkdir -p "$app/Contents/MacOS" "$resources" "$iconset"
cp "${CARGO_TARGET_DIR:-target}/$profile/pui" "$app/Contents/MacOS/pui"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" assets/icon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" assets/icon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$resources/Pui.icns"
cp scripts/ultracode.ts scripts/permissions.ts scripts/compaction.ts scripts/rewind.ts scripts/questions.ts "$resources/"
cp packaging/Info.plist "$app/Contents/Info.plist"
# Cargo is the release-version source of truth, including future GitHub checks.
version=$(cargo pkgid --offline | awk -F# '{print $NF}' | awk -F@ '{print $NF}')
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"
# Seal the local bundle (no developer identity); Apple Silicon requires a valid signature.
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"
echo "Built $app ($profile, $(uname -m))."
echo "Open with: open dist/Pui.app"
echo "Local ad-hoc signed build; Developer ID signing/notarization is not configured."
