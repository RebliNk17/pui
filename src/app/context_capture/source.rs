//! Same-label source selection only; immutable snapshots retain no full file copy.
use super::*;
use std::{ops::Range, path::Path, sync::Arc};

#[derive(Clone)]
struct SourceMenu {
    owner: egui::Id,
    response: egui::Id,
    selected: Option<Arc<Snippet>>,
    excerpt: Option<Arc<Snippet>>,
    kind: Kind,
    seen: u64,
    position: egui::Pos2,
}
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Source,
    Diff,
    Rendered,
    Console,
}
impl Kind {
    fn action(self) -> &'static str {
        match self {
            Self::Source => "Add selected source to chat",
            Self::Diff => "Add selected diff to chat",
            Self::Rendered => "Quote selected text to chat",
            Self::Console => "Add selected console text to chat",
        }
    }
    fn hint(self) -> &'static str {
        match self {
            Self::Source => {
                "Selection is one source widget only. Adds up to 24 KiB to the draft; never sends."
            }
            Self::Diff => {
                "Select content within one diff hunk. Gutters are excluded. Adds up to 24 KiB to the draft; never sends."
            }
            Self::Rendered => {
                "One rendered paragraph, code block or table cell only—not exact Markdown source. Adds up to 24 KiB to the draft; never sends."
            }
            Self::Console => {
                "Select text within one console message. No whole-message fallback; adds only to your draft, never sends."
            }
        }
    }
}

fn key() -> egui::Id {
    egui::Id::new("source-context-menu")
}

const DEMO_TEXT: &str = "// Read-only source selection fixture\nlet chosen = \"literal Unicode: 世界\";\n// Adding context does not send or run code.\n";
pub(in crate::app) fn demo(ctx: &egui::Context) -> &'static str {
    ctx.data_mut(|data| data.insert_temp(key().with("demo"), true));
    DEMO_TEXT
}
pub(in crate::app) fn clear(ctx: &egui::Context) {
    if let Some(menu) = ctx.data_mut(|data| {
        let menu = data.get_temp::<SourceMenu>(key());
        data.remove::<SourceMenu>(key());
        menu
    }) {
        egui::Popup::close_id(ctx, menu.owner);
    }
}

pub(super) fn inputs(ctx: &egui::Context) {
    let Some(menu) = ctx.data(|data| data.get_temp::<SourceMenu>(key())) else {
        return;
    };
    if !egui::Popup::is_id_open(ctx, menu.owner)
        || ctx.memory(|memory| memory.top_modal_layer().is_some())
    {
        clear(ctx);
        return;
    }
    if ctx.read_response(menu.response).is_none() {
        clear(ctx);
        return;
    }
    let escape = ctx.input_mut(|input| {
        input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
        input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
    });
    if escape {
        clear(ctx);
        ctx.memory_mut(|memory| memory.request_focus(menu.response));
    }
}

pub(super) fn finish(ctx: &egui::Context) {
    let pass = ctx.cumulative_pass_nr();
    if ctx.data(|data| {
        data.get_temp::<SourceMenu>(key())
            .is_some_and(|menu| menu.seen != pass)
    }) {
        clear(ctx);
    }
}

/// Character offsets are from the original label text (never byte indices or rendered line numbers).
fn selected(source: &str, range: Range<usize>, path: &Path, first_line: usize) -> Option<Snippet> {
    if range.start >= range.end {
        return None;
    }
    let mut start = None;
    let mut end = None;
    let mut line = first_line;
    let mut start_line = first_line;
    let mut end_line = first_line;
    let mut count = 0;
    for (index, (byte, character)) in source.char_indices().enumerate() {
        if index == range.start {
            start = Some(byte);
            start_line = line;
        }
        if index == range.end {
            end = Some(byte);
            break;
        }
        if index >= range.start {
            end_line = line;
        }
        if character == '\n' || character == '\r' && source.as_bytes().get(byte + 1) != Some(&b'\n')
        {
            line += 1;
        }
        count = index + 1;
    }
    if range.end == count {
        end.get_or_insert(source.len());
    }
    let text = source.get(start?..end?)?;
    let text = bounded(text);
    let label = format!(
        "{}:{}–{} · selected source",
        path.display(),
        start_line,
        end_line
    );
    Some(Snippet { label, text })
}
pub(super) fn bounded(source: &str) -> String {
    // Retain one extra scalar as truncation evidence; queue() adds the shared marker once.
    let mut end = source.len().min(MAX_BYTES + 4);
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    source[..end].to_owned()
}
fn enqueue(ctx: &egui::Context, snippet: &Snippet) {
    // Shared queue sanitizes labels and performs the final bound/marker once.
    queue(ctx, &snippet.label, &snippet.text);
}

