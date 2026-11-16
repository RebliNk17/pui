//! Frozen presentation only; the live CEF callback ring continues normally.
use super::*;
pub(in crate::app::webview) fn toggle(
    paused: &mut Option<std::collections::VecDeque<ConsoleLine>>,
    visible: &std::collections::VecDeque<ConsoleLine>,
    ctx: &egui::Context,
) {
    if paused.is_some() {
        *paused = None;
    } else {
        *paused = Some(visible.clone());
    }
    context_capture::clear_source(ctx);
}
pub(in crate::app::webview) fn show(ui: &mut egui::Ui, key: &str, paused: bool) -> bool {
    let response=ui.push_id(("console-pause",key,paused),|ui|ui.add_enabled(!egui::Popup::is_any_open(ui.ctx())&&ui.memory(|m|m.top_modal_layer().is_none())&&prompt_ui::keyboard_available(ui),egui::Button::new(theme::small(ui,if paused{"Resume console"}else{"Pause console"})).frame(false))).inner.on_hover_text("Freeze displayed retained messages, search and draft captures. The page keeps running and collecting into its bounded live buffer; Resume shows the current buffer, not an unlimited backlog.");
    let chord = response.has_focus()
        && ui.input_mut(|i| {
            let mut consumed = false;
            for modifiers in [egui::Modifiers::COMMAND, egui::Modifiers::CTRL] {
                consumed |= i.consume_key(modifiers, egui::Key::Enter);
                consumed |= i.consume_key(modifiers, egui::Key::Backspace);
            }
            consumed
        });
    #[cfg(test)]
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            egui::Id::new("console-pause-control"),
            (response.id, response.rect, response.enabled()),
        )
    });
    response.clicked() && !chord
}
#[cfg(test)]
mod tests;
