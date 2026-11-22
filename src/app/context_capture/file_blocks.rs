//! Explicit, bounded projection of the actual Markdown labels in one loaded file.
//! Native cross-widget selection remains untouched. No source coordinates are inferred.
use super::*;
use std::{collections::HashSet, path::Path, sync::Arc};

const MAX_BLOCKS: usize = 256;
const MAX_PROJECTION: usize = 256 * 1024;
const MAX_DOCUMENT: usize = 512 * 1024;
const MAX_PATH: usize = 8192;
fn key() -> egui::Id {
    egui::Id::new("file-rendered-block-capture")
}
fn layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground, key())
}
#[derive(Clone)]
struct Block {
    text: String,
    preview: String,
    lines: usize,
    chosen: bool,
}
#[derive(Clone, Copy)]
enum Action {
    Review,
    Capture,
}
#[derive(Clone)]
struct Activation {
    root: PathBuf,
    session: String,
    path: PathBuf,
    document: u64,
    draft: String,
    pass: u64,
}
#[derive(Clone)]
struct Launch {
    response: egui::Id,
    root: PathBuf,
    session: String,
    path: PathBuf,
    document: u64,
    pass: u64,
}

/// Called immediately before the file UI. Freeze only an explicit activation of
/// its previous live enabled launch control, before any later composer input.
pub(super) fn prepare(app: &mut Pui, ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<Arc<Activation>>(key().with("activation")));
    let Some(launch) = ctx.data(|data| data.get_temp::<Arc<Launch>>(key().with("launch-response")))
    else {
        return;
    };
    if launch.pass.checked_add(1) != Some(ctx.cumulative_pass_nr())
        || launch.root != app.prefs.cwd
        || launch.session != app.review_comment_owner()
    {
        return;
    }
    let Some(response) = ctx.read_response(launch.response) else {
        return;
    };
    if !response.enabled()
        || ctx.memory(|memory| memory.top_modal_layer().is_some())
        || egui::Popup::is_any_open(ctx)
        || prompt_ui::modal_owns_current_pass(ctx)
    {
        return;
    }
    let pointer = ctx.input(|input| {
        input.pointer.primary_released()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|pos| response.interact_rect.contains(pos))
    });
    if response.has_focus() || pointer {
        ctx.input_mut(|input| {
            input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
            input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
        });
    }
    let keyboard = response.has_focus()
        && ctx.input(|input| input.events.iter().any(|event| matches!(event,
            egui::Event::Key { key: egui::Key::Enter | egui::Key::Space, pressed: true, modifiers, .. }
                if *modifiers == egui::Modifiers::NONE
        )));
    if !(pointer || keyboard) || app.draft.len() > chips::MAX_DRAFT_BYTES {
        return;
    }
    let activation = Activation {
        root: app.prefs.cwd.clone(),
        session: app.review_comment_owner(),
        path: launch.path.clone(),
        document: launch.document,
        draft: app.draft.clone(),
        pass: ctx.cumulative_pass_nr(),
    };
    ctx.data_mut(|data| data.insert_temp(key().with("activation"), Arc::new(activation)));
}

#[derive(Clone)]
struct Selection {
    root: PathBuf,
    session: String,
    path: PathBuf,
    document: u64,
    source: Arc<str>,
    draft: Option<String>,
    blocks: Vec<Block>,
    ids: HashSet<egui::Id>,
    bytes: usize,
    projecting: bool,
    seen: u64,
    action: Option<Action>,
    reviewed: Option<Snippet>,
    error: Option<&'static str>,
    notice: Option<&'static str>,
    first: bool,
}

