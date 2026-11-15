#!/bin/bash
# CircleCI currently builds artifacts only, with no signing/publishing secrets.
set -euo pipefail
set +x
cd "$(dirname "$0")/../.."
[[ "${CIRCLECI:-}" == true ]] || { echo "This entry point requires CircleCI." >&2; exit 1; }
# Never infer GitLab protection from a CircleCI branch/tag name. A future signed
# release workflow needs restricted contexts and server-side tag verification.
export CI_PROJECT_DIR="$PWD"
export CARGO_HOME="$PWD/.cargo-ci"
export CARGO_TARGET_DIR="$PWD/target"
export CI_COMMIT_SHA="${CIRCLE_SHA1:?Missing CircleCI revision}"
[[ "$(git rev-parse HEAD)" == "$CI_COMMIT_SHA" ]] || { echo "Checkout does not match the CircleCI revision." >&2; exit 1; }
export CI_COMMIT_TAG="${CIRCLE_TAG:-}"
export CI_PIPELINE_IID="${CIRCLE_BUILD_NUM:?Missing CircleCI build number}"
export CI_COMMIT_REF_PROTECTED=false
export PUI_REQUIRE_SIGNING=false
unset PUI_PI_BIN PUI_PI_SDK
unset CI_JOB_TOKEN PUI_SIGN_IDENTITY PUI_SIGN_KEYCHAIN PUI_NOTARY_PROFILE PUI_SIGN_P12 PUI_SIGN_P12_BASE64 PUI_SIGN_PASSWORD PUI_APPLE_ID PUI_APPLE_TEAM_ID PUI_APPLE_APP_PASSWORD
source scripts/ci/setup-macos.sh
python3 scripts/release.py check
for script in scripts/bundle-macos.sh scripts/package-macos.sh scripts/notarize-macos.sh scripts/ci/*.sh; do
  bash -n "$script"
done
python3 -m unittest discover -s tests -p 'test_release.py' -v
python3 -m unittest discover -s tests -p 'test_circleci.py' -v
python3 -m unittest discover -s tests -p 'test_runtime_bundle.py' -v
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
# This restart/OS-lock test must not overlap tests that spawn subprocesses.
# Both invocations are required and fail-fast; no test is omitted or retried.
lock_test=app::automation::tests::store_lock_restart_missed_cron_and_uncertain_delivery
cargo test --locked -- --skip "$lock_test"
cargo test --locked --bin pui "$lock_test" -- --exact
# Adapter integration tests need the real Pi SDK, never a developer's home
# installation. Install a pinned SDK locally without provider credentials.
mkdir -p "$PWD/target/pi-sdk"
cp packaging/runtime/package.json packaging/runtime/package-lock.json "$PWD/target/pi-sdk/"
npm ci --prefix "$PWD/target/pi-sdk" --omit=dev --ignore-scripts --no-audit --no-fund --prefer-offline
export PUI_PI_SDK="$PWD/target/pi-sdk/node_modules/@earendil-works/pi-coding-agent/dist/index.js"
node --test tests/*.test.mjs
bash scripts/ci/build-macos.sh
