#!/usr/bin/env bash
# Complete Rust suite with bounded parallelism and serialized contention-sensitive groups.
# For development, run a focused cargo test FILTER instead. Run this at feature completion.
set -euo pipefail
cd "$(dirname "$0")/.."
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_NOSYSTEM=1
# Preserve caller toolchain, CEF_PATH and CARGO_TARGET_DIR configuration.
# No retries or ignored failures: every group must pass.
lock_test=app::automation::tests::store_lock_restart_missed_cron_and_uncertain_delivery
cargo test --offline --locked -- --test-threads=4 \
  --skip app::sync:: --skip app::files::delimited::projection:: --skip app::files::delimited::export:: \
  --skip "$lock_test"
cargo test --offline --locked --bin pui app::sync:: -- --test-threads=1
cargo test --offline --locked --bin pui app::files::delimited::projection:: -- --test-threads=1
cargo test --offline --locked --bin pui app::files::delimited::export:: -- --test-threads=1
# Restart/OS-lock test must not overlap any subprocess-spawning group.
cargo test --offline --locked --bin pui "$lock_test" -- --exact --test-threads=1