pub(in crate::app) fn offer(
    response: &egui::Response,
    path: &Path,
    source: &str,
    first_line: usize,
) {
    let ctx = &response.ctx;
    let demo = source == DEMO_TEXT
        && ctx
            .data_mut(|data| data.remove_temp::<bool>(key().with("demo")))
            .unwrap_or(false);
    menu(response, Kind::Source, demo, |range| {
        let range = if demo {
            let start = DEMO_TEXT.find('\n').unwrap() + 1;
            let end = start + DEMO_TEXT[start..].find('\n').unwrap();
            Some(DEMO_TEXT[..start].chars().count()..DEMO_TEXT[..end].chars().count())
        } else {
            range
        };
        let selected = range
            .and_then(|range| selected(source, range, path, first_line))
            .map(Arc::new);
        let excerpt = Arc::new(Snippet {
            label: format!("{}:{} · file excerpt", path.display(), first_line),
            text: bounded(source),
        });
        (selected, Some(excerpt))
    });
}

pub(in crate::app) fn console_menu(
    response: &egui::Response,
    snapshot: impl FnOnce(Option<Range<usize>>) -> Option<(String, String)>,
) {
    menu(response, Kind::Console, false, |range| {
        let selected = snapshot(range).map(|(label, text)| Arc::new(Snippet { label, text }));
        (selected, None)
    });
}

pub(super) fn menu(
    response: &egui::Response,
    kind: Kind,
    demo: bool,
    snapshot: impl FnOnce(Option<Range<usize>>) -> (Option<Arc<Snippet>>, Option<Arc<Snippet>>),
) {
    let ctx = &response.ctx;
    let owner = response.id.with("source-selection-menu");
    inputs(ctx);
    // Freeze before creating the popup; its click may deselect the label next pass.
    if response.secondary_clicked() || demo {
        // Opening-frame ownership precedes later inline approvals, including
        // secondary-click plus an approval/send/Stop chord in the same pass.
        ctx.input_mut(|input| {
            input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
            input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
        });
        let range = egui::text_selection::LabelSelectionState::load(ctx)
            .range_for_response(response)
            .map(|range| range.as_sorted_char_range());
        let (selected, excerpt) = snapshot(range);
        let seen = ctx.cumulative_pass_nr();
        ctx.data_mut(|data| {
            data.insert_temp(
                key(),
                SourceMenu {
                    owner,
                    response: response.id,
                    selected,
                    excerpt,
                    kind,
                    seen,
                    position: response
                        .interact_pointer_pos()
                        .unwrap_or(response.rect.left_top() + egui::vec2(30.0, 80.0)),
                },
            )
        });
    }
    if demo {
        egui::Popup::open_id(ctx, owner);
    }
    if let Some(mut menu) = ctx
        .data(|data| data.get_temp::<SourceMenu>(key()))
        .filter(|menu| menu.owner == owner)
    {
        menu.seen = ctx.cumulative_pass_nr();
        ctx.data_mut(|data| data.insert_temp(key(), menu));
    }
    let popup = egui::Popup::context_menu(response)
        .id(owner)
        .frame(prompt_ui::popup(ctx));
    let popup = if demo { popup.open_memory(None) } else { popup };
    let popup = if let Some(menu) = ctx
        .data(|data| data.get_temp::<SourceMenu>(key()))
        .filter(|menu| menu.owner == owner)
    {
        popup.at_position(menu.position)
    } else {
        popup
    };
    let shown = popup.show(|ui| {
        theme::menu_controls(ui);
        ui.set_width(240.0_f32.min((ctx.screen_rect().width() - 40.0).max(120.0)));
        let Some(menu) = ctx
            .data(|data| data.get_temp::<SourceMenu>(key()))
            .filter(|menu| menu.owner == owner)
        else {
            return;
        };
        let selection = ui.add_enabled(
            menu.selected.is_some(),
            egui::Button::new(menu.kind.action()).wrap(),
        );
        if response.secondary_clicked() && menu.selected.is_some() {
            ui.memory_mut(|memory| {
                memory.request_focus_with_filter(
                    selection.id,
                    egui::EventFilter {
                        escape: true,
                        ..Default::default()
                    },
                )
            });
        }
        if selection.clicked()
            && let Some(selected) = &menu.selected
        {
            enqueue(ctx, selected);
            ui.close();
        }
        if let Some(excerpt) = &menu.excerpt
            && ui
                .add_enabled(
                    !excerpt.text.is_empty(),
                    egui::Button::new("Add file excerpt to chat").wrap(),
                )
                .clicked()
        {
            enqueue(ctx, excerpt);
            ui.close();
        }
        ui.add(egui::Label::new(theme::small(ui, menu.kind.hint())).wrap());
        #[cfg(test)]
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("source-context-selection"), selection.rect)
        });
    });
    // A new inline structured question may request focus later in this pass.
    // Let this popup consume its navigation/activation first, then remove only
    // keyboard/text events so no second surface can reuse them, even on close.
    if shown.is_some() {
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
            })
        });
    }
    if !egui::Popup::is_id_open(ctx, owner)
        && ctx.data(|data| {
            data.get_temp::<SourceMenu>(key())
                .is_some_and(|menu| menu.owner == owner)
        })
    {
        clear(ctx);
    }
}

#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
