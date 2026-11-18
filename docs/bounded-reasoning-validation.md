# Bounded, provider-independent reasoning preview

## Behavior

- Structured thinking remains chronological between action groups, but is labeled Thinking with a subtle side border. Ordinary progress/final reply text is unchanged.
- Reasoning longer than 600 Unicode characters or 12 source lines defaults to a five-row plain-text preview (at most 600 characters). This bounds collapsed layout work as well as vertical space. Short thinking retains full Markdown formatting.
- Show more expands original Markdown; Show less restores the preview. Disclosure identity is scoped to turn/message/block, not model or text content; streaming growth retains state and unrelated blocks stay independent. Disclosure changes invalidate the containing turn's cached height.
- Applies consistently to Normal and Verbose views, DeepSeek, Claude and GPT. Legacy messages with a separate thinking field also use this renderer. No model-name heuristics, prose classification, protocol mutation or hidden/recovered reasoning inference.
- The installed Pi OpenAI-compatible adapter was inspected: reasoning_content/reasoning/reasoning_text are normalized to thinking_delta events. Pui already handles structured thinking deltas, indexed snapshots and history. No provider adapter changes were needed. This is not live-provider acceptance evidence or proof that every third-party endpoint labels reasoning correctly.

## Validation

Isolated branch/worktree `bounded-reasoning-preview`, based on `be6c2baa`. Main checkout was undergoing user-requested history reconciliation and unrelated task edits; those were not changed or included.

- Complete `scripts/test-rust.sh` with supported `PUI_TEST_THREADS=1`: **1,929 binary + 10 integration = 1,939 passed**, 2 existing ignored, zero failures. Every bulk/serial group included once. `/tmp/pui-bounded-reasoning-full.log`.
- Transcript focused checks: **32 passed**, 1 existing benchmark ignored. `/tmp/pui-bounded-reasoning-focused.log`.
- Strict all-target Clippy, full formatting and diff whitespace checks passed: `/tmp/pui-bounded-reasoning-{clippy,fmt}.log`.
- Native egui checks cover light/dark themes, 320/800 widths, bounded geometry, Unicode, expansion/collapse via pointer input, independent blocks, streaming-state retention and idle settling. Provider fixtures cover DeepSeek/Claude/GPT structured history in live/completed and Normal/Verbose rendering, while ordinary prose remains text. Existing streamed-delta and retry tests passed as part of the full suite.

No provider calls, manual installed-app acceptance, packaging, installation or restart.

## Latest remote master integration

User requested pushing the fix onto latest master. Created an isolated integration branch from fetched `origin/master` at `f7e9f855`, then cherry-picked only this feature as `93a75dd0`. Unrelated local history-reconciliation/task changes are not included.

Complete validation of that latest-master snapshot passed: **1,923 binary + 10 integration = 1,933 tests**, 2 existing ignored, zero failures, all runner groups included once using `PUI_TEST_THREADS=1`. `/tmp/pui-bounded-reasoning-master-full.log`. Strict all-target Clippy, full formatting and whitespace checks passed (`/tmp/pui-bounded-reasoning-master-{clippy,fmt}.log`). The lower count than the original feature-branch run reflects excluding unrelated local-only commits, not skipped tests.

Remote advanced during validation to `e5ed3546` (auto-update implementation). Merged it normally, without conflicts. That incoming commit failed compilation because its menu `widget_info` Fn closure moved a String; repaired the call to borrow `&label` (one line).

Combined-state checks: transcript tests **32 passed**, 1 existing ignored (`/tmp/pui-bounded-reasoning-latest-focused.log`); full formatting and diff checks passed. Full runner was attempted, but bulk failed with **1,741 passed, 2 failed, 2 ignored** on 3-second Git fixture deadlines in workspace identity setup and initialization, before serial groups (`/tmp/pui-bounded-reasoning-latest-fixed-full.log`). Strict Clippy identified three incoming auto-update issues: unused `Updates::show`, `install_rx` type complexity, and a collapsible conditional (`/tmp/pui-bounded-reasoning-latest-clippy.log`). These unrelated issues were not suppressed or represented as a passing combined run. Earlier complete green validation above remains scoped to the pre-auto-update snapshot.

Push uses a normal fast-forward update of remote master, with no force push or rewriting history. No application build/install is included in this request.
