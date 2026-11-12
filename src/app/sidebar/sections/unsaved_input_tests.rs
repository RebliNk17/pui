use super::*;
use crate::app::files::tests::{app, click, frame};
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
fn menu_opening_frame_modified_keys_never_approve_or_deny_in_underlying_chat() {
    for secondary in [false, true] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.sessions.clear();
        app.state = State::default();
        let id = app.ensure_agent();
        app.prefs
            .sidebar_view
            .sections
            .push(Section::new("Unsent".into()));
        app.state.busy = true;
        app.draft = "keep".into();
        app.state.dialogs.push_back(json!({"id":"pathless-opening-approval","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo safe\"}\n\nApprove this call only."}));
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        let branch = format!("agent:{id}");
        let menu_id = ctx
            .data(|data| {
                data.get_temp::<egui::Id>(egui::Id::new(("sidebar-unsaved-menu-id", &branch)))
            })
            .unwrap();
        if secondary {
            let row = ctx
                .data(|data| {
                    data.get_temp::<egui::Rect>(egui::Id::new(("sidebar-row-rect", &branch)))
                })
                .unwrap();
            let pos = row.left_center() + egui::vec2(40.0, 0.0);
            frame(
                &ctx,
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Secondary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            frame(
                &ctx,
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Secondary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                ],
            );
        } else {
            ctx.memory_mut(|memory| {
                memory.request_focus_with_filter(
                    menu_id,
                    egui::EventFilter {
                        escape: true,
                        ..Default::default()
                    },
                )
            });
            frame(
                &ctx,
                &mut app,
                vec![
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                ],
            );
        }
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
        assert!(app.state.busy);
        assert_eq!(app.draft, "keep");
    }
}
#[test]
fn hidden_sidebar_owner_dismisses_only_its_section_popup() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.sessions.clear();
    app.state = State::default();
    let id = app.ensure_agent();
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let branch = format!("agent:{id}");
    let rect = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("sidebar-unsaved-menu", &branch))))
        .unwrap();
    click(&ctx, &mut app, rect.center());
    assert!(app.section_view.unsaved_popup.is_some());
    app.prefs.hide_sidebar = true;
    frame(&ctx, &mut app, vec![]);
    assert!(app.section_view.unsaved_popup.is_none());
    assert!(!egui::Popup::is_any_open(&ctx));
}
