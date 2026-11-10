# UI conventions

## Prompts and confirmations

- The Quiet Mac prompt design is mandatory. Do not introduce or restore stock egui/rfd message dialogs, even for new features.
- Use `confirmations::Dialog` for confirmations (including destructive actions). It supplies bounded width, shared typography, danger/secondary actions, safe initial focus, and Escape/backdrop cancellation.
- Use `prompt_ui::modal` for custom modal forms or information dialogs, with `prompt_ui::heading`, `prompt_ui::button`/`confirmations::actions`, and `prompt_ui::footer`. Shared framing alone is not a complete migration.
- Keep agent questions/approvals inline using `prompt_ui` primitives. Do not replace them with global dialogs. Preserve permission scope and disabled-action checks.
- Native file pickers, the themed resizable diff utility window, full-screen image theater, and compact question-navigation controls retain their specialized behavior.
- `tests/prompt_design.rs` guards against legacy modal constructors/import aliases and stock controls in custom prompt renderers. Do not weaken it to add a new prompt; extend shared primitives instead.
- Test new prompt callers in light/dark themes, narrow windows/zoom, and cancellation/disabled states. See `docs/quiet-mac-prompts.md`.
