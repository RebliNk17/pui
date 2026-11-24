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
#[cfg(any(not(target_os = "macos"), test))]
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
                if let Some(shortcut) = action.contextual_shortcut(state.composer_focused) {
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
            transcript_view: self.prefs.transcript_view,
            alerts: self.prefs.notifications,
            delivery: self.prefs.settings.delivery,
            sidebar: !self.prefs.hide_sidebar,
            agents_board: !self.settings_open && self.kanban.open,
            automation: !self.settings_open && self.automation.open,
            inspector: !self.prefs.hide_inspector && !self.files.is_open() && !self.inspector_panel,
            inspector_available: ctx.screen_rect().width() > 1110.0,
            files: self.files.files_open(),
            browser: self.files.browser_open(),
            changes: self.inspector_panel,
            terminal: self.terminals.is_open(),
            terminal_focused: !self.settings_open && self.terminals.focused(ctx),
            text_focused: text.is_some(),
            composer_focused: self.composer_formatting_available(ctx)
                || self.side_composer_formatting_available(ctx),
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
            AgentsBoard => {
                self.settings_open = false;
                self.open_agents_board();
            }
            Automation => {
                self.settings_open = false;
                self.open_automation();
            }
            About => self.menu.about = true,
            Updates => self.updates.start(ctx),
            Settings => {
                self.settings_open = true;
                self.close_model_popovers();
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
            Close if !self.settings_open && self.files.browser_open() => {
                self.files.close_active_browser_tab();
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
                            && (ctx.memory(|m| m.has_focus(egui::Id::new("composer")))
                                || self.side_composer_formatting_available(ctx))
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
            Find => self.open_chat_find(),
            Search => self.open_search(),
            QuickOpen => self.open_quick_files(),
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
            Bold | Italic | Underline => {
                let format = match command {
                    Bold => composer_text::Format::Bold,
                    Italic => composer_text::Format::Italic,
                    _ => composer_text::Format::Underline,
                };
                if !self.format_side_composer(ctx, format) {
                    self.context_chips.clear();
                    composer_text::format(ctx, &mut self.draft, format);
                }
            }
            TranscriptNormal => self.prefs.transcript_view = crate::transcript::View::Normal,
            TranscriptVerbose => self.prefs.transcript_view = crate::transcript::View::Verbose,
            Sidebar => self.prefs.hide_sidebar = !self.prefs.hide_sidebar,
            Inspector => {
                self.kanban.open = false;
                self.automation.open = false;
                let shown = self.menu_state(ctx).inspector;
                self.files.close();
                self.inspector_panel = false;
                self.prefs.hide_inspector = shown;
            }
            Terminal => {
                self.kanban.open = false;
                self.automation.open = false;
                self.settings_open = false;
                self.terminals.toggle(ctx, &self.prefs.cwd, self.demo);
                self.focus_composer = !self.terminals.is_open();
            }
            Files => {
                self.diff_rx = None;
                self.kanban.open = false;
                self.automation.open = false;
                self.settings_open = false;
                self.files.toggle(ctx, &self.prefs.cwd);
                self.inspector_panel = false;
            }
            Browser => {
                self.diff_rx = None;
                self.kanban.open = false;
                self.automation.open = false;
                self.settings_open = false;
                self.files.toggle_browser(ctx, &self.prefs.cwd);
                self.inspector_panel = false;
            }
            Changes => {
                self.kanban.open = false;
                self.automation.open = false;
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
    pub(super) fn sidebar_navigation(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        let state = self.menu_state(ui.ctx());
        let active_tasks = self.automation.active_count();
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (label, icon, command, selected) in [
                (
                    "Agents board",
                    Icon::Board,
                    Command::AgentsBoard,
                    state.agents_board,
                ),
                (
                    "Scheduled tasks",
                    Icon::Clock,
                    Command::Automation,
                    state.automation,
                ),
                (
                    "Settings",
                    Icon::Settings,
                    Command::Settings,
                    self.settings_open,
                ),
            ] {
                if command == Command::Automation && !self.prefs.sidebar_view.scheduled {
                    continue;
                }
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), theme::COMPACT_HEIGHT),
                    Sense::click(),
                );
                let highlighted = selected || response.hovered() || response.has_focus();
                if highlighted {
                    ui.painter().rect_filled(rect, 7, colors.hover);
                }
                let color = if highlighted {
                    colors.text
                } else {
                    colors.muted
                };
                icons::draw(
                    ui,
                    egui::Rect::from_center_size(
                        rect.left_center() + egui::vec2(16.0, 0.0),
                        Vec2::splat(16.0),
                    ),
                    icon,
                    color,
                );
                let badge = (command == Command::Automation && active_tasks > 0).then(|| {
                    ui.painter().layout_no_wrap(
                        format!("{active_tasks} active"),
                        FontId::proportional(11.0),
                        colors.green,
                    )
                });
                let label_right = if let Some(badge) = badge {
                    let pos = egui::pos2(
                        rect.right() - badge.size().x - 10.0,
                        rect.center().y - badge.size().y / 2.0,
                    );
                    let left = pos.x - 8.0;
                    ui.painter().galley(pos, badge, colors.green);
                    left
                } else {
                    rect.right() - 10.0
                };
                let label_pos = rect.left_center() + egui::vec2(34.0, 0.0);
                let mut layout = egui::text::LayoutJob::simple(
                    label.into(),
                    FontId::proportional(13.0),
                    color,
                    (label_right - label_pos.x).max(0.0),
                );
                layout.wrap.max_rows = 1;
                let galley = ui.painter().layout_job(layout);
                ui.painter().galley(
                    label_pos - egui::vec2(0.0, galley.size().y / 2.0),
                    galley,
                    color,
                );
                response.widget_info(|| {
                    let label = if command == Command::Automation && active_tasks > 0 {
                        format!("{label}, {active_tasks} active")
                    } else {
                        label.into()
                    };
                    egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
                });
                if response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    // Use the same dispatch as the native menu, before next frame's
                    // close guard and text editing. Never interrupt a running chat.
                    self.menu.pending.push(command);
                    ui.ctx().request_repaint();
                }
            }
            if let Some(version) = self.updates.available_version() {
                let ready = self.updates.ready_to_install();
                let label = if ready {
                    format!("Install Pui {version}")
                } else {
                    format!("Update to {version}")
                };
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), theme::COMPACT_HEIGHT),
                    Sense::click(),
                );
                let highlighted = self.updates.open || response.hovered() || response.has_focus();
                if highlighted {
                    ui.painter().rect_filled(rect, 7, colors.hover);
                }
                let color = if ready { colors.green } else { colors.accent };
                icons::draw(
                    ui,
                    egui::Rect::from_center_size(
                        rect.left_center() + egui::vec2(16.0, 0.0),
                        Vec2::splat(16.0),
                    ),
                    Icon::Refresh,
                    color,
                );
                let label_pos = rect.left_center() + egui::vec2(34.0, 0.0);
                let mut layout = egui::text::LayoutJob::simple(
                    label.clone(),
                    FontId::proportional(13.0),
                    color,
                    (rect.right() - 10.0 - label_pos.x).max(0.0),
                );
                layout.wrap.max_rows = 1;
                let galley = ui.painter().layout_job(layout);
                ui.painter().galley(
                    label_pos - egui::vec2(0.0, galley.size().y / 2.0),
                    galley,
                    color,
                );
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        true,
                        self.updates.open,
                        &label,
                    )
                });
                if response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    self.menu.pending.push(Command::Updates);
                    ui.ctx().request_repaint();
                }
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    pub(super) fn fallback_menu_bar(&mut self, ctx: &egui::Context) {
        let mut command = None;
        let state = self.menu_state(ctx);
        egui::TopBottomPanel::top("application-menu-bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("Pui", |ui| {
                    menu_items(ui, entries::APP, state, &mut command);
                    ui.separator();
                    menu_items(
                        ui,
                        &[Entry::Action("Quit Pui", Command::Quit)],
                        state,
                        &mut command,
                    );
                });
                for (title, items) in entries::GROUPS {
                    ui.menu_button(*title, |ui| menu_items(ui, items, state, &mut command));
                }
            });
        });
        if let Some(command) = command {
            self.menu.pending.push(command);
            ctx.request_repaint();
        }
    }
    pub(super) fn menu_windows(&mut self, ctx: &egui::Context) {
        self.updates
            .show_with_preference(ctx, &mut self.prefs.settings.auto_check_updates);
        about::show(ctx, &mut self.menu.about);
        self.resource_reload_dialog(ctx);
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
            Command::AgentsBoard,
            Command::Automation,
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
    fn quick_open_menu_uses_search_permission_gate_and_shared_accelerator() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        assert_eq!(
            Command::QuickOpen.shortcut(),
            Some(egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::P
            ))
        );
        app.state.dialogs.push_back(questions::demo_dialog());
        app.menu_command(&ctx, Command::QuickOpen);
        assert!(!app.search_open);
        app.state.dialogs.clear();
        app.menu_command(&ctx, Command::QuickOpen);
        assert!(app.search_open);
    }
    #[test]
    fn overview_menu_actions_open_views_without_interrupting_chat() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.state.busy = true;
        app.draft = "Keep this draft".into();
        app.settings_open = true;
        app.state.dialogs.push_back(questions::demo_dialog());
        for command in [
            Command::AgentsBoard,
            Command::Automation,
            Command::AgentsBoard,
        ] {
            app.menu.pending.push(command);
            app.menu_commands(&ctx);
            assert!(!app.settings_open);
            assert_eq!(app.kanban.open, command == Command::AgentsBoard);
            assert_eq!(app.automation.open, command == Command::Automation);
            assert_eq!(app.menu_state(&ctx).checked(command), Some(true));
            assert!(app.state.busy);
            assert_eq!(app.draft, "Keep this draft");
            assert_eq!(app.state.dialogs.len(), 1);
            assert!(app.deliveries.is_empty());
            let mut view_actions = Vec::new();
            actions(
                entries::GROUPS
                    .iter()
                    .find(|(name, _)| *name == "View")
                    .unwrap()
                    .1,
                &mut view_actions,
            );
            assert!(view_actions.contains(&command));
        }
    }
    #[test]
    fn transcript_modes_are_checked_persisted_and_presentation_only() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.state.busy = true;
        app.draft = "Keep draft".into();
        app.state.dialogs.push_back(questions::demo_dialog());
        let defaults: Preferences = serde_json::from_str("{}").unwrap();
        assert_eq!(defaults.transcript_view, crate::transcript::View::Normal);
        for (command, view) in [
            (Command::TranscriptVerbose, crate::transcript::View::Verbose),
            (Command::TranscriptNormal, crate::transcript::View::Normal),
        ] {
            app.menu_command(&ctx, command);
            assert_eq!(app.prefs.transcript_view, view);
            assert_eq!(app.menu_state(&ctx).checked(command), Some(true));
            let saved = serde_json::to_string(&app.prefs).unwrap();
            let restored: Preferences = serde_json::from_str(&saved).unwrap();
            assert_eq!(restored.transcript_view, view);
            assert!(app.state.busy);
            assert_eq!(app.draft, "Keep draft");
            assert_eq!(app.state.dialogs.len(), 1);
            assert!(app.deliveries.is_empty());
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
    fn native_formatting_commands_target_the_embedded_composer_not_main() {
        let ctx = egui::Context::default();
        let mut app = crate::app::files::tests::app(&ctx);
        app.demo = false;
        app.client = Some(rpc::Client::stub());
        app.connected = true;
        app.new_chat = false;
        app.ensure_agent();
        assert!(app.start_docked_side_chat("hello"));
        let side = app.docked_side_chat_target().unwrap();
        for _ in 0..3 {
            crate::app::files::tests::frame(&ctx, &mut app, vec![]);
        }
        let id = egui::Id::new(("btw-editor", side.as_str()));
        ctx.memory_mut(|memory| memory.request_focus(id));
        let mut state = egui::TextEdit::load_state(&ctx, id).unwrap();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(5),
            )));
        state.store(&ctx, id);
        let main = app.draft.clone();
        assert!(app.menu_state(&ctx).composer_focused);
        app.menu_command(&ctx, Command::Bold);
        assert_eq!(app.agents.parked[&side].draft, "**hello**");
        assert_eq!(app.draft, main);
    }
    #[test]
    fn native_formatting_commands_and_accelerators_are_composer_scoped() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let frame = |app: &mut Pui| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 650.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.composer(ui, ctx));
                },
            )
        };
        frame(&mut app);
        app.draft = "hello".into();
        let id = egui::Id::new("composer");
        ctx.memory_mut(|m| m.request_focus(id));
        let mut state = egui::TextEdit::load_state(&ctx, id).unwrap();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(5),
            )));
        state.store(&ctx, id);
        assert!(app.menu_state(&ctx).composer_focused);
        assert!(Command::Sidebar.contextual_shortcut(true).is_none());
        assert!(Command::Inspector.contextual_shortcut(true).is_none());
        assert_eq!(
            Command::Bold.contextual_shortcut(true),
            Command::Sidebar.shortcut()
        );
        assert_eq!(
            Command::Italic.contextual_shortcut(true),
            Command::Inspector.shortcut()
        );
        for (command, expected) in [
            (Command::Bold, "**hello**"),
            (Command::Italic, "*hello*"),
            (Command::Underline, "<u>hello</u>"),
        ] {
            app.menu_command(&ctx, command);
            assert_eq!(app.draft, expected);
            app.menu_command(&ctx, command);
            assert_eq!(app.draft, "hello");
        }
        ctx.memory_mut(|m| m.surrender_focus(id));
        app.menu_command(&ctx, Command::Bold);
        assert_eq!(app.draft, "hello");
        assert!(Command::Bold.contextual_shortcut(false).is_none());
        assert!(Command::Sidebar.contextual_shortcut(false).is_some());
        ctx.memory_mut(|m| m.request_focus(id));
        app.settings_open = true;
        assert!(!app.composer_formatting_available(&ctx));
        app.menu_command(&ctx, Command::Underline);
        assert_eq!(app.draft, "hello");
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
    #[cfg(target_os = "macos")]
    #[test]
    fn command_w_closes_browser_tabs_without_requesting_window_close() {
        for native_menu in [false, true] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.state.busy = true;
            app.draft = "keep draft".into();
            for url in ["http://localhost:3000", "https://example.com"] {
                app.files.open_target(&ctx, &app.prefs.cwd, url).unwrap();
            }
            for expected_open in [true, true, false] {
                if native_menu {
                    app.menu.pending.push(Command::Close);
                }
                let output = ctx.run(
                    egui::RawInput {
                        events: if native_menu {
                            vec![]
                        } else {
                            vec![key_event(egui::Key::W, egui::Modifiers::COMMAND)]
                        },
                        ..Default::default()
                    },
                    |ctx| {
                        app.menu_commands(ctx);
                        app.agent_close_guard(ctx);
                        assert!(!ctx.input(|i| i.viewport().close_requested()));
                    },
                );
                assert_eq!(app.files.browser_open(), expected_open);
                assert!(!app.agents.close_prompt);
                assert!(!app.agents.exiting);
                assert!(app.state.busy);
                assert_eq!(app.draft, "keep draft");
                assert!(!output.viewport_output.values().any(|v| {
                    v.commands
                        .iter()
                        .any(|c| matches!(c, egui::ViewportCommand::Close))
                }));
            }
        }
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
