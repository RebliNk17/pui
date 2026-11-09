use super::*;

impl Pui {
    pub(super) fn top_bar(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        let mut notifications_button = None;
        egui::TopBottomPanel::top("workspace-bar-v2")
            .exact_height(if cfg!(target_os = "macos") && self.prefs.hide_sidebar {
                64.0
            } else {
                54.0
            })
            .frame(
                egui::Frame::new()
                    .fill(colors.bg)
                    .inner_margin(egui::Margin::symmetric(20, 0)),
            )
            .show(ctx, |ui| {
                if cfg!(target_os = "macos") && self.prefs.hide_sidebar {
                    ui.add_space(24.0 / ctx.zoom_factor());
                }
                ui.horizontal_centered(|ui| {
                    if self.prefs.hide_sidebar
                        && icons::button(ui, Icon::Sidebar, "Show sessions · ⌘B").clicked()
                    {
                        self.prefs.hide_sidebar = false;
                    }
                    if self.prefs.hide_sidebar {
                        self.app_menu(ui);
                    }
                    let title = if self.tasks.open {
                        self.tasks
                            .current()
                            .map(|t| {
                                if t.objective.is_empty() {
                                    "New verified task"
                                } else {
                                    t.objective.as_str()
                                }
                            })
                            .unwrap_or("Verified tasks")
                    } else if self.state.session_name.is_empty() {
                        "New conversation"
                    } else {
                        &self.state.session_name
                    };
                    let width = (ui.available_width() - 346.0).max(40.0);
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::click_and_drag());
                    let mut job = egui::text::LayoutJob::simple_singleline(
                        title.into(),
                        FontId::proportional(13.0),
                        colors.text,
                    );
                    job.wrap.max_width = width;
                    job.wrap.max_rows = 1;
                    let galley = ui.painter().layout_job(job);
                    ui.painter().galley(
                        rect.left_center() - egui::vec2(0.0, galley.size().y / 2.0),
                        galley,
                        colors.text,
                    );
                    if response.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    if response.double_clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(
                            !ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
                        ));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .small_button("Tasks")
                            .on_hover_text("Verified tasks and workflow evidence")
                            .clicked()
                        {
                            if self.tasks.selected.is_none() {
                                self.new_task();
                            } else {
                                self.tasks.open = !self.tasks.open;
                            }
                        }
                        if self.loading_chat {
                            ui.label(
                                RichText::new(if self.preview_partial {
                                    "Loading full chat…"
                                } else {
                                    "Connecting…"
                                })
                                .size(10.0)
                                .color(colors.dim),
                            );
                        }
                        notifications_button = Some(self.notifications_button(ui));
                        let response =
                            icons::button(ui, Icon::Terminal, "Terminal · current project");
                        if self.terminals.is_open() {
                            ui.painter().rect_stroke(
                                response.rect,
                                7,
                                Stroke::new(1.5, colors.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if response.clicked() {
                            self.terminals.toggle(ctx, &self.prefs.cwd, self.demo);
                            self.focus_composer = !self.terminals.is_open();
                        }
                        let response = icons::button(ui, Icon::Folder, "Browse workspace files");
                        if self.files.is_open() {
                            ui.painter().rect_stroke(
                                response.rect,
                                7,
                                Stroke::new(1.5, colors.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if response.clicked() {
                            self.files.toggle(ctx, &self.prefs.cwd);
                            self.inspector_panel = false;
                        }
                        let response = icons::button(
                            ui,
                            Icon::File,
                            "Changes · current Git and recorded edits",
                        );
                        if self.inspector_panel {
                            ui.painter().rect_filled(response.rect, 7, colors.accent_bg);
                            icons::draw(ui, response.rect, Icon::File, colors.text);
                        }
                        if response.clicked() {
                            self.files.close();
                            self.inspector_panel = !self.inspector_panel;
                            self.prefs.hide_inspector = false;
                            if self.inspector_panel {
                                self.changes.invalidate();
                                self.refresh_workspace();
                            }
                        }
                        let active = !self.prefs.hide_inspector
                            && !self.inspector_panel
                            && !self.files.is_open();
                        let response = icons::button(ui, Icon::Details, "Session details · ⌘I");
                        if active {
                            ui.painter().rect_stroke(
                                response.rect,
                                7,
                                Stroke::new(1.0, colors.hover),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if response.clicked() {
                            if self.files.is_open() {
                                self.files.close();
                                self.prefs.hide_inspector = false;
                            } else if self.inspector_panel {
                                self.inspector_panel = false;
                                self.prefs.hide_inspector = false;
                            } else {
                                self.prefs.hide_inspector = !self.prefs.hide_inspector;
                            }
                        }
                        response.context_menu(|ui| self.prefs.cards.menu(ui));
                    });
                });
            });
        if let Some(button) = notifications_button {
            self.notifications_popup(&button);
        }
    }

    pub(super) fn inspector(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        egui::SidePanel::right("floating-inspector-v2")
            .exact_width(350.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(colors.bg)
                    .inner_margin(egui::Margin {
                        left: 8,
                        right: 18,
                        top: 18,
                        bottom: 24,
                    }),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("floating-cards")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Keep floating scrollbar handles clear of the card borders.
                        ui.set_width((ui.available_width() - 12.0).max(0.0));
                        let task = self
                            .tasks
                            .open
                            .then(|| self.tasks.current().cloned())
                            .flatten();
                        let status = task
                            .as_ref()
                            .map(|t| t.phase.label())
                            .unwrap_or_else(|| self.run_label().0);
                        let task_outputs = task
                            .as_ref()
                            .and_then(|t| t.latest())
                            .map(|a| a.changes.clone())
                            .unwrap_or_default();
                        let workflow_summary = self
                            .tasks
                            .current()
                            .map(|t| t.phase.label())
                            .unwrap_or("No task")
                            .to_owned();
                        let summaries = [
                            status.to_owned(),
                            self.state
                                .context_tokens
                                .filter(|_| self.state.context_window > 0.0)
                                .map(|t| format!("{:.0}%", t / self.state.context_window * 100.0))
                                .unwrap_or_else(|| "Not reported".into()),
                            if task.is_some() {
                                task_outputs.len()
                            } else {
                                self.outputs.len()
                            }
                            .to_string(),
                            if self.demo { "4" } else { "—" }.into(),
                            self.workspace.files.len().to_string(),
                        ];
                        let mut layout = std::mem::take(&mut self.prefs.cards);
                        let original_hidden = layout.hidden.clone();
                        if task.is_some() {
                            layout
                                .hidden
                                .extend([Card::Context, Card::Pulls, Card::Changes]);
                        }
                        inspector::stack(
                            ui,
                            &mut layout,
                            |card| {
                                if card == Card::Workflow {
                                    return workflow_summary.clone();
                                }
                                summaries[match card {
                                    Card::Workflow => unreachable!(),
                                    Card::Environment => 0,
                                    Card::Context => 1,
                                    Card::Outputs => 2,
                                    Card::Pulls => 3,
                                    Card::Changes => 4,
                                }]
                                .clone()
                            },
                            |ui, card| match card {
                                Card::Workflow => self.task_workflow(ui),
                                Card::Environment => {
                                    if let Some(task) = &task {
                                        ui.label(
                                            RichText::new(if task.isolated {
                                                "Isolated task workspace"
                                            } else {
                                                "Task workspace"
                                            })
                                            .size(12.0),
                                        );
                                        inspector::detail(ui, "Status", task.phase.label());
                                        inspector::detail(
                                            ui,
                                            "Cost",
                                            &format!("$ {:.3} reported", task.cost()),
                                        );
                                        let cwd = task.workspace.as_ref().unwrap_or(&task.source);
                                        ui.add(
                                            egui::Label::new(theme::small(
                                                ui,
                                                cwd.display().to_string(),
                                            ))
                                            .wrap()
                                            .selectable(true),
                                        );
                                        ui.label(theme::small(
                                            ui,
                                            "Task evidence only; ordinary chat is separate.",
                                        ));
                                        return;
                                    }
                                    ui.horizontal(|ui| {
                                        dot(
                                            ui,
                                            if self.connected || self.demo {
                                                colors.green
                                            } else {
                                                colors.dim
                                            },
                                        );
                                        ui.vertical(|ui| {
                                            ui.spacing_mut().item_spacing.y = 2.0;
                                            ui.label(RichText::new("Local computer").size(12.0));
                                            ui.label(
                                                RichText::new(if self.demo {
                                                    "Design preview · no agent connected"
                                                } else if self.connected {
                                                    "Pi RPC connected"
                                                } else {
                                                    "Not connected"
                                                })
                                                .size(10.0)
                                                .color(colors.muted),
                                            );
                                        });
                                    });
                                    ui.add_space(8.0);
                                    inspector::detail(ui, "Status", status);
                                    inspector::detail(
                                        ui,
                                        "Repository",
                                        &sessions::project_name(&self.prefs.cwd),
                                    );
                                    if !self.workspace.branch.is_empty() {
                                        inspector::detail(ui, "Branch", &self.workspace.branch);
                                    }
                                    if let Some(timestamp) = self
                                        .state
                                        .messages
                                        .iter()
                                        .map(|m| m.timestamp)
                                        .find(|t| *t > 0)
                                    {
                                        inspector::detail(
                                            ui,
                                            "Started",
                                            &crate::transcript::time_label(timestamp),
                                        );
                                    }
                                    inspector::detail(
                                        ui,
                                        "Cost",
                                        &format!("$ {:.2}", self.state.cost),
                                    );
                                    if !self.demo
                                        && self.client.is_none()
                                        && ui.small_button("Connect to Pi").clicked()
                                        && !self.resume_agent()
                                    {
                                        self.connect(
                                            ctx,
                                            self.state.session_file.as_ref().map(PathBuf::from),
                                        );
                                    }
                                }
                                Card::Context => {
                                    crate::context::show(ui, &self.state, self.demo);
                                    if !self.demo
                                        && ui
                                            .add_enabled(
                                                self.connected && self.state.ready(),
                                                egui::Button::new(theme::small(
                                                    ui,
                                                    "Compact context",
                                                ))
                                                .frame(false),
                                            )
                                            .clicked()
                                    {
                                        self.send(protocol::command("compact"));
                                    }
                                }
                                Card::Outputs => {
                                    let outputs = if task.is_some() {
                                        &task_outputs
                                    } else {
                                        &self.outputs
                                    };
                                    if outputs.is_empty() {
                                        ui.label(theme::small(ui, "No recorded file changes."));
                                    }
                                    let mut selected = None;
                                    for (index, file) in outputs.iter().enumerate() {
                                        if output_row(
                                            ui,
                                            &file.path,
                                            &file.added.to_string(),
                                            &file.removed.to_string(),
                                        )
                                        .clicked()
                                        {
                                            selected = Some(index);
                                        }
                                    }
                                    if let Some(index) = selected {
                                        if task.is_some() {
                                            self.tasks.selected_run = None;
                                        } else {
                                            let file = &self.outputs[index];
                                            self.selected_diff =
                                                Some((file.path.clone(), file.diff.clone()));
                                        }
                                    }
                                }
                                Card::Pulls => {
                                    self.pulls.show(ui);
                                }
                                Card::Changes => self.working_tree(ui),
                            },
                        );
                        if task.is_some() {
                            for card in [Card::Context, Card::Pulls, Card::Changes] {
                                if !original_hidden.contains(&card) {
                                    layout.hidden.remove(&card);
                                }
                            }
                        }
                        self.prefs.cards = layout;
                    });
            });
    }

    fn working_tree(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        ui.label(
            RichText::new("Working tree vs HEAD, not recorded session outputs.")
                .size(10.0)
                .color(colors.dim),
        );
        let mut selected = None;
        for (index, file) in self.workspace.files.iter().enumerate() {
            if output_row(ui, &file.path, &file.added, &file.removed).clicked() {
                selected = Some(index);
            }
        }
        if self.workspace.files.is_empty() {
            ui.label(theme::small(
                ui,
                if self.workspace.is_git {
                    "Working tree is clean."
                } else {
                    "Not a Git repository."
                },
            ));
        }
        if let Some(index) = selected {
            let file = self.workspace.files[index].clone();
            let root = self.workspace.root.clone();
            let ctx = self.ctx.clone();
            let (tx, rx) = mpsc::channel();
            self.diff_rx = Some(rx);
            std::thread::spawn(move || {
                let diff = workspace::diff(&root, &file);
                let _ = tx.send((file.path, diff));
                ctx.request_repaint();
            });
        }
    }

    pub(super) fn conversation(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors.bg))
            .show(ctx, |ui| {
                egui::TopBottomPanel::bottom("composer-v2")
                    .show_separator_line(false)
                    .frame(
                        egui::Frame::new()
                            .fill(colors.bg)
                            .inner_margin(egui::Margin {
                                left: 24,
                                right: 24,
                                top: 8,
                                bottom: 3,
                            }),
                    )
                    .show_inside(ui, |ui| {
                        let width = (ui.available_width() - 24.0).min(760.0);
                        let padding = ((ui.available_width() - width) / 2.0).max(0.0);
                        ui.horizontal(|ui| {
                            ui.add_space(padding);
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                self.composer(ui, ctx);
                            });
                        });
                    });
                self.transcript.message_actions_enabled = self.history_ready();
                self.transcript.progress = self.transcript_progress();
                if self.state.messages.is_empty()
                    && !self.new_chat
                    && self.transcript.progress.is_some()
                {
                    self.transcript.show(
                        ui,
                        &[],
                        &self.state.model,
                        &self.state.thinking,
                        !self.state.ready(),
                        self.state.session_file.as_deref().unwrap_or("new"),
                    );
                } else if self.state.messages.is_empty() {
                    let width = ui.available_width();
                    let pad = ((width - 720.0) / 2.0).max(30.0);
                    egui::ScrollArea::vertical()
                        .id_salt("new-conversation-welcome")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add_space(pad);
                                ui.vertical(|ui| {
                                    ui.set_width((width - pad * 2.0).max(180.0));
                                    self.welcome(ui);
                                });
                            });
                        });
                } else {
                    self.transcript.show(
                        ui,
                        &self.state.messages,
                        &self.state.model,
                        &self.state.thinking,
                        !self.state.ready(),
                        self.state.session_file.as_deref().unwrap_or(if self.demo {
                            "preview"
                        } else {
                            "new"
                        }),
                    );
                }
            });
        self.history_action();
    }
}

