//! Agent questions are anchored to the composer, not global modal windows.
use super::*;

const ALWAYS_ALLOW: &str = "Always allow this exact action (this chat)";
pub(super) struct Approval {
    pub(super) title: String,
    pub(super) explanation: String,
    pub(super) args: Value,
    pub(super) always: bool,
}
pub(super) fn approval(dialog: &Value) -> Option<Approval> {
    let full_title = protocol::string(dialog, "title");
    let (title, body, always) = match protocol::string(dialog, "method") {
        "confirm" => (
            full_title,
            protocol::string(dialog, "message").strip_suffix("\n\nApprove this call only.")?,
            false,
        ),
        "select" => {
            if dialog["options"] != json!(["Allow once", ALWAYS_ALLOW, "Deny"]) {
                return None;
            }
            let (title, body) = full_title.split_once("\n\n")?;
            (
                title,
                body.strip_suffix("\n\nApprove this exact action.")?,
                true,
            )
        }
        _ => return None,
    };
    if !title.starts_with("Allow ") || !title.ends_with('?') {
        return None;
    }
    let start = body.find('{')?;
    let args: Value = serde_json::from_str(&body[start..]).ok()?;
    if !args.is_object() {
        return None;
    }
    Some(Approval {
        title: title.into(),
        explanation: body[..start].trim().to_owned(),
        args,
        always,
    })
}
pub(super) fn permission_response(permission: &Approval, always: bool) -> Value {
    if permission.always {
        json!({"value":if always {ALWAYS_ALLOW} else {"Allow once"}})
    } else {
        json!({"confirmed":true})
    }
}
impl Pui {
    pub(super) fn agent_question(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        let Some(dialog) = self.state.dialogs.front().cloned() else {
            self.pending_dialog = None;
            return;
        };
        let id = protocol::string(&dialog, "id");
        let method = protocol::string(&dialog, "method");
        let permission = approval(&dialog);
        let new = id != self.current_dialog;
        if new {
            self.current_dialog = id.into();
            self.dialog_text = protocol::string(&dialog, "prefill").into();
            self.pending_dialog = None;
        }
        let waiting = self.pending_dialog.as_deref() == Some(id);
        if let Some(request) = questions::Request::parse(&dialog) {
            self.focus_composer = false;
            let keyboard = !self.transcript.gallery.is_open()
                && !self.draft_images.gallery.is_open()
                && !self.settings_open
                && !self.model_picker_open
                && !self.effort_picker_open
                && !self.changes.local_comment_editor_owns_keyboard(ui.ctx())
                && !self.files.table_export_owns_keyboard(ui.ctx())
                && !self.files.svg_owns_keyboard(ui.ctx())
                && prompt_ui::keyboard_available(ui);
            if let Some(response) = self
                .questions
                .show(ui, &dialog, &request, waiting, keyboard)
            {
                self.answer_dialog(id, response);
            }
            return;
        }
        let mut response = None;
        prompt_ui::inline(ui.ctx()).show(ui, |ui| {
                prompt_ui::style(ui);
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    icons::inline(
                        ui,
                        if permission.is_some() {
                            Icon::Shield
                        } else {
                            Icon::Details
                        },
                        colors.accent,
                    );
                    ui.add(egui::Label::new(RichText::new(if permission.is_some() { "Pui needs your permission" } else { "Agent question" }).size(11.0).color(colors.muted)).wrap());
                    if self.state.dialogs.len() > 1 {
                        ui.label(theme::small(ui, format!(
                            "+{} waiting",
                            self.state.dialogs.len() - 1
                        )));
                    }
                });
                ui.add_space(8.0);
                prompt_ui::heading(ui, permission.as_ref().map(|p| p.title.as_str()).unwrap_or_else(|| protocol::string(&dialog, "title")), "");
                ui.add_space(6.0);
                let message = protocol::string(&dialog, "message");
                if permission.is_some() || !message.is_empty() {
                egui::ScrollArea::vertical()
                    .id_salt(("inline-question", id))
                    .min_scrolled_height(140.0)
                    .max_height((ui.ctx().screen_rect().height() * 0.3).clamp(100.0, 260.0))
                    .show(ui, |ui| {
                        if let Some(approval) = &permission {
                            if !approval.explanation.is_empty() {
                                ui.add(
                                    egui::Label::new(theme::small(ui, &approval.explanation)).wrap(),
                                );
                                ui.add_space(8.0);
                            }
                            super::approval_view::arguments(ui, &approval.args);
                        } else if !message.is_empty() {
                            ui.add(egui::Label::new(message).wrap().selectable(true));
                        }
                    });
                ui.add_space(8.0);
                }
                ui.add_enabled_ui(!waiting, |ui| {
                    if method == "select" && permission.is_none() {
                        if let Some(options) = dialog["options"].as_array() {
                            let max_height = (ui.ctx().screen_rect().height() - 340.0).clamp(100.0, 320.0);
                            let width = (ui.available_width() - 28.0).max(40.0);
                            let height: f32 = options.iter().filter_map(Value::as_str).map(|option| {
                                let text = ui.painter().layout(option.into(), FontId::proportional(13.0), colors.text, width);
                                (text.size().y + 14.0).max(32.0) + 8.0
                            }).sum();
                            let height = (height - 8.0).clamp(0.0, max_height);
                            egui::ScrollArea::vertical()
                                .id_salt(("inline-options", id))
                                // Reserve real row height so the bottom panel can grow;
                                // max_height alone gets clipped to last frame's panel size.
                                .min_scrolled_height(height)
                                .max_height(max_height)
                                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                                .show(ui, |ui| {
                                    for option in options.iter().filter_map(Value::as_str) {
                                        if ui
                                            .add(
                                                egui::Button::new(option).fill(prompt_ui::background(ui.ctx())).stroke(Stroke::new(1.0, prompt_ui::border(ui.ctx()))).corner_radius(7)
                                                    .min_size(egui::vec2(ui.available_width(), 32.0)).wrap(),
                                            )
                                            .clicked()
                                        {
                                            response = Some(json!({"value":option}));
                                        }
                                    }
                                });
                        }
                    } else if matches!(method, "input" | "editor") {
                        let edit = ui.add(
                            egui::TextEdit::multiline(&mut self.dialog_text)
                                .id_salt(("agent-answer", id))
                                .hint_text(protocol::string(&dialog, "placeholder"))
                                .desired_width(f32::INFINITY)
                                .desired_rows(if method == "editor" { 4 } else { 2 }),
                        );
                        if new {
                            edit.request_focus();
                            self.focus_composer = false;
                        }
                    }
                    ui.add_space(8.0);
                    prompt_ui::footer(ui, |ui| {
                            if let Some(permission) = &permission {
                                if prompt_ui::button(ui, "Allow once", prompt_ui::Button::Primary, true).on_hover_text("⌘Enter / Ctrl+Enter").clicked() {
                                    response = Some(permission_response(permission, false));
                                }
                                if permission.always && prompt_ui::button(ui, "Always allow", prompt_ui::Button::Secondary, true).on_hover_text("Remember this exact tool + arguments + workspace for this chat only. Clears when permission mode changes or the connection closes. Nested tool calls still need their own approval.").clicked() {
                                    response = Some(permission_response(permission, true));
                                }
                            } else if method == "confirm" && prompt_ui::button(ui, "Confirm", prompt_ui::Button::Primary, true).clicked() {
                                response = Some(json!({"confirmed":true}));
                            }
                            if matches!(method, "input" | "editor")
                                && prompt_ui::button(ui, "Send answer", prompt_ui::Button::Primary, true).clicked()
                            {
                                response = Some(json!({"value":self.dialog_text}));
                            }
                            if prompt_ui::button(ui, if method == "confirm" || permission.is_some() { "Deny" } else { "Cancel" }, prompt_ui::Button::Secondary, true).clicked()
                            {
                                response = Some(json!({"cancelled":true}));
                            }
                    });
                });
                if permission.is_some() {
                    let cmd = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
                    ui.label(theme::small(ui, format!("{cmd}Enter: allow once · {cmd}Backspace: deny")));
                    if permission.as_ref().is_some_and(|p| p.always) {
                        ui.add(egui::Label::new(theme::small(ui, "Always allow: exact action, this chat only. Not the whole tool.")).wrap());
                    }
                }
                if waiting {
                    ui.label(theme::small(ui, "Waiting for the owning window…"));
                }
                if !self.model_picker_open && !self.effort_picker_open && !self.transcript.gallery.is_open() && !self.draft_images.gallery.is_open() && prompt_ui::keyboard_available(ui) {
                    if let Some(permission) = &permission {
                        // Consume even during mirror acknowledgment so a repeated
                        // approval shortcut cannot unexpectedly send the draft.
                        let allow = ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
                        let deny = ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace));
                        if !waiting {
                            if allow { response = Some(permission_response(permission, false)); }
                            if deny { response = Some(json!({"cancelled":true})); }
                        }
                    }
                }
                if !waiting
                    && !self.model_picker_open
                    && !self.effort_picker_open
                    && prompt_ui::keyboard_available(ui)
                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
                {
                    response = Some(json!({"cancelled":true}));
                }
            });
        if let Some(response) = response {
            self.answer_dialog(id, response);
        }
        ui.add_space(6.0);
    }

    pub(super) fn answer_dialog(&mut self, id: &str, mut response: Value) {
        if !self.state.dialogs.iter().any(|d| d["id"] == id) {
            return;
        }
        response["type"] = "extension_ui_response".into();
        response["id"] = id.into();
        if self.is_mirror() {
            self.pending_dialog = Some(id.into());
            self.remote_action(sync::Action::Rpc(response));
            return;
        }
        let sent = if self.workflow_answer(&response) || self.demo {
            true
        } else if let Some(client) = self.client.as_mut() {
            match client.send(response) {
                Ok(()) => true,
                Err(error) => {
                    self.state.error = Some(error);
                    false
                }
            }
        } else {
            false
        };
        if sent {
            self.state.dialogs.retain(|d| d["id"] != id);
            self.current_dialog.clear();
            self.questions = Default::default();
            self.focus_composer = true;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inline_command_is_fully_visible_above_composer_and_clickable_without_modal() {
        for (width, height) in [(1000.0, 800.0), (860.0, 620.0)] {
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
                session_file: Some("/fixture/approval.jsonl".into()),
                ..Default::default()
            };
            app.state.dialogs.push_back(json!({"method":"confirm","id":"request","title":"Allow bash?","message":"Auto could not safely approve this action.\n\n{\"command\":\"cargo test --all-targets\",\"timeout\":120}\n\nApprove this call only."}));
            let render = |app: &mut Pui, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, height),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| app.conversation(ctx),
                )
            };
            render(&mut app, vec![]);
            let output = render(&mut app, vec![]);
            fn collect(
                shape: &egui::Shape,
                clip: egui::Rect,
                text: &mut Vec<(String, egui::Rect, egui::Rect)>,
            ) {
                match shape {
                    egui::Shape::Text(t) => text.push((
                        t.galley.job.text.clone(),
                        egui::Rect::from_min_size(t.pos, t.galley.size()),
                        clip,
                    )),
                    egui::Shape::Vec(v) => {
                        for s in v {
                            collect(s, clip, text);
                        }
                    }
                    _ => {}
                }
            }
            let mut text = vec![];
            for shape in output.shapes {
                collect(&shape.shape, shape.clip_rect, &mut text);
            }
            let command = text
                .iter()
                .find(|(t, _, _)| t == "cargo test --all-targets")
                .expect("command rendered");
            assert!(
                command.2.contains_rect(command.1),
                "command must not be clipped at {width}x{height}: {command:?}"
            );
            let allow = text.iter().find(|(t, _, _)| t == "Allow once").unwrap();
            let composer = text
                .iter()
                .find(|(t, _, _)| t == "Ask a follow-up…")
                .unwrap();
            assert!(command.1.bottom() < allow.1.top() && allow.1.bottom() < composer.1.top());
            assert!(allow.2.contains_rect(allow.1));
            let pos = allow.1.center();
            for pressed in [true, false] {
                render(
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
            assert!(app.state.dialogs.is_empty());
            assert!(app.state.busy, "answering must not abort the run");
        }
    }
    #[test]
    fn restore_select_options_are_fully_visible_and_each_is_clickable() {
        fn collect(
            shape: &egui::Shape,
            clip: egui::Rect,
            out: &mut Vec<(String, egui::Rect, egui::Rect)>,
        ) {
            match shape {
                egui::Shape::Text(t) => out.push((
                    t.galley.job.text.clone(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                    clip,
                )),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, clip, out);
                    }
                }
                _ => {}
            }
        }
        let options = [
            "Keep current files",
            "Restore files to that point",
            "↩ Undo last rewind",
            "Cancel navigation",
        ];
        for (width, height) in [(1000.0, 800.0), (860.0, 620.0), (540.0, 540.0)] {
            for selected in options {
                let ctx = egui::Context::default();
                let mut app = Pui::new(
                    &eframe::CreationContext::_new_kittest(ctx.clone()),
                    Options {
                        demo: true,
                        ..Default::default()
                    },
                );
                app.state.dialogs.push_back(json!({"id":"restore","method":"select","title":"Restore Options","options":options}));
                app.draft = "Keep my draft".into();
                let render = |app: &mut Pui, events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, height),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| app.conversation(ctx),
                    )
                };
                for _ in 0..4 {
                    render(&mut app, vec![]);
                }
                let mut labels = vec![];
                for shape in render(&mut app, vec![]).shapes {
                    collect(&shape.shape, shape.clip_rect, &mut labels);
                }
                let composer = labels
                    .iter()
                    .find(|(text, _, _)| text == "Keep my draft")
                    .unwrap()
                    .1;
                for option in options {
                    let (_, rect, clip) =
                        labels.iter().find(|(text, _, _)| text == option).unwrap();
                    assert!(
                        clip.contains_rect(*rect),
                        "Clipped {option} at {width}x{height}: {rect:?} in {clip:?}"
                    );
                    assert!(rect.bottom() < composer.top());
                }
                let pos = labels
                    .iter()
                    .find(|(text, _, _)| text == selected)
                    .unwrap()
                    .1
                    .center();
                for pressed in [true, false] {
                    render(
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
                assert!(app.state.dialogs.is_empty(), "{selected} must be clickable");
                assert_eq!(app.draft, "Keep my draft");
                assert!(app.deliveries.is_empty());
            }
        }
    }

    fn select_dialog() -> Value {
        let args = json!({"code":"const file = await tools.read({\n  path: \"Cargo.toml\"\n});\ntext(file);"});
        json!({"method":"select","id":"new-approval","title":format!("Allow codemode?\n\nAuto flagged this action for review.\n\n{}\n\nApprove this exact action.",args),"options":["Allow once",ALWAYS_ALLOW,"Deny"]})
    }
    #[test]
    fn approval_shortcuts_consume_send_keys_and_preserve_the_composer_draft() {
        for key in [egui::Key::Enter, egui::Key::Backspace] {
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
                session_file: Some("/fixture/chat".into()),
                ..Default::default()
            };
            app.state.dialogs.push_back(select_dialog());
            app.draft = "Do not send or delete this draft".into();
            app.focus_composer = true;
            let frame = |app: &mut Pui, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(860.0, 620.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| app.conversation(ctx),
                )
            };
            frame(&mut app, vec![]);
            frame(&mut app, vec![]);
            frame(
                &mut app,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                }],
            );
            assert!(app.state.dialogs.is_empty());
            assert_eq!(app.draft, "Do not send or delete this draft");
            assert!(app.deliveries.is_empty());
            assert!(app.state.busy);
        }
    }
    #[test]
    fn select_approvals_are_recognized_exactly_and_offer_scoped_always_allow() {
        let dialog = select_dialog();
        let parsed = approval(&dialog).unwrap();
        assert_eq!(parsed.title, "Allow codemode?");
        assert!(parsed.always);
        assert_eq!(permission_response(&parsed, true)["value"], ALWAYS_ALLOW);
        let mut arbitrary = dialog.clone();
        arbitrary["options"] = json!(["Allow once", "Trust all tools", "Deny"]);
        assert!(approval(&arbitrary).is_none());
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.client = Some(rpc::Client::stub());
        app.state.dialogs.push_back(dialog);
        let render = |app: &mut Pui, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(860.0, 620.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| app.conversation(ctx),
            )
        };
        for _ in 0..5 {
            render(&mut app, vec![]);
        }
        let output = render(&mut app, vec![]);
        fn collect(
            shape: &egui::Shape,
            clip: egui::Rect,
            out: &mut Vec<(String, egui::Rect, egui::Rect)>,
        ) {
            match shape {
                egui::Shape::Text(t) => out.push((
                    t.galley.job.text.clone(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                    clip,
                )),
                egui::Shape::Vec(v) => {
                    for s in v {
                        collect(s, clip, out)
                    }
                }
                _ => {}
            }
        }
        let mut text = vec![];
        for shape in output.shapes {
            collect(&shape.shape, shape.clip_rect, &mut text);
        }
        for label in ["Allow once", "Always allow", "Deny"] {
            let button = text.iter().find(|(t, _, _)| t == label).unwrap();
            assert!(button.2.contains_rect(button.1), "Clipped {label}");
        }
        assert!(
            text.iter()
                .any(|(t, _, _)| t.starts_with("const file = await tools.read({\n"))
        );
        assert!(!text.iter().any(|(t, _, _)| t.contains("\"code\":")));
        let pos = text
            .iter()
            .find(|(t, _, _)| t == "Always allow")
            .unwrap()
            .1
            .center();
        for pressed in [true, false] {
            render(
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
        assert!(
            app.state.dialogs.is_empty(),
            "Always allow must respond to the current select request"
        );
    }
    #[test]
    fn permission_details_are_exact_and_arbitrary_questions_are_not_json_guessed() {
        let dialog = json!({"method":"confirm","title":"Allow bash?","message":"Auto could not safely approve this action.\n\n{\"command\":\"echo \\\"hello\\\"\",\"timeout\":10}\n\nApprove this call only."});
        let parsed = approval(&dialog).unwrap();
        assert_eq!(parsed.args["command"], "echo \"hello\"");
        assert!(parsed.explanation.contains("Auto"));
        assert!(
            approval(
                &json!({"method":"confirm","title":"Are you sure?","message":"{\"example\":true}"})
            )
            .is_none()
        );
    }
}
