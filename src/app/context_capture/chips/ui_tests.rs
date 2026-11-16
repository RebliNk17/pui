use super::*;
use crate::app::files::tests::{app, frame};

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
fn rect(ctx: &egui::Context, chip: &Chip) -> egui::Rect {
    ctx.data(|d| d.get_temp::<egui::Rect>(chip.widget_id().with("rect")))
        .unwrap()
}
fn capture(ctx: &egui::Context, app: &mut Pui) {
    app.draft = "Keep my draft".into();
    queue(
        ctx,
        "src/frozen.rs:4–5 · selected source",
        "chosen frozen 世界",
    );
    app.collect_context_capture();
}

#[test]
fn actual_composer_chip_pointer_and_keyboard_removals_keep_draft_and_never_send() {
    for keyboard in [false, true] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let (client, _events, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.busy = true;
        capture(&ctx, &mut app);
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        let chip = &app.context_chips.chips[0];
        if keyboard {
            ctx.memory_mut(|m| m.request_focus(chip.widget_id()));
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Delete, egui::Modifiers::NONE)],
            );
        } else {
            let pos = rect(&ctx, chip).center();
            for pressed in [true, false] {
                frame(&ctx, &mut app, pointer(pos, pressed));
            }
        }
        assert!(app.context_chips.chips.is_empty());
        assert_eq!(app.draft, "Keep my draft");
        assert!(app.state.busy);
        assert!(commands.try_recv().is_err());
        assert!(ctx.memory(|m| m.has_focus(egui::Id::new("composer"))));
    }
}

#[test]
fn chip_focus_owns_send_approval_and_escape_without_executing_or_stopping() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo = false;
    app.connected = true;
    app.prefs.hide_sidebar = true;
    app.state.busy = true;
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    capture(&ctx, &mut app);
    let permission = json!({"id":"chip-approval", "method":"confirm", "title":"Allow bash?", "message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."});
    assert!(prompts::approval(&permission).is_some());
    app.state.dialogs.push_back(permission);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let id = app.context_chips.chips[0].widget_id();
    let draft = app.draft.clone();
    for key_code in [egui::Key::Enter, egui::Key::Backspace] {
        ctx.memory_mut(|m| m.request_focus(id));
        frame(
            &ctx,
            &mut app,
            vec![key(key_code, egui::Modifiers::COMMAND)],
        );
        assert_eq!(app.draft, draft);
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.pending_dialog.is_none());
        assert!(commands.try_recv().is_err());
    }
    ctx.memory_mut(|m| {
        m.request_focus_with_filter(
            id,
            egui::EventFilter {
                escape: true,
                ..Default::default()
            },
        )
    });
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert_eq!(app.draft, draft);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
    assert!(ctx.memory(|m| m.has_focus(egui::Id::new("composer"))));

    // A plain Enter belongs only to the focused remove control, not approval/send.
    ctx.memory_mut(|m| m.request_focus(id));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.draft, "Keep my draft");
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.pending_dialog.is_none());
    assert!(commands.try_recv().is_err());
}

#[test]
fn popup_modal_and_picker_ownership_disable_chip_keyboard_removal() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    capture(&ctx, &mut app);
    frame(&ctx, &mut app, vec![]);
    let id = app.context_chips.chips[0].widget_id();
    let draft = app.draft.clone();
    for owner in 0..3 {
        ctx.memory_mut(|m| m.request_focus(id));
        match owner {
            0 => egui::Popup::open_id(&ctx, egui::Id::new("chip-test-popup")),
            1 => app.model_picker_open = true,
            _ => app.search_open = true,
        }
        let _ = ctx.run(
            egui::RawInput {
                events: vec![key(egui::Key::Delete, egui::Modifiers::NONE)],
                ..Default::default()
            },
            |ctx| app.context_chip_inputs(ctx),
        );
        assert_eq!(app.draft, draft);
        assert_eq!(app.context_chips.chips.len(), 1);
        egui::Popup::close_all(&ctx);
        app.model_picker_open = false;
        app.search_open = false;
    }
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("chip-test-modal"), 240.0, |ui| {
            prompt_ui::heading(
                ui,
                "Information",
                "Chip keys are disabled beneath this modal.",
            );
        });
    });
    ctx.memory_mut(|m| m.request_focus(id));
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Delete, egui::Modifiers::NONE)],
            ..Default::default()
        },
        |ctx| app.context_chip_inputs(ctx),
    );
    assert_eq!(app.draft, draft);
}

