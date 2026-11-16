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
  --skip app::tasks::tests::watch_scope_tests:: --skip app::tasks::refresh::tests:: \
  --skip sessions::watch:: --skip file_watch:: \
  --skip "$lock_test" --skip app::tasks::save_conflict_tests::
# Real OS registration competes with concurrent application-context watcher setup.
# Keep deadlines/assertions unchanged; execute each excluded group once, without retries.
cargo test --offline --locked --bin pui app::tasks::tests::watch_scope_tests:: -- --test-threads=1
cargo test --offline --locked --bin pui app::tasks::refresh::tests:: -- --test-threads=1
cargo test --offline --locked --bin pui sessions::watch:: -- --test-threads=1
cargo test --offline --locked --bin pui file_watch:: -- --test-threads=1
cargo test --offline --locked --bin pui app::sync:: -- --test-threads=1
cargo test --offline --locked --bin pui app::files::delimited::projection:: -- --test-threads=1
cargo test --offline --locked --bin pui app::files::delimited::export:: -- --test-threads=1
# Ownership lock handoff assertions must not overlap fork/exec in other tests.
cargo test --offline --locked --bin pui app::tasks::save_conflict_tests:: -- --test-threads=1
# Restart/OS-lock test must not overlap any subprocess-spawning group.
cargo test --offline --locked --bin pui "$lock_test" -- --exact --test-threads=1
