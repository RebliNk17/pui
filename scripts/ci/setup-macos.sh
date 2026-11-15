#!/bin/bash
# Source this file so the toolchain and pinned CEF path survive in the CI job.
set -euo pipefail
[[ "$(uname -s)/$(uname -m)" == Darwin/arm64 ]] || { echo "Expected a native Apple Silicon macOS runner." >&2; exit 1; }
[[ "${CI_DEBUG_TRACE:-false}" != true ]] || { echo "Disable CI_DEBUG_TRACE for release jobs." >&2; exit 1; }
# Credentials must never reach branch/MR jobs (also mark them protected in GitLab).
if [[ -z "${CI_COMMIT_TAG:-}" || "${CI_COMMIT_REF_PROTECTED:-false}" != true ]]; then
  unset PUI_SIGN_IDENTITY PUI_SIGN_P12 PUI_SIGN_P12_BASE64 PUI_SIGN_PASSWORD PUI_APPLE_ID PUI_APPLE_TEAM_ID PUI_APPLE_APP_PASSWORD
fi
# Prefer image-provided tools: reinstalling Python/Node can trigger unrelated
# Homebrew post-install failures on an otherwise usable immutable CI image.
echo "Checking macOS build prerequisites..."
for tool in cmake ninja; do
  if ! command -v "$tool" >/dev/null; then brew install "$tool"; fi
done
if ! python3 -c 'import sys; assert sys.version_info >= (3, 11)' 2>/dev/null; then
  brew install python@3.13
  export PATH="$(brew --prefix python@3.13)/libexec/bin:$PATH"
fi
if ! node --input-type=module -e 'import { stripTypeScriptTypes } from "node:module"; if (typeof stripTypeScriptTypes !== "function") process.exit(1)' 2>/dev/null; then
  brew install node@22
  export PATH="$(brew --prefix node@22)/bin:$PATH"
fi
export PATH="$HOME/.cargo/bin:$PATH"
# CircleCI images may already contain rustup. Otherwise use Homebrew rather
# than curl-pipe-shell, keeping Rust proxies ahead of an unrelated system Rust.
if ! command -v rustup >/dev/null; then
  # We install the exact toolchain below; Homebrew's post-install tries to
  # initialize another default toolchain and fails on this CircleCI image.
  brew install --skip-post-install rustup
  export PATH="$(brew --prefix rustup)/bin:$PATH"
fi
echo "Installing Rust $RUST_VERSION..."
rustup toolchain install "$RUST_VERSION" --profile minimal --component rustfmt --component clippy
export RUSTUP_TOOLCHAIN="$RUST_VERSION"
export PATH="$(dirname "$(rustup which --toolchain "$RUST_VERSION" cargo)"):$PATH"
# Do not let a global CARGO_BUILD_TARGET produce binaries the native bundler cannot find.
unset CARGO_BUILD_TARGET
export MACOSX_DEPLOYMENT_TARGET=12.0
export CEF_PATH
CEF_PATH=$(python3 scripts/fetch-chromium-macos.py "$CARGO_TARGET_DIR/cef-runtime")
rustc --version
cargo --version
node --version
python3 --version
