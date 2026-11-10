use super::*;

impl Resources {
    pub(in crate::app::settings) fn show(&mut self, ui: &mut egui::Ui, cwd: &Path, demo: bool) {
        self.sync_workspace(cwd, demo);
        if !demo && self.catalog.is_none() && self.job.is_none() && self.error.is_none() {
            self.start(ui.ctx(), JobKind::List, json!({"action":"list"}));
        }
        let colors = theme::palette(ui.ctx());
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        ui.add_space(48.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Resources")
                        .size(25.0)
                        .strong()
                        .color(colors.text),
                );
                ui.label(theme::small(
                    ui,
                    "Understand and configure what Pi can use.",
                ));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(
                        self.job.is_none(),
                        egui::Button::new("+ Add…").fill(colors.accent_bg),
                    )
                    .clicked()
                {
                    self.form = Form {
                        open: true,
                        scope: if self.catalog.as_ref().is_some_and(|c| c.trusted) {
                            "project"
                        } else {
                            "user"
                        }
                        .into(),
                        ..Default::default()
                    };
                }
                if ui
                    .add_enabled(self.job.is_none(), egui::Button::new("Refresh"))
                    .on_hover_text(
                        "Rescan configuration and installed files. Does not connect to servers.",
                    )
                    .clicked()
                {
                    self.catalog = None;
                    self.error = None;
                    self.review = None;
                    self.previews.clear();
                }
            });
        });
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in Mode::ALL {
                if theme::choice(ui, self.mode == mode, label).clicked() {
                    self.mode = mode;
                    self.source_tab = false;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(theme::small(
                    ui,
                    if demo {
                        "Demo fixtures · no real changes"
                    } else if self.catalog.as_ref().is_some_and(|c| c.trusted) {
                        "Trusted project"
                    } else {
                        "Personal resources · project not trusted"
                    },
                ));
            });
        });
        ui.label(theme::small(ui, format!("Workspace · {}", cwd.display())));
        if let Some(error) = &self.error {
            ui.add(egui::Label::new(RichText::new(error).size(12.0).color(colors.red)).wrap());
        }
        if let Some(notice) = &self.notice {
            ui.add(egui::Label::new(RichText::new(notice).size(11.0).color(colors.accent)).wrap());
        }
        if let Some(catalog) = &self.catalog {
            for warning in &catalog.warnings {
                ui.add(
                    egui::Label::new(RichText::new(warning).size(11.0).color(colors.amber)).wrap(),
                );
            }
        }
        if let Some(job) = &self.job {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme::small(
                    ui,
                    match job.kind {
                        JobKind::List => "Discovering configuration…",
                        JobKind::Inspect => "Reading selected source…",
                        JobKind::Review => "Validating and preparing review…",
                        JobKind::Apply => "Applying reviewed operations…",
                    },
                ));
            });
        }
        ui.add_space(8.0);
        let height = (ui.available_height() - 69.0).max(340.0);
        egui::Frame::new()
            .fill(colors.settings_card)
            .stroke(Stroke::new(1.0, colors.border))
            .corner_radius(10)
            .show(ui, |ui| {
                ui.set_min_height(height);
                ui.set_max_height(height);
                let width = ui.available_width();
                let narrow = width < 760.0;
                if narrow {
                    self.toolbar(ui);
                    if self.mode == Mode::Packages {
                        self.package_heading(ui, demo);
                    }
                    let items = self.filtered();
                    self.ensure_selection(&items);
                    egui::ScrollArea::vertical()
                        .id_salt("resource-narrow")
                        .max_height(height - 56.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            self.list(ui, &items, demo);
                            if let Some(item) = self.selected_item() {
                                ui.separator();
                                self.inspector(ui, &item, demo);
                            }
                        });
                } else {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if self.mode != Mode::Workspace {
                            let rail_width = if self.mode == Mode::Packages {
                                204.0
                            } else {
                                176.0
                            };
                            ui.allocate_ui_with_layout(
                                Vec2::new(rail_width, height),
                                Layout::top_down(Align::LEFT),
                                |ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    self.rail(ui, height);
                                },
                            );
                            ui.separator();
                        }
                        let inspector_width = (width * 0.29).clamp(265.0, 345.0);
                        let center_width =
                            (ui.available_width() - inspector_width - 1.0).max(280.0);
                        ui.allocate_ui_with_layout(
                            Vec2::new(center_width, height),
                            Layout::top_down(Align::LEFT),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                self.toolbar(ui);
                                if self.mode == Mode::Packages {
                                    self.package_heading(ui, demo);
                                }
                                let items = self.filtered();
                                self.ensure_selection(&items);
                                egui::ScrollArea::vertical()
                                    .id_salt(("resource-list", self.mode))
                                    .auto_shrink([false, false])
                                    .max_height(ui.available_height())
                                    .show(ui, |ui| {
                                        self.list(ui, &items, demo);
                                    });
                            },
                        );
                        ui.separator();
                        ui.allocate_ui_with_layout(
                            Vec2::new(inspector_width, height),
                            Layout::top_down(Align::LEFT),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                egui::ScrollArea::vertical()
                                    .id_salt(("resource-inspector", &self.selected))
                                    .auto_shrink([false, false])
                                    .max_height(height)
                                    .show(ui, |ui| {
                                        if let Some(item) = self.selected_item() {
                                            self.inspector(ui, &item, demo);
                                        } else {
                                            ui.label(theme::small(
                                                ui,
                                                "Select a resource to inspect.",
                                            ));
                                        }
                                    });
                            },
                        );
                    });
                }
            });
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            if self.drafts.is_empty() {
                ui.label(theme::small(
                    ui,
                    "No pending changes · configuration, not live connection state",
                ));
            } else {
                ui.label(
                    RichText::new(format!("{} draft changes", self.drafts.len()))
                        .size(12.0)
                        .color(colors.accent),
                );
                if ui
                    .add_enabled(self.job.is_none(), egui::Button::new("Discard drafts"))
                    .clicked()
                {
                    self.drafts.clear();
                    self.review = None;
                    self.outcomes.clear();
                }
                if ui
                    .add_enabled(
                        self.job.is_none(),
                        egui::Button::new("Review changes →").fill(colors.accent_bg),
                    )
                    .clicked()
                {
                    self.request_review(ui.ctx(), demo);
                }
            }
        });
        ui.label(theme::small(ui,"Changes affect future Pi processes. Reopen Pui to reliably load them; running agents stay untouched."));
        self.dialogs(ui.ctx(), demo);
    }
    pub(super) fn filtered(&self) -> Vec<Resource> {
        let query = self.query.trim().to_lowercase();
        self.catalog
            .as_ref()
            .map(|c| {
                c.items
                    .iter()
                    .filter(|r| {
                        let group = if self.mode == Mode::Packages {
                            match self.source_group.as_str() {
                                "local" => {
                                    r.owner.is_empty()
                                        && r.kind != "mcp"
                                        && r.kind != "packages"
                                        && !r.path.starts_with("builtin:")
                                }
                                "mcp" => r.kind == "mcp",
                                "builtin" => r.path.starts_with("builtin:"),
                                s => r.owner == s && r.kind != "packages",
                            }
                        } else {
                            self.kind == 0 || r.kind == KINDS[self.kind].0
                        };
                        group
                            && (query.is_empty()
                                || format!(
                                    "{} {} {} {}",
                                    r.name,
                                    r.description,
                                    r.source_label(),
                                    r.path
                                )
                                .to_lowercase()
                                .contains(&query))
                            && match self.state_filter {
                                1 => !r.issue && self.effective(r),
                                2 => !r.issue && !self.effective(r),
                                3 => r.issue,
                                _ => true,
                            }
                            && match self.origin_filter {
                                1 => r.origin_scope == "user",
                                2 => r.origin_scope == "project",
                                _ => true,
                            }
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
    fn ensure_selection(&mut self, items: &[Resource]) {
        if !items.iter().any(|r| r.id == self.selected) {
            self.selected = items.first().map(|r| r.id.clone()).unwrap_or_default();
            self.source_tab = false;
        }
    }
    fn selected_item(&self) -> Option<Resource> {
        self.catalog
            .as_ref()?
            .items
            .iter()
            .find(|r| r.id == self.selected)
            .cloned()
    }
    fn rail(&mut self, ui: &mut egui::Ui, height: f32) {
        let colors = theme::palette(ui.ctx());
        egui::Frame::new()
            .fill(colors.settings_nav)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_min_height(height - 24.0);
                ui.set_width(ui.available_width());
                ui.label(theme::caption(
                    ui,
                    if self.mode == Mode::Packages {
                        "SOURCES"
                    } else {
                        "BROWSE"
                    },
                ));
                ui.add_space(8.0);
                if self.mode == Mode::Packages {
                    let packages: Vec<_> = self
                        .catalog
                        .as_ref()
                        .map(|c| {
                            c.items
                                .iter()
                                .filter(|r| r.kind == "packages")
                                .cloned()
                                .collect()
                        })
                        .unwrap_or_default();
                    if self.source_group.is_empty() {
                        self.source_group = packages
                            .first()
                            .map(|r| r.owner.clone())
                            .unwrap_or("local".into());
                    }
                    for p in packages {
                        if nav(
                            ui,
                            &p.name,
                            Icon::Folder,
                            self.source_group == p.owner,
                            if p.issue { "Missing" } else { &p.version },
                        )
                        .clicked()
                        {
                            self.source_group = p.owner;
                        }
                    }
                    ui.add_space(18.0);
                    ui.label(theme::caption(ui, "OTHER SOURCES"));
                    for (id, title, icon) in [
                        ("local", "Local resources", Icon::File),
                        ("mcp", "MCP configuration", Icon::Globe),
                        ("builtin", "Built into Pi", Icon::Code),
                    ] {
                        if nav(ui, title, icon, self.source_group == id, "").clicked() {
                            self.source_group = id.into();
                        }
                    }
                } else {
                    for (i, (kind, title, icon)) in KINDS.iter().enumerate() {
                        let count = self
                            .catalog
                            .as_ref()
                            .map(|c| {
                                c.items
                                    .iter()
                                    .filter(|r| kind.is_empty() || r.kind == *kind)
                                    .count()
                            })
                            .unwrap_or_default();
                        if nav(ui, title, *icon, self.kind == i, &count.to_string()).clicked() {
                            self.kind = i;
                            self.state_filter = 0;
                        }
                    }
                    ui.add_space(18.0);
                    if nav(
                        ui,
                        "Needs attention",
                        Icon::Bell,
                        self.state_filter == 3,
                        "",
                    )
                    .clicked()
                    {
                        self.kind = 0;
                        self.state_filter = 3;
                    }
                }
            });
    }
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new().inner_margin(12).show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            ui.horizontal(|ui| {
                let search_id = ui.id().with("resource-search");
                if !ui.ctx().wants_keyboard_input()
                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Slash))
                {
                    ui.memory_mut(|m| m.request_focus(search_id));
                }
                ui.add(
                    theme::text_field(&mut self.query, "Search name, purpose, source…")
                        .id(search_id)
                        .desired_width((ui.available_width() - 120.0).max(80.0)),
                );
                egui::ComboBox::from_id_salt("resource-state")
                    .width(102.0)
                    .selected_text(
                        ["All states", "Enabled", "Disabled", "Needs attention"][self.state_filter],
                    )
                    .show_ui(ui, |ui| {
                        for (i, s) in ["All states", "Enabled", "Disabled", "Needs attention"]
                            .iter()
                            .enumerate()
                        {
                            ui.selectable_value(&mut self.state_filter, i, *s);
                        }
                    });
            });
            ui.horizontal_wrapped(|ui| {
                if self.mode == Mode::Packages {
                    let mut groups: Vec<(String, String)> = self
                        .catalog
                        .as_ref()
                        .map(|c| {
                            c.items
                                .iter()
                                .filter(|r| r.kind == "packages")
                                .map(|r| (r.owner.clone(), r.name.clone()))
                                .collect()
                        })
                        .unwrap_or_default();
                    groups.extend([
                        ("local".into(), "Local resources".into()),
                        ("mcp".into(), "MCP configuration".into()),
                        ("builtin".into(), "Built into Pi".into()),
                    ]);
                    if self.source_group.is_empty() {
                        self.source_group = groups[0].0.clone();
                    }
                    egui::ComboBox::from_id_salt("resource-source-group")
                        .selected_text(
                            groups
                                .iter()
                                .find(|g| g.0 == self.source_group)
                                .map(|g| g.1.as_str())
                                .unwrap_or("Sources"),
                        )
                        .show_ui(ui, |ui| {
                            for (id, label) in groups {
                                ui.selectable_value(&mut self.source_group, id, label);
                            }
                        });
                }
                if self.mode == Mode::Workspace || ui.available_width() < 340.0 {
                    egui::ComboBox::from_id_salt("resource-kind")
                        .selected_text(KINDS[self.kind].1)
                        .show_ui(ui, |ui| {
                            for (i, (_, label, _)) in KINDS.iter().enumerate() {
                                ui.selectable_value(&mut self.kind, i, *label);
                            }
                        });
                }
                egui::ComboBox::from_id_salt("resource-origin")
                    .selected_text(["All origins", "Personal", "Project-local"][self.origin_filter])
                    .show_ui(ui, |ui| {
                        for (i, label) in ["All origins", "Personal", "Project-local"]
                            .iter()
                            .enumerate()
                        {
                            ui.selectable_value(&mut self.origin_filter, i, *label);
                        }
                    });
                ui.label(theme::small(
                    ui,
                    format!("{} resources", self.filtered().len()),
                ));
            });
        });
    }
    fn list(&mut self, ui: &mut egui::Ui, items: &[Resource], demo: bool) {
        let colors = theme::palette(ui.ctx());
        if items.is_empty() {
            ui.add_space(24.0);
            ui.label(theme::small(
                ui,
                "No matching resources. Change filters or add a resource.",
            ));
            return;
        }
        if !ui.ctx().wants_keyboard_input() {
            let direction = ui.input(|i| {
                if i.key_pressed(egui::Key::ArrowDown) {
                    1
                } else if i.key_pressed(egui::Key::ArrowUp) {
                    -1
                } else {
                    0
                }
            });
            if direction != 0 {
                let at = items
                    .iter()
                    .position(|r| r.id == self.selected)
                    .unwrap_or(0) as isize;
                self.selected = items[(at + direction).clamp(0, items.len() as isize - 1) as usize]
                    .id
                    .clone();
            }
        }
        if self.mode == Mode::Workspace && ui.available_width() > 720.0 {
            self.workspace_table(ui, items);
            return;
        }
        let keyboard_scroll = !ui.ctx().wants_keyboard_input()
            && ui.input(|i| {
                i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowUp)
            });
        for item in items {
            ui.push_id(&item.id, |ui| {
                let width = ui.available_width();
                let selected = self.selected == item.id;
                let frame = egui::Frame::new()
                    .fill(if selected {
                        colors.accent_bg
                    } else {
                        colors.settings_card
                    })
                    .inner_margin(egui::Margin::symmetric(15, 8));
                let shown = frame.show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.set_width((width - 30.0).max(0.0));
                    ui.horizontal_top(|ui| {
                        resource_icon(ui, item);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 4.0;
                            ui.set_width((ui.available_width() - 110.0).max(80.0));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&item.name)
                                        .size(12.5)
                                        .strong()
                                        .color(colors.text),
                                )
                                .truncate(),
                            );
                            ui.add(
                                egui::Label::new(theme::small(ui, &item.description)).truncate(),
                            )
                            .on_hover_text(&item.description);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(item.source_label())
                                        .size(10.0)
                                        .color(colors.dim),
                                )
                                .truncate(),
                            );
                        });
                        ui.with_layout(Layout::top_down(Align::RIGHT), |ui| {
                            self.state_label(ui, item);
                            if self.draft_for(&item.id) {
                                ui.label(
                                    RichText::new("Draft change").size(10.0).color(colors.amber),
                                );
                            } else {
                                ui.label(
                                    RichText::new(if item.origin_scope == "project" {
                                        "Project-local"
                                    } else if self.selection(item, "project") != "inherit" {
                                        "Project override"
                                    } else {
                                        "Personal default"
                                    })
                                    .size(10.0)
                                    .color(colors.dim),
                                );
                            }
                        });
                    });
                    if self.mode == Mode::Workspace && item.kind != "packages" {
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(theme::small(
                                ui,
                                format!(
                                    "Personal: {}",
                                    item.personal_enabled
                                        .map(|v| if v { "Enabled" } else { "Disabled" })
                                        .unwrap_or("—")
                                ),
                            ));
                            let mut state = self.selection(item, "project");
                            ui.add_enabled_ui(
                                self.catalog.as_ref().is_some_and(|c| c.trusted)
                                    && self.job.is_none(),
                                |ui| {
                                    egui::ComboBox::from_id_salt("project-override")
                                        .selected_text(state_title(&state))
                                        .show_ui(ui, |ui| {
                                            if item.inherited {
                                                ui.selectable_value(
                                                    &mut state,
                                                    "inherit".into(),
                                                    "Inherit",
                                                );
                                            }
                                            ui.selectable_value(
                                                &mut state,
                                                "on".into(),
                                                "Enable here",
                                            );
                                            ui.selectable_value(
                                                &mut state,
                                                "off".into(),
                                                "Disable here",
                                            );
                                        });
                                },
                            );
                            if state != self.selection(item, "project") {
                                self.select_state(item, "project", &state);
                            }
                        });
                    }
                });
                let select_rect = if self.mode == Mode::Workspace {
                    egui::Rect::from_min_max(
                        shown.response.rect.min,
                        egui::pos2(
                            shown.response.rect.right(),
                            shown.response.rect.top() + 65.0,
                        ),
                    )
                } else {
                    shown.response.rect
                };
                let response = ui.interact(select_rect, ui.id().with("select-row"), Sense::click());
                if response.clicked() {
                    self.selected = item.id.clone();
                    self.source_tab = false;
                }
                if selected && keyboard_scroll {
                    response.scroll_to_me(Some(Align::Center));
                }
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::SelectableLabel,
                        ui.is_enabled(),
                        selected,
                        &item.name,
                    )
                });
                if response.has_focus()
                    && ui.input(|i| {
                        i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space)
                    })
                {
                    self.selected = item.id.clone();
                }
                response.on_hover_cursor(egui::CursorIcon::PointingHand);
                let (line, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
                ui.painter().hline(
                    line.x_range(),
                    line.center().y,
                    Stroke::new(1.0, colors.border),
                );
            });
        }
        let _ = demo;
    }
    fn workspace_table(&mut self, ui: &mut egui::Ui, items: &[Resource]) {
        let colors = theme::palette(ui.ctx());
        let width = ui.available_width();
        let columns = [
            (width * 0.45 - 20.0).max(180.0),
            width * 0.17,
            width * 0.21,
            width * 0.17,
        ];
        egui::Frame::new()
            .fill(colors.settings_nav)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width(width - 28.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (label, w) in [
                        "Resource",
                        "Personal default",
                        "Project override",
                        "Effective here",
                    ]
                    .into_iter()
                    .zip(columns)
                    {
                        ui.allocate_ui_with_layout(
                            Vec2::new(w, 20.0),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.set_min_width(w);
                                ui.label(RichText::new(label).size(10.5).color(colors.muted));
                            },
                        );
                    }
                });
            });
        for item in items {
            ui.push_id(("matrix", &item.id), |ui| {
                let selected = self.selected == item.id;
                let shown = egui::Frame::new()
                    .fill(if selected {
                        colors.accent_bg
                    } else {
                        colors.settings_card
                    })
                    .inner_margin(egui::Margin::symmetric(14, 11))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.allocate_ui_with_layout(
                                Vec2::new(columns[0], 42.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.set_min_width(columns[0]);
                                    resource_icon(ui, item);
                                    ui.add_space(9.0);
                                    ui.vertical(|ui| {
                                        ui.set_width((columns[0] - 45.0).max(80.0));
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&item.name).size(12.0).strong(),
                                            )
                                            .truncate(),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(item.source_label())
                                                    .size(10.0)
                                                    .color(colors.dim),
                                            )
                                            .truncate(),
                                        );
                                    });
                                },
                            );
                            ui.allocate_ui_with_layout(
                                Vec2::new(columns[1], 42.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.set_min_width(columns[1]);
                                    ui.label(
                                        RichText::new(
                                            item.personal_enabled
                                                .map(|v| if v { "Enabled" } else { "Disabled" })
                                                .unwrap_or("— Project-local"),
                                        )
                                        .size(11.0)
                                        .color(colors.muted),
                                    );
                                },
                            );
                            ui.allocate_ui_with_layout(
                                Vec2::new(columns[2], 42.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.set_min_width(columns[2]);
                                    if item.kind != "packages" {
                                        let mut state = self.selection(item, "project");
                                        ui.add_enabled_ui(
                                            self.job.is_none()
                                                && self.catalog.as_ref().is_some_and(|c| c.trusted),
                                            |ui| {
                                                egui::ComboBox::from_id_salt("matrix-override")
                                                    .width((columns[2] - 12.0).min(120.0))
                                                    .selected_text(state_title(&state))
                                                    .show_ui(ui, |ui| {
                                                        if item.inherited {
                                                            ui.selectable_value(
                                                                &mut state,
                                                                "inherit".into(),
                                                                "Inherit",
                                                            );
                                                        }
                                                        ui.selectable_value(
                                                            &mut state,
                                                            "on".into(),
                                                            "Enable here",
                                                        );
                                                        ui.selectable_value(
                                                            &mut state,
                                                            "off".into(),
                                                            "Disable here",
                                                        );
                                                    });
                                            },
                                        );
                                        if state != self.selection(item, "project") {
                                            self.select_state(item, "project", &state);
                                        }
                                    } else {
                                        ui.label(theme::small(ui, "Package source"));
                                    }
                                },
                            );
                            ui.allocate_ui_with_layout(
                                Vec2::new(columns[3] - 8.0, 42.0),
                                Layout::top_down(Align::LEFT),
                                |ui| {
                                    ui.set_min_width(columns[3] - 8.0);
                                    self.state_label(ui, item);
                                    if self.draft_for(&item.id) {
                                        ui.label(
                                            RichText::new("Draft change")
                                                .size(10.0)
                                                .color(colors.amber),
                                        );
                                    }
                                },
                            );
                        });
                    });
                let select_rect = egui::Rect::from_min_size(
                    shown.response.rect.min,
                    Vec2::new(columns[0] + columns[1], shown.response.rect.height()),
                );
                if ui
                    .interact(select_rect, ui.id().with("select-matrix"), Sense::click())
                    .clicked()
                {
                    self.selected = item.id.clone();
                }
                if selected
                    && !ui.ctx().wants_keyboard_input()
                    && ui.input(|i| {
                        i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowUp)
                    })
                {
                    ui.scroll_to_rect(shown.response.rect, Some(Align::Center));
                }
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.separator();
            });
        }
    }
    fn state_label(&self, ui: &mut egui::Ui, item: &Resource) {
        let colors = theme::palette(ui.ctx());
        ui.label(
            RichText::new(if item.issue {
                "Not installed"
            } else if item.kind == "packages" {
                "Installed"
            } else if self.effective(item) {
                "● Enabled"
            } else {
                "○ Disabled"
            })
            .size(10.5)
            .color(if item.issue {
                colors.amber
            } else if self.effective(item) {
                colors.accent
            } else {
                colors.muted
            }),
        );
    }
    fn package_heading(&mut self, ui: &mut egui::Ui, demo: bool) {
        let package = self
            .catalog
            .as_ref()
            .and_then(|c| {
                c.items
                    .iter()
                    .find(|r| r.kind == "packages" && r.owner == self.source_group)
            })
            .cloned();
        egui::Frame::new().inner_margin(16).show(ui, |ui| {
            if let Some(p) = package {
                ui.label(RichText::new(&p.name).size(17.0).strong());
                ui.add(
                    egui::Label::new(theme::small(ui, &p.source))
                        .wrap()
                        .selectable(true),
                );
                ui.label(theme::small(
                    ui,
                    if p.issue {
                        "Not installed · contents unknown".into()
                    } else {
                        format!(
                            "{} resolved resources · installed version {}",
                            p.resource_count,
                            if p.version.is_empty() {
                                "not reported"
                            } else {
                                &p.version
                            }
                        )
                    },
                ));
                if ui
                    .add_enabled(
                        self.job.is_none(),
                        egui::Button::new(if p.issue {
                            "Review install…"
                        } else {
                            "Uninstall package…"
                        }),
                    )
                    .clicked()
                {
                    if p.issue {
                        self.stage(
                            format!("install:{}", p.id),
                            format!("Install {}", p.name),
                            json!({"action":"install","scope":p.scope,"source":p.source}),
                        );
                    } else {
                        self.remove = Some(p);
                    }
                }
            } else {
                ui.label(
                    RichText::new(match self.source_group.as_str() {
                        "mcp" => "MCP configuration",
                        "builtin" => "Built into Pi",
                        _ => "Local resources",
                    })
                    .size(17.0)
                    .strong(),
                );
                ui.label(theme::small(
                    ui,
                    "Sources remain distinct; local resources and MCP entries are not packages.",
                ));
            }
        });
        let _ = demo;
    }
    pub(super) fn inspector(&mut self, ui: &mut egui::Ui, item: &Resource, demo: bool) {
        let colors = theme::palette(ui.ctx());
        egui::Frame::new().fill(colors.settings_nav).inner_margin(20).show(ui,|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                resource_icon(ui,item);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 5.0;
                    ui.label(theme::caption(ui,item.kind_label()));
                    ui.add(egui::Label::new(RichText::new(&item.name).size(17.0).strong()).wrap());
                });
            });
            ui.add_space(10.0);ui.add(egui::Label::new(theme::small(ui,&item.description)).wrap());self.state_label(ui,item);
            ui.add_space(10.0);ui.horizontal(|ui|{if theme::choice(ui,!self.source_tab,"Overview").clicked(){self.source_tab=false;}
if theme::choice(ui,self.source_tab,"Source").clicked(){self.source_tab=true;}});ui.separator();
            if self.source_tab {
                for(label,value)in[("Resolved path",&item.path),("Source",&item.source),("Configuration",&item.config)] {ui.label(theme::caption(ui,label));ui.add(egui::Label::new(RichText::new(value).monospace().size(10.5).color(colors.muted)).wrap().selectable(true));ui.add_space(7.0);}
                if ui.button("Copy path").clicked(){ui.ctx().copy_text(item.path.clone());}
                if ui.add_enabled(!demo,egui::Button::new("Open config ↗")).clicked(){transport::open_config(item.config.clone().into());}
                return;
            }
            let editable=self.job.is_none();
            if item.kind!="packages" {
                ui.label(theme::caption(ui,"FOR THIS PROJECT"));
                ui.add_enabled_ui(editable&&self.catalog.as_ref().is_some_and(|c|c.trusted),|ui|{
                    ui.horizontal_wrapped(|ui|{for(state,label)in[("inherit","Inherit"),("on","Enable here"),("off","Disable here")] {if state=="inherit"&&!item.inherited{continue;}
if theme::choice(ui,self.selection(item,"project")==state,label).clicked(){self.select_state(item,"project",state);}}});
                });
                ui.add(egui::Label::new(theme::small(ui,if item.inherited {"Inherit uses your personal default. Overrides affect only this workspace."}else{"Defined in this project. No personal default exists."})).wrap());
                if item.inherited {
                    ui.add_space(8.0);
                    let mut personal=self.selection(item,"user")=="on";
                    ui.add_enabled_ui(editable,|ui|{ui.horizontal_wrapped(|ui|{
                        let old=personal;
                        super::super::page::toggle(ui,&mut personal,"Personal default");
                        ui.label(theme::small(ui,"Personal default"));
                        if old!=personal{self.select_state(item,"user",if personal{"on"}else{"off"});}
                    });});
                    ui.label(theme::small(ui,"Applies to all inheriting projects."));
                }
                ui.add_space(14.0);
            }
            if item.kind=="mcp" {
                ui.label(theme::caption(ui,"SERVER CONFIGURATION"));ui.label(theme::small(ui,format!("{} · {} exposure",item.transport,item.exposure)));
                ui.add(egui::Label::new(theme::small(ui,"Credentials, URLs and command arguments are not loaded here. Enabled does not mean connected.")).wrap());
                if ui.add_enabled(editable,egui::Button::new("Edit server…")).clicked(){self.edit_server(item);}
                if ui.button("Copy diagnostic command").on_hover_text("Running pi mcp list connects enabled servers. This view does not.").clicked(){ui.ctx().copy_text("pi mcp list".into());}
            } else if item.kind=="packages" {
                let owned:Vec<_>=self.catalog.as_ref().map(|c|c.items.iter().filter(|r|r.kind!="packages"&&!item.owner.is_empty()&&r.owner==item.owner).map(|r|r.name.clone()).collect()).unwrap_or_default();
                ui.label(theme::caption(ui,"PACKAGE CONTENTS"));if item.issue{ui.label(theme::small(ui,"Not installed. No remote contents are inferred."));}else{for name in owned{ui.label(theme::small(ui,name));}}
            } else if !item.path.starts_with("builtin:") {
                ui.label(theme::caption(ui,if item.kind=="extensions"{"EXECUTABLE SOURCE"}else{"LOCAL PREVIEW"}));
                if let Some(preview)=self.previews.get(&item.id) {
                    egui::Frame::new().fill(colors.settings_card).stroke(Stroke::new(1.0,colors.border)).corner_radius(6).inner_margin(12).show(ui,|ui| {
                        if item.kind=="skills" || item.kind=="prompts" { markdown::show(ui, &preview.content); }
                        else {ui.add(egui::Label::new(RichText::new(&preview.content).monospace().size(10.5)).wrap().selectable(true));}
                    });
                    ui.label(theme::small(ui,&preview.path));
                } else if ui.add_enabled(editable,egui::Button::new("Read source preview")).clicked(){if demo{self.previews.insert(item.id.clone(),Preview{id:item.id.clone(),path:item.path.clone(),content:format!("# {}\n\n{}\n\nDemo fixture. Source files are read only after an explicit request.",item.name,item.description)});}else{self.start(ui.ctx(),JobKind::Inspect,json!({"action":"inspect","id":item.id,"revision":self.catalog.as_ref().map(|c|c.revision.clone()).unwrap_or_default()}));}}
                ui.add(egui::Label::new(theme::small(ui,if item.kind=="extensions"{"Extensions can execute with your OS permissions when loaded. Previewing does not execute them."}else{"Explicit read · UTF-8 only · 64 KiB maximum · no symlinks."})).wrap());
                if item.kind=="prompts"&&ui.button("Copy slash command").clicked(){ui.ctx().copy_text(format!("/{}",item.name));}
                if item.kind=="themes"{ui.add(egui::Label::new(theme::small(ui,"Pi CLI theme; this does not change Pui's appearance.")).wrap());}
            }
            ui.add_space(12.0);ui.separator();
            ui.label(theme::caption(ui,"ORIGIN"));ui.add(egui::Label::new(theme::small(ui,item.source_label())).wrap());
            if !item.owner.is_empty()&&item.kind!="packages"&&ui.button("View owning package →").clicked(){self.mode=Mode::Packages;self.source_group=item.owner.clone();}
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!demo,egui::Button::new("Open config ↗")).clicked(){transport::open_config(item.config.clone().into());}
                if item.removable&&ui.add_enabled(editable,egui::Button::new(if item.kind=="packages"{"Uninstall…"}else{"Remove…"})).clicked(){self.remove=Some(item.clone());}
            });
        });
    }
}
fn state_title(state: &str) -> &str {
    match state {
        "on" => "Enable here",
        "off" => "Disable here",
        _ => "Inherit",
    }
}
fn resource_icon(ui: &mut egui::Ui, item: &Resource) {
    let colors = theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
    ui.painter().rect_filled(rect, 7, colors.accent_bg);
    icons::draw(ui, rect.shrink(7.0), item.icon(), colors.accent);
}
fn nav(ui: &mut egui::Ui, title: &str, icon: Icon, selected: bool, suffix: &str) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
    if selected || response.hovered() {
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
    icons::draw(
        ui,
        egui::Rect::from_min_size(rect.min + egui::vec2(8.0, 10.0), Vec2::splat(16.0)),
        icon,
        if selected {
            colors.accent
        } else {
            colors.muted
        },
    );
    let text_rect = egui::Rect::from_min_max(
        rect.min + egui::vec2(32.0, 0.0),
        egui::pos2(
            rect.right()
                - if suffix.is_empty() {
                    6.0
                } else if suffix.len() <= 2 {
                    22.0
                } else {
                    42.0
                },
            rect.bottom(),
        ),
    );
    ui.painter().with_clip_rect(text_rect).text(
        text_rect.left_center(),
        egui::Align2::LEFT_CENTER,
        title,
        FontId::proportional(11.5),
        if selected { colors.accent } else { colors.text },
    );
    if !suffix.is_empty() {
        ui.painter().text(
            rect.right_center() - egui::vec2(6.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            suffix,
            FontId::proportional(10.0),
            colors.dim,
        );
    }
    response.on_hover_text(title)
}
