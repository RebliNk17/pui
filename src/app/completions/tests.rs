use super::*;

#[test]
fn skill_reload_is_an_accent_text_link_at_the_right_of_the_footer() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        for zoom in [1.0, 1.5, 2.0] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            focus(&ctx, &mut app, "/ski");
            for _ in 0..4 {
                frame(&ctx, &mut app, vec![]);
            }
            let output = frame(&ctx, &mut app, vec![]);
            let popup = app.completions.popup_rect.unwrap();
            let link = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "Reload chat resources" => {
                        Some(text)
                    }
                    _ => None,
                })
                .expect("Reload text link");
            let rect = egui::Rect::from_min_size(link.pos, link.galley.size());
            assert!(popup.contains_rect(rect));
            assert!(popup.right() - rect.right() < 12.0);
            assert_eq!(
                link.galley.job.sections[0].format.color,
                theme::palette(&ctx).accent
            );
            assert!(link.galley.job.sections[0].format.font_id.size <= 11.0);
            assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Rect(bg) if bg.rect.contains(rect.center()) && bg.rect.width() < popup.width() - 20.0
            )), "Text link must not have button chrome");
        }
    }
}

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
    app.state = State::default();
    app.state.session_file = Some("/fixture/chat.jsonl".into());
    app.client = Some(rpc::Client::stub());
    app
}
fn frame(ctx: &egui::Context, app: &mut Pui, events: Vec<egui::Event>) -> egui::FullOutput {
    sized_frame(ctx, app, egui::vec2(1000.0, 800.0), events)
}
fn sized_frame(
    ctx: &egui::Context,
    app: &mut Pui,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space((ui.available_height() - 120.0).clamp(0.0, 400.0));
                app.composer(ui, ctx);
            });
        },
    )
}
fn focus(ctx: &egui::Context, app: &mut Pui, draft: &str) {
    app.draft = draft.into();
    app.focus_composer = true;
    frame(ctx, app, vec![]);
    let id = egui::Id::new("composer");
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(CCursorRange::one(CCursor::new(draft.chars().count()))));
    state.store(ctx, id);
    frame(ctx, app, vec![]);
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
#[test]
fn tokens_are_cursor_aware_unicode_safe_and_ignore_email_and_prose_slashes() {
    assert_eq!(token("hello x@y.com", 13), None);
    assert_eq!(token("hello /model", 12), None);
    assert_eq!(
        token("é @src/old.rs tail", 7),
        Some(Token {
            start: 3,
            end: 14,
            query: "src/".into(),
            file: true
        })
    );
    assert_eq!(token("/mod", 4).unwrap().query, "mod");
    assert_eq!(token("/model args", 11), None);
}
#[test]
fn btw_and_main_execute_via_composer_completion_without_forwarding_commands() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let main = app.ensure_agent();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    focus(&ctx, &mut app, "/bt");
    frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(app.draft, "/btw ");
    assert_eq!(app.agents.active, main);
    frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(app.agents.active, main);
    let side = app.docked_side_chat_target().unwrap();
    assert_ne!(side, main);
    assert!(app.draft.is_empty());
    assert!(app.agents.parked[&side].client.is_some());
    app.view_agent(&side);
    app.loading_chat = false;
    app.connected = true;
    app.ultracode_ready = true;
    app.new_chat = false;
    focus(&ctx, &mut app, "/mai");
    frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(app.draft, "/main ");
    assert_eq!(app.agents.active, side);
    frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(app.agents.active, main);
    assert!(app.draft.is_empty());
    for command in commands.try_iter() {
        assert_ne!(command["type"], "prompt");
        assert_ne!(command["type"], "abort");
    }
}
#[test]
fn enter_completes_without_sending_then_enter_executes_local_action() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    focus(&ctx, &mut app, "/set");
    frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(app.draft, "/settings ");
    assert!(!app.settings_open);
    assert!(app.deliveries.is_empty());
    frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert!(app.settings_open);
    assert!(app.draft.is_empty());
    assert!(app.deliveries.is_empty());
}
#[test]
fn escape_dismisses_without_losing_draft_and_arrows_do_not_recall_history() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.messages.push(Message {
        role: "user".into(),
        text: "old prompt".into(),
        ..Default::default()
    });
    focus(&ctx, &mut app, "/");
    frame(&ctx, &mut app, vec![key(egui::Key::ArrowDown)]);
    assert_eq!(app.completions.selected, 1);
    assert_eq!(app.draft, "/");
    frame(&ctx, &mut app, vec![key(egui::Key::ArrowUp)]);
    assert_eq!(app.completions.selected, 0);
    frame(&ctx, &mut app, vec![key(egui::Key::Escape)]);
    assert!(!app.completion_active(&ctx));
    assert_eq!(app.draft, "/");
}
#[test]
fn catalog_commands_and_skills_are_completed_and_sent_to_pi() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.apply(
        json!({"type":"response","command":"get_commands","success":true,"data":{"commands":[
            {"name":"skill:review","description":"Review code","source":"skill"},
            {"name":"pui-history","description":"Internal","source":"extension"}
        ]}}),
    );
    focus(&ctx, &mut app, "/skill:rev");
    assert_eq!(app.completions.choices.len(), 1);
    frame(&ctx, &mut app, vec![key(egui::Key::Tab)]);
    assert_eq!(app.draft, "/skill:review ");
    frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(
        app.deliveries.len(),
        1,
        "draft={:?} focus={} pending={} error={:?} notice={:?}",
        app.draft,
        ctx.memory(|m| m.has_focus(egui::Id::new("composer"))),
        app.state.pending,
        app.state.error,
        app.state.notice
    );
    assert_eq!(app.deliveries[0].message.text, "/skill:review");
}
#[test]
fn clicking_a_completion_inserts_without_sending() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    focus(&ctx, &mut app, "/set");
    let output = frame(&ctx, &mut app, vec![]);
    let pos = files::tests::text(&output, "/settings");
    for pressed in [true, false] {
        frame(
            &ctx,
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
    assert_eq!(app.draft, "/settings ");
    assert!(app.deliveries.is_empty());
}
fn selected_row(output: &egui::FullOutput, ctx: &egui::Context) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Rect(rect) if rect.fill == theme::palette(ctx).selection_bg => {
                Some(rect.rect)
            }
            _ => None,
        })
        .expect("Full-row selection highlight")
}

