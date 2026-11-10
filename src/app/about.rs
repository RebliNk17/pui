//! Product identity, with build provenance secondary to the app description.
use super::*;

pub(super) fn show(ctx: &egui::Context, open: &mut bool) {
    if !*open {
        return;
    }
    let response = prompt_ui::modal(ctx, egui::Id::new("about-pui"), 280.0, |ui| {
        let mut close = prompt_ui::close(ui);
        ui.vertical_centered(|ui| {
            let texture = ctx
                .data(|data| data.get_temp::<egui::TextureHandle>(egui::Id::new("about-icon")))
                .unwrap_or_else(|| {
                    let image = image::load_from_memory(include_bytes!("../../assets/icon.png"))
                        .expect("bundled application icon")
                        .into_rgba8();
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width() as usize, image.height() as usize],
                        image.as_raw(),
                    );
                    let texture =
                        ctx.load_texture("about-icon", color, egui::TextureOptions::LINEAR);
                    ctx.data_mut(|data| {
                        data.insert_temp(egui::Id::new("about-icon"), texture.clone())
                    });
                    texture
                });
            ui.add_space(6.0);
            ui.image((texture.id(), Vec2::splat(58.0)));
            ui.add_space(10.0);
            ui.label(RichText::new("Pui").size(27.0).strong());
            ui.label(theme::small(
                ui,
                format!("Version {}", env!("CARGO_PKG_VERSION")),
            ));
            ui.add_space(14.0);
            ui.label("Your workspace for Pi.");
            ui.label(theme::small(ui, "Models, tools and conversations."));
            ui.label(theme::small(ui, "A little more room to think."));
        });
        ui.add_space(16.0);
        egui::CollapsingHeader::new(theme::small(ui, "Build details"))
            .show(ui, |ui| {
                ui.add(egui::Label::new(theme::small(ui, "Pi handles models, tools and authentication. Pui provides the desktop workspace.")).wrap());
                ui.add(egui::Label::new(theme::small(ui, "Local build · signing and notarization status are not verified by this panel.")).wrap());
            });
        egui::CollapsingHeader::new(theme::small(ui, "Font license")).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(theme::small(
                            ui,
                            include_str!("../../assets/fonts/OFL-NotoSansHebrew.txt"),
                        ))
                        .wrap()
                        .selectable(true),
                    );
                });
        });
        ui.add_space(14.0);
        ui.vertical_centered(|ui| {
            close |= prompt_ui::button(ui, "Done", prompt_ui::Button::Secondary, true).clicked();
        });
        close
    });
    if response.should_close() || response.inner {
        *open = false;
    }
}
