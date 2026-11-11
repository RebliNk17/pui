//! Compact, read-only diff utility. Remains movable and resizable, not a modal.
use super::*;

pub(super) fn show(ctx: &egui::Context, path: &str, diff: &str, recorded: bool) -> bool {
    let colors = theme::palette(ctx);
    let bounds = ctx.screen_rect().shrink(12.0);
    let maximum = (bounds.size() - Vec2::splat(26.0)).max(Vec2::splat(120.0));
    let mut open = true;
    egui::Window::new("File changes")
        .id(egui::Id::new("diff-viewer"))
        .title_bar(false)
        .collapsible(false)
        .constrain_to(bounds)
        .default_size(Vec2::new(920.0, 620.0).min(maximum))
        .min_size(Vec2::new(320.0, 240.0).min(maximum))
        .max_size(maximum)
        .frame(prompt_ui::surface(ctx).inner_margin(12).corner_radius(10))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
            ui.spacing_mut().interact_size.y = 28.0;
            let (added, removed) = counts(diff);
            let added = format!("+{added}");
            let removed = format!("−{removed}");
            let counts_width: f32 = [&added, &removed]
                .into_iter()
                .map(|label| {
                    ui.painter()
                        .layout_no_wrap(label.clone(), FontId::proportional(11.0), colors.text)
                        .size()
                        .x
                })
                .sum();
            ui.horizontal(|ui| {
                let actions_width = 2.0 * 28.0 + 4.0 * ui.spacing().item_spacing.x + counts_width;
                ui.allocate_ui_with_layout(
                    Vec2::new((ui.available_width() - actions_width).max(40.0), 44.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        let file = std::path::Path::new(path);
                        let name = file
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or(path);
                        ui.add(
                            egui::Label::new(RichText::new(name).size(14.0).strong()).truncate(),
                        )
                        .on_hover_text(path);
                        let parent = file.parent().filter(|p| !p.as_os_str().is_empty());
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    parent
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_else(|| "File changes".into()),
                                )
                                .size(11.0)
                                .color(colors.muted),
                            )
                            .truncate(),
                        )
                        .on_hover_text(path);
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let close = icons::button(ui, Icon::Close, "Close diff · Esc");
                    close.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close diff")
                    });
                    if close.clicked() {
                        open = false;
                    }
                    let copy = icons::button(ui, Icon::Copy, "Copy diff");
                    copy.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Copy diff")
                    });
                    if copy.clicked() {
                        ctx.copy_text(diff.into());
                    }
                    #[cfg(test)]
                    ctx.data_mut(|data| {
                        data.insert_temp(egui::Id::new("diff-close"), close.rect);
                        data.insert_temp(egui::Id::new("diff-copy"), copy.rect);
                    });
                    ui.label(RichText::new(&removed).size(11.0).color(colors.red));
                    ui.label(RichText::new(&added).size(11.0).color(colors.green));
                });
            });
            ui.separator();
            egui::TopBottomPanel::bottom("diff-viewer-footer")
                .exact_height(24.0)
                .frame(egui::Frame::new())
                .show_inside(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.label(
                            RichText::new(if recorded {
                                "Recorded edits · read-only"
                            } else {
                                "Working tree vs HEAD · read-only"
                            })
                            .size(10.0)
                            .color(colors.muted),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new("Esc to close").size(10.0).color(colors.dim));
                        });
                    });
                });
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(colors.code_bg).inner_margin(4))
                .show_inside(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(("diff-viewer-lines", path))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if diff.trim().is_empty() {
                                ui.label(theme::small(ui, "No changes to display."));
                            } else {
                                changes::numbered_diff(ui, diff);
                            }
                        });
                });
        });
    if ctx.memory(|memory| memory.top_modal_layer().is_none())
        && !egui::Popup::is_any_open(ctx)
        && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        open = false;
    }
    open
}

fn counts(diff: &str) -> (usize, usize) {
    diff.lines().fold((0, 0), |(added, removed), line| {
        (
            added + usize::from(line.starts_with('+') && !line.starts_with("+++")),
            removed + usize::from(line.starts_with('-') && !line.starts_with("---")),
        )
    })
}

#[cfg(test)]
mod tests;
