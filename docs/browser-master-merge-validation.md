# Browser → master local integration

User requested merging browser commit `c97539b34aa0a2f5af8cde1cb38497868b26197a` into local master. Prepared in isolated `merge-browser-completion` from master `95b67ea3eedc3d7e463c155ad75a5829179a6915`; original checkout has concurrent uncommitted connector/message edits, so no reset/stash/overwrite is permitted implicitly.

The normal merge is conflict-free. Master added docked side-chat Files rendering since browser base `7a8b47c8b`; integration moves existing browser/file activation preparation and owner binding before the dock/ordinary Files render branch. This preserves both UI layouts rather than restoring older browser chrome. The existing dock theme/zoom test now explicitly checks capture ownership in the docked path.

Combined-source checks in private Cargo target `/tmp/pui-browser-completion-owned.hav6QQ`:

- Browser **99 passed** (`/tmp/pui-browser-merge-final-focused.log`).
- Side-chat **31 passed** before the additional owner assertion (`/tmp/pui-browser-merge-side-chat.log`); dock group **4 passed** afterward (`/tmp/pui-browser-merge-dock.log`).
- Prompt architecture **3 passed** (`/tmp/pui-browser-merge-prompts.log`).
- Feature diff whitespace checks pass. Whole-tree formatting check identifies pre-existing `src/app/views.rs` connector-row formatting on master; not silently rewritten as browser work.

Master concurrently advanced to `aff58872151419b2cd543b6bbbd831899c9d5332` (Git activity summaries). Merged that tip normally into the prepared integration (`73f62943`), with no conflicts. Combined browser **99**, Git activity **11** and transcript activity **1** tests pass (`/tmp/pui-browser-master-final-{browser,activity,transcript}.log`). Original checkout's unrelated unstaged connector/message edits were not included.

The complete browser validation, disposable actual CEF acceptance, and **11 existing Git-related full-suite failures** remain disclosed in `browser-completion-validation.md`. A new full suite on the combined master is not claimed; this merge does not fix those unrelated Git failures or incorporate another worktree's pending changes. No push, install, release or installed-app restart is authorized by this local merge request.
