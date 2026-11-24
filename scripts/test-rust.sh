#!/usr/bin/env bash
# Complete Rust suite with bounded parallelism and serialized contention-sensitive groups.
# For development, run a focused cargo test FILTER instead. Run this at feature completion.
set -euo pipefail
cd "$(dirname "$0")/.."
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_NOSYSTEM=1
# Fixture seeds are copied while other test processes run. Git's detached
# auto-maintenance can add/remove lock files in a seed mid-copy (NotFound), so
# no test Git process may start it. Real Git behavior is otherwise unchanged.
export GIT_CONFIG_COUNT=2
export GIT_CONFIG_KEY_0=gc.auto GIT_CONFIG_VALUE_0=0
export GIT_CONFIG_KEY_1=maintenance.auto GIT_CONFIG_VALUE_1=false
# /usr/bin/git is a macOS developer-tool launcher, not the Git executable.
# Avoid repeating launcher resolution for every subprocess in bounded real-Git
# fixtures. Preserve explicitly selected Git installs and all production limits.
if [[ "$(uname -s)" == Darwin && "$(command -v git)" == /usr/bin/git ]]; then
  developer_git=$(xcrun --find git)
  [[ -x "$developer_git" ]] || { echo "Developer Git is unavailable." >&2; exit 1; }
  export PATH="$(dirname "$developer_git"):$PATH"
  # The directory also contains the real compiler, bypassing Apple's /usr/bin
  # SDK-selecting shim. Preserve a caller SDK or select the active SDK explicitly.
  if [[ -z "${SDKROOT:-}" ]]; then
    export SDKROOT="$(xcrun --show-sdk-path)"
  fi
fi
# Preserve caller toolchain, CEF_PATH and CARGO_TARGET_DIR configuration.
# No retries or ignored failures: every group must pass.
#
# Contention-sensitive groups are excluded from the bulk pass and each runs
# exactly once in its own `--test-threads=1` process (see docs/test-execution.md).
# Every group below is skipped in bulk by construction.
#
# Exclusive: real OS watcher registration competes with application-context and
# watcher setup load. These run alone, with no other test process running.
exclusive_groups=(
  app::tasks::tests::watch_scope_tests::
  app::tasks::refresh::tests::
  sessions::watch::
  file_watch::
)
# Lanes: real Git fixture deadlines, Python helper startup, projection permits and
# ownership-lock handoffs failed under in-process bulk concurrency. Each group keeps
# its own single-threaded process, so no test overlaps another test (or its
# fork/exec) in the same process; lanes only overlap other single-test processes.
# A group is a test-name filter, optionally followed by `--skip FILTER` when a
# sibling group runs that subset; the first word is always skipped in bulk.
# Balanced by measured duration under CI-like CPU limits (3 cores); lane_4 starts
# with sleep/deadline-bound groups that use little CPU. Lanes run concurrently;
# set PUI_SERIAL_LANES=1 to run them one after another instead.
lane_1=(
  "tasks::apply::tests:: --skip tasks::apply::tests::additional::"
  app::changes::index_actions::tests::
  app::tasks::save_conflict_tests::
  app::tasks::store_safety_tests::
)
lane_2=(
  tasks::apply::tests::additional::
  app::changes::commit::tests::
  app::changes::git_files::modes::tests::
  app::files::loader::svg_tests::
  svg_preview::process::lifecycle_tests::
)
lane_3=(
  app::files::delimited::export::
  app::files::delimited::projection::
)
lane_4=(
  workspace::commit::tests::
  app::sync::
  app::tasks::recovery::tests::
)
# Exclusive, last: restart/OS-lock and owner-exit lock takeover tests must not
# overlap any subprocess-spawning test.
lock_test=app::automation::tests::store_lock_restart_missed_cron_and_uncertain_delivery
orphan_lock_test=app::automation::delete::tests::cross_window_orphan_delete_uses_real_transport_without_creator_and_handles_owner_exit
exact_tests=("$lock_test" "$orphan_lock_test")

skips=()
for group in "${exclusive_groups[@]}" "${lane_1[@]}" "${lane_2[@]}" "${lane_3[@]}" "${lane_4[@]}" \
  "${exact_tests[@]}"; do
  skips+=(--skip "${group%% *}")
done
cargo test --offline --locked -- --test-threads="${PUI_TEST_THREADS:-4}" "${skips[@]}"

run_group() {
  local spec=($1)
  cargo test --offline --locked --bin pui "${spec[0]}" -- --test-threads=1 "${spec[@]:1}"
}
for group in "${exclusive_groups[@]}"; do
  run_group "$group"
done

lane_logs=$(mktemp -d "${TMPDIR:-/tmp}/pui-test-lanes.XXXXXX")
trap 'rm -rf "$lane_logs"' EXIT
run_lane() {
  local name=$1 group
  shift
  for group in "$@"; do
    run_group "$group" || { echo "Serial lane $name failed in $group." >&2; return 1; }
  done
}
lanes=() pids=()
start_lane() {
  local name=$1
  shift
  if [[ "${PUI_SERIAL_LANES:-4}" == 1 ]]; then
    run_lane "$name" "$@"
  else
    run_lane "$name" "$@" >"$lane_logs/$name.log" 2>&1 &
    lanes+=("$name") pids+=($!)
  fi
}
start_lane lane_1 "${lane_1[@]}"
start_lane lane_2 "${lane_2[@]}"
start_lane lane_3 "${lane_3[@]}"
start_lane lane_4 "${lane_4[@]}"
failed=0
for i in "${!pids[@]}"; do
  wait "${pids[$i]}" || failed=1
  echo "===== serial ${lanes[$i]} ====="
  cat "$lane_logs/${lanes[$i]}.log"
done
[[ $failed == 0 ]] || { echo "Serial test lanes failed." >&2; exit 1; }

for test in "${exact_tests[@]}"; do
  cargo test --offline --locked --bin pui "$test" -- --exact --test-threads=1
done
