//! Keep the interface stationary until a scale gesture has finished.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Scale {
    pending: Option<u32>,
}

impl Scale {
    pub(super) fn show(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(Layout::top_down(Align::RIGHT), |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.slider(ui);
            });
            if ui
                .add_enabled(
                    self.pending.is_some() || (ui.ctx().zoom_factor() - 1.0).abs() > 0.001,
                    egui::Button::new("Reset to 100%"),
                )
                .clicked()
            {
                self.pending = None;
                ui.ctx().set_zoom_factor(1.0);
            }
        });
    }

    fn slider(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let mut percent = self
            .pending
            .unwrap_or_else(|| (ui.ctx().zoom_factor() * 100.0).round() as u32);
        ui.spacing_mut().slider_width = 110.0;
        // Reserve the same value-field width for 80% and 140%.
        ui.spacing_mut().interact_size.x = 60.0;
        theme::slider_style(ui);
        let response = ui.add(
            egui::Slider::new(&mut percent, 80..=140)
                .step_by(5.0)
                .suffix("%")
                .update_while_editing(false),
        );
        if response.changed() {
            self.pending = Some(percent);
        }
        // This also covers dragging the numeric value, releasing outside the rail,
        // and keyboard/text edits. A changed value alone must never resize the UI
        // while the pointer still owns the gesture.
        if !response.is_pointer_button_down_on() && !response.dragged() {
            if let Some(percent) = self.pending.take() {
                ui.ctx().set_zoom_factor(percent as f32 / 100.0);
            }
        }
        response
    }
}