#[test]
fn pointer_activation_from_composer_owns_chords_before_inline_approval() {
    for chord in [egui::Key::Enter, egui::Key::Backspace] {
        for chord_on_press in [false, true] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.demo = false;
            app.connected = true;
            app.state.busy = true;
            app.prefs.hide_sidebar = true;
            let (client, _events, commands) = rpc::Client::stub_with_records();
            app.client = Some(client);
            capture(&ctx, &mut app);
            let permission = json!({"id":"pointer-chip", "method":"confirm", "title":"Allow bash?", "message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."});
            assert!(prompts::approval(&permission).is_some());
            app.state.dialogs.push_back(permission);
            for _ in 0..5 {
                frame(&ctx, &mut app, vec![]);
            }
            let pos = rect(&ctx, &app.context_chips.chips[0]).center();
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("composer")));
            for pressed in [true, false] {
                let mut events = pointer(pos, pressed);
                if pressed == chord_on_press {
                    events.push(key(chord, egui::Modifiers::COMMAND));
                }
                frame(&ctx, &mut app, events);
                assert_eq!(app.state.dialogs.len(), 1);
                assert!(app.pending_dialog.is_none());
                assert!(commands.try_recv().is_err());
            }
            assert_eq!(app.draft, "Keep my draft");
            assert!(app.context_chips.chips.is_empty());
            assert!(app.state.busy);
        }
    }
}

#[test]
fn native_tab_and_shift_tab_then_immediate_escape_cannot_stop_or_deny() {
    for approval in [false, true] {
        for backwards in [false, true] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.demo = false;
            app.connected = true;
            app.state.busy = true;
            app.prefs.hide_sidebar = true;
            let (client, _events, commands) = rpc::Client::stub_with_records();
            app.client = Some(client);
            capture(&ctx, &mut app);
            queue(&ctx, "second capture", "second body");
            app.collect_context_capture();
            if approval {
                app.state.dialogs.push_back(json!({"id":"native-tab-chip", "method":"confirm", "title":"Allow bash?", "message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."}));
            }
            for _ in 0..5 {
                frame(&ctx, &mut app, vec![]);
            }
            let draft = app.draft.clone();
            if backwards {
                ctx.memory_mut(|m| m.request_focus(egui::Id::new("composer")));
                frame(
                    &ctx,
                    &mut app,
                    vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
                );
            } else {
                // Acquire solely by native Tab navigation, never injecting chip focus.
                for _ in 0..150 {
                    frame(
                        &ctx,
                        &mut app,
                        vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
                    );
                    if ctx.memory(|m| m.focused()) == Some(app.context_chips.chips[0].widget_id()) {
                        break;
                    }
                }
                assert!(
                    ctx.memory(|m| m.has_focus(app.context_chips.chips[0].widget_id())),
                    "native Tab must reach chip"
                );
            }
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            );
            assert_eq!(app.draft, draft);
            assert_eq!(app.context_chips.chips.len(), 2);
            assert!(
                app.state.busy,
                "{approval}/{backwards}: Escape must not Stop"
            );
            assert_eq!(app.state.dialogs.len(), usize::from(approval));
            assert!(app.pending_dialog.is_none());
            assert!(
                commands.try_recv().is_err(),
                "{approval}/{backwards}: no approval or abort RPC"
            );
            assert!(ctx.memory(|m| m.has_focus(egui::Id::new("composer"))));
        }
    }
}

#[test]
fn actual_space_pair_removes_without_leaking_text_to_composer() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    capture(&ctx, &mut app);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let id = app.context_chips.chips[0].widget_id();
    ctx.memory_mut(|m| m.request_focus(id));
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Space, egui::Modifiers::NONE),
            egui::Event::Text(" ".into()),
        ],
    );
    assert_eq!(app.draft, "Keep my draft");
    assert!(app.context_chips.chips.is_empty());
}

