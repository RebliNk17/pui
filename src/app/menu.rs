//! Native macOS menus, with an in-app fallback on Linux/Windows.
use super::*;
#[path = "menu_entries.rs"]
mod entries;
use entries::{Command, Entry, MenuState};
pub(super) struct Menu {
    #[cfg(all(target_os = "macos", not(test)))]
    native: Option<macos::Native>,
    pub about: bool,
    pending: Vec<Command>,
}
impl Menu {
    pub fn new(ctx: &egui::Context) -> Self {
        #[cfg(not(all(target_os = "macos", not(test))))]
        let _ = ctx;
        Self {
            #[cfg(all(target_os = "macos", not(test)))]
            native: macos::Native::new(ctx),
            about: false,
            pending: Vec::new(),
        }
    }
    fn take(&mut self) -> Vec<Command> {
        let commands = std::mem::take(&mut self.pending);
        #[cfg(all(target_os = "macos", not(test)))]
        if let Some(native) = &self.native {
            return commands.into_iter().chain(native.rx.try_iter()).collect();
        }
        commands
    }
}
fn menu_items(
    ui: &mut egui::Ui,
    entries: &[Entry],
    state: MenuState,
    selected: &mut Option<Command>,
) {
    theme::menu_controls(ui);
    let mut separated = true;
    let mut separator_pending = false;
    for entry in entries.iter().filter(|e| e.supported()) {
        if matches!(entry, Entry::Separator) {
            separator_pending = !separated;
            continue;
        }
        if separator_pending {
            ui.separator();
            separator_pending = false;
        }
        separated = false;
        match entry {
            Entry::Action(label, action) => {
                let mut button = egui::Button::new(*label);
                if let Some(checked) = state.checked(*action) {
                    button = button.selected(checked);
                }
                if let Some(shortcut) = action.shortcut() {
                    button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
                }
                if ui.add_enabled(state.enabled(*action), button).clicked() {
                    *selected = Some(*action);
                    ui.close();
                }
            }
            Entry::Submenu(label, children) => {
                ui.menu_button(*label, |ui| menu_items(ui, children, state, selected));
            }
            Entry::Separator => unreachable!(),
        }
    }
}
fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
impl Pui {
    pub(super) fn menu_commands(&mut self, ctx: &egui::Context) {
        for command in self.menu.take() {
            self.menu_command(ctx, command);
        }
        // Native accelerators are consumed by AppKit. These also make the
        // in-app fallback work where there is no global application menubar.
        if !cfg!(target_os = "macos") && !self.settings_open && self.terminals.focused(ctx) {
            return;
        }
        for (modifiers, key, command) in [
            (
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::N,
                Command::NewWindow,
            ),
            (egui::Modifiers::COMMAND, egui::Key::O, Command::OpenProject),
            (egui::Modifiers::COMMAND, egui::Key::W, Command::Close),
            (egui::Modifiers::COMMAND, egui::Key::Q, Command::Quit),
        ] {
            if ctx.input_mut(|i| i.consume_key(modifiers, key)) {
                self.menu_command(ctx, command);
            }
        }
    }
    fn menu_state(&self, ctx: &egui::Context) -> MenuState {
        let text = ctx
            .memory(|m| m.focused())
            .and_then(|id| egui::text_edit::TextEditState::load(ctx, id));
        MenuState {
            theme: self.prefs.theme,
            alerts: self.prefs.notifications,
            delivery: self.prefs.settings.delivery,
            sidebar: !self.prefs.hide_sidebar,
            inspector: !self.prefs.hide_inspector && !self.files.is_open() && !self.inspector_panel,
            inspector_available: ctx.screen_rect().width() > 1110.0,
            files: self.files.files_open(),
            browser: self.files.browser_open(),
            changes: self.inspector_panel,
            terminal: self.terminals.is_open(),
            terminal_focused: !self.settings_open && self.terminals.focused(ctx),
            text_focused: text.is_some(),
            text_selected: text
                .as_ref()
                .and_then(|s| s.cursor.char_range())
                .is_some_and(|r| !r.is_empty()),
            fullscreen: ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)),
            zoom: ctx.zoom_factor(),
        }
    }
    pub(super) fn refresh_menu(&mut self, ctx: &egui::Context) {
        #[cfg(all(target_os = "macos", not(test)))]
        {
            let state = self.menu_state(ctx);
            if let Some(native) = &mut self.menu.native {
                native.refresh(state);
            }
        }
        #[cfg(not(all(target_os = "macos", not(test))))]
        let _ = ctx;
    }
    fn menu_command(&mut self, ctx: &egui::Context, command: Command) {
        if !self.menu_state(ctx).enabled(command) {
            return;
        }
        use Command::*;
        match command {
            About => self.menu.about = true,
            Updates => self.updates.start(ctx),
            Settings => {
                self.settings_open = true;
                self.model_picker_open = false;
            }
            Shortcuts => {
                self.settings_view.select("Keyboard shortcuts");
                self.settings_open = true;
            }
            NewChat => {
                self.settings_open = false;
                self.files.close();
                self.new_session();
            }
            OpenProject => {
                self.settings_open = false;
                self.files.close();
                if !self.new_chat {
                    self.new_session();
                }
                self.choose_chat_folder(self.prefs.cwd.clone());
            }
            NewWindow => {
                #[cfg(not(test))]
                match std::env::current_exe().and_then(|exe| {
                    let mut command = std::process::Command::new(exe);
                    command
                        .arg("--cwd")
                        .arg(&self.prefs.cwd)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null());
                    command.spawn()
                }) {
                    Ok(mut child) => {
                        std::thread::spawn(move || {
                            let _ = child.wait();
                        });
                    }
                    Err(_) => self.state.error = Some("Could not open a new Pui window.".into()),
                }
            }
            Close | Quit => {
                // Feed the normal close-request path, including active-agent
                // confirmation. Never terminate NSApplication behind eframe.
                ctx.input_mut(|i| {
                    i.raw
                        .viewports
                        .entry(egui::ViewportId::ROOT)
                        .or_default()
                        .events
                        .push(egui::ViewportEvent::Close)
                });
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Undo | Redo | Cut | Copy | Paste | SelectAll => {
                let event = match command {
                    Undo => key_event(egui::Key::Z, egui::Modifiers::COMMAND),
                    Redo => key_event(
                        egui::Key::Z,
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    ),
                    Cut => egui::Event::Cut,
                    Copy => egui::Event::Copy,
                    SelectAll => key_event(egui::Key::A, egui::Modifiers::COMMAND),
                    Paste => {
                        if !self.settings_open
                            && ctx.memory(|m| m.has_focus(egui::Id::new("composer")))
                        {
                            #[cfg(target_os = "macos")]
                            if attachments::clipboard_has_image() {
                                self.paste_requested
                                    .store(true, std::sync::atomic::Ordering::Relaxed);
                                return;
                            }
                        }
                        match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                            Ok(text) => egui::Event::Paste(text),
                            Err(_) => return,
                        }
                    }
                    _ => unreachable!(),
                };
                ctx.input_mut(|i| i.events.push(event));
            }
            Search => {
                self.settings_open = false;
                self.prefs.hide_sidebar = false;
                self.focus_search = true;
                self.focus_composer = false;
            }
            DarkTheme | LightTheme => {
                self.prefs.theme = if command == DarkTheme {
                    theme::Theme::Dark
                } else {
                    theme::Theme::Light
                };
                self.prefs.theme.apply(ctx);
                ctx.request_repaint();
            }
            CompletionSound | DesktopNotification => {
                let before = self.prefs.notifications;
                if command == CompletionSound {
                    self.prefs.notifications.sound = !before.sound;
                } else {
                    self.prefs.notifications.desktop = !before.desktop;
                }
                self.propagate_notifications(ctx, before);
            }
            Steer | Queue => {
                let before = self.prefs.settings.clone();
                self.prefs.settings.delivery = if command == Steer {
                    settings::DeliveryMode::Steer
                } else {
                    settings::DeliveryMode::Queue
                };
                self.propagate_settings(&before);
            }
            Sidebar => self.prefs.hide_sidebar = !self.prefs.hide_sidebar,
            Inspector => {
                self.kanban.open = false;
                let shown = self.menu_state(ctx).inspector;
                self.files.close();
                self.inspector_panel = false;
                self.prefs.hide_inspector = shown;
            }
            Terminal => {
                self.kanban.open = false;
                self.settings_open = false;
                self.terminals.toggle(ctx, &self.prefs.cwd, self.demo);
                self.focus_composer = !self.terminals.is_open();
            }
            Files => {
                self.kanban.open = false;
                self.settings_open = false;
                self.files.toggle(ctx, &self.prefs.cwd);
                self.inspector_panel = false;
            }
            Browser => {
                self.kanban.open = false;
                self.settings_open = false;
                self.files.toggle_browser(ctx, &self.prefs.cwd);
                self.inspector_panel = false;
                self.terminals.close(ctx);
            }
            Changes => {
                self.kanban.open = false;
                self.settings_open = false;
                self.files.close();
                self.inspector_panel = !self.inspector_panel;
                self.prefs.hide_inspector = false;
                if self.inspector_panel {
                    self.changes.invalidate();
                    self.refresh_workspace();
                }
            }
            ZoomReset => ctx.set_zoom_factor(1.0),
            ZoomIn => ctx.set_zoom_factor((ctx.zoom_factor() + 0.1).min(1.4)),
            ZoomOut => ctx.set_zoom_factor((ctx.zoom_factor() - 0.1).max(0.8)),
            Fullscreen => ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(
                !ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)),
            )),
            Minimize => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
            Maximize => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(
                !ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
            )),
        }
    }
    pub(super) fn app_menu(&mut self, ui: &mut egui::Ui) {
        let mut command = None;
        let state = self.menu_state(ui.ctx());
        ui.menu_button("Pui", |ui| {
            menu_items(ui, entries::APP, state, &mut command);
            ui.separator();
            for (title, items) in entries::GROUPS {
                ui.menu_button(*title, |ui| menu_items(ui, items, state, &mut command));
            }
            ui.separator();
            menu_items(
                ui,
                &[Entry::Action("Quit Pui", Command::Quit)],
                state,
                &mut command,
            );
        });
        if let Some(command) = command {
            // Execute at the start of the next frame, before the close guard
            // and text editing, rather than closing behind an active run.
            self.menu.pending.push(command);
            ui.ctx().request_repaint();
        }
    }
    pub(super) fn menu_windows(&mut self, ctx: &egui::Context) {
        self.updates.show(ctx);
        about::show(ctx, &mut self.menu.about);
    }
}

