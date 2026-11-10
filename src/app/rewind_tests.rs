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
    app.ultracode_ready = true;
    app.client = Some(rpc::Client::stub());
    let image =
        crate::media::Attachment::from_bytes(include_bytes!("../../assets/icon.png")).unwrap();
    app.state = State {
        session_file: Some("/fixture/history.jsonl".into()),
        messages: vec![
            Message {
                role: "user".into(),
                text: "Original prompt".into(),
                timestamp: 100,
                images: vec![image],
                ..Default::default()
            },
            Message {
                role: "assistant".into(),
                text: "Completed response".into(),
                timestamp: 200,
                ..Default::default()
            },
        ],
        history_points: vec![
            protocol::HistoryPoint {
                entry_id: "user-entry".into(),
                role: "user".into(),
                text: "Original prompt".into(),
                timestamp: 100,
                code_reason: Some("No checkpoint for this old message.".into()),
                ..Default::default()
            },
            protocol::HistoryPoint {
                entry_id: "response-entry".into(),
                role: "assistant".into(),
                text: "Completed response".into(),
                timestamp: 200,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    app
}
fn result(id: &str, data: Value) -> Value {
    let mut data = data;
    data["id"] = id.into();
    json!({"type":"extension_ui_request","method":"setStatus","statusKey":"pui-history-result","statusText":data.to_string()})
}
fn text(shape: &egui::Shape, labels: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(t) => labels.push((
            t.galley.job.text.clone(),
            egui::Rect::from_min_size(t.pos, t.galley.size()),
        )),
        egui::Shape::Vec(v) => {
            for s in v {
                text(s, labels)
            }
        }
        _ => {}
    }
}
#[test]
fn rewind_waits_for_ack_restores_prompt_and_images_and_preserves_unsent_draft() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "Unsent draft".into();
    app.transcript.message_action = Some((0, MessageAction::Edit));
    app.history_action();
    assert!(!app.history_view.is_open());
    assert!(app.history_pending.as_ref().unwrap().request.mode == Mode::Messages);
    let id = app.history_pending.as_ref().unwrap().id.clone();
    app.submit(&ctx);
    assert_eq!(app.draft, "Unsent draft");
    assert!(app.deliveries.is_empty());
    assert!(app.history_event(&result("obsolete", json!({"success":true}))));
    assert!(app.history_pending.is_some());
    assert!(app.history_event(&result(&id, json!({"success":true,"files":0}))));
    assert_eq!(app.draft, "Original prompt");
    assert_eq!(app.draft_images.images().len(), 1);
    assert_eq!(app.history_drafts[0].text, "Unsent draft");
    assert!(app.deliveries.is_empty());
    assert!(!app.state.pending);
    assert!(app.history_pending.is_none());
}
#[test]
fn edit_and_rewind_skip_confirmation_only_when_there_is_no_file_choice() {
    for operation in [MessageAction::Edit, MessageAction::Rewind] {
        for (changes, reason, immediate) in [
            (Some(0), None, true),
            (Some(2), None, false),
            (None, None, false),
            (
                None,
                Some("Untracked shell effects; cannot restore files"),
                true,
            ),
        ] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.state.history_points[0].file_changes = changes;
            app.state.history_points[0].code_reason = reason.map(str::to_owned);
            app.draft = "Unsent draft".into();
            app.transcript.message_action = Some((0, operation));
            app.history_action();
            assert_eq!(app.history_pending.is_some(), immediate);
            assert_eq!(app.history_view.is_open(), !immediate);
            assert_eq!(app.draft, "Unsent draft", "wait for Pi acknowledgment");
            assert!(app.deliveries.is_empty());
            if let Some(pending) = &app.history_pending {
                assert!(
                    pending.request.mode == Mode::Messages,
                    "never auto-restore files"
                );
            }
        }
    }
}

#[test]
fn busy_other_agent_does_not_require_a_useless_restore_confirmation() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.history_points[0].code_reason = None;
    app.state.history_points[0].file_changes = Some(1);
    app.ensure_agent();
    let mut other = app.agents.nodes.remove(&app.agents.active).unwrap();
    other.id = "other-agent".into();
    other.path = None;
    other.stopped = false;
    app.agents.nodes.insert(other.id.clone(), other);
    assert!(!app.workspace_restore_idle());
    app.transcript.message_action = Some((0, MessageAction::Edit));
    app.history_action();
    assert!(!app.history_view.is_open());
    assert!(app.history_pending.as_ref().unwrap().request.mode == Mode::Messages);
}

