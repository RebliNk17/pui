use super::*;

#[test]
fn compact_questions_keep_descriptions_and_drop_redundant_copy() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        for zoom in [1.0, 1.5, 2.0] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            for _ in 0..5 {
                frame(&mut app, 860.0, 620.0, vec![]);
            }
            let output = texts(frame(&mut app, 860.0, 620.0, vec![]));
            assert!(
                !output
                    .iter()
                    .any(|(t, _, _)| t.contains("continues automatically"))
            );
            for label in ["How should users start a task?", "Skip", "Cancel"] {
                let (_, rect, clip) = output.iter().find(|(t, _, _)| t == label).expect(label);
                assert!(clip.contains_rect(*rect), "{label} clipped at zoom {zoom}");
            }
            frame(&mut app, 860.0, 620.0, vec![key(egui::Key::Num4)]);
            for _ in 0..3 {
                frame(&mut app, 860.0, 620.0, vec![]);
            }
            let output = texts(frame(&mut app, 860.0, 620.0, vec![]));
            assert!(!output.iter().any(|(t, _, _)| t == "Your answer"));
            assert!(
                output
                    .iter()
                    .any(|(t, rect, clip)| t.starts_with("Share your preference")
                        && clip.contains_rect(*rect))
            );
        }
    }
}

#[test]
fn waiting_card_keeps_status_and_rejects_keyboard_and_pointer_actions() {
    let ctx = egui::Context::default();
    let dialog = demo_dialog();
    let request = Request::parse(&dialog).unwrap();
    let mut v = View::default();
    let mut output = None;
    for event in [
        None,
        Some(key(egui::Key::Num1)),
        Some(key(egui::Key::Escape)),
    ] {
        output = Some(ctx.run(
            egui::RawInput {
                events: event.into_iter().collect(),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    assert!(v.show(ui, &dialog, &request, true, true).is_none());
                });
            },
        ));
    }
    let output = texts(output.unwrap());
    assert!(
        output
            .iter()
            .any(|(t, _, _)| t == "Waiting for the owning window…")
    );
    assert_eq!(v.page, 0);
    assert!(v.answers.iter().all(Option::is_none));
    let pos = output
        .iter()
        .find(|(t, _, _)| t == "Cancel")
        .unwrap()
        .1
        .center();
    for pressed in [true, false] {
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    assert!(v.show(ui, &dialog, &request, true, true).is_none());
                });
            },
        );
    }
    assert!(v.answers.iter().all(Option::is_none));
}

