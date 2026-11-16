use super::*;
use crate::app::files::tests::{app, frame};
#[test]
fn focused_sidebar_edge_escape_and_send_stop_chords_never_dispatch() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.busy = true;
    app.draft = "keep resize draft".into();
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let id = egui::Id::new("sessions-v4").with("__resize");
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(&ctx, &mut app, vec![]);
    for key in [egui::Key::Enter, egui::Key::Backspace, egui::Key::Escape] {
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: if key == egui::Key::Escape {
                    egui::Modifiers::NONE
                } else {
                    egui::Modifiers::COMMAND
                },
            }],
        );
    }
    assert_eq!(app.draft, "keep resize draft");
    assert!(app.state.busy);
    assert!(!ctx.memory(|memory| memory.has_focus(id)));
    for command in commands.try_iter() {
        assert_ne!(command["type"], "prompt");
        assert_ne!(command["type"], "abort");
    }
}
