use super::tests::{apply, point};
use super::*;
use crate::app::files::tests as file_tests;

fn app(ctx: &egui::Context) -> Pui {
    let mut app = file_tests::app(ctx);
    let path = app.prefs.cwd.join("projected.csv");
    fs::write(&path, "Name,Note\nBeta,=1+2\nAlpha,שלום\n").unwrap();
    app.files
        .open_target(ctx, &app.prefs.cwd.clone(), path.to_str().unwrap())
        .unwrap();
    let tab = app.files.tabs.last_mut().unwrap();
    tab.content = Some(
        tab.rx
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap(),
    );
    for _ in 0..5 {
        file_tests::frame(ctx, &mut app, vec![]);
    }
    app
}
fn preview(app: &mut Pui) -> &mut Preview {
    let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
        panic!();
    };
    preview
}
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        pressed: true,
        repeat: false,
        physical_key: None,
    }
}
fn right_click(ctx: &egui::Context, app: &mut Pui, pos: egui::Pos2) {
    for pressed in [true, false] {
        file_tests::frame(
            ctx,
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    for _ in 0..3 {
        file_tests::frame(ctx, app, vec![]);
    }
}
#[test]
fn sorted_pointer_quote_retains_actual_original_record_and_literal_value() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    // Apply directly through the same async request path; subsequent interactions
    // use full app frames, not isolated table rendering.
    apply(&ctx, preview(&mut app), "", Order::Ascending);
    for _ in 0..6 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    let rect = ctx
        .data(|data| {
            data.get_temp::<egui::Rect>(egui::Id::new(("delimited-cell", 2_usize, 2_usize, false)))
        })
        .unwrap();
    let viewport = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
        .unwrap();
    assert!(rect.intersect(viewport).is_positive());
    right_click(&ctx, &mut app, rect.intersect(viewport).center());
    let out = file_tests::frame(&ctx, &mut app, vec![]);
    let count = app.state.messages.len();
    file_tests::click(&ctx, &mut app, point(&out, "Add cell to chat"));
    assert!(app.draft.contains("record 2, column 2"), "{}", app.draft);
    assert!(app.draft.contains("=1+2"));
    assert_eq!(app.state.messages.len(), count);
    assert_eq!(
        fs::read_to_string(app.prefs.cwd.join("projected.csv")).unwrap(),
        "Name,Note\nBeta,=1+2\nAlpha,שלום\n"
    );
}
#[test]
fn projection_install_closes_frozen_menu_and_old_same_path_result_cannot_retarget() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let p = preview(&mut app);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    p.projection.before_scan = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    }));
    p.projection.draft = "Alpha".into();
    assert!(
        p.projection
            .start(&ctx, p.source.clone(), p.index.as_ref().unwrap().clone())
    );
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let rect = ctx
        .data(|data| {
            data.get_temp::<egui::Rect>(egui::Id::new(("delimited-cell", 2_usize, 2_usize, false)))
        })
        .unwrap();
    let viewport = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
        .unwrap();
    assert!(rect.intersect(viewport).is_positive());
    right_click(&ctx, &mut app, rect.intersect(viewport).center());
    assert!(egui::Popup::is_any_open(&ctx));
    assert_eq!(
        preview(&mut app).projection.rows(3, true).record(1),
        Some(1)
    );
    release_tx.send(()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while preview(&mut app).projection.pending.is_some() {
        file_tests::frame(&ctx, &mut app, vec![]);
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!egui::Popup::is_any_open(&ctx));
    assert!(app.draft.is_empty());
    assert_eq!(
        preview(&mut app).projection.rows(3, true).record(1),
        Some(2)
    );

    let p = preview(&mut app);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    p.projection.before_scan = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    }));
    assert!(
        p.projection
            .start(&ctx, p.source.clone(), p.index.as_ref().unwrap().clone())
    );
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    *preview(&mut app) = Preview::new("New,Content\nOnly,new\n".into(), b',');
    release_tx.send(()).unwrap();
    for _ in 0..6 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    assert!(preview(&mut app).projection.map.is_none());
    assert_eq!(preview(&mut app).source.as_str(), "New,Content\nOnly,new\n");
    assert!(app.draft.is_empty());
}
#[test]
fn native_tab_entry_and_pointer_press_release_own_toolbar_shortcuts_before_render() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.busy = true;
    app.draft = "Kept".into();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
    let out = file_tests::frame(&ctx, &mut app, vec![]);
    let pos = point(&out, "Filter");
    for pressed in [true, false] {
        let mut events = vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        events.extend([
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            key(egui::Key::Escape, egui::Modifiers::NONE),
        ]);
        file_tests::frame(&ctx, &mut app, events);
        assert!(app.state.busy);
        assert_eq!(app.draft, "Kept");
        assert!(commands.try_recv().is_err());
    }
    assert!(preview(&mut app).projection.expanded);
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
    let controls = preview(&mut app).projection.controls.clone();
    let mut entered = None;
    for _ in 0..100 {
        file_tests::frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
        );
        if let Some(id) = ctx
            .memory(|memory| memory.focused())
            .filter(|id| controls.contains(id))
        {
            entered = Some(id);
            break;
        }
    }
    assert!(
        entered.is_some(),
        "Native Tab must reach a table toolbar control"
    );
    file_tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert_ne!(ctx.memory(|memory| memory.focused()), entered);
    assert!(app.state.busy);
    assert_eq!(app.draft, "Kept");
    assert!(commands.try_recv().is_err());
}
#[test]
fn full_app_toolbar_shortcuts_and_arriving_structured_questions_do_not_send_approve_or_stop() {
    for question in [false, true] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let out = file_tests::frame(&ctx, &mut app, vec![]);
        file_tests::click(&ctx, &mut app, point(&out, "Filter"));
        for _ in 0..3 {
            file_tests::frame(&ctx, &mut app, vec![]);
        }
        app.state.busy = true;
        app.draft = "Kept draft".into();
        let count = app.state.messages.len();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let controls = preview(&mut app).projection.controls.clone();
        for id in controls {
            if !ctx
                .read_response(id)
                .is_some_and(|response| response.enabled())
            {
                continue;
            }
            ctx.memory_mut(|memory| memory.request_focus(id));
            if question {
                let dialog = json!({"id":"table-question", "method":"input", "title":"Pui questions", "placeholder":json!({"version":1, "request_id":"table-request", "questions":[{"id":"pick", "question":"Choose", "multiSelect":true, "options":[{"label":"One"}]}]}).to_string()});
                assert!(questions::Request::parse(&dialog).is_some());
                app.state.dialogs.push_back(dialog);
            } else {
                app.state.dialogs.push_back(json!({"id":"table-approval", "method":"confirm", "title":"Allow bash?", "message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."}));
            }
            file_tests::frame(
                &ctx,
                &mut app,
                vec![
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                    key(egui::Key::Space, egui::Modifiers::NONE),
                    egui::Event::Text(" ".into()),
                    key(egui::Key::Escape, egui::Modifiers::NONE),
                ],
            );
            assert_eq!(app.state.dialogs.len(), 1);
            assert!(commands.try_recv().is_err());
            assert!(app.state.busy);
            assert_eq!(app.draft, "Kept draft");
            assert_eq!(app.state.messages.len(), count);
            app.state.dialogs.clear();
            app.current_dialog.clear();
            app.questions = Default::default();
        }
        assert_eq!(preview(&mut app).projection.started, 0);
    }
}
