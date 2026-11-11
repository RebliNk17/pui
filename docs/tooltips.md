# Tooltip policy

Tooltips should explain hidden information, not echo the interface.

- Keep concise action names/shortcuts for icon-only buttons, including overflow (`…`), close, navigation and browser tools.
- Reveal a full title, name, path or preview only when its rendered galley is actually elided. Do not guess from character counts. Short subagent names need no tooltip.
- Keep information not otherwise visible: status behind a dot, full paths behind basenames, URL targets, attachment filenames, errors, cost semantics, permission scope and non-obvious/destructive behavior.
- Use disabled-hover hints for unavailable-action explanations. Never rely on a tooltip as the only place to explain a required approval or warning.
- Avoid competing whole-row/card tooltips: put hidden status on the status icon and full names on the truncated text area. Keep click/keyboard behavior intact.
- `egui::Label` already reveals elided text automatically. Do not add another full-text tooltip. Use `show_tooltip_when_elided(false)` when supplying a richer path/metadata tooltip instead.

## Shared presentation

`src/theme.rs` sets a **0.3 s** still-pointer delay (was 0.5 s), **0.15 s** grace period and **340 logical-pixel** maximum width. Tooltips use **11 pt** text and respect UI zoom and light/dark themes; body text, menus and dialogs are unchanged.

The vendored egui tooltip container supports the optional named `TextStyle::Name("Tooltip")`. Automatic label-elision tooltips honor the same font instead of inheriting a large label's font. Without that named style, egui retains upstream behavior. See `vendor/README.md`.

## Audit scope

Reviewed sidebar chats/subagents and project headings, Kanban cards, navigation breadcrumbs, workflow summaries/steps/picker, resource navigation/descriptions, terminal paths, changes/output rows, attachments, model/effort selection, expandable sections, inspector actions, transcript controls, file/browser tabs, icon toolbars, permission prompts and scheduled-task details.

Repeated visible titles were removed or made truncation-only. Icon actions, path/URL metadata, attachment/error hints and behavior/scope explanations are retained. Long selected-model labels now reveal their hidden ending. Sidebar overflow controls explicitly say “More options”; short subagent rows stay quiet while status remains available over the icon.

## Regression checks

- `theme::tooltip_tests`: compact scoped typography and automatic-elision recovery in both themes/zoom levels; shorter delay and click dismissal.
- `app::sidebar_tree::tests::child_tooltips_only_reveal_elided_names_and_keep_status_on_icon`: short vs long names, flat/project views, both themes, and independent status hover.
- Existing card/section/workflow layout, clicking, inline rename and prompt-design tests continue to cover unaffected interactions.
