# Inline PR capsule — selected design A

The user selected **A — Soft capsule** from three inline design previews.

## Presentation

Chat PR references render a lifecycle-colored vector PR/merge icon, bold PR
number, subtle divider, and separately colored CI icon/text within one quiet
rounded surface. Merged/open/closed states use accent/green/red; CI passed,
running and failed use green/amber/red. Unknown/no-checks states remain muted.
Merging never implies passing CI. Running CI uses a static clock in the capsule,
not an endless animation; the shared status cache continues updating it.

Named Markdown links and bare GitHub PR URLs use the same capsule in paragraphs,
assistant lists, RTL prose and table cells. The surrounding paragraph remains a
single native selectable label and retains its existing click-versus-drag
semantics. Selection/copy yields readable PR number and CI text with nonbreaking
spaces, not an opaque placeholder; original messages and exports are unchanged.
Normal links and code literals retain existing behavior. Capsule hover shows
repository, lifecycle/check/review details, PR update time and refresh errors.
The time is explicitly **PR updated**, not a fabricated last-poll timestamp.

Width is measured before wrapping using normal font metrics plus tracking in a
nonbreaking text run. Find-highlight splits are coalesced for that run, preserving
capsule width without growing surrounding baselines. The capsule's own colors
remain independent of find highlighting; native selection still operates on the
paragraph text. No new network call, modal, permission or background process is
introduced by rendering a capsule.

Files: `src/markdown/pr_capsule.rs`, its tests, Markdown parser/render integration,
list/table layout integration, and the read-only `pulls::reference` cache lookup.

## Validation

- Four new capsule tests pass: independent colors/hover metadata; light/dark,
  three zoom factors, narrow/wide prose/list/table/RTL bounds; dynamic cached CI
  updates and click-versus-selection drag; native clipboard copy and find width.
- Existing Markdown focused run: **29 passed** before the fourth capsule test
  was added. The complete final run includes all four capsule tests.
- Complete `bash scripts/test-rust.sh`: **2,362 passed, 0 failed, 2 existing
  ignored**. Every contention-sensitive group ran once through the repository
  runner. Log: `/tmp/pui-pr-capsule-full-rust.log`.
- `cargo clippy --offline --locked --all-targets`: completed successfully, only
  the existing cloud/remote warnings; no new capsule warnings. Log:
  `/tmp/pui-pr-capsule-clippy.log`.
- Targeted rustfmt for all changed Rust files and `git diff --check`: passed.

Earlier test compilation found a missing fixture renderer identity argument and
an omitted `Some` in find setup; both test-only mistakes were fixed before the
passing final run. No test assertion was weakened.

No installed-app rebuild/restart, live GitHub lookup, release publication or
live-app screenshot capture was performed. The HTML shown for design selection
was a mockup, not a screenshot of the installed application.
