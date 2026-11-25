#!/bin/bash
# Credential-free GitHub Actions macOS builder; publication uses a separate job.
set -euo pipefail
set +x
cd "$(dirname "$0")/../.."
[[ "${GITHUB_ACTIONS:-}" == true ]] || { echo "This entry point requires GitHub Actions." >&2; exit 1; }
export CI_PROJECT_DIR="$PWD"
export CARGO_HOME="$PWD/.cargo-ci"
export CARGO_TARGET_DIR="$PWD/target"
mkdir -p "$CARGO_HOME"
cp scripts/ci/cargo-config.toml "$CARGO_HOME/config.toml"
export CI_COMMIT_SHA="${GITHUB_SHA:?Missing GitHub Actions revision}"
[[ "$(git rev-parse HEAD)" == "$CI_COMMIT_SHA" ]] || { echo "Checkout does not match the GitHub Actions revision." >&2; exit 1; }
export CI_COMMIT_TAG=""
if [[ "${GITHUB_REF_TYPE:-}" == tag && "${GITHUB_REF_NAME:-}" == v* ]]; then
  export CI_COMMIT_TAG="$GITHUB_REF_NAME"
fi
export CI_PIPELINE_IID="${GITHUB_RUN_NUMBER:?Missing GitHub Actions run number}"
export CI_COMMIT_REF_PROTECTED=false
export PUI_REQUIRE_SIGNING=false
export PUI_GITHUB_REPOSITORY="${GITHUB_REPOSITORY:-RebliNk17/pui}"
unset PUI_PI_BIN PUI_PI_SDK
unset PUI_GITLAB_RELEASE_TOKEN GITHUB_TOKEN GH_TOKEN
unset CI_JOB_TOKEN PUI_SIGN_IDENTITY PUI_SIGN_KEYCHAIN PUI_NOTARY_PROFILE PUI_SIGN_P12 PUI_SIGN_P12_BASE64 PUI_SIGN_PASSWORD PUI_APPLE_ID PUI_APPLE_TEAM_ID PUI_APPLE_APP_PASSWORD
# Suite selection from scripts/ci/affected.py (pull requests only). Unset
# means true, so pushes, tags and manual runs always validate everything.
for suite in RUST NODE PYTHON PACKAGE; do
  var="PUI_CI_$suite"
  case "${!var:-true}" in true|false) export "$var=${!var:-true}" ;; *) echo "$var must be true or false." >&2; exit 1 ;; esac
done
if [[ -n "$CI_COMMIT_TAG" ]]; then
  PUI_CI_RUST=true PUI_CI_NODE=true PUI_CI_PYTHON=true PUI_CI_PACKAGE=true
fi
export PUI_CI_NEEDS_RUST=false
[[ "$PUI_CI_RUST" == true || "$PUI_CI_PACKAGE" == true ]] && PUI_CI_NEEDS_RUST=true
echo "Selected suites: rust=$PUI_CI_RUST node=$PUI_CI_NODE python=$PUI_CI_PYTHON package=$PUI_CI_PACKAGE"
source scripts/ci/setup-macos.sh
python3 scripts/release.py check
for script in scripts/bundle-macos.sh scripts/package-macos.sh scripts/notarize-macos.sh scripts/ci/*.sh; do
  bash -n "$script"
done
if [[ "$PUI_CI_PYTHON" == true ]]; then
  python3 -m unittest discover -s tests -p 'test_*.py' -v
fi
if [[ "$PUI_CI_RUST" == true ]]; then
  cargo fmt --check
  cargo clippy --locked --all-targets -- -D warnings
  # Complete suite: bounded bulk plus all serial contention-sensitive groups.
  bash scripts/test-rust.sh
fi
# streaming.test.mjs replays pinned-SDK events through the Rust reducer, so it
# follows Rust changes and needs the toolchain; other Node tests need neither.
node_tests=()
if [[ "$PUI_CI_NODE" == true ]]; then
  for file in tests/*.test.mjs; do
    [[ "$file" == tests/streaming.test.mjs && "$PUI_CI_NEEDS_RUST" != true ]] || node_tests+=("$file")
  done
elif [[ "$PUI_CI_RUST" == true ]]; then
  node_tests=(tests/streaming.test.mjs)
fi
if (( ${#node_tests[@]} )); then
  # Adapter integration tests need the real Pi SDK, never a developer's home
  # installation. Install a pinned SDK locally without provider credentials.
  mkdir -p "$PWD/target/pi-sdk"
  cp packaging/runtime/package.json packaging/runtime/package-lock.json "$PWD/target/pi-sdk/"
  npm ci --prefix "$PWD/target/pi-sdk" --omit=dev --ignore-scripts --no-audit --no-fund --prefer-offline
  export PUI_PI_SDK="$PWD/target/pi-sdk/node_modules/@earendil-works/pi-coding-agent/dist/index.js"
  node --test "${node_tests[@]}"
fi
if [[ "$PUI_CI_PACKAGE" == true ]]; then
  bash scripts/ci/build-macos.sh
fi
