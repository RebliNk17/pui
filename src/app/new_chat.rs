//! New chats open immediately. Native folder selection is an explicit action
//! inside the blank chat, never the side effect of pressing +.
use super::*;

impl Pui {
    fn remember_project(&mut self, folder: &std::path::Path) {
        self.prefs.projects.retain(|p| p != folder);
        self.prefs.projects.insert(0, folder.to_path_buf());
        self.prefs.projects.truncate(32);
    }
    fn chat_projects(&self) -> Vec<PathBuf> {
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
            if !path.as_os_str().is_empty() && seen.insert(path.clone()) {
                projects.push(path.clone());
            }
        }
        projects
    }
    pub(super) fn new_chat_project(&mut self, ui: &mut egui::Ui) {
        ui.label(theme::small(ui, "Project"));
        let current = self.prefs.cwd.clone();
        let mut selection = None;
        let mut browse = false;
        let width = 240.0_f32.min(ui.available_width());
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            egui::ComboBox::from_id_salt("new-chat-project")
                .selected_text(sessions::project_name(&current))
                .width(width)
                .show_ui(ui, |ui| {
                    ui.set_max_width(420.0);
                    for project in self.chat_projects() {
                        let label = format!(
                            "{}\n{}",
                            sessions::project_name(&project),
                            project.display()
                        );
                        if ui
                            .selectable_label(project == current, RichText::new(label).size(11.5))
                            .on_hover_text(project.display().to_string())
                            .clicked()
                        {
                            selection = Some(project);
                        }
                    }
                    ui.separator();
                    if ui.button("Open folder…").clicked() {
                        browse = true;
                        ui.close();
                    }
                });
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
        let draft = std::mem::take(&mut self.draft);
        let images = std::mem::take(&mut self.draft_images);
        self.start_chat_in(folder);
        self.draft = draft;
        self.draft_images = images;
    }
    pub(super) fn start_chat_in(&mut self, folder: PathBuf) {
        if self.deletion.in_progress() {
            return;
        }
        self.tasks.open = false;
        if !folder.is_dir() {
            self.state.error =
                Some("That project folder is no longer available. Choose another folder.".into());
            return;
        }
        let folder = sync::identity(&folder);
        let models = self.state.models.clone();
        if !self.demo {
            // Connecting/busy/approval/mirror chats keep their original state
            // and connection, including drafts, images and pending deliveries.
            self.ensure_agent();
            self.publish_live();
            self.park_tree();
        } else {
            self.demo = false;
            self.sessions.clear();
            self.usage = crate::usage::Usage::default();
            self.agents = Default::default();
        }
        self.new_chat = true;
        self.prefs.cwd = folder.clone();
        self.remember_project(&folder);
        self.closed_projects.remove(&folder);
        self.workspace_rx = None;
        self.workspace = Workspace::default();
        self.selected_diff = None;
        self.diff_rx = None;
        self.inspector_panel = false;
        self.model_picker_open = false;
        self.effort_draft = None;
        self.bypass_pending = None;
        self.state = State {
            models,
            ..Default::default()
        };
        self.draft.clear();
        self.draft_images.clear();
        self.recall = Default::default();
        self.connect(&self.ctx.clone(), None);
        let id = self.ensure_agent();
        self.agents.nodes.get_mut(&id).unwrap().name = "New conversation".into();
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
        let text = labels(&mut app);
        let other = text
            .iter()
            .find(|(s, _)| s.starts_with("another-project\n"))
            .expect("Existing project choice with its path");
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
