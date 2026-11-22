use super::*;

struct Harness {
    ctx: egui::Context,
    app: Pui,
    rows: [egui::Rect; 2],
    action: Option<Action>,
}
impl Harness {
    fn new() -> Self {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.sessions.truncate(2);
        app.draft = "Keep this draft".into();
        let mut h = Self {
            ctx,
            app,
            rows: [egui::Rect::NOTHING; 2],
            action: None,
        };
        h.frame(vec![]);
        h.frame(vec![]);
        h
    }
    fn frame(&mut self, mut events: Vec<egui::Event>) -> egui::FullOutput {
        // Model taps, not permanently held keys (egui synthesizes repeats).
        let releases: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    ..
                } => Some(egui::Event::Key {
                    key: *key,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: *modifiers,
                }),
                _ => None,
            })
            .collect();
        events.extend(releases);
        self.action = None;
        self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 700.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_width(280.0);
                    for index in 0..2 {
                        let top = ui.cursor().min;
                        if let Some(action) = self.app.chat_row(ui, index, None, None) {
                            self.action = Some(action);
                        }
                        self.rows[index] = egui::Rect::from_min_max(
                            top,
                            egui::pos2(ui.max_rect().right(), ui.cursor().top()),
                        );
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.app.draft)
                            .id(egui::Id::new("menu-test-draft")),
                    );
                });
            },
        )
    }
    fn click(&mut self, pos: egui::Pos2, button: egui::PointerButton) {
        for pressed in [true, false] {
            self.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
    }
    fn open(&mut self, context: bool) {
        self.ctx
            .memory_mut(|m| m.request_focus(egui::Id::new("menu-test-draft")));
        let row = self.rows[1];
        self.click(
            if context {
                row.left_center() + egui::vec2(60.0, 0.0)
            } else {
                row.right_center() - egui::vec2(12.0, 0.0)
            },
            if context {
                egui::PointerButton::Secondary
            } else {
                egui::PointerButton::Primary
            },
        );
        self.frame(vec![]);
        assert!(egui::Popup::is_any_open(&self.ctx));
    }
    fn press(&mut self, key: egui::Key) -> egui::FullOutput {
        self.frame(vec![
            key_event(key, egui::Modifiers::NONE, false),
            egui::Event::Text(key.name().to_lowercase()),
        ])
    }
    fn path(&self) -> PathBuf {
        self.app.sessions[1].path.clone()
    }
}
fn key_event(key: egui::Key, modifiers: egui::Modifiers, repeat: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers,
    }
}
fn label(output: &egui::FullOutput, text: &str) -> egui::Rect {
    fn find(shape: &egui::Shape, text: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(t) if t.galley.job.text == text => {
                Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, text)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|s| find(&s.shape, text))
        .unwrap_or_else(|| panic!("Missing {text}"))
}

#[test]
fn accelerators_only_affect_the_open_menu_owner_from_either_entry_point() {
    for context in [false, true] {
        for key in [
            egui::Key::A,
            egui::Key::P,
            egui::Key::R,
            egui::Key::M,
            egui::Key::C,
            egui::Key::E,
            egui::Key::H,
            egui::Key::D,
        ] {
            let mut h = Harness::new();
            h.open(context);
            let path = h.path();
            let output = h.press(key);
            match key {
                egui::Key::A => assert!(h.app.prefs.chats[&path].archived),
                egui::Key::P => assert!(h.app.prefs.chats[&path].pinned),
                egui::Key::R => assert!(matches!(&h.action, Some(Action::Rename(p)) if p == &path)),
                egui::Key::M => assert!(matches!(&h.action, Some(Action::Move(p)) if p == &path)),
                egui::Key::E => assert!(matches!(&h.action, Some(Action::Export(p)) if p == &path)),
                egui::Key::H => assert!(matches!(&h.action, Some(Action::Hide(p)) if p == &path)),
                egui::Key::D => {
                    assert!(matches!(&h.action, Some(Action::Delete(deletion::Target::Chat(p))) if p == &path));
                    assert!(!h.app.deletion.deleting_chat(&path), "Only request confirmation, never start deletion");
                }
                egui::Key::C => assert!(output.platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(text) if text == &path.display().to_string()))),
                _ => unreachable!(),
            }
            assert!(!egui::Popup::is_any_open(&h.ctx), "{key:?}");
            assert_eq!(h.app.draft, "Keep this draft", "{key:?}");
            let other = &h.app.prefs.chats[&h.app.sessions[0].path];
            assert!(!other.archived && !other.pinned && !other.hidden);
        }
    }
}

