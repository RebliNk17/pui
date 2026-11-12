//! Read-only file review content embedded in the workspace Changes pane.
use super::*;
mod chrome;
mod navigation;
pub(super) use chrome::FileNavigation;
pub(super) use navigation::{Document, Navigation, demo as demo_hunks, demo_wrap};

pub(super) fn inputs(ctx: &egui::Context, document: &mut Document) -> bool {
    if !document.2.owns_focus(ctx)
        || ctx.memory(|memory| memory.top_modal_layer().is_some())
        || egui::Popup::is_any_open(ctx)
    {
        return false;
    }
    document.2.inputs(ctx, &document.1);
    ctx.input_mut(|input| {
        input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
        input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
    })
}

/// One header for file identity/actions; no floating Area/Window or extra file toolbar.
#[cfg(test)]
pub(super) fn contents_with_files(
    ui: &mut egui::Ui,
    document: &mut Document,
    files: Option<&FileNavigation>,
) -> chrome::Action {
    contents_with_review(ui, document, files, None)
}

pub(super) fn contents_with_review(
    ui: &mut egui::Ui,
    document: &mut Document,
    files: Option<&FileNavigation>,
    comments: Option<&mut changes::review_comments::Comments>,
) -> chrome::Action {
    let Document(path, diff, navigation, recorded) = document;
    let ctx = ui.ctx().clone();
    let colors = theme::palette(&ctx);
    navigation.restore_label_focus(&ctx);
    let empty = navigation.untracked() || diff.trim().is_empty();
    let mut action = chrome::Action::default();
    ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
    ui.spacing_mut().interact_size.y = 24.0;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let header = ui.horizontal(|ui| {
                let response = icons::button(
                    ui,
                    Icon::ArrowLeft,
                    "Back to changes list · Escape when review is focused",
                );
                navigation.register(3, response.id);
                action.back = response.clicked();
                let name = std::path::Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(path);
                // Measure the actual counter/counts so narrow panes never acquire
                // an oversized minimum width or center a filename in dead space.
                let (added, removed) = if empty { (0, 0) } else { counts(diff) };
                let counts_width = if empty || (added == 0 && removed == 0) {
                    0.0
                } else {
                    [format!("+{added}"), format!("−{removed}")]
                        .into_iter()
                        .map(|label| {
                            ui.painter()
                                .layout_no_wrap(label, FontId::proportional(11.0), colors.text)
                                .size()
                                .x
                                + 6.0
                        })
                        .sum::<f32>()
                };
                let file_width = files.map_or(0.0, |files| {
                    let label = if files.loading {
                        "Loading…".into()
                    } else if let Some((index, count)) = files.position {
                        format!("{} / {count}", index + 1)
                    } else {
                        "—".into()
                    };
                    56.0 + ui
                        .painter()
                        .layout_no_wrap(label, FontId::proportional(11.0), colors.text)
                        .size()
                        .x
                });
                let title_width =
                    (ui.available_width() - file_width - counts_width - 42.0).max(8.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(title_width, 24.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_min_width(title_width);
                        ui.add(
                            egui::Label::new(RichText::new(name).size(13.0).strong()).truncate(),
                        )
                        .on_hover_text(path.as_str());
                    },
                );
                if let Some(files) = files {
                    action.next = chrome::files(ui, navigation, files);
                }
                if !empty && (added > 0 || removed > 0) {
                    ui.label(
                        RichText::new(format!("+{added}"))
                            .size(11.0)
                            .color(colors.green),
                    );
                    ui.label(
                        RichText::new(format!("−{removed}"))
                            .size(11.0)
                            .color(colors.red),
                    );
                }
                let copy = icons::button(ui, Icon::Copy, "Copy diff");
                navigation.register(4, copy.id);
                if copy.clicked() {
                    ctx.copy_text(diff.clone());
                }
                #[cfg(test)]
                ctx.data_mut(|data| {
                    data.insert_temp(egui::Id::new("diff-close"), response.rect);
                    data.insert_temp(egui::Id::new("diff-copy"), copy.rect);
                });
            });
            #[cfg(test)]
            ctx.data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("diff-review-file-heading"),
                    header.response.rect,
                )
            });
            let _ = header;
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::small(
                    ui,
                    if *recorded {
                        "Recorded edits · read-only"
                    } else {
                        "Working tree vs HEAD · read-only"
                    },
                ));
                if !empty {
                    ui.add_space(8.0);
                    navigation.show(ui, diff);
                }
            });
            if let Some(warning) = files.and_then(|files| files.warning.as_ref()) {
                ui.add(egui::Label::new(theme::small(ui, warning)).wrap());
            }
            if let Some(comments) = comments {
                comments.show(ui, path, diff, *recorded, navigation);
            }
        });
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.cursor().top(),
        Stroke::new(1.0, colors.border),
    );
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(colors.bg).inner_margin(8))
        .show_inside(ui, |ui| {
            let rect = ui.max_rect();
            let response = ui.interact(rect,navigation.focus_id(),Sense::focusable_noninteractive());
            if ui.input(|input|input.pointer.primary_pressed()&&input.pointer.interact_pos().is_some_and(|pos|rect.contains(pos)))
                && prompt_ui::keyboard_available(ui) && !egui::Popup::is_any_open(&ctx) { response.request_focus(); }
            egui::ScrollArea::vertical().id_salt(("diff-review-lines",path.as_str())).auto_shrink([false,false]).show(ui,|ui| {
                navigation.layout_width(ui.available_width());
                if empty {
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(16,24)).show(ui,|ui| {
                        ui.set_max_width(340.0_f32.min(ui.available_width()));
                        ui.horizontal(|ui| {
                            let (rect,_) = ui.allocate_exact_size(Vec2::splat(20.0),Sense::hover());
                            icons::draw(ui,rect,Icon::File,colors.dim);
                            ui.label(RichText::new(if navigation.untracked(){"Untracked file"}else{"No changes to display."}).size(14.0).color(colors.text));
                        });
                        ui.add_space(6.0);
                        ui.add(egui::Label::new(theme::small(ui,if navigation.untracked(){"This file isn’t tracked by Git, so there’s no diff to show."}else{"The current comparison contains no changes."})).wrap());

                    });
                } else {
                    let id=changes::numbered_diff_target(ui,path,diff,*recorded,navigation.take_target(),navigation.wrap());
                    navigation.register(5,id);
                }
            });
            response.widget_info(||egui::WidgetInfo::labeled(egui::WidgetType::Other,ui.is_enabled(),format!("Read-only file review: {path}")));
            #[cfg(test)]
            ctx.data_mut(|data|data.insert_temp(egui::Id::new("diff-review-code-bounds"),rect));
        });
    navigation.finish_focus(&ctx);
    action
}
#[cfg(test)]
pub(super) fn contents(ui: &mut egui::Ui, document: &mut Document) -> bool {
    contents_with_files(ui, document, None).back
}

// Tests render exactly the embedded content, never an alternative floating UI.
#[cfg(test)]
pub(super) fn show_document(ctx: &egui::Context, document: &mut Document, recorded: bool) -> bool {
    document.3 = recorded;
    let mut back = inputs(ctx, document);
    egui::CentralPanel::default().show(ctx, |ui| {
        #[cfg(test)]
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("diff-review-bounds"), ui.max_rect()));
        back |= contents(ui, document);
    });
    !back
}
#[cfg(test)]
pub(super) fn show(ctx: &egui::Context, path: &str, diff: &str, recorded: bool) -> bool {
    let mut document: Document = (path.to_owned(), diff.to_owned()).into();
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        document.2.focus(ctx);
    }
    show_document(ctx, &mut document, recorded)
}

fn counts(diff: &str) -> (usize, usize) {
    changes::lines::rows(diff).fold((0, 0), |(added, removed), row| {
        (
            added + usize::from(row.added),
            removed + usize::from(row.removed),
        )
    })
}
#[cfg(test)]
mod chrome_tests;
#[cfg(test)]
mod tests;