pub(super) fn clear(ctx: &egui::Context) {
    ctx.data_mut(|data| {
        data.remove::<Arc<Selection>>(key());
        data.remove::<Arc<Launch>>(key().with("launch-response"));
        data.remove::<Arc<Activation>>(key().with("activation"));
    });
}
fn take(ctx: &egui::Context) -> Option<Arc<Selection>> {
    ctx.data_mut(|data| {
        let selection = data.get_temp::<Arc<Selection>>(key());
        data.remove::<Arc<Selection>>(key());
        selection
    })
}
fn put(ctx: &egui::Context, selection: Arc<Selection>) {
    ctx.data_mut(|data| data.insert_temp(key(), selection));
}
pub(super) fn inputs(ctx: &egui::Context) {
    if ctx
        .data(|data| data.get_temp::<Arc<Selection>>(key()))
        .is_none()
    {
        return;
    }
    if ctx.memory(|memory| memory.top_modal_layer().is_some_and(|top| top != layer())) {
        clear(ctx);
        return;
    }
    let cancel = ctx.input_mut(|input| {
        input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
        input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
    });
    if cancel {
        clear(ctx);
        isolate(ctx);
    }
}
fn isolate(ctx: &egui::Context) {
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            !matches!(
                event,
                egui::Event::Key { .. }
                    | egui::Event::Text(_)
                    | egui::Event::Paste(_)
                    | egui::Event::Copy
                    | egui::Event::Cut
                    | egui::Event::Ime(_)
            )
        });
    });
}