#[test]
fn popup_is_compact_and_full_width_in_both_themes_at_narrow_sizes_and_zoom() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        for zoom in [1.0, 1.5, 2.0] {
            for size in [egui::vec2(1000.0, 800.0), egui::vec2(360.0, 480.0)] {
                for file in [false, true] {
                    let ctx = egui::Context::default();
                    let mut app = app(&ctx);
                    mode.apply(&ctx);
                    ctx.set_zoom_factor(zoom);
                    focus(&ctx, &mut app, "/");
                    if file {
                        // A deterministic filename-only result set, without a workspace scan.
                        app.draft = "@".into();
                        app.completions.token = token("@", 1);
                        app.completions.choices = (0..20)
                            .map(|i| Choice {
                                label: format!("src/nested/directory_{i}/a_very_long_filename.rs"),
                                description: "Workspace file reference".into(),
                                insert: format!("@/fixture/file_{i} "),
                            })
                            .collect();
                        app.completions.limited = true;
                    }
                    for _ in 0..4 {
                        sized_frame(&ctx, &mut app, size, vec![]);
                    }
                    let output = sized_frame(&ctx, &mut app, size, vec![]);
                    let popup = app.completions.popup_rect.expect("Completion popup");
                    let row = selected_row(&output, &ctx);
                    assert!(
                        popup.width() <= if file { 481.0 } else { 441.0 },
                        "{popup:?}"
                    );
                    assert!(popup.height() < 290.0, "{popup:?}");
                    assert!((row.height() - ROW_HEIGHT).abs() < 1.0, "{row:?}");
                    assert!(row.width() > popup.width() - 24.0, "{row:?} in {popup:?}");
                    assert!(
                        ctx.screen_rect().expand(1.0).contains_rect(popup),
                        "{popup:?}"
                    );
                    for shape in &output.shapes {
                        if let egui::Shape::Text(text) = &shape.shape
                            && text.galley.job.text.starts_with(if file {
                                "src/nested/"
                            } else {
                                "/"
                            })
                            && shape.clip_rect.intersects(popup)
                        {
                            assert_eq!(text.galley.rows.len(), 1, "Completion rows never wrap");
                            assert!(
                                text.pos.x + text.galley.size().x <= popup.right(),
                                "Long names must truncate inside the popup"
                            );
                        }
                    }
                    if !file && row.width() >= 356.0 {
                        files::tests::text(&output, "Start a new chat");
                    }
                }
            }
        }
    }
}

