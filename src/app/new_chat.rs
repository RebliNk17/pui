//! New chats open immediately. Native folder selection is an explicit action
//! inside the blank chat, never the side effect of pressing +.
use super::*;

pub(super) mod project_picker;

impl Pui {
    /// Only newly allocated, never-used chats are disposable. An empty active
    /// branch of saved history is not evidence that its transcript is empty.
    pub(super) fn discard_empty_new_chat(&mut self) -> bool {
        let id = self.agents.active.clone();
        let node = self.agents.nodes.get(&id);
        let path = self
            .state
            .session_file
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| node.and_then(|node| node.path.clone()));
        if self.demo
            || !self.new_chat
            || !self.state.ready()
            || self.state.run_observed
            || !self.state.messages.is_empty()
            || !self.state.pending_messages.is_empty()
            || !self.state.history_points.is_empty()
            || self.state.chat_title().is_some()
            || !self.state.dialogs.is_empty()
            || !self.draft.is_empty()
            || !self.draft_images.is_empty()
            || self.image_picker_rx.is_some()
            || !self.deliveries.is_empty()
            || self.interrupt.is_some()
            || self.resource_reload.pending()
            || self.history_pending.is_some()
            || self.is_mirror()
            || self.pending_session.is_some()
            || self.preview_rx.is_some()
            || node.is_some_and(|node| {
                node.parent.is_some() || !node.children.is_empty() || node.setup_pending()
            })
            || self.agents.permission_changes.contains_key(&id)
            || self.agent_mail_pending(&id)
            || self.automation.has_chat_jobs(&id, path.as_deref())
            || self.tasks.parents.values().any(|parent| parent == &id)
            || self.tasks.records.iter().any(|task| {
                path.as_ref().is_some_and(|path| {
                    task.parent_session.as_ref() == Some(path)
                        || task
                            .runs
                            .iter()
                            .any(|run| run.session.as_ref() == Some(path))
                })
            })
            || path.as_ref().is_some_and(|path| {
                self.prefs.chats.contains_key(path)
                    || self
                        .waiting
                        .get(path)
                        .is_some_and(|queue| !queue.is_empty())
                    || self
                        .agents
                        .nodes
                        .values()
                        .any(|node| node.id != id && node.path.as_ref() == Some(path))
            })
        {
            return false;
        }
        if let Some(path) = &path {
            // Reserve the path before detaching the client. Failure preserves
            // the chat; another window's session must never be deleted.
            if !self.begin_empty_chat_cleanup(path.clone()) {
                return false;
            }
        }
        self.leave_session_layout();
        let mut chat = self.take_chat();
        if let Some(client) = chat.client.take() {
            client.retire();
        }
        self.session_layouts.forget_agent(&id);
        if let Some(path) = path.as_deref() {
            self.session_layouts.forget_path(path);
        }
        self.prefs
            .workspace_layouts
            .retain(|layout| layout.key != format!("agent:{id}"));
        self.forget_deleted_agent_ids(&std::collections::BTreeSet::from([id]));
        self.new_chat = true;
        true
    }

    fn remember_project(&mut self, folder: &std::path::Path) {
        self.prefs.projects.retain(|p| p != folder);
        self.prefs.projects.insert(0, folder.to_path_buf());
        self.prefs.projects.truncate(32);
    }
    pub(super) fn chat_projects(&self) -> Vec<PathBuf> {
        let mut projects = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        // No filesystem scans while rendering. Availability is validated when
        // the user selects a project; paths come from known sessions/settings.
        for path in std::iter::once(&self.prefs.cwd)
            .chain(self.prefs.projects.iter())
            .chain(self.sessions.iter().map(|s| &s.project))
            .chain(self.prefs.chats.values().filter_map(|c| c.project.as_ref()))
            .chain(
                self.agents
                    .nodes
                    .values()
                    .filter(|n| n.parent.is_none())
                    .map(|n| &n.cwd),
            )
        {
            let known_worktree = self.agents.nodes.values().any(|node| {
                node.worktree
                    .as_ref()
                    .is_some_and(|root| path.starts_with(root))
            });
            if !path.as_os_str().is_empty() && !known_worktree && seen.insert(path.clone()) {
                projects.push(path.clone());
            }
        }
        projects
    }
    pub(super) fn new_chat_project(&mut self, ui: &mut egui::Ui) {
        let active_id = self.agents.active.clone();
        let active_cloud = self
            .agents
            .nodes
            .get(&active_id)
            .and_then(|n| n.cloud_host.clone());
        let active_cloud_repo = self
            .agents
            .nodes
            .get(&active_id)
            .and_then(|n| n.cloud_repo.clone());
        let width = 240.0_f32.min(ui.available_width());

        if let Some(cloud_cfg) = active_cloud {
            ui.label(theme::small(ui, "Repository (org/repo)"));
            let fallback_name = sessions::project_name(&self.prefs.cwd);
            let mut picked_repo = None;
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                picked_repo = self.project_picker.show_cloud_repos(
                    ui,
                    &active_id,
                    &cloud_cfg,
                    active_cloud_repo.as_ref(),
                    &fallback_name,
                    width,
                );
            });
            if let Some(repo) = picked_repo {
                if let Some(node) = self.agents.nodes.get_mut(&active_id) {
                    node.cloud_repo = Some(repo);
                    node.path = None;
                }
                let ctx = self.ctx.clone();
                let draft = std::mem::take(&mut self.draft);
                let chips = std::mem::take(&mut self.context_chips);
                let images = std::mem::take(&mut self.draft_images);
                self.connect(&ctx, None);
                self.draft = draft;
                self.context_chips = chips;
                self.draft_images = images;
            }
        } else {
            ui.label(theme::small(ui, "Project"));
            let current = self.prefs.cwd.clone();
            let mut selection = None;
            let mut browse = false;
            let projects = self.chat_projects();
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                (selection, browse) = self.project_picker.show(
                    ui,
                    &self.agents.active,
                    &current,
                    width,
                    projects,
                    self.chat_folder_rx.is_none(),
                );
            });
            ui.add(
                egui::Label::new(theme::small(ui, current.display().to_string()))
                    .wrap()
                    .selectable(true),
            );
            ui.add_space(6.0);
            if ui
                .add_enabled(
                    self.chat_folder_rx.is_none(),
                    egui::Button::new("Open folder…").frame(false),
                )
                .clicked()
            {
                browse = true;
            }
            if self.chat_folder_rx.is_some() {
                ui.label(theme::small(
                    ui,
                    "Choose a folder in the file picker, or cancel to keep this project.",
                ));
            }
            if let Some(project) = selection {
                self.select_chat_project(project);
            }
            if browse {
                self.choose_chat_folder(self.prefs.cwd.clone());
            }
        }

        // Unified Environment selector (Local computer vs Cloud host) right inside New Chat!
        if let Some(cloud_cfg) = self.prefs.remote.cloud_host.clone() {
            let colors = theme::palette(ui.ctx());
            let active_id = self.agents.active.clone();
            let is_cloud = self
                .agents
                .nodes
                .get(&active_id)
                .is_some_and(|n| n.cloud_host.is_some());
            let can_switch = self.new_chat && self.deliveries.is_empty();

            ui.add_space(12.0);
            ui.label(theme::small(ui, "Environment"));
            ui.add_space(4.0);
            let mut desired_cloud = is_cloud;
            let pill_w = 240.0_f32.min(ui.available_width());
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - pill_w) / 2.0).max(0.0));
                egui::Frame::new()
                    .fill(colors.sidebar)
                    .stroke(Stroke::new(1.0_f32, colors.border))
                    .corner_radius(9)
                    .inner_margin(3)
                    .show(ui, |ui| {
                        ui.add_enabled_ui(can_switch, |ui| {
                            ui.spacing_mut().item_spacing.x = 3.0;
                            let half = ((pill_w - 12.0) * 0.5).max(80.0);
                            for (label, target_cloud) in
                                [("Local computer", false), (cloud_cfg.name.as_str(), true)]
                            {
                                let selected = is_cloud == target_cloud;
                                let (rect, resp) =
                                    ui.allocate_exact_size(Vec2::new(half, 26.0), Sense::click());
                                let fill = if selected {
                                    colors.accent.gamma_multiply(0.20)
                                } else if resp.hovered() {
                                    colors.hover
                                } else {
                                    Color32::TRANSPARENT
                                };
                                let stroke = if selected {
                                    Stroke::new(1.0_f32, colors.accent.gamma_multiply(0.50))
                                } else {
                                    Stroke::NONE
                                };
                                ui.painter()
                                    .rect(rect, 6, fill, stroke, egui::StrokeKind::Inside);
                                let dot_color = if target_cloud {
                                    if selected { colors.green } else { colors.dim }
                                } else if selected {
                                    colors.accent
                                } else {
                                    colors.dim
                                };
                                let fg = if selected { colors.text } else { colors.muted };
                                let galley = ui.painter().layout_no_wrap(
                                    label.to_owned(),
                                    FontId::proportional(11.5),
                                    fg,
                                );
                                let total_w = 11.0 + galley.size().x;
                                let start_x = rect.center().x - total_w * 0.5;
                                ui.painter().circle_filled(
                                    egui::pos2(start_x + 3.0, rect.center().y),
                                    3.0,
                                    dot_color,
                                );
                                ui.painter().galley(
                                    egui::pos2(
                                        start_x + 11.0,
                                        rect.center().y - galley.size().y * 0.5,
                                    ),
                                    galley,
                                    fg,
                                );
                                if resp
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    desired_cloud = target_cloud;
                                }
                            }
                        });
                    });
            });

            if is_cloud {
                ui.add_space(4.0);
                let repo = sessions::project_name(&self.prefs.cwd);
                ui.label(theme::small(
                    ui,
                    format!(
                        "Runs 24/7 on {} in Auto mode · isolated pui_agent/… worktree on {repo} · opens PR when done",
                        cloud_cfg.name
                    ),
                ));
            }

            if desired_cloud != is_cloud && can_switch {
                self.set_new_chat_cloud_target(desired_cloud.then_some(cloud_cfg));
            }
        }
    }

    pub(super) fn set_new_chat_cloud_target(
        &mut self,
        cloud: Option<crate::remote::CloudHostConfig>,
    ) {
        if !self.new_chat || !self.deliveries.is_empty() {
            return;
        }
        let id = self.ensure_agent();
        let is_cloud = cloud.is_some();
        if let Some(node) = self.agents.nodes.get_mut(&id) {
            node.cloud_host = cloud;
            node.path = None;
            if is_cloud {
                // Cloud agents always run in Auto mode inside an isolated pui_agent/… worktree.
                node.permission = Some(settings::PermissionMode::Auto);
            }
        }
        let ctx = self.ctx.clone();
        let draft = std::mem::take(&mut self.draft);
        let chips = std::mem::take(&mut self.context_chips);
        let images = std::mem::take(&mut self.draft_images);
        self.connect(&ctx, None);
        self.draft = draft;
        self.context_chips = chips;
        self.draft_images = images;
    }
    pub(super) fn choose_chat_folder(&mut self, initial: PathBuf) {
        if self.chat_folder_rx.is_some() || !self.new_chat {
            return;
        }
        self.chat_folder_target = Some(self.ensure_agent());
        let (tx, rx) = mpsc::channel();
        self.chat_folder_rx = Some(rx);
        #[cfg(not(test))]
        {
            let ctx = self.ctx.clone();
            std::thread::spawn(move || {
                let folder = rfd::FileDialog::new()
                    .set_title("Open a project folder")
                    .set_directory(initial)
                    .pick_folder();
                let _ = tx.send(folder);
                ctx.request_repaint();
            });
        }
        #[cfg(test)]
        {
            let _ = (tx, initial);
        }
    }
    pub(super) fn poll_chat_folder(&mut self) {
        let Some(rx) = self.chat_folder_rx.as_ref() else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => None,
        };
        self.chat_folder_rx = None;
        let target = self.chat_folder_target.take();
        // Never redirect a different chat if navigation/send happened while
        // the native picker was open. Cancellation is completely non-mutating.
        if self.new_chat && target.as_deref() == Some(&self.agents.active) {
            if let Some(folder) = result {
                self.select_chat_project(folder);
            }
        }
    }
    fn select_chat_project(&mut self, folder: PathBuf) {
        if !self.new_chat || !self.state.ready() || !self.deliveries.is_empty() {
            return;
        }
        if !folder.is_dir() {
            self.state.error =
                Some("That project folder is no longer available. Choose another folder.".into());
            return;
        }
        let folder = sync::identity(&folder);
        if folder == self.prefs.cwd {
            self.remember_project(&folder);
            return;
        }
        let context_chips = std::mem::take(&mut self.context_chips);
        let draft = std::mem::take(&mut self.draft);
        let images = std::mem::take(&mut self.draft_images);
        self.start_chat_in(folder);
        self.draft = draft;
        self.context_chips = context_chips;
        self.draft_images = images;
    }
    pub(super) fn start_chat_in(&mut self, folder: PathBuf) {
        self.start_chat_with_permission(folder, None);
    }
    pub(super) fn start_chat_with_permission(
        &mut self,
        folder: PathBuf,
        permission: Option<settings::PermissionMode>,
    ) {
        if self.deletion.in_progress() {
            return;
        }
        self.kanban.open = false;
        self.automation.open = false;
        self.tasks.open = false;
        if !folder.is_dir() {
            self.state.error =
                Some("That project folder is no longer available. Choose another folder.".into());
            return;
        }
        let folder = sync::identity(&folder);
        if !self.demo {
            self.discard_empty_new_chat();
            // Connecting/busy/approval/mirror chats keep their original state
            // and connection, including drafts, images and pending deliveries.
            if !self.agents.active.is_empty()
                || self.client.is_some()
                || self.state.session_file.is_some()
            {
                self.ensure_agent();
                self.publish_live();
                self.park_tree();
            }
        } else {
            self.demo = false;
            self.sessions.clear();
            self.usage = crate::usage::Usage::default();
            self.agents = Default::default();
        }
        self.new_chat = true;
        self.show_hidden = false;
        self.show_archived = false;
        self.prefs.cwd = folder.clone();
        self.remember_project(&folder);
        self.closed_projects.remove(&folder);
        self.workspace_rx = None;
        self.workspace = Workspace::default();
        self.diff_rx = None;
        self.inspector_panel = false;
        self.close_model_popovers();
        self.bypass_pending = None;
        self.state = State::default();
        self.draft.clear();
        self.context_chips.clear();
        self.draft_images.clear();
        self.recall = Default::default();
        let id = self.ensure_agent();
        self.agents.nodes.get_mut(&id).unwrap().permission = permission;
        self.connect(&self.ctx.clone(), None);
        self.agents.nodes.get_mut(&id).unwrap().name = "New conversation".into();
        self.enter_session_layout();
        self.focus_composer = true;
        self.refresh_sessions();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn app(ctx: &egui::Context) -> Pui {
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.connected = true;
        app.state = State {
            busy: true,
            run_observed: true,
            session_file: Some("/fixture/original".into()),
            ..Default::default()
        };
        app.client = Some(rpc::Client::stub());
        app
    }
    fn folder() -> PathBuf {
        let cwd = std::env::temp_dir().join(format!("pui-new-chat-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&cwd).unwrap();
        sync::identity(&cwd)
    }
    fn image() -> crate::media::Attachment {
        crate::media::Attachment::from_bytes(include_bytes!("../../assets/icon.png")).unwrap()
    }
    #[test]
    fn plus_opens_blank_chat_immediately_and_keeps_original_run_draft_and_approval() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.draft = "keep me".into();
        app.draft_images.set_images(vec![image()]);
        app.state
            .dialogs
            .push_back(json!({"id":"approval", "method":"confirm"}));
        let original = app.ensure_agent();
        app.new_session();
        assert!(app.new_chat);
        assert!(app.chat_folder_rx.is_none());
        assert!(app.loading_chat);
        assert!(app.pending_session.is_none());
        assert!(app.state.messages.is_empty());
        assert!(app.draft.is_empty());
        app.view_agent(&original);
        assert!(!app.new_chat);
        assert!(app.state.busy);
        assert_eq!(app.draft, "keep me");
        assert_eq!(app.state.dialogs.len(), 1);
        assert_eq!(app.draft_images.images().len(), 1);
        assert!(app.client.is_some());
        app.shutdown_agents();
    }
    #[test]
    fn command_n_opens_chat_without_native_picker_even_while_agent_is_running() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let original = app.ensure_agent();
        app.draft = "original draft".into();
        let mut window = eframe::Frame::_new_kittest();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 900.0),
                )),
                events: vec![egui::Event::Key {
                    key: egui::Key::N,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                }],
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut window),
        );
        assert!(app.new_chat);
        assert!(app.chat_folder_rx.is_none());
        assert!(app.draft.is_empty());
        app.view_agent(&original);
        assert!(app.state.busy);
        assert_eq!(app.draft, "original draft");
    }
    #[test]
    fn project_selection_and_native_result_keep_unsent_text_and_images() {
        for native in [false, true] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.new_session();
            let new = folder();
            app.draft = "start here".into();
            app.draft_images.set_images(vec![image()]);
            if native {
                let (tx, rx) = mpsc::channel();
                app.chat_folder_rx = Some(rx);
                app.chat_folder_target = Some(app.agents.active.clone());
                tx.send(Some(new.clone())).unwrap();
                app.poll_chat_folder();
            } else {
                app.select_chat_project(new.clone());
            }
            assert_eq!(app.prefs.cwd, new);
            assert!(app.new_chat);
            assert_eq!(app.draft, "start here");
            assert_eq!(app.draft_images.images().len(), 1);
            assert!(app.loading_chat);
            assert!(app.deliveries.is_empty());
            assert!(app.chat_projects().contains(&new));
        }
    }
    #[test]
    fn cancel_invalid_folder_and_stale_native_result_do_not_redirect_chats() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let original = app.ensure_agent();
        app.new_session();
        app.draft = "unchanged".into();
        let cwd = app.prefs.cwd.clone();
        let (tx, rx) = mpsc::channel();
        app.chat_folder_rx = Some(rx);
        app.chat_folder_target = Some(app.agents.active.clone());
        tx.send(None).unwrap();
        app.poll_chat_folder();
        assert_eq!(app.prefs.cwd, cwd);
        assert_eq!(app.draft, "unchanged");
        app.select_chat_project(PathBuf::from("/not/a/real/pui/project"));
        assert_eq!(app.prefs.cwd, cwd);
        assert_eq!(app.draft, "unchanged");
        let (tx, rx) = mpsc::channel();
        app.chat_folder_rx = Some(rx);
        app.chat_folder_target = Some(app.agents.active.clone());
        app.view_agent(&original);
        tx.send(Some(folder())).unwrap();
        app.poll_chat_folder();
        assert_eq!(app.state.session_file.as_deref(), Some("/fixture/original"));
        assert!(app.state.busy);
        assert_eq!(app.prefs.cwd, cwd);
    }
    #[test]
    fn blank_chat_shows_projects_while_connecting_and_only_open_folder_starts_picker() {
        fn texts(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(t) => out.push((
                    t.galley.job.text.clone(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                )),
                egui::Shape::Vec(v) => {
                    for s in v {
                        texts(s, out);
                    }
                }
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let next = folder().join("another-project");
        std::fs::create_dir(&next).unwrap();
        app.prefs.projects.push(next.clone());
        app.new_session();
        app.draft = "keep this draft".into();
        let frame = |app: &mut Pui, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| app.conversation(ctx),
            )
        };
        let labels = |app: &mut Pui| {
            for _ in 0..5 {
                frame(app, vec![]);
            }
            let mut labels = Vec::new();
            for shape in frame(app, vec![]).shapes {
                texts(&shape.shape, &mut labels);
            }
            labels
        };
        let click = |app: &mut Pui, pos| {
            for pressed in [true, false] {
                frame(
                    app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        };
        let text = labels(&mut app);
        assert!(app.loading_chat);
        assert!(text.iter().any(|(s, _)| s == "Project"));
        assert!(app.chat_folder_rx.is_none());
        let selected = sessions::project_name(&app.prefs.cwd);
        click(
            &mut app,
            text.iter()
                .find(|(s, _)| s == &selected)
                .unwrap()
                .1
                .center(),
        );
        for _ in 0..100 {
            if labels(&mut app).iter().any(|(s, _)| s == "another-project") {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let text = labels(&mut app);
        let other = text
            .iter()
            .find(|(s, _)| s == "another-project")
            .expect("Existing project choice with a separate muted path");
        click(&mut app, other.1.center());
        assert_eq!(app.prefs.cwd, next);
        assert_eq!(app.draft, "keep this draft");
        assert!(app.chat_folder_rx.is_none());
        let text = labels(&mut app);
        click(
            &mut app,
            text.iter()
                .find(|(s, _)| s == "Open folder…")
                .unwrap()
                .1
                .center(),
        );
        assert!(app.chat_folder_rx.is_some());
        assert_eq!(
            app.chat_folder_target.as_deref(),
            Some(app.agents.active.as_str())
        );
    }
    #[test]
    fn new_chats_sort_before_saved_history_and_survive_session_discovery() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let old = std::time::SystemTime::now() - Duration::from_secs(86_400 * 21);
        app.sessions = vec![Session {
            path: "/fixture/old.jsonl".into(),
            cwd: app.prefs.cwd.clone(),
            project: app.prefs.cwd.clone(),
            title: "Old conversation".into(),
            created: Some(old),
            modified: old,
        }];
        app.new_session();
        let id = app.agents.active.clone();
        for discovered in [false, true] {
            if discovered {
                let path = PathBuf::from("/fixture/new.jsonl");
                app.state.session_file = Some(path.display().to_string());
                app.agents.nodes.get_mut(&id).unwrap().path = Some(path.clone());
                app.sessions.push(Session {
                    path,
                    cwd: app.prefs.cwd.clone(),
                    project: app.prefs.cwd.clone(),
                    title: "New conversation".into(),
                    created: None,
                    modified: old,
                });
            }
            let rows = sidebar_tree::forest(&app);
            let newest = rows.iter().find(|row| row.active(&app)).unwrap();
            assert!(newest.created.unwrap() > old);
            assert!(newest.modified > old);
            assert!(newest.activity.unwrap() > old);
            for sort in [sidebar_order::Sort::Activity, sidebar_order::Sort::Created] {
                let mut sorted: Vec<_> = rows.iter().collect();
                sorted.sort_by(|a, b| {
                    sidebar_order::Options {
                        sort,
                        reverse: false,
                    }
                    .compare(a, b)
                });
                assert_eq!(sorted[0].key, newest.key);
            }
        }
    }
    #[test]
    fn abandoned_blank_chats_are_removed_on_new_chat_and_navigation() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let original = app.ensure_agent();
        for _ in 0..3 {
            app.new_session();
        }
        assert_eq!(app.agents.nodes.len(), 2);
        let blank = app.agents.active.clone();
        app.view_agent(&original);
        assert!(!app.agents.nodes.contains_key(&blank));
        assert!(!app.agents.parked.contains_key(&blank));
        assert_eq!(app.agents.nodes.len(), 1);
        assert!(app.state.busy);
    }
    #[test]
    fn new_chat_reveals_top_of_scrolled_history_and_exits_archive() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.projects = false;
        let old = std::time::SystemTime::now() - Duration::from_secs(86_400);
        app.sessions = (0..80)
            .map(|index| Session {
                path: format!("/fixture/history-{index}.jsonl").into(),
                cwd: app.prefs.cwd.clone(),
                project: app.prefs.cwd.clone(),
                title: format!("Older conversation {index}"),
                created: Some(old),
                modified: old,
            })
            .collect();
        for _ in 0..3 {
            files::tests::frame(&ctx, &mut app, vec![]);
        }
        let pos = egui::pos2(100.0, 500.0);
        files::tests::frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -3000.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..15 {
            files::tests::frame(&ctx, &mut app, vec![]);
        }
        app.show_archived = true;
        app.new_session();
        assert!(!app.show_archived);
        assert!(!app.show_hidden);
        for _ in 0..5 {
            files::tests::frame(&ctx, &mut app, vec![]);
        }
        let output = files::tests::frame(&ctx, &mut app, vec![]);
        let new = files::tests::text(&output, "New conversation");
        assert!(new.x < 300.0 && new.y < 450.0, "{new:?}");
    }
    #[test]
    fn allocated_empty_chat_file_is_cleaned_up_without_deleting_saved_messages() {
        for has_message in [false, true] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            let original = app.ensure_agent();
            app.new_session();
            let blank = app.agents.active.clone();
            let path = folder().join("allocated.jsonl");
            let mut content = "{\"type\":\"session\",\"cwd\":\"/fixture\"}\n".to_string();
            if has_message {
                content.push_str("{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"Keep this\"}}\n");
            }
            std::fs::write(&path, &content).unwrap();
            app.state.session_file = Some(path.display().to_string());
            app.agents.nodes.get_mut(&blank).unwrap().path = Some(path.clone());
            // No history request is pending: this is a newly allocated path,
            // not an existing session whose transcript is still loading.
            app.view_agent(&original);
            assert!(!app.agents.nodes.contains_key(&blank));
            assert_eq!(app.agents.active, original);
            for _ in 0..200 {
                app.poll_deletions();
                if !app.deletion.deleting_chat(&path) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(!app.deletion.deleting_chat(&path));
            assert_eq!(path.exists(), has_message);
            assert!(app.state.busy);
            if has_message {
                assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
                assert!(app.state.notice.as_deref().unwrap().contains("kept"));
            }
        }
    }
    #[test]
    fn navigating_from_blank_to_unopened_saved_chat_does_not_park_blank() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.new_session();
        let blank = app.agents.active.clone();
        app.open_chat(Session {
            path: "/fixture/other.jsonl".into(),
            cwd: app.prefs.cwd.clone(),
            project: app.prefs.cwd.clone(),
            title: "Other chat".into(),
            created: None,
            modified: std::time::SystemTime::now(),
        });
        assert!(!app.agents.nodes.contains_key(&blank));
        assert!(!app.agents.parked.contains_key(&blank));
        assert_eq!(
            app.state.session_file.as_deref(),
            Some("/fixture/other.jsonl")
        );
    }
    #[test]
    fn empty_new_chats_with_drafts_or_pending_work_are_preserved() {
        for case in 0..10 {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            let original = app.ensure_agent();
            app.new_session();
            app.loading_chat = false;
            app.connected = true;
            let blank = app.agents.active.clone();
            match case {
                0 => app.draft = "Unsent draft".into(),
                1 => app.draft_images.set_images(vec![image()]),
                2 => app.state.busy = true,
                3 => app
                    .state
                    .dialogs
                    .push_back(json!({"id":"approval", "method":"confirm"})),
                4 => app.state.messages.push(protocol::Message::from_json(
                    &json!({"role":"user", "content":"Keep me"}),
                )),
                5 => app.new_chat = false, // Existing chat with an empty active branch.
                6 => app.state.pending = true,
                7 => app.pending_session = Some("/fixture/loading.jsonl".into()),
                8 => app.state.session_name = "Named empty conversation".into(),
                9 => {
                    app.state.session_file = Some("/fixture/pinned.jsonl".into());
                    app.prefs
                        .chats
                        .entry("/fixture/pinned.jsonl".into())
                        .or_default()
                        .pinned = true;
                }
                _ => unreachable!(),
            }
            app.view_agent(&original);
            assert!(app.agents.nodes.contains_key(&blank), "case {case}");
            assert!(app.agents.parked.contains_key(&blank), "case {case}");
        }
    }
    #[test]
    fn existing_projects_are_unique_and_include_same_named_folder_paths() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let base = folder();
        let a = base.join("a/project");
        let b = base.join("b/project");
        app.prefs.projects = vec![a.clone(), b.clone(), a.clone()];
        let projects = app.chat_projects();
        assert_eq!(projects.iter().filter(|p| **p == a).count(), 1);
        assert!(projects.contains(&b));
    }
}
