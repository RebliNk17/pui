//! Captured context (reply quotes, source/diff/terminal/console/browser excerpts) lives
//! outside the typed draft as cards above the composer, so typing never detaches or
//! corrupts it. Captures are composed into the message as `[Context: …]` blocks only
//! when sending; nothing here ever sends by itself.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) const MAX_CAPTURES: usize = 16;
pub(super) const MAX_DRAFT_BYTES: usize = 256 * 1024;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// One frozen, untrusted excerpt shown as a card above the composer.
#[derive(Clone)]
pub(in crate::app) struct Quote {
    id: u64,
    label: String,
    text: String,
}

/// Reply quotes (rendered selections or whole replies) are titled "Pi reply".
pub(super) fn is_reply_quote(label: &str) -> bool {
    label.starts_with("Assistant reply") || label.starts_with("Quoted assistant reply")
}

/// Pastes at least this long collapse into a "Pasted text" card instead of the text box.
const PASTE_COLLAPSE_LINES: usize = 8;
const PASTE_COLLAPSE_CHARS: usize = 600;

pub(super) fn is_paste(label: &str) -> bool {
    label.starts_with("Pasted text")
}

/// Whether a paste is long enough to become a card.
pub(in crate::app) fn collapses_paste(text: &str) -> bool {
    text.lines().count() >= PASTE_COLLAPSE_LINES || text.chars().count() >= PASTE_COLLAPSE_CHARS
}

fn paste_label(text: &str) -> String {
    let lines = text.lines().count().max(1);
    format!(
        "Pasted text · {lines} line{} · {} chars",
        if lines == 1 { "" } else { "s" },
        text.chars().count()
    )
}

/// Short card title and icon for a capture label.
fn card_title(label: &str) -> (String, Icon) {
    if is_reply_quote(label) {
        return ("Pi reply".into(), Icon::Quote);
    }
    if is_paste(label) {
        return (label.to_owned(), Icon::Copy);
    }
    let icon = if label.starts_with("Terminal") {
        Icon::Terminal
    } else if label.starts_with("Browser") {
        Icon::Globe
    } else if label.contains("diff") {
        Icon::Code
    } else {
        Icon::File
    };
    (label.split(" · ").next().unwrap_or(label).to_owned(), icon)
}