#[test]
fn reverse_actions_and_disabled_hide_keep_mouse_semantics() {
    let mut h = Harness::new();
    let path = h.path();
    h.app.prefs.chats.entry(path.clone()).or_default().pinned = true;
    h.open(false);
    h.press(egui::Key::H);
    assert!(h.action.is_none());
    assert!(!h.app.prefs.chats[&path].hidden);
    assert!(egui::Popup::is_any_open(&h.ctx));
    h.press(egui::Key::P);
    assert!(!h.app.prefs.chats[&path].pinned);
    h.app.prefs.chats.get_mut(&path).unwrap().archived = true;
    h.open(false);
    h.press(egui::Key::A);
    assert!(!h.app.prefs.chats[&path].archived);
    h.app.prefs.chats.get_mut(&path).unwrap().hidden = true;
    h.open(true);
    h.press(egui::Key::H);
    assert!(matches!(&h.action, Some(Action::Restore(p)) if p == &path));
}

#[test]
fn closed_menu_modifiers_and_repeat_cannot_activate_actions() {
    let mut h = Harness::new();
    h.press(egui::Key::A);
    assert!(!h.app.prefs.chats[&h.path()].archived);
    h.open(false);
    for modifiers in [
        egui::Modifiers::COMMAND,
        egui::Modifiers::CTRL,
        egui::Modifiers::ALT,
    ] {
        h.frame(vec![key_event(egui::Key::A, modifiers, false)]);
        assert!(!h.app.prefs.chats[&h.path()].archived);
    }
    // egui derives repeat from its held-key set, not the backend flag.
    h.ctx.input_mut(|input| {
        input.keys_down.insert(egui::Key::A);
    });
    h.frame(vec![key_event(egui::Key::A, egui::Modifiers::NONE, true)]);
    assert!(!h.app.prefs.chats[&h.path()].archived);
    h.frame(vec![
        key_event(egui::Key::A, egui::Modifiers::SHIFT, false),
        egui::Event::Text("A".into()),
    ]);
    assert!(h.app.prefs.chats[&h.path()].archived);
    assert_eq!(h.app.draft, "Keep this draft");
}

#[test]
fn submenu_and_escape_never_trigger_parent_actions() {
    for submenu in ["Move to section", "Transcript view"] {
        let mut h = Harness::new();
        h.open(false);
        let output = h.frame(vec![]);
        let pos = label(&output, submenu).center();
        h.click(pos, egui::PointerButton::Primary);
        h.frame(vec![]);
        h.press(egui::Key::D);
        assert!(h.action.is_none());
        assert!(egui::Popup::is_any_open(&h.ctx));
        assert_eq!(h.app.draft, "Keep this draft");
    }
    let mut h = Harness::new();
    h.open(false);
    h.frame(vec![
        key_event(egui::Key::Escape, egui::Modifiers::NONE, false),
        key_event(egui::Key::D, egui::Modifiers::NONE, false),
    ]);
    assert!(h.action.is_none());
    assert!(!egui::Popup::is_any_open(&h.ctx));
}

#[test]
fn shortcut_hints_fit_light_dark_narrow_and_zoomed_menus() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let mut h = Harness::new();
            h.app.prefs.color_styles.apply(&h.ctx, theme);
            h.ctx.set_zoom_factor(zoom);
            h.frame(vec![]);
            h.frame(vec![]);
            h.open(false);
            let output = h.frame(vec![]);
            for (text, key) in [
                ("Rename", "R"),
                ("Pin", "P"),
                ("Move to project…", "M"),
                ("Copy session path", "C"),
                ("Export as Markdown", "E"),
                ("Archive", "A"),
                ("Hide from Pui", "H"),
                ("Delete chat…", "D"),
            ] {
                let item = label(&output, text);
                let hint = label(&output, key);
                assert!(hint.left() > item.right(), "{text}");
                assert!(hint.right() <= h.ctx.screen_rect().right());
            }
        }
    }
}