/// Called only by the file viewer, not unscoped tool/thinking text or a browser DOM.
/// Rendering is not virtualized: even offscreen labels are offered during this pass.
pub(in crate::app) fn show(
    ui: &mut egui::Ui,
    path: &Path,
    source: &str,
    document: u64,
    render: impl FnOnce(&mut egui::Ui),
) {
    let ctx = ui.ctx().clone();
    let pass = ctx.cumulative_pass_nr();
    let owner = ctx
        .data(|data| {
            data.get_temp::<(PathBuf, String, u64)>(egui::Id::new("pui-file-capture-owner"))
        })
        .filter(|(root, session, bound)| {
            *bound == pass && root.as_os_str().len() <= MAX_PATH && session.len() <= MAX_PATH
        });
    let enabled = ui.is_enabled()
        && owner.is_some()
        && source.len() <= MAX_DOCUMENT
        && path.as_os_str().len() <= MAX_PATH
        && !egui::Popup::is_any_open(&ctx)
        && ctx.memory(|memory| memory.top_modal_layer().is_none())
        && !prompt_ui::modal_owns_current_pass(&ctx);
    let launch = ui
        .push_id(key().with("launch-id"), |ui| {
            prompt_ui::button(
                ui,
                "Capture rendered blocks…",
                prompt_ui::Button::Secondary,
                enabled,
            )
        })
        .inner;
    // Retain only one bounded previous live launch identity, never document text.
    if let Some((root, session, _)) = &owner
        && path.as_os_str().len() <= MAX_PATH
    {
        ctx.data_mut(|data| {
            data.insert_temp(
                key().with("launch-response"),
                Arc::new(Launch {
                    response: launch.id,
                    root: root.clone(),
                    session: session.clone(),
                    path: path.to_owned(),
                    document,
                    pass,
                }),
            )
        });
    }
    #[cfg(test)]
    ctx.data_mut(|data| data.insert_temp(key().with("launch"), launch.rect));
    let activation = ctx
        .data(|data| data.get_temp::<Arc<Activation>>(key().with("activation")))
        .filter(|activation| {
            activation.pass == pass
                && activation.path == path
                && activation.document == document
                && owner.as_ref().is_some_and(|(root, session, _)| {
                    root == &activation.root && session == &activation.session
                })
        });
    if launch.clicked()
        && enabled
        && let Some(activation) = activation
    {
        let (root, session, _) = owner.clone().unwrap();
        source::clear(&ctx);
        put(
            &ctx,
            Arc::new(Selection {
                root,
                session,
                path: path.to_owned(),
                document,
                source: source.into(),
                draft: Some(activation.draft.clone()),
                blocks: Vec::new(),
                ids: HashSet::new(),
                bytes: 0,
                projecting: true,
                seen: pass,
                action: None,
                reviewed: None,
                error: None,
                notice: None,
                first: true,
            }),
        );
        // Do not let opening-frame chords reach an approval/composer later in this pass.
        isolate(&ctx);
    }
    if let Some(mut state) = take(&ctx) {
        let state_mut = Arc::make_mut(&mut state);
        if state_mut.path == path
            && state_mut.document == document
            && ui.is_enabled()
            && owner.as_ref().is_some_and(|(root, session, _)| {
                root == &state_mut.root && session == &state_mut.session
            })
            && ctx.memory(|memory| memory.top_modal_layer().is_none_or(|top| top == layer()))
            && !prompt_ui::modal_owns_current_pass(&ctx)
        {
            state_mut.seen = pass;
            put(&ctx, state);
        }
        // A different document invalidates, never retargets the frozen selection.
    }
    render(ui);
    let Some(mut state) = take(&ctx) else { return };
    let state_mut = Arc::make_mut(&mut state);
    state_mut.projecting = false;
    state_mut.ids.clear();
    let mut dismiss = false;
    let shown = prompt_ui::modal(&ctx, key(), 540.0, |ui| {
        prompt_ui::heading(
            ui,
            if state_mut.reviewed.is_some() {
                "Review captured blocks"
            } else {
                "Choose rendered blocks"
            },
            "Untrusted displayed text · draft only · never sends or runs",
        );
        let mut path_preview: String = path.to_string_lossy().chars().take(160).collect();
        if path.to_string_lossy().chars().count() > 160 {
            path_preview.push('…');
        }
        ui.add(egui::Label::new(theme::small(ui, &path_preview)).wrap());
        ui.add(egui::Label::new(theme::small(ui, "Block numbers follow rendered label order; line numbers are local to each block, not Markdown source lines. Omitted blocks remain explicit gaps.")).wrap());
        egui::ScrollArea::vertical()
            .id_salt("rendered-block-options")
            .max_height((ctx.screen_rect().height() * 0.42).clamp(70.0, 360.0))
            .show(ui, |ui| {
                if let Some(reviewed) = &state_mut.reviewed {
                    ui.add(egui::Label::new(&reviewed.text).selectable(true).wrap());
                } else {
                    for (index, block) in state_mut.blocks.iter_mut().enumerate() {
                        let toggle = prompt_ui::compact_toggle_row(
                            ui,
                            &mut block.chosen,
                            &format!(
                                "Block {} · rendered lines 1–{} · {} bytes",
                                index + 1,
                                block.lines,
                                block.text.len()
                            ),
                        );
                        #[cfg(test)]
                        ctx.data_mut(|data| {
                            data.insert_temp(key().with(("block", index)), toggle.rect)
                        });
                        let _ = toggle;
                        // Preview only; the complete immutable block, including blank lines,
                        // is selected. An ellipsis here is never part of captured text.
                        ui.add(egui::Label::new(theme::small(ui, &block.preview)).wrap());
                        ui.separator();
                    }
                }
            });
        if let Some(error) = state_mut.error.or(state_mut.notice) {
            ui.add(egui::Label::new(theme::small(ui, error)).wrap());
        }
        prompt_ui::footer(ui, |ui| {
            let action = prompt_ui::button(
                ui,
                if state_mut.reviewed.is_some() {
                    "Add to draft"
                } else {
                    "Review selection"
                },
                prompt_ui::Button::Primary,
                state_mut.error.is_none()
                    && state_mut.draft.is_some()
                    && state_mut.blocks.iter().any(|block| block.chosen),
            );
            if action.clicked() {
                // Only explicit actions compare the bounded complete document snapshot.
                if state_mut.source.as_ref() != source {
                    state_mut.error = Some("Document changed. Cancel and start a new capture.");
                } else {
                    state_mut.action = Some(if state_mut.reviewed.is_some() {
                        Action::Capture
                    } else {
                        Action::Review
                    });
                }
            }
            let cancel = prompt_ui::button(ui, "Cancel", prompt_ui::Button::Secondary, true);
            if state_mut.first {
                cancel.request_focus();
                state_mut.first = false;
            }
            dismiss = cancel.clicked();
            #[cfg(test)]
            ctx.data_mut(|data| {
                data.insert_temp(key().with("action"), action.rect);
                data.insert_temp(key().with("cancel"), cancel.rect);
            });
        });
    });
    dismiss |= shown.should_close();
    isolate(&ctx);
    if !dismiss {
        put(&ctx, state);
    }
}

