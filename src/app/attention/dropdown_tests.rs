use super::tests::{app, frame, key, pointer};
use super::*;

fn setup(
    mode: theme::Theme,
    zoom: f32,
    hidden: bool,
) -> (egui::Context, Pui, mpsc::Receiver<Value>) {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.prefs.theme = mode;
    app.prefs.hide_sidebar = hidden;
    ctx.set_zoom_factor(zoom);
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let owner = app.ensure_agent();
    app.agents.nodes.get_mut(&owner).unwrap().name = "Unread source".into();
    app.observe_attention(Kind::Completed);
    app.park_tree();
    app.ensure_agent();
    app.draft = "keep draft".into();
    app.state.busy = true;
    app.state.dialogs.push_back(json!({"type":"extension_ui_request","method":"confirm","id":"approval","title":"Allow bash?","message":"{}\n\nApprove this call only."}));
    app.attention.trigger_owner = app.attention_owner();
    app.attention.open = true;
    app.attention.focus_close = true;
    (ctx, app, commands)
}

#[test]
fn dropdown_is_bounded_anchored_compact_and_marks_read_without_navigation() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            for hidden in [true, false] {
                let (ctx, mut app, commands) = setup(mode, zoom, hidden);
                let size = Vec2::new(440.0, 600.0);
                for _ in 0..5 {
                    frame(&mut app, vec![], size);
                }
                let popup = app.attention.popup_rect.unwrap();
                let anchor = app.attention.anchor.unwrap().0;
                assert!(
                    ctx.screen_rect().expand(1.0).contains_rect(popup),
                    "{popup:?} {:?}",
                    ctx.screen_rect()
                );
                assert!(
                    popup.height() < 220.0,
                    "single update should be compact: {popup:?}"
                );
                assert!(
                    (popup.top() - anchor.bottom()).abs() < 20.0,
                    "must open beneath clock: {popup:?} {anchor:?}"
                );
                let current = app.agents.active.clone();
                let mark = ctx
                    .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("attention-mark-read")))
                    .unwrap();
                for pressed in [true, false] {
                    frame(&mut app, pointer(mark.center(), pressed), size);
                }
                assert_eq!(app.attention.unread(), 0);
                assert!(app.attention.open);
                assert_eq!(app.agents.active, current);
                assert_eq!(app.draft, "keep draft");
                assert_eq!(app.state.dialogs.len(), 1);
                assert!(!commands.try_iter().any(|c| matches!(
                    protocol::string(&c, "type"),
                    "extension_ui_response" | "prompt" | "abort"
                )));
                app.shutdown_agents();
            }
        }
    }
}

#[test]
fn resized_dropdown_reanchors_in_the_same_frame() {
    let (_, mut app, _) = setup(theme::Theme::Dark, 1.0, false);
    for _ in 0..4 {
        frame(&mut app, vec![], Vec2::new(1000.0, 800.0));
    }
    app.prefs.hide_sidebar = true;
    frame(&mut app, vec![], Vec2::new(460.0, 600.0));
    let popup = app.attention.popup_rect.unwrap();
    let anchor = app.attention.anchor.unwrap().0;
    assert!(
        (popup.top() - anchor.bottom()).abs() < 20.0,
        "{popup:?} {anchor:?}"
    );
    assert!(app.ctx.screen_rect().expand(1.0).contains_rect(popup));
    app.shutdown_agents();
}

#[test]
fn dropdown_tab_stays_inside_and_close_returns_trigger_focus() {
    let (ctx, mut app, commands) = setup(theme::Theme::Dark, 1.0, false);
    let size = Vec2::new(860.0, 650.0);
    for _ in 0..4 {
        frame(&mut app, vec![], size);
    }
    for modifiers in [egui::Modifiers::NONE, egui::Modifiers::SHIFT] {
        for _ in 0..12 {
            frame(&mut app, vec![key(egui::Key::Tab, modifiers)], size);
            assert!(
                ctx.memory(|m| m.focused())
                    .is_some_and(|id| app.attention.focus_ids.contains(&id))
            );
        }
    }
    app.attention.trigger_focus = app.attention.last_button;
    let (_, close) = ctx
        .data(|d| d.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("attention-close")))
        .unwrap();
    for pressed in [true, false] {
        frame(&mut app, pointer(close.center(), pressed), size);
    }
    assert!(!app.attention.open);
    assert!(ctx.memory(|m| m.focused()) == app.attention.last_button);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy);
    assert!(!commands.try_iter().any(|c| matches!(
        protocol::string(&c, "type"),
        "extension_ui_response" | "prompt" | "abort"
    )));
    app.shutdown_agents();
}

#[test]
fn outside_click_dismisses_without_approving_underlying_question() {
    for combined in [false, true] {
        let (ctx, mut app, commands) = setup(theme::Theme::Dark, 1.0, false);
        let size = Vec2::new(1000.0, 800.0);
        let mut output = frame(&mut app, vec![], size);
        for _ in 0..5 {
            output = frame(&mut app, vec![], size);
        }
        let pos = files::tests::text(&output, "Allow once");
        assert!(!app.attention.popup_rect.unwrap().contains(pos));
        if combined {
            let mut events = pointer(pos, true);
            events.extend(pointer(pos, false));
            frame(&mut app, events, size);
        } else {
            for pressed in [true, false] {
                frame(&mut app, pointer(pos, pressed), size);
            }
        }
        assert!(!app.attention.open);
        assert_eq!(
            app.state.dialogs.len(),
            1,
            "outside click only dismisses the dropdown"
        );
        assert_eq!(app.draft, "keep draft");
        assert!(app.state.busy);
        assert!(!egui::Popup::is_id_open(&ctx, ui::popup_id()));
        assert!(!commands.try_iter().any(|c| matches!(
            protocol::string(&c, "type"),
            "extension_ui_response" | "prompt" | "abort"
        )));
        app.shutdown_agents();
    }
}
