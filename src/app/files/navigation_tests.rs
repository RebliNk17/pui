use super::tests::{app, click, frame};
use super::*;

#[cfg(target_os = "macos")]
#[test]
fn address_enter_and_go_preserve_chat_draft_and_live_work() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.prefs.theme = mode;
        app.state.busy = true;
        app.draft = "Do not submit or replace this".into();
        let messages = app.state.messages.len();
        app.files.toggle_browser(&ctx, &app.prefs.cwd);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let (editor, _) = ctx
            .data(|data| {
                data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("browser-address-editor"))
            })
            .unwrap();
        ctx.memory_mut(|memory| memory.request_focus(editor));
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Text("http://localhost:3000/one".into())],
        );
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(
            app.files.selected_url.as_deref(),
            Some("http://localhost:3000/one")
        );
        app.files.address = "http://localhost:3000/two".into();
        frame(&ctx, &mut app, vec![]);
        let go = ctx
            .data(|data| {
                data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", "Go to address")))
            })
            .unwrap()
            .center();
        click(&ctx, &mut app, go);
        assert_eq!(
            app.files.selected_url.as_deref(),
            Some("http://localhost:3000/two")
        );
        assert_eq!(app.draft, "Do not submit or replace this");
        assert_eq!(app.state.messages.len(), messages);
        assert!(app.state.busy);
    }
}
