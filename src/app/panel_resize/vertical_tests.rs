use super::*;
use crate::app::files::tests::app;
fn frame(ctx: &egui::Context, app: &mut Pui, events: Vec<egui::Event>) {
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(620.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    );
}
fn event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
#[test]
fn terminal_files_changes_edges_resize_and_escape_without_dispatch() {
    for pane in 0..3 {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.prefs.hide_sidebar = true;
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.busy = true;
        app.draft = "keep vertical draft".into();
        let name = match pane {
            0 => {
                app.terminals.toggle(&ctx, &app.prefs.cwd, true);
                "project-terminal"
            }
            1 => {
                app.files.toggle(&ctx, &app.prefs.cwd);
                "workspace-files-stack"
            }
            _ => {
                app.inspector_panel = true;
                "workspace-changes-stack"
            }
        };
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        let panel = egui::Id::new(name);
        let before = egui::containers::panel::PanelState::load(&ctx, panel)
            .unwrap()
            .rect;
        let edge = panel.with("__resize");
        app.focus_composer = false;
        ctx.memory_mut(|memory| memory.request_focus(edge));
        frame(&ctx, &mut app, vec![]);
        frame(
            &ctx,
            &mut app,
            vec![event(egui::Key::ArrowUp, egui::Modifiers::NONE)],
        );
        let after = egui::containers::panel::PanelState::load(&ctx, panel)
            .unwrap()
            .rect;
        assert!(
            after.height() >= before.height() + 7.0,
            "{name} {before:?} {after:?}"
        );
        assert!(
            ctx.memory(|memory| memory.has_focus(edge)),
            "{name} focused {:?} edge {:?}",
            ctx.memory(|m| m.focused()),
            edge
        );
        let start = after.center_top();
        for (pos, pressed) in [
            (start, true),
            (start - egui::vec2(0.0, 24.0), true),
            (start - egui::vec2(0.0, 24.0), false),
        ] {
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
        let dragged = egui::containers::panel::PanelState::load(&ctx, panel)
            .unwrap()
            .rect;
        assert!(
            dragged.height() > after.height() + 10.0,
            "{name} pointer resize {after:?} {dragged:?}"
        );
        ctx.memory_mut(|memory| memory.request_focus(edge));
        frame(&ctx, &mut app, vec![]);
        frame(
            &ctx,
            &mut app,
            vec![
                event(egui::Key::Enter, egui::Modifiers::COMMAND),
                event(egui::Key::Backspace, egui::Modifiers::COMMAND),
                event(egui::Key::Escape, egui::Modifiers::NONE),
            ],
        );
        assert_eq!(app.draft, "keep vertical draft");
        assert!(app.state.busy);
        assert!(!ctx.memory(|memory| memory.has_focus(edge)));
        for command in commands.try_iter() {
            assert_ne!(command["type"], "prompt");
            assert_ne!(command["type"], "abort");
        }
    }
}
