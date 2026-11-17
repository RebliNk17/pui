//! Search only known project paths. Linked-worktree detection runs off the UI thread.
use super::*;

#[derive(Default)]
pub(in crate::app) struct Picker {
    owner: String,
    query: String,
    projects: Vec<PathBuf>,
    rx: Option<Receiver<Vec<PathBuf>>>,
    selected: usize,
    focus: bool,
}

fn popup_id() -> egui::Id {
    egui::Id::new("new-chat-project-popup")
}

fn linked_worktree(path: &std::path::Path) -> bool {
    for ancestor in path.ancestors() {
        let git = ancestor.join(".git");
        if git.is_dir() {
            return false;
        }
        if git.is_file() {
            // Submodules also have a gitfile, but only linked worktrees have
            // commondir. Do not mistake a perfectly valid submodule for one.
            return std::fs::read_to_string(git)
                .ok()
                .and_then(|text| text.trim().strip_prefix("gitdir: ").map(str::to_owned))
                .is_some_and(|dir| ancestor.join(dir).join("commondir").is_file());
        }
    }
    false
}

fn display_path(path: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(relative) = path.strip_prefix(home)
    {
        return format!("~/{}", relative.display());
    }
    path.display().to_string()
}

fn matches(path: &std::path::Path, query: &str) -> bool {
    let path = path.to_string_lossy().to_lowercase();
    query.split_whitespace().all(|word| path.contains(word))
}

pub(in crate::app) fn paint_text(
    ui: &egui::Ui,
    text: &str,
    rect: egui::Rect,
    size: f32,
    color: Color32,
) {
    let mut job =
        egui::text::LayoutJob::simple_singleline(text.into(), FontId::proportional(size), color);
    job.wrap.max_width = rect.width().max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        rect.left_center() - egui::vec2(0.0, galley.size().y / 2.0),
        galley,
        color,
    );
}

