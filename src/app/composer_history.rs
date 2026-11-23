//! Per-session composer recall. History comes only from the visible chat's user
//! messages; navigation drafts are parked with that chat, never kept globally.
use super::*;
use egui::{
    text::{CCursor, CCursorRange},
    widgets::text_edit::TextEditState,
};
#[derive(Default)]
pub(super) struct Recall {
    session: Option<String>,
    index: Option<usize>,
    entries: Vec<run_status::WaitingMessage>,
    edits: std::collections::BTreeMap<usize, run_status::WaitingMessage>,
    original: Option<run_status::WaitingMessage>,
}
impl Pui {
    pub(super) fn recall_before_edit(&mut self, ctx: &egui::Context) {
        if self.recall.session != self.state.session_file {
            self.recall = Recall {
                session: self.state.session_file.clone(),
                ..Default::default()
            };
        }
        let id = egui::Id::new("composer");
        if !ctx.memory(|m| m.has_focus(id)) || self.loading_chat {
            return;
        }
        let direction = ctx.input(|i| {
            if i.modifiers != egui::Modifiers::NONE
                || i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Text(_) | egui::Event::Paste(_) | egui::Event::Ime(_)
                    )
                })
            {
                return None;
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                Some(true)
            } else if i.key_pressed(egui::Key::ArrowDown) {
                Some(false)
            } else {
                None
            }
        });
        let Some(older) = direction else {
            return;
        };
        let mut state = TextEditState::load(ctx, id).unwrap_or_default();
        let cursor = state.cursor.char_range().and_then(|r| r.single());
        if !self.draft.is_empty() && cursor.is_none() {
            return;
        }
        let cursor = cursor.unwrap_or_else(|| CCursor::new(0));
        let galley = composer_text::galley(ctx, &self.draft, self.recall_width.max(100.0));
        let row = galley.layout_from_cursor(cursor).row;
        if (older && row != 0) || (!older && row + 1 < galley.rows.len()) {
            return;
        }
        if self.recall.index.is_none() {
            if !older {
                return;
            }
            let entries: Vec<_> = self
                .state
                .messages
                .iter()
                .rev()
                .filter(|m| m.role == "user" && (!m.text.is_empty() || !m.images.is_empty()))
                .take(100)
                .map(|m| run_status::WaitingMessage {
                    text: crate::transcript::user_display_text(m).to_owned(),
                    images: m.images.clone(),
                    mode: self.prefs.settings.delivery,
                })
                .collect();
            if entries.is_empty() {
                return;
            }
            self.recall.entries = entries;
            self.recall.original = Some(run_status::WaitingMessage {
                text: self.draft.clone(),
                images: self.draft_images.images(),
                mode: self.prefs.settings.delivery,
            });
        }
        if let Some(index) = self.recall.index {
            self.recall.edits.insert(
                index,
                run_status::WaitingMessage {
                    text: self.draft.clone(),
                    images: self.draft_images.images(),
                    mode: self.prefs.settings.delivery,
                },
            );
        }
        let next = match self.recall.index {
            None => Some(0),
            Some(i) if older => Some((i + 1).min(self.recall.entries.len() - 1)),
            Some(0) => None,
            Some(i) => Some(i - 1),
        };
        let message = if let Some(index) = next {
            self.recall
                .edits
                .get(&index)
                .unwrap_or(&self.recall.entries[index])
                .clone()
        } else {
            self.recall.original.take().unwrap_or_default()
        };
        self.recall.index = next;
        if next.is_none() {
            self.recall = Recall::default();
        }
        self.context_chips.clear();
        self.draft = message.text;
        self.draft_images.set_images(message.images);
        let cursor = CCursor::new(if older { 0 } else { self.draft.chars().count() });
        state.cursor.set_char_range(Some(CCursorRange::one(cursor)));
        state.store(ctx, id);
        ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::NONE,
                if older {
                    egui::Key::ArrowUp
                } else {
                    egui::Key::ArrowDown
                },
            );
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composer_hint_is_faded_and_selected_text_has_a_visible_tinted_background() {
        fn text_shape(shape: &egui::Shape, expected: &str) -> Option<egui::text::Galley> {
            match shape {
                egui::Shape::Text(t) if t.galley.job.text == expected => Some((*t.galley).clone()),
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| text_shape(s, expected)),
                _ => None,
            }
        }
        for theme in [theme::Theme::Dark, theme::Theme::Light] {
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
            app.draft.clear();
            theme.apply(&ctx);
            let colors = theme.palette();
            let frame = |app: &mut Pui| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(900.0, 620.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| app.composer(ui, ctx));
                    },
                )
            };
            frame(&mut app);
            let output = frame(&mut app);
            let hint = output
                .shapes
                .iter()
                .find_map(|s| text_shape(&s.shape, "Message Pi…"))
                .expect("placeholder rendered");
            assert!(
                hint.job
                    .sections
                    .iter()
                    .all(|s| s.format.color == colors.dim)
            );
            assert_ne!(colors.dim, colors.text);
            assert!(app.draft.is_empty(), "placeholder is never input text");
            app.draft = "This is selected text".into();
            frame(&mut app);
            let id = egui::Id::new("composer");
            let mut state = TextEditState::load(&ctx, id).unwrap();
            state.cursor.set_char_range(Some(CCursorRange::two(
                CCursor::new(0),
                CCursor::new(app.draft.chars().count()),
            )));
            state.store(&ctx, id);
            ctx.memory_mut(|m| m.request_focus(id));
            let output = frame(&mut app);
            let text = output
                .shapes
                .iter()
                .find_map(|s| text_shape(&s.shape, &app.draft))
                .expect("typed text rendered");
            assert!(
                text.rows
                    .iter()
                    .flat_map(|row| &row.visuals.mesh.vertices)
                    .any(|v| v.color == colors.selection_bg),
                "selection must paint its own background"
            );
            assert!(
                text.job
                    .sections
                    .iter()
                    .all(|s| s.format.color == colors.text)
            );
        }
    }

    #[test]
    fn composer_has_no_settings_button_after_permission_picker() {
        fn permission_rect(shape: &egui::Shape) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Text(text) if text.galley.job.text == "Auto" => {
                    Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(permission_rect),
                _ => None,
            }
        }
        for theme in [theme::Theme::Dark, theme::Theme::Light] {
            let ctx = egui::Context::default();
            let mut app = Pui::new(
                &eframe::CreationContext::_new_kittest(ctx.clone()),
                Options {
                    demo: true,
                    ..Default::default()
                },
            );
            theme.apply(&ctx);
            app.draft = "Keep my draft".into();
            let frame = |app: &mut Pui, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(900.0, 620.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| app.composer(ui, ctx));
                    },
                )
            };
            frame(&mut app, vec![]);
            let output = frame(&mut app, vec![]);
            let permission = output
                .shapes
                .iter()
                .find_map(|shape| permission_rect(&shape.shape))
                .expect("Permission picker remains in the composer");
            // The former settings button followed the label, chevron and spacing.
            let pos = permission.right_center() + egui::vec2(39.0, 0.0);
            for pressed in [true, false] {
                frame(
                    &mut app,
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
            assert!(!app.settings_open, "No settings shortcut in the composer");
            assert_eq!(app.draft, "Keep my draft");
        }
    }

    #[test]
    fn recall_uses_only_visible_session_and_restores_local_draft() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.state = State {
            session_file: Some("/fixture/a".into()),
            messages: vec![
                Message {
                    role: "user".into(),
                    text: "old a".into(),
                    ..Default::default()
                },
                Message {
                    role: "assistant".into(),
                    text: "not a prompt".into(),
                    ..Default::default()
                },
                Message {
                    role: "user".into(),
                    text: "new a".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        app.draft = "local draft".into();
        let frame = |app: &mut Pui, key: Option<egui::Key>| {
            let events = key
                .map(|key| {
                    vec![
                        egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                        egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed: false,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                })
                .unwrap_or_default();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(900.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.composer(ui, ctx));
                },
            );
        };
        frame(&mut app, None);
        frame(&mut app, None);
        frame(&mut app, Some(egui::Key::ArrowUp));
        assert_eq!(app.draft, "new a");
        frame(&mut app, Some(egui::Key::ArrowUp));
        assert_eq!(app.draft, "old a");
        frame(&mut app, Some(egui::Key::ArrowDown));
        assert_eq!(app.draft, "new a");
        frame(&mut app, Some(egui::Key::ArrowDown));
        assert_eq!(app.draft, "local draft");
        app.ensure_agent();
        app.park_tree();
        app.state = State {
            session_file: Some("/fixture/b".into()),
            messages: vec![Message {
                role: "user".into(),
                text: "only b".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        app.focus_composer = true;
        frame(&mut app, None);
        frame(&mut app, Some(egui::Key::ArrowUp));
        assert_eq!(app.draft, "only b");
        frame(&mut app, Some(egui::Key::ArrowUp));
        assert_eq!(app.draft, "only b", "must not reach session a");
        app.shutdown_agents();
    }
}
