#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != Darwin ]]; then
  echo "The .app packager runs on macOS only." >&2
  exit 1
fi
profile=release
cargo_flags=(--release --locked)
case "${1:-}" in
  '') ;;
  --debug) profile=debug; cargo_flags=(--locked) ;;
  *) echo "Usage: $0 [--debug]" >&2; exit 1 ;;
esac
[[ $# -le 1 ]] || { echo "Usage: $0 [--debug]" >&2; exit 1; }
[[ -z "${CARGO_BUILD_TARGET:-}" ]] || { echo "Use a native build without CARGO_BUILD_TARGET." >&2; exit 1; }
# CEF's build requires CMake/Ninja. Download only the pinned, hash-checked
# runtime before Cargo so custom TLS roots can use the system curl stack.
command -v cmake >/dev/null || { echo "CMake is required to build Chromium." >&2; exit 1; }
command -v ninja >/dev/null || { echo "Ninja is required to build Chromium." >&2; exit 1; }
if [[ -z "${CEF_PATH:-}" ]]; then
  CEF_PATH=$(python3 scripts/fetch-chromium-macos.py "${CARGO_TARGET_DIR:-target}/cef-runtime")
  export CEF_PATH
fi
cargo build --bins "${cargo_flags[@]}"
# Build into a fresh bundle: overlaying an older signed CEF framework leaves
# stale resources/signatures behind. Never delete an existing local bundle.
mkdir -p dist
staging=$(mktemp -d "$PWD/dist/.pui-bundle.XXXXXXXX")
trap 'python3 -c '\''import shutil,sys; shutil.rmtree(sys.argv[1])'\'' "$staging"' EXIT
app="$staging/Pui.app"
resources="$app/Contents/Resources"
iconset="$staging/pui.iconset"
mkdir -p "$app/Contents/MacOS" "$resources" "$iconset"
cp "${CARGO_TARGET_DIR:-target}/$profile/pui" "$app/Contents/MacOS/pui"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" assets/icon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" assets/icon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$resources/Pui.icns"
cp scripts/ultracode.ts scripts/permissions.ts scripts/compaction.ts scripts/context.ts scripts/rewind.ts scripts/questions.ts scripts/open.ts scripts/workflows.ts scripts/antigravity.ts scripts/accounts.ts scripts/dictation.swift "$resources/"
cp -R assets/pui-engineering "$resources/pui-engineering"
# Retain the new renderer's original notices in distributed local/CI bundles.
mkdir -p "$resources/ThirdParty"
cp -R assets/third-party/svg "$resources/ThirdParty/svg"
python3 scripts/bundle-chromium-macos.py "$app" "${CARGO_TARGET_DIR:-target}" "$profile"
python3 scripts/bundle-runtime-macos.py "$app" "${CARGO_TARGET_DIR:-target}"
cp packaging/Info.plist "$app/Contents/Info.plist"
# Cargo is the release-version source of truth.
version=$(cargo pkgid --offline | awk -F# '{print $NF}' | awk -F@ '{print $NF}')
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion ${CI_PIPELINE_IID:-1}" "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"
python3 scripts/sign-macos.py "$app"
if [[ -e dist/Pui.app ]]; then
  previous=$(mktemp -d "$PWD/dist/Pui.previous.XXXXXXXX")
  mv dist/Pui.app "$previous/Pui.app"
  echo "Previous bundle retained at $previous/Pui.app"
fi
mv "$app" dist/Pui.app
echo "Built $PWD/dist/Pui.app ($profile, $(uname -m))."
echo "Open with: open dist/Pui.app"
if [[ "${PUI_SIGN_IDENTITY:--}" == - ]]; then
  echo "Local ad-hoc signed build; not Developer ID signed or notarized."
else
  echo "Developer ID signed; notarization is performed separately by CI."
fi