#[cfg(all(target_os = "macos", not(test)))]
#[path = "menu_macos.rs"]
mod macos;
#[cfg(test)]
mod tests {
    use super::*;
    fn app(ctx: &egui::Context) -> Pui {
        Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        )
    }
    fn actions(entries: &[Entry], out: &mut Vec<Command>) {
        for entry in entries.iter().filter(|e| e.supported()) {
            match entry {
                Entry::Action(_, command) => out.push(*command),
                Entry::Submenu(_, children) => actions(children, out),
                Entry::Separator => {}
            }
        }
    }
    #[test]
    fn menus_expose_supported_actions_only_and_share_one_definition() {
        let mut exposed = Vec::new();
        actions(entries::APP, &mut exposed);
        for (_, entries) in entries::GROUPS {
            actions(entries, &mut exposed);
        }
        assert_eq!(exposed.contains(&Command::Updates), updates::configured());
        assert_eq!(
            exposed.contains(&Command::CompletionSound),
            notifications::Notifications::supported()
        );
        for command in [
            Command::NewChat,
            Command::OpenProject,
            Command::LightTheme,
            Command::DarkTheme,
            Command::Steer,
            Command::Queue,
            Command::Terminal,
        ] {
            assert!(exposed.contains(&command));
        }
        exposed.sort_by_key(|c| *c as isize);
        exposed.dedup();
        assert_eq!(
            exposed.len(),
            Command::ALL
                .iter()
                .filter(|c| c.supported() && **c != Command::Quit)
                .count()
        );
        if !updates::configured() {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.menu_command(&ctx, Command::Updates);
            assert!(!app.updates.open);
        }
    }
    #[test]
    fn quick_settings_are_live_checked_persisted_and_leave_active_chat_untouched() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.state.busy = true;
        app.draft = "Keep this unsent message".into();
        app.settings_open = true;
        app.menu_command(&ctx, Command::LightTheme);
        assert_eq!(app.prefs.theme, theme::Theme::Light);
        assert!(!ctx.style().visuals.dark_mode);
        assert_eq!(
            app.menu_state(&ctx).checked(Command::LightTheme),
            Some(true)
        );
        assert_eq!(
            app.menu_state(&ctx).checked(Command::DarkTheme),
            Some(false)
        );
        app.menu_command(&ctx, Command::Queue);
        assert_eq!(app.prefs.settings.delivery.label(), "Queue");
        assert_eq!(app.menu_state(&ctx).checked(Command::Queue), Some(true));
        let sound = app.prefs.notifications.sound;
        app.menu_command(&ctx, Command::CompletionSound);
        if notifications::Notifications::supported() {
            assert_eq!(app.prefs.notifications.sound, !sound);
        }
        let persisted: Preferences =
            serde_json::from_value(serde_json::to_value(&app.prefs).unwrap()).unwrap();
        assert_eq!(persisted.theme, theme::Theme::Light);
        assert_eq!(persisted.settings.delivery.label(), "Queue");
        assert!(persisted.notifications == app.prefs.notifications);
        // Settings/composer/mirror updates are reflected, not stale copies held by the menu.
        app.prefs.theme = theme::Theme::Dark;
        app.prefs.settings.delivery = settings::DeliveryMode::Steer;
        assert_eq!(app.menu_state(&ctx).checked(Command::DarkTheme), Some(true));
        assert_eq!(app.menu_state(&ctx).checked(Command::Steer), Some(true));
        assert!(app.settings_open);
        assert!(app.state.busy);
        assert_eq!(app.draft, "Keep this unsent message");
        assert!(app.deliveries.is_empty());
    }
    #[test]
    fn in_app_theme_choice_uses_the_same_command_path() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let mut selected = None;
        let Entry::Submenu(_, themes) = &entries::GROUPS
            .iter()
            .find(|(name, _)| *name == "View")
            .unwrap()
            .1[0]
        else {
            panic!("Missing theme menu")
        };
        let mut frame = |app: &mut Pui, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 650.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    let state = app.menu_state(ctx);
                    egui::CentralPanel::default()
                        .show(ctx, |ui| menu_items(ui, themes, state, &mut selected));
                },
            )
        };
        let output = frame(&mut app, vec![]);
        let point = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == "Light" => {
                    Some(t.pos + egui::vec2(3.0, 3.0))
                }
                _ => None,
            })
            .unwrap();
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(selected, Some(Command::LightTheme));
        app.menu.pending.push(selected.unwrap());
        app.menu_commands(&ctx);
        assert_eq!(app.prefs.theme, theme::Theme::Light);
    }
    #[test]
    fn inspector_menu_opens_details_from_files_and_disables_when_too_narrow() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| {
                app.prefs.hide_inspector = false;
                app.files.toggle(ctx, &app.prefs.cwd);
                assert!(app.files.is_open());
                app.menu_command(ctx, Command::Inspector);
                assert!(!app.files.is_open());
                assert!(!app.prefs.hide_inspector);
                assert_eq!(app.menu_state(ctx).checked(Command::Inspector), Some(true));
                app.menu_command(ctx, Command::Inspector);
                assert!(app.prefs.hide_inspector);
            },
        );
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 650.0),
                )),
                ..Default::default()
            },
            |ctx| {
                assert!(!app.menu_state(ctx).enabled(Command::Inspector));
                app.menu_command(ctx, Command::Inspector);
                assert!(app.prefs.hide_inspector);
            },
        );
    }
    #[test]
    fn native_edit_commands_reach_the_focused_composer() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.state = State::default();
        app.focus_composer = true;
        let mut elapsed = 0.0;
        let mut frame = |app: &mut Pui, command: Option<Command>, events| {
            elapsed += 1.0;
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 650.0),
                    )),
                    time: Some(elapsed),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    if let Some(command) = command {
                        app.menu_command(ctx, command);
                    }
                    egui::CentralPanel::default().show(ctx, |ui| app.composer(ui, ctx));
                },
            )
        };
        frame(&mut app, None, vec![]);
        frame(&mut app, None, vec![egui::Event::Text("hello".into())]);
        assert_eq!(app.draft, "hello");
        frame(&mut app, None, vec![]);
        frame(&mut app, Some(Command::SelectAll), vec![]);
        let output = frame(&mut app, Some(Command::Copy), vec![]);
        assert!(
            output
                .platform_output
                .commands
                .iter()
                .any(|c| matches!(c,egui::OutputCommand::CopyText(t) if t=="hello"))
        );
        frame(&mut app, Some(Command::Cut), vec![]);
        assert!(app.draft.is_empty());
        frame(&mut app, Some(Command::Undo), vec![]);
        assert_eq!(app.draft, "hello");
        frame(&mut app, Some(Command::Redo), vec![]);
        assert!(app.draft.is_empty());
    }
    #[test]
    fn native_menu_commands_use_shared_actions_and_close_guard() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.state.busy = true;
        app.draft = "keep draft".into();
        app.menu_command(&ctx, Command::Settings);
        assert!(app.settings_open);
        assert_eq!(app.draft, "keep draft");
        assert!(app.state.busy);
        app.menu_command(&ctx, Command::ZoomReset);
        assert_eq!(ctx.zoom_factor(), 1.0);
        let _ = ctx.run(Default::default(), |ctx| {
            app.menu_command(ctx, Command::Quit);
            app.agent_close_guard(ctx);
        });
        assert!(app.agents.close_prompt);
        assert!(app.state.busy);
        assert!(matches!(Command::from_tag(1), Some(Command::About)));
        assert!(Command::from_tag(999).is_none());
    }
}