#[test]
fn fork_targets_assistant_and_new_chat_contains_no_replayed_response_draft() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let original = app.ensure_agent();
    app.draft = "Original chat draft".into();
    app.transcript.message_action = Some((0, MessageAction::Fork));
    app.history_action();
    assert!(app.history_view.request.is_none());
    app.transcript.message_action = Some((1, MessageAction::Fork));
    app.history_action();
    let request = app.history_view.request.clone().unwrap();
    assert_eq!(request.point.entry_id, "response-entry");
    app.begin_history(request);
    let id = app.history_pending.as_ref().unwrap().id.clone();
    app.history_event(&result(
        &id,
        json!({"success":true,"path":"/fixture/fork.jsonl"}),
    ));
    assert!(app.draft.is_empty());
    assert!(app.draft_images.is_empty());
    assert!(app.deliveries.is_empty());
    assert!(app.loading_chat);
    assert_eq!(
        app.pending_session.as_deref(),
        Some(std::path::Path::new("/fixture/fork.jsonl"))
    );
    app.view_agent(&original);
    assert_eq!(app.draft, "Original chat draft");
    assert_eq!(app.state.messages[1].text, "Completed response");
}
#[test]
fn duplicate_prompts_code_unavailable_and_stale_navigation_are_rejected() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state
        .history_points
        .push(app.state.history_points[0].clone());
    app.transcript.message_action = Some((0, MessageAction::Rewind));
    app.history_action();
    assert!(app.history_view.request.is_none());
    app.state.history_points.pop();
    app.state.history_points[0].code_reason = None;
    app.transcript.message_action = Some((0, MessageAction::Rewind));
    app.history_action();
    let mut request = app.history_view.request.clone().unwrap();
    request.mode = Mode::Both;
    app.state.history_points[0].code_reason = Some("Checkpoint is no longer available".into());
    app.begin_history(request.clone());
    assert!(app.history_pending.is_none());
    request.mode = Mode::Messages;
    app.state.session_file = Some("/other/chat".into());
    app.begin_history(request);
    assert!(app.history_pending.is_none());
    let _ = ctx.run(Default::default(), |ctx| app.history_confirmation(ctx));
    assert!(app.history_view.request.is_none());
}
#[test]
fn code_only_does_not_replace_draft_and_timeout_never_retries_or_sends() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.history_points[0].code_reason = None;
    app.draft = "keep draft".into();
    app.transcript.message_action = Some((0, MessageAction::Rewind));
    app.history_action();
    let mut request = app.history_view.request.clone().unwrap();
    request.mode = Mode::Code;
    app.begin_history(request);
    let pending = app.history_pending.as_mut().unwrap();
    pending.started -= Duration::from_secs(31);
    let id = pending.id.clone();
    app.poll_history();
    assert!(app.history_pending.as_ref().unwrap().timed_out);
    app.submit(&ctx);
    assert!(app.deliveries.is_empty());
    assert_eq!(app.draft, "keep draft");
    app.history_event(&result(&id, json!({"success":true,"files":1})));
    assert_eq!(app.draft, "keep draft");
    assert!(app.history_drafts.is_empty());
    assert!(app.state.error.is_none());
}
#[test]
fn fork_during_poll_cannot_apply_old_source_records_to_the_new_chat() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (client, inject) = rpc::Client::stub_with_events();
    app.client = Some(client);
    let original = app.ensure_agent();
    app.transcript.message_action = Some((1, MessageAction::Fork));
    app.history_action();
    app.begin_history(app.history_view.request.clone().unwrap());
    let id = app.history_pending.as_ref().unwrap().id.clone();
    inject
        .send(rpc::Event::Record(result(
            &id,
            json!({"success":true,"path":"/fixture/new-fork.jsonl"}),
        )))
        .unwrap();
    inject.send(rpc::Event::Record(json!({"type":"extension_ui_request","method":"set_editor_text","text":"stale source draft"}))).unwrap();
    app.poll_rpc();
    assert_ne!(app.agents.active, original);
    assert!(app.draft.is_empty());
    assert_eq!(
        app.state.session_file.as_deref(),
        Some("/fixture/new-fork.jsonl")
    );
    assert!(app.loading_chat);
}
#[test]
fn restore_confirmation_has_three_choices_and_cancel_is_non_mutating() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "keep me".into();
    app.state.history_points[0].code_reason = None;
    app.state.history_points[0].file_changes = Some(2);
    app.transcript.message_action = Some((0, MessageAction::Edit));
    app.history_action();
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
            |ctx| app.history_confirmation(ctx),
        )
    };
    for _ in 0..4 {
        render(&mut app, vec![]);
    }
    let mut labels = Vec::new();
    for shape in render(&mut app, vec![]).shapes {
        let mut rendered = vec![];
        text(&shape.shape, &mut rendered);
        for (label, rect) in &rendered {
            if [
                "Keep current files",
                "Restore files and conversation",
                "Restore files only",
                "Cancel",
                "Edit message",
            ]
            .contains(&label.as_str())
            {
                assert!(
                    shape.clip_rect.contains_rect(*rect),
                    "Clipped {label}: {rect:?} in {:?}",
                    shape.clip_rect
                );
            }
        }
        labels.extend(rendered);
    }
    for title in [
        "Keep current files",
        "Restore files and conversation",
        "Restore files only",
        "Edit message",
    ] {
        assert!(labels.iter().any(|(s, _)| s == title));
    }
    assert!(
        labels
            .iter()
            .any(|(s, _)| s.contains("2 captured file change"))
    );
    let pos = labels
        .iter()
        .find(|(s, _)| s == "Cancel")
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
    assert!(app.history_view.request.is_none());
    assert!(app.history_pending.is_none());
    assert_eq!(app.draft, "keep me");
}
#[test]
fn footer_actions_match_metadata_size_color_and_alignment_in_both_themes() {
    fn collect(
        shape: &egui::Shape,
        labels: &mut Vec<(String, egui::Rect, Color32)>,
        icons: &mut Vec<egui::Rect>,
    ) {
        match shape {
            egui::Shape::Text(t) => labels.push((
                t.galley.job.text.clone(),
                egui::Rect::from_min_size(t.pos, t.galley.size()),
                t.override_text_color
                    .unwrap_or(t.galley.job.sections[0].format.color),
            )),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, labels, icons);
                }
            }
            egui::Shape::LineSegment { .. }
            | egui::Shape::Path(_)
            | egui::Shape::Rect(_)
            | egui::Shape::Circle(_) => icons.push(shape.visual_bounding_rect()),
            _ => {}
        }
    }
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        for zoom in [1.0, 1.25] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            theme.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            app.state.messages[1].cost = 1.15;
            app.state.messages[1].prompt_usage = crate::protocol::PromptUsage {
                input: 1.0,
                cache_read: 99.0,
                ..Default::default()
            };
            let render = |app: &mut Pui| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1100.0, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.conversation(ctx),
                )
            };
            for _ in 0..5 {
                render(&mut app);
            }
            let mut labels = vec![];
            let mut shapes = vec![];
            for shape in render(&mut app).shapes {
                collect(&shape.shape, &mut labels, &mut shapes);
            }
            let metadata = labels
                .iter()
                .find(|(t, _, _)| t.contains("99% cached"))
                .unwrap();
            for label in ["Copy", "Fork"] {
                let action = labels
                    .iter()
                    .find(|(t, r, _)| {
                        t == label && (r.center().y - metadata.1.center().y).abs() < 1.0
                    })
                    .unwrap();
                assert!(
                    (action.1.center().y - metadata.1.center().y).abs() <= 0.5,
                    "{label} is not centered with metadata: {action:?} / {metadata:?}"
                );
                assert!(
                    (action.1.height() - metadata.1.height()).abs() <= 0.5,
                    "{label} has different font metrics"
                );
                assert_eq!(action.2, metadata.2, "resting footer color must match");
                let icon_area = egui::Rect::from_min_max(
                    egui::pos2(action.1.left() - 16.0, action.1.center().y - 6.5),
                    egui::pos2(action.1.left() - 4.0, action.1.center().y + 6.5),
                );
                let geometry: Vec<_> = shapes
                    .iter()
                    .filter(|r| icon_area.contains(r.center()))
                    .collect();
                assert!(!geometry.is_empty(), "{label} needs a vector icon");
                for bounds in geometry {
                    assert!(
                        icon_area.expand(0.5).contains_rect(*bounds),
                        "{label} icon is oversized: {bounds:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn response_footer_copy_and_fork_remain_distinct_from_user_inline_actions() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let render = |app: &mut Pui, events| {
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
    for _ in 0..5 {
        render(&mut app, vec![]);
    }
    let mut labels = Vec::new();
    for shape in render(&mut app, vec![]).shapes {
        text(&shape.shape, &mut labels);
    }
    let fork = labels
        .iter()
        .find(|(t, _)| t == "Fork")
        .expect("Fork after assistant response")
        .1;
    let response = labels
        .iter()
        .find(|(t, _)| t == "Completed response")
        .unwrap()
        .1;
    assert!(fork.top() > response.bottom());
    let copy = labels
        .iter()
        .find(|(t, r)| t == "Copy" && r.top() > response.bottom())
        .expect("assistant Copy label with a vector icon")
        .1;
    assert!(!labels.iter().any(|(t, _)| t.contains('⧉')));
    let mut copied = false;
    for pressed in [true, false] {
        let output = render(
            &mut app,
            vec![
                egui::Event::PointerMoved(copy.center()),
                egui::Event::PointerButton {
                    pos: copy.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        copied |=
            output.platform_output.commands.iter().any(
                |c| matches!(c, egui::OutputCommand::CopyText(t) if t == "Completed response"),
            );
    }
    assert!(
        copied,
        "icon + label hit target must still copy the response"
    );
    for pressed in [true, false] {
        render(
            &mut app,
            vec![
                egui::Event::PointerMoved(fork.center()),
                egui::Event::PointerButton {
                    pos: fork.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(app.history_view.request.as_ref().unwrap().operation == MessageAction::Fork);
    assert_eq!(
        app.history_view.request.as_ref().unwrap().message.role,
        "assistant"
    );
}
