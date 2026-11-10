# Quiet Mac prompt implementation

Implemented in the isolated `quiet-mac-prompts` worktree from `master` at `22c0006`. The earlier design comparison is retained separately in `prompt-redesign-options`.

## Changes

- `src/app/prompt_ui.rs`: shared light/dark surfaces, subtle fills, borders, badge/header, explicit primary/danger/secondary controls, bounded footers, option cards, popup frames and modal-layer keyboard guard.
- `src/app/about.rs`: centered app icon/name/version, concise description and secondary build disclosure. Signing status is explicitly unverified rather than asserting every build is ad-hoc signed.
- `src/app/updates.rs`: Quiet Mac status dialog with all existing update states, bounded error copy, opt-in checks, no install/restart behavior, and a shared centered dot/version row. Release URLs still come only from the configured repository.
- Existing quit, terminal-stop, rewind/edit/fork and permission-bypass confirmations use the same visual system. Restore modes are option cards; existing availability/safety checks remain.
- Composer prompts, structured questions, custom-answer fields, worker prompts, task/exceptional-model review and resource change/form panels share the chosen visual language. All exact action arguments and safety descriptions remain inspectable.
- Model/mode/notification popovers remain anchored and use consistent surfaces. Recorded-diff viewer uses the same utility-window frame.
- macOS proportional typography uses the installed SFNS font; portable egui defaults remain the fallback. Existing bold transcript family is unchanged.
- `--demo-about` and `--demo-updates` provide deterministic, no-agent previews. Opening demo Updates does not perform a network request.

## Deliberately preserved

- **Inline rename is unchanged** (`src/app/sidebar.rs` has no diff). Existing Enter-save, Escape/focus-loss discard and blank-title rejection tests still pass.
- Native open/save panels remain native. The existing full-screen image theater retains its specialized layout/zoom/navigation rather than becoming a modal card.
- No widened approval scope, protocol changes, automatic provider calls, auto-update behavior, file restoration changes, commits, merges, deployments or installations.
- Keyboard approval shortcuts do not reach prompts beneath information modals. Escape cancels bypass without granting permission. Application stop/image-input/webview gates also recognize About and Updates as modal surfaces.

## Validation

- `cargo fmt --check`: passed.
- `cargo clippy --offline --all-targets -- -D warnings`: passed.
- `cargo test --offline -- --test-threads=1`: 346 passed, 2 intentionally ignored benchmark tests.
- `node --test tests/*.test.mjs`: 61 passed with the installed Rust toolchain and cached CEF target environment.
- `git diff --check`: passed.
- New Rust regressions: light/dark Updates dot alignment; six update-copy states; small-window/zoom bounds for About and Updates; Escape close; information-modal approval isolation; no-session demo flags. Existing rename/permission/confirmation/question/rewind/worker tests retained.
- Real-app screenshot runs inspected: About light/dark, Updates light/dark, command approval, question choices, quit confirmation light/dark, model picker and notifications. Screenshot files are in `/tmp/pui-quiet-*`; not committed.

### Environment / flaky-test notes

The toolchain exists at `/Users/user/.rustup/toolchains/stable-aarch64-apple-darwin/bin` but is not on the default PATH. Offline validation uses the local Cargo cache. Existing cached CEF runtime/build artifacts in `in-app-file-browser/target` were reused; no download or installed application replacement was performed.

The first parallel Rust run hit an existing cross-window owner timeout; the complete serialized run passed. The first Node run lacked the cached CEF target environment and its nested Cargo replay failed; the complete configured rerun passed. No unrelated tests or timeouts were weakened.

Approved for integration into `master`. This change does not install or replace `/Applications/Pui.app`; the installed application needs a separate rebuild/install.
