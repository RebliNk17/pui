use super::*;
use crate::app::files::tests::{app, frame};
fn setup() -> (egui::Context, Pui) {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let source = "<svg width='16' height='8'/>".to_owned();
    let path = app.prefs.cwd.join("static.svg");
    let preview = Preview {
        retained: source.capacity(),
        source,
        pixels: None,
        error: Some("Unsupported fixture; Source remains available.".into()),
        controls: [None; 3],
        keyboard_pass: None,
    };
    let lease = budget::Lease::new(&ctx);
    lease.reserve(preview.retained_bytes()).unwrap();
    app.files.workspace(&ctx, &app.prefs.cwd);
    app.files.open = true;
    app.files.browser_mode = false;
    app.files.selected = Some(path.clone());
    app.files.tabs.push(OpenFile {
        path,
        content: Some(Ok(Content::Svg(Box::new(preview)))),
        rx: None,
        texture: None,
        source: false,
        external: false,
        line_target: None,
        line_notice: None,
        _budget: lease,
        _load: None,
    });
    app.prefs.hide_sidebar = true;
    app.focus_composer = false;
    app.draft = "Kept draft".into();
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app)
}
fn controls(app: &Pui) -> [egui::Id; 2] {
    let Some(Ok(Content::Svg(preview))) = &app.files.tabs.last().unwrap().content else {
        panic!()
    };
    [preview.controls[0].unwrap(), preview.controls[1].unwrap()]
}
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
#[test]
fn svg_foreign_popup_hidden_panel_and_replaced_preview_do_not_claim_input() {
    let (ctx, mut app) = setup();
    let owner = controls(&app)[1];
    ctx.memory_mut(|memory| memory.request_focus(owner));
    egui::Popup::open_id(&ctx, egui::Id::new("foreign-svg-popup"));
    let event = key(egui::Key::Enter, egui::Modifiers::COMMAND);
    let _ = ctx.run(
        egui::RawInput {
            events: vec![event],
            ..Default::default()
        },
        |ctx| {
            let before = ctx.input(|input| input.events.clone());
            app.files.context_inputs(ctx, true, true);
            assert_eq!(ctx.input(|input| input.events.clone()), before);
            assert!(!app.files.svg_owns_keyboard(ctx));
        },
    );
    egui::Popup::close_all(&ctx);
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
            ..Default::default()
        },
        |ctx| {
            let before = ctx.input(|input| input.events.clone());
            app.files.context_inputs(ctx, false, true);
            assert_eq!(ctx.input(|input| input.events.clone()), before);
            assert!(!app.files.svg_owns_keyboard(ctx));
        },
    );
    app.files.tabs.last_mut().unwrap().content = Some(Ok(Content::Text("replacement".into())));
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
            ..Default::default()
        },
        |ctx| {
            let before = ctx.input(|input| input.events.clone());
            app.files.context_inputs(ctx, true, true);
            assert_eq!(ctx.input(|input| input.events.clone()), before);
            assert!(!app.files.svg_owns_keyboard(ctx));
        },
    );
}

#[test]
fn svg_control_pointer_chords_and_plain_activation_never_send_approve_stop() {
    for pressed_chord in [true, false] {
        let (ctx, mut app) = setup();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.busy = true;
        app.state.dialogs.push_back(json!({"id":"svg-approval","method":"confirm","title":"Allow bash?","message":"Controlled fixture"}));
        let id = controls(&app)[1];
        let pos = ctx.read_response(id).unwrap().interact_rect.center();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
        for pressed in [true, false] {
            let mut events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            if pressed == pressed_chord {
                events.extend([
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                    key(egui::Key::Enter, egui::Modifiers::NONE),
                    key(egui::Key::Space, egui::Modifiers::NONE),
                    egui::Event::Text(" ".into()),
                ]);
            }
            frame(&ctx, &mut app, events);
        }
        assert!(app.files.tabs.last().unwrap().source);
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
        assert!(app.state.busy);
        assert_eq!(app.draft, "Kept draft");
    }
}
#[test]
fn svg_native_tab_reverse_escape_and_new_question_do_not_reuse_keys_or_steal_focus() {
    let (ctx, mut app) = setup();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.busy = true;
    let ids = controls(&app);
    ctx.memory_mut(|memory| memory.request_focus(ids[0]));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
    );
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(ids[1]));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
    );
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(ctx.memory(|memory| memory.focus_lost_by_escape(ids[0])));
    assert!(app.state.busy);
    ctx.memory_mut(|memory| memory.request_focus(ids[1]));
    let mut dialog = questions::demo_dialog();
    let mut payload: Value = serde_json::from_str(dialog["placeholder"].as_str().unwrap()).unwrap();
    payload["questions"] = json!([payload["questions"][0].clone()]);
    dialog["placeholder"] = json!(payload.to_string());
    app.state.dialogs.push_back(dialog);
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Space, egui::Modifiers::NONE),
            egui::Event::Text(" ".into()),
        ],
    );
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.pending_dialog.is_none());
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(ids[1]));
    assert!(commands.try_recv().is_err());
    assert_eq!(app.draft, "Kept draft");
    assert!(app.state.busy);
}
