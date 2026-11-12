//! Bounded page console records; explicit draft capture is the only agent-facing action.
use super::*;
use std::sync::Arc;
mod selection;

#[cfg(any(test, target_os = "macos"))]
const MAX_CHARS: usize = 4_000;
#[cfg(any(test, target_os = "macos"))]
const MAX_LINES: usize = 300;

#[derive(Clone)]
pub(super) struct ConsoleLine {
    id: u64,
    pub error: bool,
    text: Arc<str>,
    source: Arc<str>,
    page: Arc<str>,
    line: i32,
    limited: bool,
    omitted_data_source: bool,
}
impl ConsoleLine {
    #[cfg(any(test, target_os = "macos"))]
    fn new(error: bool, message: &str, source: &str, line: i32) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let mut limited = false;
        let mut remaining = MAX_CHARS;
        let mut bounded = |text: &str| -> Arc<str> {
            let result: String = text.chars().take(remaining).collect();
            remaining = remaining.saturating_sub(result.chars().count());
            limited |= result.len() < text.len();
            result.into()
        };
        let text = bounded(message);
        let omitted_data_source = source.starts_with("data:");
        let source = bounded(if omitted_data_source {
            "[inline local HTML]"
        } else {
            source
        });
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            error,
            text,
            source,
            page: "".into(),
            line,
            limited,
            omitted_data_source,
        }
    }
    fn snippet(&self, key: &str) -> (String, String) {
        let label = format!(
            "Browser console {} · {} · message {}",
            if self.error { "error" } else { "message" },
            page_label(key, &self.page),
            self.id
        );
        let mut text = String::with_capacity(self.text.len() + self.source.len() + 120);
        text.push_str(&self.text);
        if !self.source.is_empty() {
            // Local HTML data URLs can contain the entire document. Never attach that as provenance.
            let source = &self.source;
            text.push_str("\nSource: ");
            text.push_str(source);
            if self.line > 0 {
                text.push(':');
                text.push_str(&self.line.to_string());
            }
        }
        if self.limited {
            text.push_str("\n[Console message/source limited to 4,000 characters total]");
        }
        (label, text)
    }
}
fn page_label<'a>(key: &'a str, location: &'a str) -> &'a str {
    if web_url(location) {
        location
    } else if web_url(key) {
        "web page (location unavailable at message time)"
    } else {
        "local HTML preview"
    }
}

/// Called by CEF's UI callback. Clones share immutable strings across page snapshots.
#[cfg(any(test, target_os = "macos"))]
pub(super) fn push(state: &mut PageState, error: bool, message: &str, source: &str, line: i32) {
    if state.console.len() >= MAX_LINES {
        state.console.pop_front();
    }
    let mut message = ConsoleLine::new(error, message, source, line);
    if state.location.len() <= 8192 && web_url(&state.location) {
        // The label is bounded again by the draft queue; do not retain long query strings per row.
        let mut end = state.location.len().min(1024);
        while !state.location.is_char_boundary(end) {
            end -= 1;
        }
        message.page = state.location[..end].into();
    }
    state.console.push_back(message);
}

pub(super) fn show(ui: &mut egui::Ui, key: &str, line: &ConsoleLine) {
    let colors = theme::palette(ui.ctx());
    ui.push_id(("console-message", key, line.id), |ui| {
        ui.horizontal_top(|ui| {
            let width = (ui.available_width() - 28.0 - ui.spacing().item_spacing.x).max(1.0);
            let response = ui
                .allocate_ui_with_layout(
                    Vec2::new(width, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(width);
                        ui.add(
                            egui::Label::new(
                                RichText::new(line.text.as_ref())
                                    .monospace()
                                    .size(11.0)
                                    .color(if line.error { colors.red } else { colors.muted }),
                            )
                            .wrap()
                            .selectable(true),
                        )
                    },
                )
                .inner;
            selection::offer(&response, key, line);
            let (lane, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
            // A long message can begin above the scrolled viewport. Keep its action
            // inside the visible part of that same row, never over the native page.
            let top = response
                .rect
                .top()
                .max(ui.clip_rect().top())
                .min((response.rect.bottom() - 28.0).max(response.rect.top()));
            let rect = egui::Rect::from_min_size(egui::pos2(lane.left(), top), Vec2::splat(28.0));
            let add = ui
                .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    chrome::icon_button(ui, Icon::Details, "Add console message to chat", false)
                })
                .inner;
            add.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    "Add console message to chat",
                )
            });
            let approval_shortcut = add.has_focus()
                && ui.input_mut(|input| {
                    let allow = input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                    let deny = input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
                    allow || deny
                });
            if add.clicked() && !approval_shortcut {
                let (label, text) = line.snippet(key);
                context_capture::queue(ui.ctx(), &label, &text);
            }
            if !line.source.is_empty() {
                response.on_hover_text(if line.omitted_data_source {
                    "Inline local HTML".into()
                } else {
                    format!("{}:{}", line.source, line.line)
                });
            }
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new(("console-context-action", key, line.id)),
                    add.rect,
                );
                data.insert_temp(
                    egui::Id::new(("console-context-focus", key, line.id)),
                    add.id,
                );
            });
        });
        if line.limited {
            ui.label(theme::small(
                ui,
                "Message/source shortened to 4,000 characters total",
            ));
        }
    });
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
