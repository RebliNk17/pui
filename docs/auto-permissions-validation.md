# Auto permission routing and non-interactive denials

## Behavior

- Settings → Permissions → **Classifier model** defaults to **Automatic · cheapest available**. It ranks available authenticated text models within Pi's configured model scope across providers by configured input + output prices, then provider/model ID. Text input, at least 8,192 context tokens and 256 output tokens are required. Unknown/invalid prices are excluded from automatic ranking; configured zero prices are respected, not assumed to be actual free billing.
- An explicit provider/model override is saved in preferences and sent through startup settings, live settings updates and attached-window settings synchronization. It never changes the conversation model, and an unavailable/out-of-scope override never silently falls back. Unknown-price eligible models can be selected explicitly.
- The UI displays the resolved model, discloses that proposed calls/recent requests go to its provider, and reports classifier cost separately. Runtime catalog updates are sent only when changed; usage-only updates preserve the picker catalog.
- The classifier prompt evaluates actual dangerous effects, not narrow task relevance or incidental dangerous words in source/search patterns. It explicitly allows routine supporting tests/builds/lint/docs/Git inspection, handles continuing short requests, and treats actions/tool metadata as untrusted.
- Classifier denial, uncertainty, failure, malformed/incomplete output, missing model/context and built-in high-risk rules return blocked **non-terminal** tool results without generic approval UI. The agent is instructed to continue safe work, not retry the same denied effect through another route or ask for escalation, and report blocked steps in its final answer.
- A final-message backstop lists blocked tool names/counts and attribution (classifier denial/uncertainty, unavailable check, or built-in rule), without recording raw action arguments/content in that summary. It never claims blocked calls executed or blames a model that was not called. State clears between runs/session/branch changes; aborted/error/intermediate tool-calling messages are not rewritten.
- Dedicated workflow and scheduling consent, requirements questions, read-only restrictions, unmanaged-agent guards and other permission modes retain their existing behavior. This remains a probabilistic tool boundary, not an OS sandbox.

## Validation

### Passed

- `node --test tests/permissions.test.mjs tests/ultracode.test.mjs`: **46 passed, zero skipped**, including the real installed Pi agent loop with a deterministic, network-free stream: a denied Bash call is not executed, a subsequent read runs, and the final message includes the deciding classifier.
- `cargo test --offline --locked --bin pui auto_classifier -- --test-threads=4`: **3 passed** (preference/payload round-trip; runtime catalog/protocol serialization and usage-only preservation; picker interaction in light/dark themes at 520px/125% and 900px/100%, explicit selection, Escape cancellation, unavailable override and returning to Automatic while preserving busy state/draft).
- `cargo test --offline --locked --bin pui shared_permissions_and_compaction_can_change_while_owner_is_working -- --test-threads=1`: **1 passed**, now also covering the classifier override across attached windows.
- Full JavaScript suite `node --test tests/*.test.mjs`: **128 passed, 1 existing skip, zero failures**. `PUI_PI_SDK` points at the installed Pui SDK, so the agent-loop and streaming/Rust replay tests run rather than skip. The existing skipped test is terminal-footer RPC/TUI coverage.
- `/opt/homebrew/bin/python3 -m unittest discover -s tests -p 'test_*.py' -v`: **41 passed**. Initial system Python 3.9 failed imports because it lacks `tomllib`; rerunning with installed Python 3.14 corrected the environment.
- `rustfmt --check --edition 2024 --config skip_children=true` for the four changed Rust files and `git diff --check`: passed.

### Repository-wide validation is NOT green

- Required `scripts/test-rust.sh` was attempted. Initial all-target linking lacked macOS SDK libraries; explicitly exporting `SDKROOT=$(xcrun --show-sdk-path)` repaired that environment issue.
- The SDK-configured run reached the bulk group: **1,812 passed, 8 failed, 2 ignored, 189 filtered for later serial groups**. All three new Auto tests passed. The runner stops on failure, so its remaining serial groups were **not executed by that run**. No failure retries, removed assertions, skipped groups or runner changes were used to claim a pass.
- Bulk failures: two concurrently edited `app::sidebar::menu_keys::tests` cases; the concurrently edited `transcript::agent_message_tests` collapsed-card case; three `workspace::index_action::{discard,recovery}` timeout cases; `workspace::read_only::tests::stalled_stdout_closed_stdout_oversize_cancel_and_success_reap_direct_child` timeout; `workspace::push::tests::signing_config_drift_refuses_even_when_target_is_unchanged` assertion. These files are outside this feature. Timeouts are observed symptoms, not an established diagnosis.
- `cargo clippy --offline --locked --all-targets -- -D warnings` fails outside this feature: `src/app/changes.rs:1300` (`too_many_arguments`) and `src/app/context_capture/diff/ui_tests.rs:275` (unused variable). An earlier invocation also observed incomplete concurrent sidebar/transcript files; those transient missing-file errors disappeared in the final invocation.
- `cargo fmt --check` fails on existing formatting in `src/app/changes.rs` and its branches/clone/identity_setup/index_actions/push/create/tests/worktrees modules. Changed Auto files pass targeted formatting. Unrelated formatting was not rewritten.

Logs: `/tmp/pui-auto-focused-js.log`, `/tmp/pui-auto-final-focused.log`, `/tmp/pui-auto-full-js.log`, `/tmp/pui-auto-python-fixed.log`, `/tmp/pui-auto-full-rust-sdk.log`, `/tmp/pui-auto-final-clippy.log`, `/tmp/pui-auto-final-format.log`.

## Limitations

No live paid-provider traffic or empirical Flash/Haiku quality/false-positive benchmark was run. Routing/prompt/error behavior is tested deterministically; fewer real model false positives are an intended prompt improvement, not a measured guarantee. Independent review could not launch because Pui's 128 retained-agent limit was reached; direct diff review was used. No installation, packaging, deployment or push was performed.