#[derive(Default)]
pub(in crate::app) struct Draft {
    quotes: Vec<Quote>,
}
impl Draft {
    #[cfg(test)]
    pub(in crate::app) fn quote_ids(&self) -> Vec<u64> {
        self.quotes.iter().map(|q| q.id).collect()
    }
    #[cfg(test)]
    pub(in crate::app) fn labels(&self) -> Vec<String> {
        self.quotes.iter().map(|q| q.label.clone()).collect()
    }
    pub(in crate::app) fn has_quotes(&self) -> bool {
        !self.quotes.is_empty()
    }
    /// Drop all cards (after sending, or when the composer is reset for another chat).
    pub(in crate::app) fn clear_quotes(&mut self) {
        self.quotes.clear();
    }
    /// The typed draft was replaced or edited. Cards are independent of that text, so
    /// this intentionally keeps them; call [`Self::clear_quotes`] to discard captures.
    pub(in crate::app) fn clear(&mut self) {}
    #[cfg(test)]
    pub(super) fn add_quote(&mut self, snippet: Snippet) -> Result<(), &'static str> {
        self.add_with_draft(0, snippet)
    }
    fn add_with_draft(&mut self, draft_len: usize, snippet: Snippet) -> Result<(), &'static str> {
        if self.quotes.len() >= MAX_CAPTURES {
            return Err(
                "Up to 16 captured context cards fit in one message. Remove one or send when ready. Existing text was kept.",
            );
        }
        let total: usize = self
            .quotes
            .iter()
            .map(|q| q.text.len() + q.label.len())
            .sum();
        // The whole composed message (typed text + every block) stays within the draft bound.
        if draft_len + total + snippet.text.len() + snippet.label.len() + 32 > MAX_DRAFT_BYTES {
            return Err("Draft is too large to add more context. Existing text was kept.");
        }
        self.quotes.push(Quote {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            label: snippet.label,
            text: snippet.text,
        });
        Ok(())
    }
    /// Every capture becomes a card; the typed draft is never modified.
    pub(super) fn append(&mut self, draft: &str, snippet: Snippet) -> Result<(), &'static str> {
        self.add_with_draft(draft.len(), snippet)
    }
    /// Append the quote popover's follow-up question to the typed draft.
    pub(super) fn append_question(&mut self, draft: &mut String, question: &str) {
        if question.is_empty() || draft.len() + question.len() + 2 > MAX_DRAFT_BYTES {
            return;
        }
        if !draft.is_empty() {
            draft.push_str("\n\n");
        }
        draft.push_str(question);
    }
    /// Message text sent to Pi: the typed draft, then each capture as a `[Context: …]`
    /// block (the same format captures always used). Sent bubbles show blocks on top.
    pub(in crate::app) fn compose(&self, draft: &str) -> String {
        if self.quotes.is_empty() {
            return draft.to_owned();
        }
        let mut out = draft.trim().to_owned();
        for quote in &self.quotes {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&format!(
                "[Context: {}]\n{}\n[/Context]",
                quote.label, quote.text
            ));
        }
        out
    }
    /// Move well-formed `[Context: …]` blocks out of `text` into cards; returns the rest.
    pub(in crate::app) fn adopt_quotes(&mut self, text: &str) -> String {
        let mut rest = String::new();
        for segment in crate::transcript::context_segments(text) {
            match segment {
                crate::transcript::Segment::Context { label, body }
                    if self.quotes.len() < MAX_CAPTURES =>
                {
                    self.quotes.push(Quote {
                        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                        label: label.to_owned(),
                        text: body.to_owned(),
                    });
                }
                crate::transcript::Segment::Context { label, body } => {
                    rest.push_str(&format!("[Context: {label}]\n{body}\n[/Context]"));
                }
                crate::transcript::Segment::Text(t) => rest.push_str(t),
            }
        }
        rest.trim().to_owned()
    }
}