fn request() -> Request {
    Request::parse(&demo_dialog()).unwrap()
}
fn view(request: &Request) -> View {
    let mut view = View {
        drafts: vec![String::new(); request.questions.len()],
        checked: vec![vec![]; request.questions.len()],
        custom_enabled: vec![false; request.questions.len()],
        answers: vec![None; request.questions.len()],
        ..Default::default()
    };
    view.go(0, request);
    view
}
#[test]
fn choices_advance_multi_select_waits_and_last_answer_returns_one_batch() {
    let request = request();
    let mut v = view(&request);
    assert!(v.act(Action::Choose(0), &request).is_none());
    assert_eq!(v.page, 1);
    v.act(Action::Back, &request);
    assert_eq!(
        v.answers[0].as_ref().unwrap().answer,
        request.questions[0].options[0].label
    );
    v.act(Action::Next, &request);
    v.act(Action::Skip, &request);
    assert_eq!(v.page, 2);
    assert!(v.act(Action::SubmitCustom, &request).is_none());
    for index in [0, 1, 0, 2] {
        assert!(v.act(Action::Choose(index), &request).is_none());
        assert_eq!(v.page, 2);
    }
    let result = v.act(Action::SubmitCustom, &request).unwrap();
    let result: Value = serde_json::from_str(result["value"].as_str().unwrap()).unwrap();
    assert_eq!(result["answers"][1]["skipped"], true);
    assert_eq!(
        result["answers"][2]["selections"],
        json!(["UI interaction tests", "Visual checks"])
    );
}
#[test]
fn custom_can_accompany_multiple_selections_and_drafts_survive_navigation() {
    let request = request();
    let mut v = view(&request);
    v.go(2, &request);
    v.act(Action::Choose(1), &request);
    v.act(Action::Custom, &request);
    v.drafts[2] = "Also check accessibility".into();
    v.act(Action::Back, &request);
    v.act(Action::Next, &request);
    assert!(v.editing);
    assert_eq!(v.drafts[2], "Also check accessibility");
    assert!(
        v.act(Action::SubmitCustom, &request).is_none(),
        "unanswered pages must not submit"
    );
    let answer = v.answers[2].as_ref().unwrap();
    assert!(answer.custom);
    assert_eq!(answer.selections, vec!["UI interaction tests"]);
    v.go(2, &request);
    v.act(Action::Choose(0), &request);
    assert!(
        v.answers[2].is_none(),
        "changed selection invalidates a saved answer"
    );
    assert_eq!(
        v.act(Action::Cancel, &request).unwrap(),
        json!({"cancelled":true})
    );
}
#[test]
fn malformed_requests_are_not_guessed_from_arbitrary_input_prompts() {
    let mut dialog = demo_dialog();
    dialog["title"] = "Any other prompt".into();
    assert!(Request::parse(&dialog).is_none());
    dialog["title"] = TITLE.into();
    dialog["placeholder"] = json!({"version":2,"request_id":"x","questions":[]})
        .to_string()
        .into();
    assert!(Request::parse(&dialog).is_none());
}
#[test]
fn closing_one_wait_or_settling_does_not_leave_stale_question_cards() {
    let mut state = State::default();
    state.apply(demo_dialog());
    let mut other = demo_dialog();
    other["id"] = "other".into();
    let mut payload: Value = serde_json::from_str(other["placeholder"].as_str().unwrap()).unwrap();
    payload["request_id"] = "other-request".into();
    other["placeholder"] = payload.to_string().into();
    state.apply(other);
    state.apply(json!({"type":"extension_ui_request","method":"setStatus","statusKey":"pui-questions","statusText":"{\"closed\":\"demo-request\"}"}));
    assert_eq!(state.dialogs.len(), 1);
    assert_eq!(state.dialogs[0]["id"], "other");
    state.apply(json!({"type":"agent_settled","aborted":true}));
    assert!(state.dialogs.is_empty());
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
    app.client = Some(rpc::Client::stub());
    app.state = State {
        busy: true,
        session_file: Some("/fixture/questions".into()),
        ..Default::default()
    };
    app.state.dialogs.push_back(demo_dialog());
    app.draft = "Keep my unsent message".into();
    app
}
fn frame(app: &mut Pui, width: f32, height: f32, events: Vec<egui::Event>) -> egui::FullOutput {
    app.ctx.clone().run(
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
}
fn texts(output: egui::FullOutput) -> Vec<(String, egui::Rect, egui::Rect)> {
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
                for s in shapes {
                    collect(s, clip, out);
                }
            }
            _ => {}
        }
    }
    let mut out = vec![];
    for s in output.shapes {
        collect(&s.shape, s.clip_rect, &mut out);
    }
    out
}
fn click(app: &mut Pui, text: &str, width: f32, height: f32) {
    let output = texts(frame(app, width, height, vec![]));
    let (_, rect, clip) = output
        .iter()
        .find(|(t, _, _)| t.contains(text))
        .unwrap_or_else(|| panic!("Missing {text}: {output:?}"));
    assert!(
        clip.contains_rect(*rect),
        "{text} is clipped: {rect:?} / {clip:?}"
    );
    let pos = rect.center();
    for pressed in [true, false] {
        frame(
            app,
            width,
            height,
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
fn every_normal_option_and_description_is_visible_and_clickable_in_both_themes() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        for (width, height) in [(1480.0, 960.0), (860.0, 620.0)] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            theme.apply(&ctx);
            for _ in 0..4 {
                frame(&mut app, width, height, vec![]);
            }
            let output = texts(frame(&mut app, width, height, vec![]));
            for expected in [
                "New task action",
                "From ordinary chat",
                "Support both",
                "Something else",
                "Skip",
            ] {
                let (_, rect, clip) = output
                    .iter()
                    .find(|(t, _, _)| t.contains(expected))
                    .expect(expected);
                assert!(
                    clip.contains_rect(*rect),
                    "{expected} clipped at {width}x{height}: {rect:?}/{clip:?}"
                );
            }
            click(&mut app, "Support both", width, height);
            assert_eq!(app.questions.page, 1);
            click(&mut app, "Isolated Git worktree", width, height);
            assert_eq!(app.questions.page, 2);
            frame(&mut app, width, height, vec![]);
            click(&mut app, "Unit tests", width, height);
            click(&mut app, "Visual checks", width, height);
            assert_eq!(app.questions.checked[2], vec![0, 2]);
            assert_eq!(
                app.state.dialogs.len(),
                1,
                "multi-select never submits on click"
            );
            click(&mut app, "Submit & continue", width, height);
            assert!(app.state.dialogs.is_empty());
            assert!(
                app.state.busy,
                "tool answer resumes the existing run, never aborts it"
            );
            assert_eq!(app.draft, "Keep my unsent message");
            assert!(app.deliveries.is_empty(), "no new user prompt is sent");
        }
    }
}
#[test]
fn free_text_enter_answers_without_sending_the_composer_and_cancel_is_separate() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let mut dialog = demo_dialog();
    let mut payload: Value = serde_json::from_str(dialog["placeholder"].as_str().unwrap()).unwrap();
    payload["questions"] = json!([{"id":"free","question":"Any constraints?","options":[]}]);
    dialog["placeholder"] = payload.to_string().into();
    app.state.dialogs = [dialog].into();
    for _ in 0..3 {
        frame(&mut app, 860.0, 620.0, vec![]);
    }
    frame(
        &mut app,
        860.0,
        620.0,
        vec![egui::Event::Text("Keep it local".into())],
    );
    assert_eq!(app.questions.drafts[0], "Keep it local");
    frame(&mut app, 860.0, 620.0, vec![key(egui::Key::Enter)]);
    assert!(app.state.dialogs.is_empty());
    assert!(app.deliveries.is_empty());
    assert_eq!(app.draft, "Keep my unsent message");
    app.state.dialogs.push_back(demo_dialog());
    for _ in 0..2 {
        frame(&mut app, 860.0, 620.0, vec![]);
    }
    frame(&mut app, 860.0, 620.0, vec![key(egui::Key::Escape)]);
    assert!(app.state.dialogs.is_empty());
    assert!(app.state.busy);
}

