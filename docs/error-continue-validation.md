# Continue after a chat error

## Behavior

- The latest settled assistant error has a right-aligned **Continue** button. Long error text wraps to its left.
- Clicking sends an explicit follow-up through the existing permission-controlled prompt/delivery path in the same session, asking the agent to continue from retained context/completed work rather than restart.
- The draft, attachments, recorded changes and canonical conversation are not replaced or rewound. This is a contextual follow-up, not replay of failed tools or restoration of an exact provider execution checkpoint.
- Automatic retries/running turns hide the action. Connection, adapter, approval, history, pending-delivery and queued-message guards disable it; dispatch revalidates the latest error and message index. Pending delivery prevents double sends.
- Historical errors do not offer continuation after a newer request. Shared-window mirrors use the existing disabled history-action guard; continuation must be requested in the owning connected chat.

## Validation

Final focused runs on the combined working checkout:

- `cargo test --offline --locked --bin pui transcript::error:: -- --test-threads=1`: **2 passed**. Light/dark, 240/736-point widths, 1x/1.5x zoom, long unbroken addresses, enabled/disabled click handling, Escape, exact transcript continuation action, stale errors and running retries.
- `cargo test --offline --locked --bin pui app::run_status:: -- --test-threads=1`: **30 passed**, including 2 new continuation regressions covering same-session prompt delivery, preserved draft/history, double clicks, stale indices and normal send guards.
- `cargo test --offline --locked --bin pui transcript::retry_tests:: -- --test-threads=1`: **4 passed**.
- `cargo clippy --offline --locked --all-targets -- -D warnings`: completed successfully, as recorded in `/tmp/pui-error-continue-clippy.log`. Its enclosing tool call later timed out during the following prompt-design command.
- Changed/new feature-only files pass targeted rustfmt checks; `git diff --check` passed.

Logs: `/tmp/pui-error-continue-{ui-final,delivery-final,retry-final,clippy}.log`.

## Broader checkout limitations

- An earlier broader focused run encountered a concurrent unrelated connector-test visibility error (`src/app/resource_reload/connector/tests.rs`); it subsequently compiled without any changes to those files by this task.
- `./scripts/test-rust.sh` was attempted once; the tool timed out after 1,200 seconds during bulk execution. Eight unrelated Git/UI tests had reported failures (branch interactions/history, bounded branch-worker completion, conflict staging, Git read/filter fixtures and identity setup). The remaining serial groups were not reached. This is **not** a passing complete-suite result; no failing checks were retried or weakened.
- A separate prompt-design command reached its test executable but did not finish before the enclosing 240-second tool deadline. No passing prompt-design result is claimed.
- The checkout contains extensive concurrent edits. They were preserved and are not included in this feature's commit. No baseline comparison, live-provider continuation, physical-pointer/native screenshot acceptance, packaging, installation, push or deployment was performed.

Validation used the cached stable Rust toolchain, existing pinned CEF distribution, explicit macOS SDK and two build jobs. No dependencies were installed and no other task's process was stopped.
