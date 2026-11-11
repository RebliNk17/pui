//! Docked changes inspector. Git and recorded edits are deliberately separate sources.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Source {
    Git,
    #[default]
    Recorded,
}
#[derive(Default)]
struct Diff {
    content: Option<String>,
    rx: Option<Receiver<String>>,
}
#[derive(Default)]
pub(super) struct Changes {
    expanded: bool,
    source: Source,
    query: String,
    identity: (PathBuf, String),
    open_rows: BTreeSet<(bool, String)>,
    diffs: BTreeMap<String, Diff>,
    refresh: bool,
}

impl Changes {
    pub(super) fn take_refresh(&mut self) -> bool {
        std::mem::take(&mut self.refresh)
    }

    pub(super) fn invalidate(&mut self) {
        self.diffs.clear();
    }

    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        cwd: &std::path::Path,
        session: &str,
        workspace: &Workspace,
        outputs: &[RecordedFile],
        loading: bool,
    ) -> bool {
        let session_changed = self.identity.1 != session;
        if self.identity != (cwd.to_path_buf(), session.to_owned()) {
            self.identity = (cwd.to_path_buf(), session.to_owned());
            self.open_rows.clear();
            self.diffs.clear();
            self.query.clear();
            if session_changed {
                self.source = Source::Recorded;
            }
        }
        for diff in self.diffs.values_mut() {
            if let Some(content) = diff.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
                diff.content = Some(content);
                diff.rx = None;
            }
        }
        let colors = theme::palette(ctx);
        let expanded = self.expanded;
        if expanded {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(colors.bg))
                .show(ctx, |ui| self.contents(ui, workspace, outputs, loading));
        } else {
            let available = ctx.available_rect().width();
            egui::SidePanel::right("workspace-changes-dock")
                .default_width((available * 0.58).clamp(420.0, 760.0))
                .width_range(360.0..=(available - 220.0).max(360.0))
                .resizable(true)
                .frame(egui::Frame::new().fill(colors.bg))
                .show(ctx, |ui| self.contents(ui, workspace, outputs, loading));
        }
        expanded
    }

    fn contents(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        outputs: &[RecordedFile],
        loading: bool,
    ) {
        let colors = theme::palette(ui.ctx());
        egui::TopBottomPanel::top("changes-heading")
            .exact_height(44.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 0)))
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("Changes").size(13.0).color(colors.muted));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icons::button(
                            ui,
                            if self.expanded {
                                Icon::Collapse
                            } else {
                                Icon::Expand
                            },
                            if self.expanded {
                                "Restore changes panel"
                            } else {
                                "Expand changes panel"
                            },
                        )
                        .clicked()
                        {
                            self.expanded = !self.expanded;
                        }
                        if ui
                            .add(egui::Button::new(theme::small(ui, "Refresh")).frame(false))
                            .clicked()
                        {
                            self.refresh = true;
                            self.invalidate();
                        }
                        if loading {
                            ui.label(theme::small(ui, "Loading…"));
                        }
                    });
                });
            });
        egui::TopBottomPanel::top("changes-controls")
            .frame(egui::Frame::new().inner_margin(16))
            .show_inside(ui, |ui| {
                ui.add(
                    egui::Label::new(theme::small(
                        ui,
                        if workspace.is_git {
                            format!("{} · {}", workspace.root.display(), workspace.branch)
                        } else if loading {
                            "Loading the session's repository…".into()
                        } else {
                            "Git changes require the session's repository root".into()
                        },
                    ))
                    .wrap(),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let tabs = ["Current Git", "Recorded edits"].map(|label| {
                        ui.painter()
                            .layout_no_wrap(
                                label.into(),
                                ui.style().text_styles[&egui::TextStyle::Button].clone(),
                                colors.text,
                            )
                            .size()
                            .x
                            + 2.0 * ui.spacing().button_padding.x
                    });
                    // Include the search frame and widget margins. Under-counting
                    // them makes content a few pixels wider than its parent;
                    // egui remembers that larger panel width on every frame.
                    let width = (ui.available_width()
                        - tabs.iter().sum::<f32>()
                        - ui.spacing().item_spacing.x * 2.0
                        - 18.0)
                        .max(40.0);
                    egui::Frame::new()
                        .fill(colors.surface)
                        .stroke(Stroke::new(1.0, colors.border))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(8, 5))
                        .show(ui, |ui| {
                            ui.add_sized(
                                [width, 18.0],
                                egui::TextEdit::singleline(&mut self.query)
                                    .frame(false)
                                    .desired_width(width - 8.0)
                                    .hint_text("Search changes…")
                                    .id_salt("changes-search"),
                            );
                        });
                    ui.selectable_value(&mut self.source, Source::Git, "Current Git");
                    ui.selectable_value(&mut self.source, Source::Recorded, "Recorded edits");
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("changes-rows", self.source == Source::Git))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        egui::Frame::new().inner_margin(16).show(ui, |ui| {
                            ui.label(theme::small(
                                ui,
                                if self.source == Source::Recorded {
                                    "Recorded edits, in order—not a net Git diff."
                                } else {
                                    "Current working tree vs HEAD—not recorded session edits."
                                },
                            ));
                        });
                        let query = self.query.trim().to_lowercase();
                        let mut count = 0;
                        match self.source {
                            Source::Recorded => {
                                // Outputs card is newest-file-first; this panel follows recorded order.
                                for file in outputs
                                    .iter()
                                    .rev()
                                    .filter(|file| file.path.to_lowercase().contains(&query))
                                {
                                    count += 1;
                                    if self.header(
                                        ui,
                                        &file.path,
                                        &file.added.to_string(),
                                        &file.removed.to_string(),
                                        false,
                                    ) {
                                        numbered_diff(ui, &file.diff);
                                    }
                                }
                            }
                            Source::Git => {
                                for file in workspace
                                    .files
                                    .iter()
                                    .filter(|file| file.path.to_lowercase().contains(&query))
                                {
                                    count += 1;
                                    if self.header(ui, &file.path, &file.added, &file.removed, true)
                                    {
                                        if !self.diffs.contains_key(&file.path) {
                                            self.load_diff(ui.ctx(), &workspace.root, file.clone());
                                        }
                                        match self
                                            .diffs
                                            .get(&file.path)
                                            .and_then(|diff| diff.content.as_deref())
                                        {
                                            None => {
                                                ui.label(theme::small(ui, "Loading diff…"));
                                            }
                                            Some(content) => numbered_diff(ui, content),
                                        }
                                    }
                                }
                            }
                        }
                        if count == 0 {
                            egui::Frame::new().inner_margin(16).show(ui, |ui| {
                                ui.label(theme::small(
                                    ui,
                                    if !query.is_empty() {
                                        "No matching changes."
                                    } else if self.source == Source::Recorded {
                                        "No successful recorded edits in this chat."
                                    } else if loading {
                                        "Loading Git changes…"
                                    } else if workspace.is_git {
                                        "Working tree is clean."
                                    } else {
                                        "Not a Git repository. Recorded edits are still available."
                                    },
                                ));
                            });
                        }
                    });
            });
    }

    fn header(
        &mut self,
        ui: &mut egui::Ui,
        path: &str,
        added: &str,
        removed: &str,
        git: bool,
    ) -> bool {
        let colors = theme::palette(ui.ctx());
        let key = (git, path.to_owned());
        let mut open = self.open_rows.contains(&key);
        let (_, rect) = ui.allocate_space(Vec2::new(ui.available_width(), 42.0));
        let response = ui
            .interact(
                rect,
                ui.make_persistent_id(("change-row", &key)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked() {
            open = !open;
            if open {
                self.open_rows.insert(key);
            } else {
                self.open_rows.remove(&key);
            }
        }
        ui.painter().rect_filled(
            rect,
            0,
            if response.hovered() {
                colors.hover
            } else {
                colors.sidebar
            },
        );
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            Stroke::new(1.0, colors.border),
        );
        icons::chevron(
            ui,
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(18.0, 0.0),
                Vec2::splat(12.0),
            ),
            open,
            colors.dim,
        );
        icons::draw(
            ui,
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(40.0, 0.0),
                Vec2::splat(16.0),
            ),
            Icon::File,
            colors.text,
        );
        let mut job = egui::text::LayoutJob::simple_singleline(
            path.into(),
            FontId::proportional(12.0),
            colors.text,
        );
        job.wrap.max_width = (rect.width() - 160.0).max(30.0);
        job.wrap.max_rows = 1;
        let galley = ui.painter().layout_job(job);
        if galley.elided {
            response.on_hover_text(path);
        }
        ui.painter().galley(
            rect.left_center() + egui::vec2(60.0, -galley.size().y / 2.0),
            galley,
            colors.text,
        );
        ui.painter().text(
            rect.right_center() - egui::vec2(58.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!("+{}", if added.is_empty() { "0" } else { added }),
            FontId::proportional(12.0),
            colors.green,
        );
        ui.painter().text(
            rect.right_center() - egui::vec2(14.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!("−{}", if removed.is_empty() { "0" } else { removed }),
            FontId::proportional(12.0),
            colors.red,
        );
        open
    }

    fn load_diff(
        &mut self,
        ctx: &egui::Context,
        root: &std::path::Path,
        file: workspace::FileChange,
    ) {
        let root = root.to_path_buf();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.diffs.insert(
            file.path.clone(),
            Diff {
                content: None,
                rx: Some(rx),
            },
        );
        std::thread::spawn(move || {
            let _ = tx.send(workspace::diff(&root, &file));
            ctx.request_repaint();
        });
    }
}

