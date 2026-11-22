# Compact incoming agent messages

## Behavior

- Incoming custom/agent messages use the shared 32pt single-line disclosure, collapsed by default, with a trailing chevron.
- Direct messages say “Message from [sender]”; automatic completion reports say “Report from [sender]”. Unknown formats use “Agent message received”. No processing-state claim is made.
- Clicking or keyboard activation reveals the complete original Markdown message. Sender labels elide at narrow widths; hover retains the full label.
- Expansion is independent per message and chat. Closed bodies do not execute Markdown layout. Agent-message spacing is reduced to 8pt; user-turn navigation and source messages are unchanged.

## Validation

- Three new agent-message tests pass: labels/fallbacks, light/dark × narrow/wide × zoom pointer/keyboard/disabled interaction, and transcript height-cache/culling/chat-scope regression coverage. `/tmp/pui-agent-message-focused.log`.
- All 37 active transcript tests passed in the bulk run; one existing transcript benchmark remains ignored.
- `scripts/test-rust.sh` bulk: 1,807 passed, 13 failed, 2 ignored. Failures are outside the changed files: two concurrently developed sidebar menu-key assertions, ten Git index-action fixture deadlines, and one read-only subprocess fixture deadline. The runner stopped before serial groups. No failures were retried or production deadlines changed. `/tmp/pui-agent-message-full.log`.
- Executed all 18 remaining serial group commands from the checked-in runner once, without overlapping those groups: 189 passed, zero failures. `/tmp/pui-agent-message-serial.log`.
- All 10 integration UI guards passed. `/tmp/pui-agent-message-ui-guards.log`. An earlier attempt to select the helper/integration targets with `--tests` unexpectedly selected the main binary suite too and hit the command timeout; that incomplete run is not counted as validation (`/tmp/pui-agent-message-integration.log`).
- Strict all-target Clippy remains blocked by unrelated `too_many_arguments` at `src/app/changes.rs:1300` and an unused test variable at `src/app/context_capture/diff/ui_tests.rs:275`. `/tmp/pui-agent-message-clippy.log`.
- Scoped rustfmt and `git diff --check` pass. No packaging, deployment, or application profiling performed. This is not a green full-checkout validation claim.
