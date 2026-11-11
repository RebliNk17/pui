#!/bin/bash
set -euo pipefail
set +x
: "${PUI_NOTARY_PROFILE:?Missing notarytool profile}"
: "${PUI_SIGN_KEYCHAIN:?Missing temporary signing keychain}"
# A wait timeout or rejected submission is a hard failure, never an ad-hoc fallback.
xcrun notarytool submit "$1" --keychain-profile "$PUI_NOTARY_PROFILE" \
  --keychain "$PUI_SIGN_KEYCHAIN" --wait --timeout 30m --output-format json |
  python3 -c 'import json,sys; result=json.load(sys.stdin); print("Notarization status:", result.get("status", "unknown")); sys.exit(0 if result.get("status") == "Accepted" else 1)'