/// Projection is active for one render only; no background work or retained document cache.
pub(crate) fn offer(response: &egui::Response, text: &str) {
    let ctx = &response.ctx;
    let Some(mut state) = take(ctx) else { return };
    let selection = Arc::make_mut(&mut state);
    if selection.projecting
        && selection.error.is_none()
        && !text.is_empty()
        && selection.ids.insert(response.id)
    {
        if selection.blocks.len() >= MAX_BLOCKS
            || selection.bytes.saturating_add(text.len()) > MAX_PROJECTION
        {
            selection.error = Some(
                "Document projection exceeds 256 blocks or 256 KiB. Nothing was selected; use ordinary single-block selection instead.",
            );
            selection.blocks.clear();
        } else {
            selection.bytes += text.len();
            let mut chars = text.chars();
            let mut preview: String = chars.by_ref().take(160).collect();
            if chars.next().is_some() {
                preview.push('…');
            }
            selection.blocks.push(Block {
                text: text.to_owned(),
                preview,
                lines: lines(text),
                chosen: false,
            });
        }
    }
    put(ctx, state);
}
fn lines(text: &str) -> usize {
    // A trailing newline terminates the last line rather than inventing a new one.
    text.lines().count().max(1)
}
fn selected(selection: &Selection) -> Result<Snippet, &'static str> {
    let mut text = String::new();
    let mut count = 0;
    for (index, block) in selection
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| block.chosen)
    {
        let heading = format!(
            "[Rendered block {} · lines 1–{} within this block]\n",
            index + 1,
            block.lines
        );
        let separator = if text.is_empty() { "" } else { "\n\n" };
        if text
            .len()
            .saturating_add(separator.len())
            .saturating_add(heading.len())
            .saturating_add(block.text.len())
            > MAX_BYTES
        {
            return Err(
                "Selection exceeds 24 KiB including provenance. Deselect blocks; no content was added or truncated.",
            );
        }
        text.push_str(separator);
        text.push_str(&heading);
        text.push_str(&block.text);
        count += 1;
    }
    if count == 0 {
        return Err("Choose at least one nonempty rendered block. Nothing was added.");
    }
    Ok(Snippet {
        // The exact (bounded) path is body provenance, not the chip's compact label.
        label: format!("Markdown preview · {count} explicit rendered blocks (not source lines)"),
        text,
    })
}

/// Keep the activation-time draft snapshot; validate on explicit Review/Add.
/// Existing draft-only captures are allowed in demo/read-only chats too: no tool,
/// permission reply, provider call, file write or automatic send is involved.
pub(super) fn collect(app: &mut Pui) {
    let ctx = app.ctx.clone();
    let Some(mut state) = take(&ctx) else { return };
    let selection = Arc::make_mut(&mut state);
    if selection.seen != ctx.cumulative_pass_nr()
        || selection.root != app.prefs.cwd
        || selection.session != app.review_comment_owner()
    {
        return;
    }
    if let Some(action) = selection.action.take() {
        if selection.draft.as_deref() != Some(app.draft.as_str()) {
            selection.error =
                Some("Draft changed. Cancel and start a new capture; nothing was added.");
        } else {
            match action {
                Action::Review => match selected(selection) {
                    Ok(mut snippet) => {
                        let provenance = format!(
                            "Document: {}\nRendered snapshot; block gaps are intentional.\n\n",
                            selection.path.display()
                        );
                        if provenance.len().saturating_add(snippet.text.len()) > MAX_BYTES {
                            selection.notice = Some(
                                "Selection plus document provenance exceeds 24 KiB. Nothing was added.",
                            );
                        } else {
                            snippet.text.insert_str(0, &provenance);
                            selection.notice = None;
                            selection.reviewed = Some(snippet);
                            selection.first = true;
                        }
                    }
                    Err(error) => {
                        // Selection validation is recoverable without rebasing snapshots.
                        selection.notice = Some(error);
                    }
                },
                Action::Capture => {
                    if let Some(snippet) = selection.reviewed.take() {
                        if let Err(error) = app.context_chips.append(&mut app.draft, snippet) {
                            app.state.error = Some(error.into());
                        } else {
                            app.focus_composer = true;
                        }
                        // Consume exactly once, including a failed whole-or-reject append.
                        ctx.request_repaint();
                        return;
                    }
                }
            }
        }
        ctx.request_repaint();
    }
    put(&ctx, state);
}

#[cfg(test)]
mod activation_tests;
#[cfg(test)]
mod tests;
