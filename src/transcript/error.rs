//! A final provider error and its explicit, same-chat continuation action.
use super::*;

pub(super) fn row(ui: &mut egui::Ui, error: &str, enabled: Option<bool>) -> bool {
    let colors = crate::theme::palette(ui.ctx());
    let Some(enabled) = enabled else {
        ui.add(egui::Label::new(RichText::new(error).color(colors.red)).wrap());
        return false;
    };
    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
        // Reserve the action first so even unbroken provider addresses wrap to
        // its left, rather than pushing Continue outside a narrow chat column.
        let response = ui
            .add_enabled(
                enabled,
                egui::Button::new(RichText::new("Continue").size(12.0))
                    .fill(colors.surface)
                    .stroke(Stroke::new(1.0, colors.border))
                    .corner_radius(7),
            )
            .on_hover_text(
                "Continue this task in the same chat, keeping completed work and your draft.",
            );
        ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
            ui.add(egui::Label::new(RichText::new(error).color(colors.red)).wrap());
        });
        response.clicked()
    })
    .inner
}

#[cfg(test)]
mod tests;