#[test]
fn shortcut_badges_align_descriptions_and_numbers_never_submit_while_typing() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    for _ in 0..3 {
        frame(&mut app, 1000.0, 800.0, vec![]);
    }
    let output = texts(frame(&mut app, 1000.0, 800.0, vec![]));
    let title = output
        .iter()
        .find(|(t, _, _)| t == "New task action (Recommended)")
        .unwrap();
    let description = output
        .iter()
        .find(|(t, _, _)| t.starts_with("A dedicated entry point."))
        .unwrap();
    assert!((title.1.left() - description.1.left()).abs() < 0.5);
    assert!(
        output.iter().any(|(t, _, _)| t == "4"),
        "Something else has its own shortcut"
    );
    frame(&mut app, 1000.0, 800.0, vec![key(egui::Key::Num4)]);
    frame(&mut app, 1000.0, 800.0, vec![]);
    assert!(app.questions.editing);
    frame(
        &mut app,
        1000.0,
        800.0,
        vec![key(egui::Key::Num2), egui::Event::Text("2 tests".into())],
    );
    assert_eq!(
        app.questions.page, 0,
        "numbers in the editor are text, not choices"
    );
    assert_eq!(app.questions.drafts[0], "2 tests");
    frame(&mut app, 1000.0, 800.0, vec![key(egui::Key::Enter)]);
    assert_eq!(app.questions.page, 1);
    assert_eq!(app.questions.answers[0].as_ref().unwrap().answer, "2 tests");
    assert_eq!(app.draft, "Keep my unsent message");
}

