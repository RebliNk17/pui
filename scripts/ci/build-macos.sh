#!/bin/bash
set -euo pipefail
# Do not expose passwords via shell tracing, including inherited SHELLOPTS.
set +x
[[ "${CI_DEBUG_TRACE:-false}" != true ]] || { echo "Disable CI_DEBUG_TRACE for release jobs." >&2; exit 1; }
cd "$(dirname "$0")/../.."
python3 scripts/release.py check
case "${PUI_REQUIRE_SIGNING:-false}" in true|false) ;; *) echo "PUI_REQUIRE_SIGNING must be true or false." >&2; exit 1 ;; esac
export PUI_SIGNING_STATUS=ad-hoc
signing=false
if [[ -n "${CI_COMMIT_TAG:-}" && "${CI_COMMIT_REF_PROTECTED:-false}" == true ]]; then
  if [[ -n "${PUI_SIGN_IDENTITY:-}${PUI_SIGN_P12:-}${PUI_SIGN_P12_BASE64:-}${PUI_SIGN_PASSWORD:-}${PUI_APPLE_ID:-}${PUI_APPLE_TEAM_ID:-}${PUI_APPLE_APP_PASSWORD:-}" ]]; then
    : "${PUI_SIGN_IDENTITY:?All signing/notarization variables must be configured}"
    [[ -n "${PUI_SIGN_P12:-}${PUI_SIGN_P12_BASE64:-}" ]] || { echo "Configure PUI_SIGN_P12_BASE64 (protected File variable) or PUI_SIGN_P12." >&2; exit 1; }
    [[ -z "${PUI_SIGN_P12:-}" || -z "${PUI_SIGN_P12_BASE64:-}" ]] || { echo "Configure only one certificate source." >&2; exit 1; }
    : "${PUI_SIGN_PASSWORD:?Missing certificate password}"
    : "${PUI_APPLE_ID:?Missing notarization Apple ID}"
    : "${PUI_APPLE_TEAM_ID:?Missing Apple team ID}"
    : "${PUI_APPLE_APP_PASSWORD:?Missing app-specific notarization password}"
    [[ "$PUI_SIGN_IDENTITY" == 'Developer ID Application:'* ]] || { echo "Expected Developer ID Application identity." >&2; exit 1; }
    [[ -f "${PUI_SIGN_P12:-${PUI_SIGN_P12_BASE64:-}}" ]] || { echo "The certificate must be a GitLab File variable." >&2; exit 1; }
    signing=true
  elif [[ "${PUI_REQUIRE_SIGNING:-false}" == true ]]; then
    echo "This project requires signing credentials for version tags." >&2
    exit 1
  fi
fi
if [[ "$signing" == true ]]; then
  private=$(mktemp -d "${TMPDIR:-/tmp}/pui-signing.XXXXXXXX")
  chmod 700 "$private"
  security list-keychains -d user > "$private/search-list"
  cleanup() {
    local result=$?
    trap - EXIT
    # Restore the original search list, even on import/notarization failure.
    local keychains=()
    while IFS= read -r path; do keychains+=("$path"); done < <(python3 -c 'import shlex,sys; print("\n".join(shlex.split(open(sys.argv[1]).read())))' "$private/search-list")
    security list-keychains -d user -s "${keychains[@]}" || true
    security delete-keychain "$private/release.keychain-db" >/dev/null 2>&1 || true
    python3 -c 'import shutil,sys; shutil.rmtree(sys.argv[1])' "$private"
    exit "$result"
  }
  trap cleanup EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  keychain_password=$(openssl rand -hex 32)
  export PUI_SIGN_KEYCHAIN="$private/release.keychain-db"
  security create-keychain -p "$keychain_password" "$PUI_SIGN_KEYCHAIN"
  security set-keychain-settings -lut 21600 "$PUI_SIGN_KEYCHAIN"
  security unlock-keychain -p "$keychain_password" "$PUI_SIGN_KEYCHAIN"
  if [[ -n "${PUI_SIGN_P12_BASE64:-}" ]]; then
    python3 -c 'import base64,pathlib,sys; pathlib.Path(sys.argv[2]).write_bytes(base64.b64decode(b"".join(pathlib.Path(sys.argv[1]).read_bytes().split()), validate=True))' "$PUI_SIGN_P12_BASE64" "$private/certificate.p12"
    PUI_SIGN_P12="$private/certificate.p12"
  fi
  security import "$PUI_SIGN_P12" -k "$PUI_SIGN_KEYCHAIN" -P "$PUI_SIGN_PASSWORD" -T /usr/bin/codesign -T /usr/bin/security >/dev/null
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$PUI_SIGN_KEYCHAIN" >/dev/null
  security list-keychains -d user -s "$PUI_SIGN_KEYCHAIN"
  export PUI_NOTARY_PROFILE=pui-release
  xcrun notarytool store-credentials "$PUI_NOTARY_PROFILE" --keychain "$PUI_SIGN_KEYCHAIN" \
    --apple-id "$PUI_APPLE_ID" --team-id "$PUI_APPLE_TEAM_ID" --password "$PUI_APPLE_APP_PASSWORD" >/dev/null
  unset keychain_password PUI_SIGN_PASSWORD PUI_APPLE_APP_PASSWORD
else
  # No ambient identity/profile may accidentally sign an untrusted build.
  unset PUI_SIGN_IDENTITY PUI_SIGN_KEYCHAIN PUI_NOTARY_PROFILE PUI_SIGN_P12 PUI_SIGN_P12_BASE64 PUI_SIGN_PASSWORD PUI_APPLE_ID PUI_APPLE_TEAM_ID PUI_APPLE_APP_PASSWORD
fi
bash scripts/bundle-macos.sh
if [[ "$signing" == true ]]; then
  ditto -c -k --sequesterRsrc --keepParent dist/Pui.app "$private/notarize.zip"
  bash scripts/notarize-macos.sh "$private/notarize.zip"
  xcrun stapler staple dist/Pui.app
  xcrun stapler validate dist/Pui.app
  spctl --assess --type execute --verbose=2 dist/Pui.app
  export PUI_SIGNING_STATUS=developer-id-notarized
fi
bash scripts/package-macos.sh
python3 scripts/release.py prepare