impl Pui {
    fn context_chips_available(&self, ctx: &egui::Context) -> bool {
        !self.settings_open
            && !self.search_open
            && !self.model_picker_open
            && !self.effort_picker_open
            && !self.kanban.open
            && !self.automation.open
            && (!self.tasks.open
                || self
                    .tasks
                    .current()
                    .is_some_and(|task| self.task_in_chat(task)))
            && ctx
                .read_response(composer_text::widget_id(ctx, "composer"))
                .is_some()
            && self.terminals.layout() != (true, true)
            && self.question_designs.is_none()
            && !self.transcript.gallery.is_open()
            && !self.draft_images.gallery.is_open()
            && !egui::Popup::is_any_open(ctx)
            && !self.completion_active(ctx)
            && ctx.memory(|m| m.top_modal_layer().is_none())
    }
    /// Backspace in an empty, focused composer removes the last card, and long pastes
    /// become a "Pasted text" card. Runs before approvals/composer so these inputs are
    /// owned here and never reach anything else.
    pub(in crate::app) fn context_chip_inputs(&mut self, ctx: &egui::Context) {
        if ctx.memory(|m| m.has_focus(composer_text::widget_id(ctx, "composer"))) {
            let pastes: Vec<String> = ctx.input_mut(|i| {
                let mut long = Vec::new();
                i.events.retain(|event| match event {
                    egui::Event::Paste(text) if collapses_paste(text) => {
                        long.push(text.replace("\r\n", "\n"));
                        false
                    }
                    _ => true,
                });
                long
            });
            for text in pastes {
                let snippet = Snippet {
                    label: paste_label(&text),
                    text,
                    ask: None,
                };
                if let Err(error) = self.context_chips.append(&self.draft, snippet) {
                    self.state.error = Some(error.into());
                }
                self.focus_composer = true;
            }
        }
        if self.context_chips.quotes.is_empty()
            || !self.draft.is_empty()
            || !self.context_chips_available(ctx)
            || !ctx.memory(|m| m.has_focus(composer_text::widget_id(ctx, "composer")))
        {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)) {
            self.context_chips.quotes.pop();
            self.focus_composer = true;
        }
    }
    /// Context cards above the composer: icon + source, preview, × to remove.
    pub(in crate::app) fn context_chips(&mut self, ui: &mut egui::Ui) {
        if self.context_chips.quotes.is_empty() {
            return;
        }
        let ctx = ui.ctx().clone();
        let colors = theme::palette(&ctx);
        let mut remove = None;
        for quote in &self.context_chips.quotes {
            let id = egui::Id::new("composer-quote-card").with(quote.id);
            let expanded = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
            let reply = is_reply_quote(&quote.label);
            let (title, icon) = card_title(&quote.label);
            let frame = egui::Frame::new()
                .fill(colors.accent_bg.gamma_multiply(0.55))
                .stroke(Stroke::new(1.0_f32, colors.accent.gamma_multiply(0.25)))
                .corner_radius(10)
                .inner_margin(egui::Margin {
                    left: 12,
                    right: 6,
                    top: 6,
                    bottom: 6,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 5.0;
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(11.0, 11.0), Sense::hover());
                        icons::draw(ui, rect, icon, colors.accent);
                        ui.add(
                            egui::Label::new(RichText::new(&title).size(10.5).color(colors.muted))
                                .truncate(),
                        )
                        .on_hover_text(if is_paste(&quote.label) {
                            "Collapsed paste · sent with your message. Hover the preview to read it.".to_owned()
                        } else {
                            format!(
                                "{}\nFrozen untrusted excerpt · {} bytes · sent as context with your message; never sends by itself.",
                                quote.label,
                                quote.text.len()
                            )
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (rect, response) =
                                ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
                            if response.hovered() {
                                ui.painter().rect_filled(rect, 5, colors.hover);
                            }
                            icons::draw(
                                ui,
                                egui::Rect::from_center_size(rect.center(), egui::vec2(11.0, 11.0)),
                                Icon::Close,
                                if response.hovered() { colors.text } else { colors.dim },
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    format!("Remove context: {}", quote.label),
                                )
                            });
                            #[cfg(test)]
                            ui.ctx().data_mut(|d| d.insert_temp(id.with("remove-rect"), rect));
                            if response.on_hover_text("Remove").clicked() {
                                remove = Some(quote.id);
                            }
                        });
                    });
                    if is_paste(&quote.label) {
                        let first = quote.text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                        ui.add(
                            egui::Label::new(RichText::new(first).size(11.5).color(colors.muted))
                                .truncate()
                                .sense(Sense::hover()),
                        )
                        .on_hover_ui(|ui| {
                            ui.set_max_width(520.0);
                            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                                ui.label(RichText::new(&quote.text).size(12.0).color(colors.text));
                            });
                        });
                        return;
                    }
                    let mut job = egui::text::LayoutJob::simple(
                        quote.text.clone(),
                        if reply {
                            FontId::proportional(12.0)
                        } else {
                            FontId::monospace(11.5)
                        },
                        colors.text.gamma_multiply(0.85),
                        ui.available_width(),
                    );
                    if !expanded {
                        job.wrap.max_rows = 2;
                    }
                    let long = quote.text.lines().count() > 2 || quote.text.chars().count() > 160;
                    let body = ui.add(egui::Label::new(job).sense(if long {
                        Sense::click()
                    } else {
                        Sense::hover()
                    }));
                    if long
                        && body
                            .on_hover_text(if expanded { "Collapse" } else { "Expand" })
                            .clicked()
                    {
                        ctx.data_mut(|d| d.insert_temp(id, !expanded));
                    }
                });
            let rect = frame.response.rect;
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    rect.left_top() + egui::vec2(0.5, 0.5),
                    egui::vec2(3.0, rect.height() - 1.0),
                ),
                egui::CornerRadius {
                    nw: 10,
                    sw: 10,
                    ne: 0,
                    se: 0,
                },
                colors.accent,
            );
        }
        if let Some(id) = remove {
            self.context_chips.quotes.retain(|q| q.id != id);
            self.focus_composer = true;
        }
        ui.add_space(2.0);
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
