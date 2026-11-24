# Connector reconnect and user-message rendering validation

## Summary of changes

- **Per-connector MCP reconnect** (`src/app/resource_reload/connector.rs`, `src/app/resource_reload.rs`, `src/app/views.rs`, `src/app.rs`, `src/app/agents.rs`, `src/app/agents/idle.rs`, `src/app/deletion.rs`, `docs/reference.md`, `tests/prompt_design.rs`, `tests/mcp-reconnect.test.mjs`):
  - **Inspector → Connectors → Reconnect** dispatches `/mcp reconnect <name>` to the current chat's existing Pi process instead of restarting the chat runtime via `request_resource_reload()`.
  - Verifies `/mcp` command presence and connector name validity (`[A-Za-z0-9_-]+`) before sending so an unsupported command or malformed name can never fall through to an LLM prompt.
  - When the chat is active (`!state.ready()`), a Quiet Mac `confirmations::Dialog` (`reconnect-mcp-connector`) warns that Pi closes the selected connector's transport and any in-flight requests using that connector can fail without automatic retry, while the chat, other connectors, history, drafts, attachments, and temporary approvals remain intact.
  - Requires both Pi's `Reconnected to MCP server "<name>" (` info notification and the correlated `prompt` command response (`disposition: "handled"`) before marking the connector reconnected.
  - Tracks operation-owned reconnect errors separately (`connector_error`) so a later successful reconnect or retry clears only its own timeout/failure message without erasing unrelated chat errors.
  - Lifecycle protection preserves chats with a pending connector reconnect across chat switches (`retain_chat_on_switch`), continues background RPC polling so the 120s timeout fires while parked (`poll_rpc`, `parked_needs_poll`), and blocks idle retirement, parked-root eviction, and chat/agent deletion (`Chat::deletion_busy`, `is_idle_retirable`, `agent_deletion_blocked`, `chat_deletion_blocked`).
- **Sent user bubble Markdown and fenced code block rendering** (`src/app/composer_text.rs`, `src/app/composer_text/message_tests.rs`, `src/app/composer_text/message_interaction_tests.rs`, `src/transcript.rs`):
  - Sent user bubbles render through `composer_text::show_message` / `message_galley`, sharing the composer's Markdown presentation (fenced code blocks with background/border, inline code, bold/italic, links, list bullets/indentation, and per-paragraph Unicode first-strong RTL alignment) instead of a plain-text `LayoutJob::simple`.
  - Hidden opening and closing fence lines (`s.hidden && s.block`) collapse their row height (`size.y = 0.0`) and shift subsequent rows up so hidden fences never leave a blank row at the top or bottom of a code block, while authored blank lines inside code blocks keep their full height and revealing a fence with the editor caret restores its row height.
  - Precomputes character-to-byte offsets (`byte_offsets`) once per layout pass and rebuilds display sections when find-in-chat highlights split source sections, preserving exact original source text for selection, copy, edit, rewind, and find match rectangles.

## Complete validation

Executed with `CARGO_TARGET_DIR=/tmp/pui-btw-clean-target`, Rust 1.88.0, pinned cached CEF (`cef_binary_156.0.26+g8ea0c73+chromium-156.0.8078.88_macosarm64_minimal`), explicit `SDKROOT`, and `PUI_TEST_THREADS=1`:

- **Complete Rust suite** (`/tmp/pui-mcp-render-full-suite.log`): **2,313 unit tests passed + 10 integration tests passed = 2,323 passed, 0 failed, 2 ignored** (bulk 2,120 passed + 4 `panel_keyboard` + 3 `panel_vertical_keyboard` + 3 `prompt_design` + 193 across all serial groups: `watch_scope_tests` 4, `tasks::refresh` 3, `sessions::watch` 3, `file_watch` 4, `app::sync` 34, `delimited::projection` 19, `delimited::export` 43, `changes::commit` 8, `changes::git_files::modes` 3, `files::loader::svg_tests` 2, `changes::index_actions` 8, `workspace::commit` 14, `svg_preview::process::lifecycle_tests` 1, `tasks::save_conflict_tests` 2, `tasks::recovery` 14, `tasks::store_safety_tests` 7, `tasks::apply` 22, `store_lock_restart_missed_cron_and_uncertain_delivery` 1, `cross_window_orphan_delete_uses_real_transport_without_creator_and_handles_owner_exit` 1).
- **Focused feature groups**:
  - `cargo test --offline --locked --bin pui app::resource_reload:: -- --test-threads=1`: **10 passed**, 0 failed.
  - `cargo test --offline --locked --bin pui app::composer_text:: -- --test-threads=1`: **26 passed**, 0 failed.
  - `cargo test --offline --locked --bin pui transcript:: -- --test-threads=1`: **55 passed**, 0 failed, 1 ignored.
  - `cargo test --offline --locked --bin pui app::deletion:: -- --test-threads=1`: **26 passed**, 0 failed.
  - `cargo test --offline --locked --bin pui app::agents::idle:: -- --test-threads=1`: **14 passed**, 0 failed.
- **Complete JS test suite & SDK compatibility** (`/tmp/pui-mcp-render-js.log`):
  - `PUI_PI_SDK=/Users/shai.cohen/personal/pui/dist/Pui.app/Contents/Resources/runtime/pi/node_modules/@earendil-works/pi-coding-agent/dist/index.js node --test tests/mcp-reconnect.test.mjs` (Pi 1.5.2): **3 passed**, 0 failed.
  - `PUI_PI_SDK=/Applications/Pui.app/Contents/Resources/runtime/pi/node_modules/@earendil-works/pi-coding-agent/dist/index.js node --test tests/*.test.mjs` (Pi 1.6.0): **176 passed**, 0 failed, 1 pre-existing skipped (`statusline-rpc.test.mjs`).
- **Formatting, lint, and whitespace checks**:
  - `cargo fmt --all -- --check`: **passed**.
  - `cargo clippy --offline --locked --all-targets -- -D warnings`: **passed** with zero warnings.
  - `git diff --check`: **passed**.