#[test]
fn docked_files_and_changes_keep_chip_removal_enabled_beside_chat() {
    for files in [true, false] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.prefs.hide_sidebar = true;
        if files {
            let path = app.prefs.cwd.join("README.md");
            app.files
                .open_target(&ctx, &app.prefs.cwd, path.to_str().unwrap())
                .unwrap();
        } else {
            app.inspector_panel = true;
            app.changes
                .test_diff("src/selected.rs", "@@ -1 +1 @@\n-old\n+new");
        }
        capture(&ctx, &mut app);
        for _ in 0..6 {
            frame(&ctx, &mut app, vec![]);
        }
        assert!(ctx.read_response(egui::Id::new("composer")).is_some());
        assert!(app.context_chips_available(&ctx));
        let chip = &app.context_chips.chips[0];
        let response = ctx.read_response(chip.widget_id()).unwrap();
        assert!(response.sense.senses_click());
        let pos = rect(&ctx, chip).center();
        for pressed in [true, false] {
            frame(&ctx, &mut app, pointer(pos, pressed));
        }
        assert_eq!(app.draft, "Keep my draft");
        assert!(app.context_chips.chips.is_empty());
    }
}

#[test]
fn actual_composer_edit_detaches_all_chips_before_same_length_text_can_be_removed() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    capture(&ctx, &mut app);
    frame(&ctx, &mut app, vec![]);
    let id = app.context_chips.chips[0].id;
    let composer = egui::Id::new("composer");
    ctx.memory_mut(|m| m.request_focus(composer));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::A, egui::Modifiers::COMMAND)],
    );
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("edited unrelated text".into())],
    );
    assert!(app.context_chips.chips.is_empty());
    let draft = app.draft.clone();
    assert!(!app.context_chips.remove(&mut app.draft, id));
    assert_eq!(app.draft, draft);
    assert!(app.draft.contains("edited unrelated text"));
}

#[test]
fn native_inline_chip_geometry_wraps_in_themes_narrow_windows_and_zoom() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for (width, scale) in [(720.0, 1.0), (320.0, 1.0), (320.0, 1.5)] {
            let ctx = egui::Context::default();
            ctx.set_zoom_factor(scale);
            let mut app = app(&ctx);
            app.prefs.theme = theme;
            app.prefs.color_styles.apply(&ctx, theme);
            app.focus_composer = false;
            for i in 0..16 {
                queue(
                    &ctx,
                    &format!(
                        "src/{i}/{}:14–16 · frozen selected source",
                        "long-file-世界-".repeat(20)
                    ),
                    "short literal excerpt",
                );
                app.collect_context_capture();
            }
            let mut bounds = egui::Rect::NOTHING;
            for _ in 0..3 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 1000.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            bounds = ui.available_rect_before_wrap();
                            app.context_chips(ui);
                        });
                    },
                );
            }
            let mut rows = Vec::new();
            for chip in &app.context_chips.chips {
                let rect = rect(&ctx, chip);
                assert!(rect.left() >= bounds.left() - 1.0, "{rect:?} {bounds:?}");
                assert!(rect.right() <= bounds.right() + 1.0, "{rect:?} {bounds:?}");
                assert!(rect.width() <= 220.0);
                assert_eq!(rect.height(), 24.0);
                if !rows.contains(&rect.top()) {
                    rows.push(rect.top());
                }
                let response = ctx
                    .read_response(chip.widget_id())
                    .expect("actual focusable chip response");
                assert!(response.enabled());
            }
            assert!(rows.len() > 1);
            assert!(
                app.context_chips
                    .chips
                    .iter()
                    .map(|c| c.label.capacity())
                    .sum::<usize>()
                    < 16 * 4 * 513
            );
        }
    }
}

#[test]
fn no_chips_are_inferred_from_arbitrary_user_text_or_streamed_reply_content() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "[Context: manual]\nmy own literal\n[/Context]".into();
    app.state.busy = true;
    for i in 0..4 {
        app.state.messages.push(Message {
            role: "assistant".into(),
            text: format!("stream {i} [Context: not an explicit capture]"),
            ..Default::default()
        });
        frame(&ctx, &mut app, vec![]);
        assert!(app.context_chips.chips.is_empty());
    }
    capture(&ctx, &mut app);
    let draft = app.draft.clone();
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    assert_eq!(app.context_chips.chips.len(), 1);
    assert_eq!(app.draft, draft);
    assert!(app.state.busy);
}