fn output_row(ui: &mut egui::Ui, path: &str, added: &str, removed: &str) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4, colors.hover);
    }
    icons::draw(
        ui,
        egui::Rect::from_center_size(rect.left_center() + egui::vec2(8.0, 0.0), Vec2::splat(14.0)),
        Icon::File,
        colors.dim,
    );
    let name = std::path::Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let mut job = egui::text::LayoutJob::simple_singleline(
        name.into_owned(),
        FontId::proportional(11.0),
        colors.text,
    );
    job.wrap.max_width = (rect.width() - 100.0).max(40.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        rect.left_center() + egui::vec2(26.0, -galley.size().y / 2.0),
        galley,
        colors.text,
    );
    let mut x = rect.right() - 15.0;
    if !removed.is_empty() && removed != "0" {
        ui.painter().text(
            egui::pos2(x, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("−{removed}"),
            FontId::proportional(10.0),
            colors.red,
        );
        x -= 30.0;
    }
    if !added.is_empty() && added != "0" {
        ui.painter().text(
            egui::pos2(x, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("+{added}"),
            FontId::proportional(10.0),
            colors.green,
        );
    }
    icons::chevron(
        ui,
        egui::Rect::from_center_size(
            rect.right_center() - egui::vec2(3.0, 0.0),
            Vec2::splat(10.0),
        ),
        false,
        colors.dim,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_is_below_output_and_stop_is_next_to_send_not_model() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.connected = true;
        app.client = Some(rpc::Client::stub());
        app.state = State {
            busy: true,
            session_file: Some("/fixture/progress.jsonl".into()),
            model_name: "Fixture model".into(),
            messages: vec![Message {
                role: "assistant".into(),
                text: "Latest output text".into(),
                images: vec![
                    crate::media::Attachment::from_bytes(include_bytes!("../../assets/icon.png"))
                        .unwrap(),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let render = |app: &mut Pui, time: f64| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1000.0, 800.0),
                    )),
                    time: Some(time),
                    ..Default::default()
                },
                |ctx| app.conversation(ctx),
            )
        };
        render(&mut app, 0.0);
        let out = render(&mut app, 0.1);
        fn inspect(
            shape: &egui::Shape,
            text: &mut Vec<(String, egui::Pos2)>,
            stops: &mut Vec<egui::Rect>,
            sends: &mut Vec<egui::Rect>,
            images: &mut Vec<egui::Rect>,
        ) {
            match shape {
                egui::Shape::Text(t) => text.push((t.galley.job.text.clone(), t.pos)),
                egui::Shape::Rect(r) if r.rect.size() == Vec2::splat(8.0) => stops.push(r.rect),
                egui::Shape::Rect(r) if r.rect.size() == Vec2::splat(28.0) => sends.push(r.rect),
                egui::Shape::Rect(r) if r.rect.size() == Vec2::new(112.0, 76.0) => {
                    images.push(r.rect)
                }
                egui::Shape::Vec(v) => {
                    for s in v {
                        inspect(s, text, stops, sends, images);
                    }
                }
                _ => {}
            }
        }
        let mut text = vec![];
        let mut stops = vec![];
        let mut sends = vec![];
        let mut images = vec![];
        for shape in out.shapes {
            inspect(&shape.shape, &mut text, &mut stops, &mut sends, &mut images);
        }
        let progress = text
            .iter()
            .find(|(t, _)| t == "Thinking…")
            .expect("working verb rendered")
            .1;
        let output = text
            .iter()
            .find(|(t, _)| t == "Latest output text")
            .unwrap()
            .1;
        let composer = text
            .iter()
            .find(|(t, _)| t.starts_with("Message Pi…"))
            .unwrap()
            .1;
        assert!(
            progress.y > output.y + 76.0 && progress.y < composer.y,
            "status belongs after image outputs and before composer: {progress:?}"
        );
        if let Some(image) = images.first() {
            assert!(progress.y > image.bottom());
        }
        assert!(!text.iter().any(|(t, _)| t == "Working…" || t == "Idle"));
        let stop = stops
            .iter()
            .find(|r| r.center().x > 800.0)
            .expect("stop on right");
        let send = sends
            .iter()
            .find(|r| {
                r.center().x > stop.center().x && (r.center().y - stop.center().y).abs() < 1.0
            })
            .expect("adjacent send button");
        assert!((send.center().x - stop.center().x - 32.0).abs() < 1.0);
        let total = app.transcript.total;
        render(&mut app, 4.1);
        assert!(
            (app.transcript.total - total).abs() < 0.1,
            "changing verbs must not move the transcript"
        );
        app.state.busy = false;
        render(&mut app, 5.0);
        assert!(app.transcript.progress.is_none());
    }
    #[test]
    fn mcp_warning_does_not_change_conversation_viewport() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        let mut frame = eframe::Frame::_new_kittest();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1480.0, 960.0),
            )),
            ..Default::default()
        };
        for _ in 0..3 {
            let _ = ctx.run(input(), |ctx| {
                eframe::App::update(&mut app, ctx, &mut frame)
            });
        }
        let height = app.transcript.viewport;
        app.state.apply(json!({"type":"extension_ui_request","method":"notify","message":"MCP servers need attention:\nLaunchDarkly: HTTP 503 Service Unavailable\nConnection closed\nRun /mcp to fix."}));
        let _ = ctx.run(input(), |ctx| {
            eframe::App::update(&mut app, ctx, &mut frame)
        });
        assert_eq!(app.transcript.viewport, height);
        assert_eq!(app.state.diagnostics.len(), 1);
        assert!(app.state.notice.is_none());
    }
}
