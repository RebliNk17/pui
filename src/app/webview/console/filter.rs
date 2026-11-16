//! Read-only retained-message projection, no page calls or data removal.
use super::*;
pub(in crate::app::webview) fn show(
    ui: &mut egui::Ui,
    errors_only: &mut bool,
    lines: &std::collections::VecDeque<ConsoleLine>,
) {
    ui.horizontal_wrapped(|ui| {
        for (value, label) in [(false, "All"), (true, "Errors")] {
            let response = ui
                .push_id(("console-filter", label), |ui| {
                    ui.add_enabled(
                        !egui::Popup::is_any_open(ui.ctx())
                            && ui.memory(|m| m.top_modal_layer().is_none()),
                        egui::Button::selectable(*errors_only == value, theme::small(ui, label))
                            .frame(false),
                    )
                })
                .inner;
            let chord = response.has_focus()
                && ui.input_mut(|i| {
                    let send = i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                    let stop = i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
                    send || stop
                });
            if response.clicked() && !chord {
                *errors_only = value;
                context_capture::clear_source(ui.ctx());
            }
            #[cfg(test)]
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    egui::Id::new(("console-filter", label)),
                    (response.id, response.rect),
                )
            });
        }
        let errors = lines.iter().filter(|line| line.error).count();
        ui.label(theme::small(
            ui,
            format!(
                "{} shown / {} retained",
                if *errors_only { errors } else { lines.len() },
                lines.len()
            ),
        ));
    });
}
#[cfg(test)]
mod tests;
