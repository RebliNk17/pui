#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ "$(uname -s)/$(uname -m)" == Darwin/arm64 ]] || { echo "Release packaging requires native Apple Silicon macOS." >&2; exit 1; }
version=$(python3 scripts/release.py check)
status=${PUI_SIGNING_STATUS:-ad-hoc}
suffix=-adhoc
case "$status" in
  ad-hoc) ;;
  developer-id-notarized)
    : "${PUI_SIGN_IDENTITY:?Missing signing identity}"
    xcrun stapler validate dist/Pui.app
    spctl --assess --type execute dist/Pui.app
    suffix=
    ;;
  *) echo "Unknown signing status." >&2; exit 1 ;;
esac
app="$PWD/dist/Pui.app"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")" == "$version" ]] || { echo "Bundle version mismatch." >&2; exit 1; }
[[ "$(lipo -archs "$app/Contents/MacOS/pui")" == arm64 ]] || { echo "Expected an arm64 app." >&2; exit 1; }
codesign --verify --deep --strict "$app"
output="$PWD/dist/releases"
mkdir -p "$output"
# Avoid shipping stale release artifacts from a previous local run.
if [[ -n "$(find "$output" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
  echo "dist/releases must be empty; move previous artifacts aside first." >&2
  exit 1
fi
name="Pui-${version}-macos-arm64${suffix}"
ditto -c -k --sequesterRsrc --keepParent "$app" "$output/$name.zip"
staging=$(mktemp -d "${TMPDIR:-/tmp}/pui-dmg.XXXXXXXX")
trap 'python3 -c '\''import shutil,sys; shutil.rmtree(sys.argv[1])'\'' "$staging"' EXIT
# ditto preserves framework symlinks, executable permissions, and stapled tickets.
ditto "$app" "$staging/Pui.app"
ln -s /Applications "$staging/Applications"
hdiutil create -volname "Pui $version" -srcfolder "$staging" -format UDZO -ov "$output/$name.dmg"
if [[ "$status" == developer-id-notarized ]]; then
  codesign --force --sign "$PUI_SIGN_IDENTITY" --keychain "$PUI_SIGN_KEYCHAIN" --timestamp "$output/$name.dmg"
  bash scripts/notarize-macos.sh "$output/$name.dmg"
  xcrun stapler staple "$output/$name.dmg"
  xcrun stapler validate "$output/$name.dmg"
  spctl --assess --type open --context context:primary-signature "$output/$name.dmg"
fi
hdiutil verify "$output/$name.dmg"
echo "Packaged $name.zip and $name.dmg ($status)."
