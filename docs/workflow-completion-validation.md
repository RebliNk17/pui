# Workflow system implementation and validation

Branch: `workflow-system-completion`, initially based on `cc5bc74f`. Original-checkout concurrent permissions/settings/protocol edits are not included or overwritten. This is an incremental implementation record, not a declaration that the entire redesign is finished.

## Slice 1: reserve workflow offers for hard work

- Centralized guidance used by the tool and ordinary-chat system prompt: inspect and work normally; offer only for genuinely hard/high-risk changes or an explicit workflow request. Ordinary features, fixes, UI polish, tests, reviews, planning, multiple files and subagent use are not qualifying reasons. Ultracode explicitly keeps this threshold.
- Native **Regular development** responses persist a conversation-branch preference in Pi custom entries. Repeated tool attempts return regular-development guidance without reopening a native question. Reload restores the preference; another conversation does not inherit it.
- A positive explicit human workflow request can permit a fresh question. Negative requests, ordinary follow-ups and extension-authored messages do not reset suppression. This is not approval and never changes permissions. Revision/cancellation/abort do not persist a decline or execution authority.
- Focused workflow/agent/permissions JS: **51 passed**.
- Complete JS with installed SDK encoder and cached CEF/build environment: **123 passed, 0 failed, 1 pre-existing skipped**. `/tmp/pui-workflow-completion-offers-js.log`.
- Complete Rust runner was attempted with every group accounted for. Initial setup stopped before compilation because inherited `CARGO_HTTP_PROXY_CAINFO` requires an unset proxy configuration; unset that variable for subsequent offline commands. The complete runner then passed bulk **1,809 binary + 10 integration** tests and its first seven serial groups, but stopped at `app::changes::commit::tests::native_commit_dialog_escape_themes_zoom_preserves_message_and_never_executes` on the production three-second Git staged-inspection deadline. **No failed test was retried and no deadline/assertion was changed.** `/tmp/pui-workflow-completion-offers-rust.log`.
- All ten serial groups not reached after that stop were run once, serially: **73 passed**. `/tmp/pui-workflow-completion-offers-rust-tail.log`. Total across the scheduled binary groups: **1,997 passed, 1 failed, 2 ignored**, plus **10 integration passed**. This is not a green full-suite claim. The remaining-group build also includes an in-progress environment-classification edit, not a single immutable release snapshot.
- Existing unrelated warning: unused `output` in `src/app/changes/branches/tests.rs:275`. Not suppressed or treated as strict-lint success.
- No provider requests, installations, live task-record edits, application replacement or push. Independent managed worker launch refused by the existing 128 retained-session limit; no hidden fallback.