/// Selectable source with quiet line numbers and full-width addition/deletion backgrounds.
fn numbered_diff(ui: &mut egui::Ui, source: &str) {
    let colors = theme::palette(ui.ctx());
    ui.push_id(ui.next_auto_id(), |ui| {
        egui::ScrollArea::horizontal()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let mut job = egui::text::LayoutJob::default();
                let mut backgrounds = Vec::new();
                let mut old = 0;
                let mut new = 0;
                for line in source.lines() {
                    if line.starts_with("@@") {
                        if let Some((before, after)) =
                            line.strip_prefix("@@ -").and_then(|s| s.split_once(" +"))
                        {
                            old = before
                                .split(',')
                                .next()
                                .and_then(|s| s.parse::<usize>().ok())
                                .unwrap_or(1)
                                .saturating_sub(1);
                            new = after
                                .split([',', ' '])
                                .next()
                                .and_then(|s| s.parse::<usize>().ok())
                                .unwrap_or(1)
                                .saturating_sub(1);
                        }
                        backgrounds.push(None);
                        job.append(
                            "        …\n",
                            0.0,
                            egui::TextFormat {
                                font_id: FontId::monospace(12.0),
                                color: colors.dim,
                                line_height: Some(21.0),
                                ..Default::default()
                            },
                        );
                        continue;
                    }
                    let added = line.starts_with('+') && !line.starts_with("+++");
                    let removed = line.starts_with('-') && !line.starts_with("---");
                    let context = line.starts_with(' ');
                    if added || context {
                        new += 1;
                    }
                    if removed || context {
                        old += 1;
                    }
                    let number = if added || context {
                        new.to_string()
                    } else if removed {
                        old.to_string()
                    } else {
                        String::new()
                    };
                    backgrounds.push(if added {
                        Some(colors.diff_add_bg)
                    } else if removed {
                        Some(colors.diff_remove_bg)
                    } else {
                        None
                    });
                    job.append(
                        &format!("{number:>5}   "),
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::monospace(12.0),
                            color: colors.dim,
                            line_height: Some(21.0),
                            ..Default::default()
                        },
                    );
                    job.append(
                        &format!("{line}\n"),
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::monospace(12.0),
                            color: colors.text,
                            line_height: Some(21.0),
                            ..Default::default()
                        },
                    );
                }
                let background = ui.painter().add(egui::Shape::Noop);
                let response = ui.add(egui::Label::new(job).selectable(true).extend());
                let width = response.rect.width().max(ui.available_width());
                ui.painter().set(
                    background,
                    egui::Shape::Vec(
                        backgrounds
                            .into_iter()
                            .enumerate()
                            .filter_map(|(i, color)| {
                                color.map(|color| {
                                    egui::Shape::rect_filled(
                                        egui::Rect::from_min_size(
                                            response.rect.min + egui::vec2(0.0, i as f32 * 21.0),
                                            Vec2::new(width, 21.0),
                                        ),
                                        0,
                                        color,
                                    )
                                })
                            })
                            .collect(),
                    ),
                );
            });
    });
}

#[cfg(test)]
mod tests;
