//! Search only known project paths. Linked-worktree detection runs off the UI thread.
use super::*;

#[derive(Default)]
pub(in crate::app) struct Picker {
    owner: String,
    query: String,
    projects: Vec<PathBuf>,
    rx: Option<Receiver<Vec<PathBuf>>>,
    cloud_repos: Vec<crate::remote::cloud::CloudRepoItem>,
    cloud_rx: Option<Receiver<Result<Vec<crate::remote::cloud::CloudRepoItem>, String>>>,
    cloud_error: Option<String>,
    selected: usize,
    focus: bool,
}

fn cloud_popup_id() -> egui::Id {
    egui::Id::new("new-chat-cloud-repo-popup")
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

    /// Unified searchable `org/repo` dropdown for Cloud sessions — matches the local project picker
    /// pixel-for-pixel, showing already-cloned repos first and all GitHub `org/repo` repositories below.
    pub(in crate::app) fn show_cloud_repos(
        &mut self,
        ui: &mut egui::Ui,
        owner: &str,
        cloud_cfg: &crate::remote::CloudHostConfig,
        current_repo: Option<&crate::remote::cloud::CloudRepoItem>,
        fallback_label: &str,
        width: f32,
    ) -> Option<crate::remote::cloud::CloudRepoItem> {
        let colors = theme::palette(ui.ctx());
        if self.owner != owner {
            self.owner = owner.into();
            self.query.clear();
            self.selected = 0;
            egui::Popup::close_id(ui.ctx(), cloud_popup_id());
        }

        let trigger_label = current_repo
            .map(|r| r.full_name.as_str())
            .unwrap_or(fallback_label);

        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 34.0), Sense::click());
        ui.painter().rect(
            rect,
            8,
            if response.hovered() {
                colors.hover
            } else {
                colors.sidebar
            },
            Stroke::new(
                1.0_f32,
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
            Icon::Branch,
            colors.accent,
        );
        paint_text(
            ui,
            trigger_label,
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

        let trigger_fetch = |picker: &mut Self,
                             cfg: crate::remote::CloudHostConfig,
                             ctx: egui::Context,
                             refresh: bool| {
            let (tx, rx) = mpsc::channel();
            picker.cloud_rx = Some(rx);
            picker.cloud_error = None;
            std::thread::spawn(move || {
                let res = crate::remote::cloud::fetch_cloud_repos(&cfg, refresh);
                let _ = tx.send(res);
                ctx.request_repaint();
            });
        };

        let was_open = egui::Popup::is_id_open(ui.ctx(), cloud_popup_id());
        if response.clicked() && !was_open {
            self.query.clear();
            self.selected = 0;
            self.focus = true;
            if self.cloud_rx.is_none() {
                trigger_fetch(self, cloud_cfg.clone(), ui.ctx().clone(), false);
            }
        }
        if let Some(rx) = &self.cloud_rx {
            match rx.try_recv() {
                Ok(Ok(repos)) => {
                    self.cloud_repos = repos;
                    self.cloud_error = None;
                    self.cloud_rx = None;
                }
                Ok(Err(e)) => {
                    self.cloud_error = Some(e);
                    self.cloud_rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.cloud_rx = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        let mut selection: Option<crate::remote::cloud::CloudRepoItem> = None;
        let popup_width = 390.0_f32.min((ui.ctx().screen_rect().width() - 24.0).max(120.0));
        let popup = egui::Popup::from_toggle_button_response(&response)
            .id(cloud_popup_id())
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
                    RichText::new("Select repository (org/repo)")
                        .size(12.0)
                        .color(colors.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            self.cloud_rx.is_none(),
                            egui::Button::new(RichText::new("↻").size(12.0).color(colors.muted))
                                .frame(false),
                        )
                        .on_hover_text("Refresh GitHub repositories")
                        .clicked()
                    {
                        trigger_fetch(self, cloud_cfg.clone(), ui.ctx().clone(), true);
                    }
                    let count_text = if self.cloud_rx.is_some() {
                        "Loading…".to_owned()
                    } else {
                        format!("{} repos", self.cloud_repos.len())
                    };
                    ui.label(theme::small(ui, count_text));
                });
            });

            let search = egui::Frame::new()
                .fill(colors.bg)
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(8, 5))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        icons::inline(ui, Icon::Search, colors.dim);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.query)
                                .id_salt("cloud-repo-search")
                                .hint_text("Search org/repo…")
                                .frame(false)
                                .desired_width(ui.available_width()),
                        )
                    })
                    .inner
                })
                .inner;
            if self.focus {
                search.request_focus();
                self.focus = false;
            }
            if search.changed() {
                self.selected = 0;
            }

            let q = self.query.trim().to_lowercase();
            let words: Vec<&str> = q.split_whitespace().collect();
            let filtered: Vec<crate::remote::cloud::CloudRepoItem> = self
                .cloud_repos
                .iter()
                .filter(|r| {
                    if words.is_empty() {
                        return true;
                    }
                    let hay = format!("{} {}", r.full_name, r.name).to_lowercase();
                    words.iter().all(|w| hay.contains(w))
                })
                .cloned()
                .collect();

            let mut enter = false;
            let mut moved = false;
            if !filtered.is_empty() {
                self.selected = self.selected.min(filtered.len() - 1);
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
                    self.selected = (self.selected + 1) % filtered.len();
                    moved = true;
                }
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
                    self.selected = (self.selected + filtered.len() - 1) % filtered.len();
                    moved = true;
                }
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                    enter = true;
                }
            }

            ui.add_space(6.0);
            if let Some(ref err) = self.cloud_error {
                ui.label(RichText::new(err).size(11.0).color(colors.red));
                ui.add_space(4.0);
            }

            egui::ScrollArea::vertical()
                .max_height(270.0)
                .show(ui, |ui| {
                    let row_w = ui.available_width();
                    if filtered.is_empty() {
                        let empty_msg = if self.cloud_rx.is_some() {
                            "Loading GitHub repositories from cloud host…"
                        } else {
                            "No matching repositories."
                        };
                        ui.label(theme::small(ui, empty_msg));
                    } else {
                        for (index, repo) in filtered.iter().enumerate() {
                            let is_current = current_repo
                                .is_some_and(|c| c.full_name.eq_ignore_ascii_case(&repo.full_name));
                            let (rect, row) =
                                ui.allocate_exact_size(Vec2::new(row_w, 42.0), Sense::click());
                            if row.hovered() || index == self.selected {
                                ui.painter().rect_filled(rect, 6, colors.hover);
                            }
                            let tile = egui::Rect::from_center_size(
                                rect.left_center() + egui::vec2(21.0, 0.0),
                                Vec2::splat(28.0),
                            );
                            ui.painter().rect(
                                tile,
                                6,
                                colors.bg,
                                Stroke::new(1.0_f32, colors.border),
                                egui::StrokeKind::Inside,
                            );
                            icons::draw(
                                ui,
                                tile.shrink(7.0),
                                if repo.cloned {
                                    Icon::Folder
                                } else {
                                    Icon::Branch
                                },
                                if repo.cloned {
                                    colors.green
                                } else {
                                    colors.muted
                                },
                            );
                            let left = rect.left() + 43.0;
                            let right = rect.right() - 24.0;
                            paint_text(
                                ui,
                                &repo.full_name,
                                egui::Rect::from_min_max(
                                    egui::pos2(left, rect.top() + 5.0),
                                    egui::pos2(right, rect.top() + 24.0),
                                ),
                                12.0,
                                colors.text,
                            );
                            let sub = if repo.cloned {
                                "Cloned on cloud · ready"
                            } else if repo.private {
                                "Private · auto-clones on demand"
                            } else {
                                "Auto-clones on demand"
                            };
                            paint_text(
                                ui,
                                sub,
                                egui::Rect::from_min_max(
                                    egui::pos2(left, rect.top() + 24.0),
                                    egui::pos2(right, rect.bottom() - 4.0),
                                ),
                                10.0,
                                colors.dim,
                            );
                            if is_current {
                                let c = rect.right_center() - egui::vec2(12.0, 0.0);
                                ui.painter().add(egui::Shape::line(
                                    vec![
                                        c + egui::vec2(-4.0, 0.0),
                                        c + egui::vec2(-1.0, 3.0),
                                        c + egui::vec2(5.0, -4.0),
                                    ],
                                    Stroke::new(1.4_f32, colors.accent),
                                ));
                            }
                            if moved && index == self.selected {
                                row.scroll_to_me(Some(Align::Center));
                            }
                            if row
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                                || (enter && index == self.selected)
                            {
                                selection = Some(repo.clone());
                                ui.close();
                            }
                        }
                    }

                    // Allow typing any custom `org/repo` directly
                    let raw_q = self.query.trim();
                    if raw_q.contains('/')
                        && !raw_q.contains(' ')
                        && !filtered
                            .iter()
                            .any(|r| r.full_name.eq_ignore_ascii_case(raw_q))
                    {
                        ui.add_space(4.0);
                        let (rect, row) =
                            ui.allocate_exact_size(Vec2::new(row_w, 34.0), Sense::click());
                        ui.painter()
                            .rect_filled(rect, 6, colors.accent.gamma_multiply(0.16));
                        paint_text(
                            ui,
                            &format!("Clone & use {raw_q}"),
                            rect.shrink2(Vec2::new(10.0, 0.0)),
                            12.0,
                            colors.text,
                        );
                        if row
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            let repo_name = raw_q
                                .trim_end_matches(".git")
                                .rsplit(['/', ':'])
                                .next()
                                .unwrap_or(raw_q)
                                .to_owned();
                            let remote =
                                if raw_q.starts_with("git@") || raw_q.starts_with("https://") {
                                    raw_q.to_owned()
                                } else {
                                    format!("git@github.com:{raw_q}.git")
                                };
                            selection = Some(crate::remote::cloud::CloudRepoItem {
                                full_name: raw_q.to_owned(),
                                name: repo_name,
                                git_remote: remote,
                                private: false,
                                cloned: false,
                                path: None,
                            });
                            ui.close();
                        }
                    }
                });
        });

        if was_open && !egui::Popup::is_id_open(ui.ctx(), cloud_popup_id()) {
            response.request_focus();
        }
        selection
    }
}

#[cfg(test)]
mod tests;
