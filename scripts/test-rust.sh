#!/usr/bin/env bash
# Complete Rust suite with bounded parallelism and serialized contention-sensitive groups.
# For development, run a focused cargo test FILTER instead. Run this at feature completion.
set -euo pipefail
cd "$(dirname "$0")/.."
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_NOSYSTEM=1
# /usr/bin/git is a macOS developer-tool launcher, not the Git executable.
# Avoid repeating launcher resolution for every subprocess in bounded real-Git
# fixtures. Preserve explicitly selected Git installs and all production limits.
if [[ "$(uname -s)" == Darwin && "$(command -v git)" == /usr/bin/git ]]; then
  developer_git=$(xcrun --find git)
  [[ -x "$developer_git" ]] || { echo "Developer Git is unavailable." >&2; exit 1; }
  export PATH="$(dirname "$developer_git"):$PATH"
fi
# Preserve caller toolchain, CEF_PATH and CARGO_TARGET_DIR configuration.
# No retries or ignored failures: every group must pass.
lock_test=app::automation::tests::store_lock_restart_missed_cron_and_uncertain_delivery
cargo test --offline --locked -- --test-threads="${PUI_TEST_THREADS:-4}" \
  --skip app::sync:: --skip app::files::delimited::projection:: --skip app::files::delimited::export:: \
  --skip app::tasks::tests::watch_scope_tests:: --skip app::tasks::refresh::tests:: \
  --skip sessions::watch:: --skip file_watch:: \
  --skip app::changes::commit::tests:: --skip app::changes::git_files::modes::tests:: \
  --skip app::files::loader::svg_tests:: \
  --skip workspace::commit::tests:: --skip svg_preview::process::lifecycle_tests:: \
  --skip app::tasks::recovery::tests:: --skip app::tasks::store_safety_tests:: --skip tasks::apply::tests:: \
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
# Real Git fixture deadlines and Python helper protocol startup failed under
# bulk context/process load. Preserve deadlines/assertions and run each once,
# without overlapping other test groups. This is scheduling, not a timeout repair.
cargo test --offline --locked --bin pui app::changes::commit::tests:: -- --test-threads=1
cargo test --offline --locked --bin pui app::changes::git_files::modes::tests:: -- --test-threads=1
cargo test --offline --locked --bin pui app::files::loader::svg_tests:: -- --test-threads=1
cargo test --offline --locked --bin pui workspace::commit::tests:: -- --test-threads=1
cargo test --offline --locked --bin pui svg_preview::process::lifecycle_tests:: -- --test-threads=1
# Ownership lock handoff assertions must not overlap fork/exec in other tests.
cargo test --offline --locked --bin pui app::tasks::save_conflict_tests:: -- --test-threads=1
cargo test --offline --locked --bin pui app::tasks::recovery::tests:: -- --test-threads=1
cargo test --offline --locked --bin pui app::tasks::store_safety_tests:: -- --test-threads=1
cargo test --offline --locked --bin pui tasks::apply::tests:: -- --test-threads=1
# Restart/OS-lock test must not overlap any subprocess-spawning group.
cargo test --offline --locked --bin pui "$lock_test" -- --exact --test-threads=1
