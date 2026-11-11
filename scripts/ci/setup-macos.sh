#!/bin/bash
# Source this file so the toolchain and pinned CEF path survive in the CI job.
set -euo pipefail
[[ "$(uname -s)/$(uname -m)" == Darwin/arm64 ]] || { echo "Expected a native Apple Silicon macOS runner." >&2; exit 1; }
[[ "${CI_DEBUG_TRACE:-false}" != true ]] || { echo "Disable CI_DEBUG_TRACE for release jobs." >&2; exit 1; }
# Credentials must never reach branch/MR jobs (also mark them protected in GitLab).
if [[ -z "${CI_COMMIT_TAG:-}" || "${CI_COMMIT_REF_PROTECTED:-false}" != true ]]; then
  unset PUI_SIGN_IDENTITY PUI_SIGN_P12 PUI_SIGN_P12_BASE64 PUI_SIGN_PASSWORD PUI_APPLE_ID PUI_APPLE_TEAM_ID PUI_APPLE_APP_PASSWORD
fi
brew install cmake ninja node@22 python@3.13 rustup
export PATH="$(brew --prefix node@22)/bin:$(brew --prefix python@3.13)/libexec/bin:$(brew --prefix rustup)/bin:$HOME/.cargo/bin:$PATH"
# rustup is provided by Homebrew; no curl-pipe-shell bootstrap.
rustup toolchain install "$RUST_VERSION" --profile minimal --component rustfmt --component clippy
export RUSTUP_TOOLCHAIN="$RUST_VERSION"
# Do not let a global CARGO_BUILD_TARGET produce binaries the native bundler cannot find.
unset CARGO_BUILD_TARGET
export MACOSX_DEPLOYMENT_TARGET=12.0
export CEF_PATH
CEF_PATH=$(python3 scripts/fetch-chromium-macos.py "$CARGO_TARGET_DIR/cef-runtime")
rustc --version
cargo --version
node --version
python3 --version