#[test]
fn multi_select_footer_and_editor_stay_compact_across_frames_and_resizes() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    for _ in 0..3 {
        frame(&mut app, 1480.0, 960.0, vec![]);
    }
    frame(&mut app, 1480.0, 960.0, vec![key(egui::Key::Num1)]);
    frame(&mut app, 1480.0, 960.0, vec![]);
    frame(&mut app, 1480.0, 960.0, vec![key(egui::Key::Num1)]);
    assert_eq!(app.questions.page, 2);
    for (width, height) in [(1480.0, 960.0), (860.0, 620.0), (1200.0, 800.0)] {
        let mut positions = vec![];
        for _ in 0..40 {
            let output = texts(frame(&mut app, width, height, vec![]));
            let last = output
                .iter()
                .find(|(t, _, _)| t == "Write your own answer")
                .unwrap()
                .1;
            let submit = output
                .iter()
                .find(|(t, _, _)| t == "Submit & continue")
                .unwrap();
            assert!(
                submit.2.contains_rect(submit.1),
                "submit must remain visible"
            );
            assert!(
                submit.1.top() - last.bottom() < 100.0,
                "footer expanded into empty space"
            );
            positions.push(submit.1.top());
        }
        assert!(
            positions[4..]
                .iter()
                .all(|y| (y - positions[4]).abs() < 0.5),
            "height grows across frames: {positions:?}"
        );
    }
    frame(&mut app, 1200.0, 800.0, vec![key(egui::Key::Num4)]);
    for _ in 0..3 {
        frame(&mut app, 1200.0, 800.0, vec![]);
    }
    let output = texts(frame(&mut app, 1200.0, 800.0, vec![]));
    let hint = output
        .iter()
        .find(|(t, _, _)| t.starts_with("Share your preference"))
        .unwrap();
    assert!(
        hint.2.contains_rect(hint.1),
        "the custom input must scroll into view"
    );
}

#[test]
fn ninth_shortcut_opens_custom_answer_with_eight_choices() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let mut dialog = demo_dialog();
    let mut payload: Value = serde_json::from_str(dialog["placeholder"].as_str().unwrap()).unwrap();
    payload["questions"] = json!([{"id":"eight","question":"Pick one","options":(1..=8).map(|n| json!({"label":format!("Option {n}")})).collect::<Vec<_>>()}]);
    dialog["placeholder"] = payload.to_string().into();
    app.state.dialogs = [dialog].into();
    for _ in 0..3 {
        frame(&mut app, 1000.0, 800.0, vec![]);
    }
    frame(&mut app, 1000.0, 800.0, vec![key(egui::Key::Num9)]);
    frame(&mut app, 1000.0, 800.0, vec![]);
    assert!(app.questions.editing);
    assert!(!app.state.dialogs.is_empty());
}

#[test]
fn keyboard_answers_and_chat_switches_preserve_the_composer_and_question_state() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    for _ in 0..3 {
        frame(&mut app, 860.0, 620.0, vec![]);
    }
    frame(&mut app, 860.0, 620.0, vec![key(egui::Key::Num2)]);
    assert_eq!(app.questions.page, 1);
    let original = Session {
        path: PathBuf::from("/fixture/questions"),
        cwd: app.prefs.cwd.clone(),
        project: app.prefs.cwd.clone(),
        title: "Questions".into(),
        created: None,
        modified: std::time::SystemTime::now(),
    };
    let other = Session {
        path: PathBuf::from("/fixture/other"),
        title: "Other".into(),
        ..original.clone()
    };
    app.open_chat(other);
    assert_eq!(app.questions.page, 0);
    app.open_chat(original);
    assert_eq!(app.questions.page, 1);
    frame(&mut app, 860.0, 620.0, vec![]);
    frame(&mut app, 860.0, 620.0, vec![key(egui::Key::Num1)]);
    assert_eq!(app.questions.page, 2);
    frame(&mut app, 860.0, 620.0, vec![]);
    frame(&mut app, 860.0, 620.0, vec![key(egui::Key::Num1)]);
    frame(&mut app, 860.0, 620.0, vec![key(egui::Key::Num3)]);
    assert_eq!(app.questions.checked[2], vec![0, 2]);
    frame(&mut app, 860.0, 620.0, vec![key(egui::Key::Enter)]);
    assert!(app.state.dialogs.is_empty());
    assert_eq!(app.draft, "Keep my unsent message");
    assert!(app.deliveries.is_empty());
}