#[test]
fn clicking_the_empty_right_side_of_a_row_inserts_without_sending() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    focus(&ctx, &mut app, "/set");
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let output = frame(&ctx, &mut app, vec![]);
    let row = selected_row(&output, &ctx);
    let pos = row.right_center() - egui::vec2(8.0, 0.0);
    for pressed in [true, false] {
        frame(
            &ctx,
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
    assert_eq!(app.draft, "/settings ");
    assert!(app.deliveries.is_empty());
}

#[test]
fn keyboard_selection_scrolls_to_compact_rows_and_wraps() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    focus(&ctx, &mut app, "/");
    frame(&ctx, &mut app, vec![key(egui::Key::ArrowUp)]);
    assert_eq!(app.completions.selected, LOCAL.len() - 1);
    for _ in 0..10 {
        frame(&ctx, &mut app, vec![]);
    }
    let output = frame(&ctx, &mut app, vec![]);
    let row = selected_row(&output, &ctx);
    assert!(app.completions.popup_rect.unwrap().contains_rect(row));
    files::tests::text(&output, "/help");
    frame(&ctx, &mut app, vec![key(egui::Key::Tab)]);
    assert_eq!(app.draft, "/help ");
    assert!(app.deliveries.is_empty());
}

#[test]
fn shift_enter_is_a_newline_and_escape_does_not_stop_a_running_agent() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.busy = true;
    focus(&ctx, &mut app, "/set");
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Escape)],
            ..Default::default()
        },
        |ctx| {
            eframe::App::update(&mut app, ctx, &mut native);
        },
    );
    assert_eq!(app.draft, "/set");
    assert!(app.state.busy);
    focus(&ctx, &mut app, "/set");
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        }],
    );
    assert_eq!(app.draft, "/set\n");
    assert!(app.deliveries.is_empty());
}
#[test]
fn completion_and_pending_picker_stay_with_their_chat() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let original = app.ensure_agent();
    focus(&ctx, &mut app, "/");
    frame(&ctx, &mut app, vec![key(egui::Key::ArrowDown)]);
    let (tx, rx) = mpsc::channel();
    app.image_picker_rx = Some(rx);
    app.start_chat_in(app.prefs.cwd.clone());
    assert!(app.image_picker_rx.is_none());
    assert!(app.completions.token.is_none());
    tx.send(vec![]).unwrap();
    app.view_agent(&original);
    assert_eq!(app.draft, "/");
    assert_eq!(app.completions.selected, 1);
    assert!(app.image_picker_rx.is_some());
}
#[test]
fn reload_is_local_and_skill_popup_offers_it_without_replacing_the_draft() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.ensure_agent();
    focus(&ctx, &mut app, "/ski");
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let output = frame(&ctx, &mut app, vec![]);
    let pos = files::tests::text(&output, "Reload chat resources");
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(app.resource_reload.confirming());
    assert_eq!(app.draft, "/ski");
    assert!(app.deliveries.is_empty());
    app.resource_reload = Default::default();
    app.draft = "/reload".into();
    assert!(app.submit_command());
    assert!(app.resource_reload.confirming());
    assert_eq!(app.draft, "/reload");
    assert!(app.client.is_some()); // Confirmation, not an immediate reconnect.
    assert!(app.deliveries.is_empty());
}

#[test]
fn unknown_command_is_not_sent_as_an_agent_prompt() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "/login".into();
    app.submit(&ctx);
    assert!(app.deliveries.is_empty());
    assert_eq!(app.draft, "/login");
    assert!(app.state.error.as_ref().unwrap().contains("Pi CLI"));
}
#[test]
fn file_mentions_find_names_without_reading_and_insert_quoted_absolute_paths() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let root = std::env::temp_dir().join(format!("pui-completions-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("data")).unwrap();
    std::fs::write(root.join("data/report file.csv"), b"name,value\na,2\n").unwrap();
    std::fs::write(root.join(".env"), "SECRET=fixture").unwrap();
    app.prefs.cwd = root.clone();
    focus(&ctx, &mut app, "Inspect @report");
    for _ in 0..100 {
        frame(&ctx, &mut app, vec![]);
        if app.completions.rx.is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.completions.choices.len(), 1);
    frame(&ctx, &mut app, vec![key(egui::Key::Tab)]);
    let expected = format!(
        "Inspect @{} ",
        serde_json::to_string(&root.join("data/report file.csv").to_string_lossy()).unwrap()
    );
    assert_eq!(app.draft, expected);
    assert!(
        app.draft_images.is_empty(),
        "mentions reference files, they do not read or copy them"
    );
    assert!(app.deliveries.is_empty());
}