impl Picker {
    pub(in crate::app) fn show(
        &mut self,
        ui: &mut egui::Ui,
        owner: &str,
        current: &std::path::Path,
        width: f32,
        candidates: Vec<PathBuf>,
        can_browse: bool,
    ) -> (Option<PathBuf>, bool) {
        let colors = theme::palette(ui.ctx());
        if self.owner != owner {
            *self = Self {
                owner: owner.into(),
                ..Default::default()
            };
            egui::Popup::close_id(ui.ctx(), popup_id());
        }
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 34.0), Sense::click());
        ui.painter().rect(
            rect,
            7,
            colors.surface,
            Stroke::new(
                1.0,
                if response.has_focus() {
                    colors.accent
                } else {
                    colors.border
                },
            ),
            egui::StrokeKind::Inside,
        );
        icons::draw(
            ui,
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(17.0, 0.0),
                Vec2::splat(14.0),
            ),
            Icon::Folder,
            colors.muted,
        );
        paint_text(
            ui,
            &sessions::project_name(current),
            egui::Rect::from_min_max(
                rect.min + egui::vec2(32.0, 0.0),
                rect.max - egui::vec2(25.0, 0.0),
            ),
            12.0,
            colors.text,
        );
        icons::chevron(
            ui,
            egui::Rect::from_center_size(
                rect.right_center() - egui::vec2(13.0, 0.0),
                Vec2::splat(12.0),
            ),
            true,
            colors.dim,
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::ComboBox,
                ui.is_enabled(),
                format!("Project: {}", sessions::project_name(current)),
            )
        });
        let was_open = egui::Popup::is_id_open(ui.ctx(), popup_id());
        if response.clicked() && !was_open {
            self.query.clear();
            self.projects.clear();
            self.selected = 0;
            self.focus = true;
            // No directory traversal or Git subprocess per render frame. Only
            // inspect known paths when opening; no background idle polling.
            let (tx, rx) = mpsc::channel();
            self.rx = Some(rx);
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let projects = candidates
                    .into_iter()
                    .filter(|path| !linked_worktree(path))
                    .collect();
                let _ = tx.send(projects);
                ctx.request_repaint();
            });
        }
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(projects) => {
                    self.projects = projects;
                    self.rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.rx = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut selection = None;
        let mut browse = false;
        let popup_width = 390.0_f32.min((ui.ctx().screen_rect().width() - 24.0).max(120.0));
        let popup = egui::Popup::from_toggle_button_response(&response)
            .id(popup_id())
            .width(popup_width)
            .gap(6.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .frame(prompt_ui::popup(ui.ctx()).inner_margin(10));
        popup.show(|ui| {
            theme::menu_controls(ui);
            ui.set_width((popup_width - 20.0).max(0.0));
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Switch project")
                        .size(12.0)
                        .color(colors.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(theme::small(
                        ui,
                        format!("{} projects", self.projects.len()),
                    ));
                });
            });
            let previous_focus = ui.memory(|m| m.focused());
            let search = egui::Frame::new()
                .fill(colors.bg)
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(8, 5))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        icons::inline(ui, Icon::Search, colors.dim);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.query)
                                .id_salt("project-search")
                                .font(FontId::proportional(11.5))
                                .frame(false)
                                .hint_text("Find a project…")
                                .char_limit(256)
                                .desired_width(ui.available_width()),
                        )
                    })
                    .inner
                })
                .inner;
            if self.focus && !ui.is_sizing_pass() {
                search.request_focus();
                self.focus = false;
            }
            if search.changed() {
                self.selected = 0;
            }
            let query = self.query.to_lowercase();
            let filtered: Vec<_> = self
                .projects
                .iter()
                .filter(|path| matches(path, &query))
                .collect();
            self.selected = self.selected.min(filtered.len().saturating_sub(1));
            let mut moved = false;
            let mut enter = false;
            if was_open
                && (search.has_focus() || previous_focus == Some(search.id))
                && !ui.is_sizing_pass()
                && prompt_ui::keyboard_available(ui)
            {
                ui.input_mut(|i| {
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                        self.selected = (self.selected + 1).min(filtered.len().saturating_sub(1));
                        moved = true;
                    }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                        self.selected = self.selected.saturating_sub(1);
                        moved = true;
                    }
                    enter = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                });
            }
            if moved {
                search.request_focus();
            }
            let max_height = (ui.ctx().screen_rect().height() - 220.0).clamp(48.0, 250.0);
            egui::ScrollArea::vertical()
                .id_salt("project-results")
                .max_height(max_height)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if self.rx.is_some() {
                        ui.label(theme::small(ui, "Loading projects…"));
                    } else if filtered.is_empty() {
                        ui.label(theme::small(ui, "No matching projects."));
                    }
                    for (index, path) in filtered.iter().enumerate() {
                        ui.push_id(path, |ui| {
                            let name = sessions::project_name(path);
                            let (rect, row) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 48.0),
                                Sense::click(),
                            );
                            let selected = path.as_path() == current;
                            if selected
                                || row.hovered()
                                || row.has_focus()
                                || (search.has_focus() && index == self.selected)
                            {
                                ui.painter().rect_filled(
                                    rect,
                                    6,
                                    if selected {
                                        colors.accent_bg
                                    } else {
                                        colors.hover
                                    },
                                );
                            }
                            if row.has_focus() {
                                ui.painter().rect_stroke(
                                    rect.shrink(0.5),
                                    6,
                                    Stroke::new(1.0, colors.accent),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            let tile = egui::Rect::from_center_size(
                                rect.left_center() + egui::vec2(21.0, 0.0),
                                Vec2::splat(28.0),
                            );
                            ui.painter().rect(
                                tile,
                                6,
                                colors.bg,
                                Stroke::new(1.0, colors.border),
                                egui::StrokeKind::Inside,
                            );
                            icons::draw(ui, tile.shrink(7.0), Icon::Folder, colors.muted);
                            let left = rect.left() + 43.0;
                            let right = rect.right() - 24.0;
                            paint_text(
                                ui,
                                &name,
                                egui::Rect::from_min_max(
                                    egui::pos2(left, rect.top() + 5.0),
                                    egui::pos2(right, rect.top() + 25.0),
                                ),
                                12.0,
                                colors.text,
                            );
                            paint_text(
                                ui,
                                &display_path(path),
                                egui::Rect::from_min_max(
                                    egui::pos2(left, rect.top() + 25.0),
                                    egui::pos2(right, rect.bottom() - 4.0),
                                ),
                                10.0,
                                colors.dim,
                            );
                            if selected {
                                let c = rect.right_center() - egui::vec2(12.0, 0.0);
                                ui.painter().add(egui::Shape::line(
                                    vec![
                                        c + egui::vec2(-4.0, 0.0),
                                        c + egui::vec2(-1.0, 3.0),
                                        c + egui::vec2(5.0, -4.0),
                                    ],
                                    Stroke::new(1.4, colors.accent),
                                ));
                            }
                            row.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::SelectableLabel,
                                    ui.is_enabled(),
                                    selected,
                                    format!("{name}, {}", path.display()),
                                )
                            });
                            if moved && index == self.selected {
                                row.scroll_to_me(Some(Align::Center));
                            }
                            if row
                                .clone()
                                .on_hover_text(path.display().to_string())
                                .clicked()
                                || (enter && index == self.selected)
                            {
                                selection = Some((*path).clone());
                                ui.close();
                            }
                        });
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if icons::labeled_button(ui, Icon::Folder, "Open folder…", can_browse).clicked() {
                    browse = true;
                    ui.close();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new("↑↓ navigate · Enter select")
                            .size(9.0)
                            .color(colors.dim),
                    );
                });
            });
        });
        if was_open && !egui::Popup::is_id_open(ui.ctx(), popup_id()) {
            response.request_focus();
        }
        (selection, browse)
    }
}

#[cfg(test)]
mod tests;
