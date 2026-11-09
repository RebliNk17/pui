use super::*;

pub(crate) fn show(ui: &mut egui::Ui, answers: &crate::protocol::QuestionAnswers) {
    let colors = crate::theme::palette(ui.ctx());
    egui::Frame::new()
        .fill(colors.sidebar)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(14)
        .inner_margin(16)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                icons::inline(ui, Icon::Details, colors.accent);
                ui.label(
                    RichText::new(if answers.cancelled {
                        "Questions cancelled"
                    } else {
                        "Your answers"
                    })
                    .size(14.0)
                    .strong(),
                );
            });
            if answers.cancelled {
                ui.label(crate::theme::small(
                    ui,
                    "No answers or approval were given.",
                ));
            } else {
                for entry in &answers.entries {
                    ui.add_space(10.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(&entry.question)
                                .size(12.0)
                                .color(colors.muted),
                        )
                        .wrap(),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(&entry.answer).size(13.5).color(colors.text),
                        )
                        .wrap()
                        .selectable(true),
                    );
                }
            }
        });
    ui.add_space(10.0);
}
