//! Case-sensitive literal projection over the bounded retained buffer, no page requests.
use super::*;
pub(in crate::app::webview) fn matches(line: &ConsoleLine, query: &str) -> bool {
    line.text.contains(query) || line.source.contains(query)
}
pub(in crate::app::webview) fn show(
    ui: &mut egui::Ui,
    key: &str,
    query: &mut String,
    lines: &std::collections::VecDeque<ConsoleLine>,
    errors_only: bool,
) {
    let available = ui.is_enabled()
        && !egui::Popup::is_any_open(ui.ctx())
        && ui.memory(|m| m.top_modal_layer().is_none())
        && prompt_ui::keyboard_available(ui);
    ui.push_id(("console-search", key), |ui| {
        ui.horizontal_wrapped(|ui| {
            let response = ui.add_enabled(
                available,
                egui::TextEdit::singleline(query)
                    .hint_text("Find message or source…")
                    .char_limit(128)
                    .desired_width((ui.available_width() - 70.0).clamp(80.0, 300.0)),
            );
            let focused =
                response.has_focus() || ui.memory(|m| m.focus_lost_by_escape(response.id));
            if focused {
                ui.input_mut(|i| {
                    for modifiers in [egui::Modifiers::COMMAND, egui::Modifiers::CTRL] {
                        i.consume_key(modifiers, egui::Key::Enter);
                        i.consume_key(modifiers, egui::Key::Backspace);
                    }
                });
            }
            let clear = ui.add_enabled(
                available && !query.is_empty(),
                egui::Button::new(theme::small(ui, "Clear search")).frame(false),
            );
            let chord = clear.has_focus()
                && ui.input_mut(|i| {
                    let mut consumed = false;
                    for modifiers in [egui::Modifiers::COMMAND, egui::Modifiers::CTRL] {
                        consumed |= i.consume_key(modifiers, egui::Key::Enter);
                        consumed |= i.consume_key(modifiers, egui::Key::Backspace);
                    }
                    consumed
                });
            let escape = focused
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if response.changed() || clear.clicked() && !chord || escape {
                if clear.clicked() && !chord || escape {
                    query.clear();
                }
                context_capture::clear_source(ui.ctx());
            }
            #[cfg(test)]
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("console-search-input"),
                    (response.id, response.rect, response.enabled()),
                );
                d.insert_temp(
                    egui::Id::new("console-search-clear"),
                    (clear.id, clear.rect, clear.enabled()),
                );
            });
        });
        if !query.is_empty() {
            let count = lines
                .iter()
                .filter(|l| (!errors_only || l.error) && matches(l, query))
                .count();
            ui.label(theme::small(
                ui,
                format!(
                    "{count} matching · literal, case-sensitive · retained message/source only"
                ),
            ));
        }
    });
}
#[cfg(test)]
mod tests;
