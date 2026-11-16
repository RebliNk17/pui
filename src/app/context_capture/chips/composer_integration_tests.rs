//! Integration coverage for preserved concurrent formatting callers.
use super::*;
use crate::app::files::tests::{app, frame};
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
fn select(ctx: &egui::Context) {
    let id = egui::Id::new("composer");
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(3),
        )));
    state.store(ctx, id);
    ctx.memory_mut(|m| m.request_focus(id));
}
#[test]
fn formatting_shortcuts_detach_capture_metadata_without_damaging_context_text() {
    for code in [egui::Key::B, egui::Key::I, egui::Key::U] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.draft = "prefix".into();
        queue(&ctx, "formatting source", "literal body");
        app.collect_context_capture();
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        select(&ctx);
        frame(&ctx, &mut app, vec![key(code, egui::Modifiers::COMMAND)]);
        assert!(app.context_chips.chips.is_empty());
        assert!(app.draft.contains("literal body"));
        assert!(app.draft.contains("[Context: formatting source]"));
    }
}
#[test]
fn handled_list_indent_detaches_but_unhandled_tab_keeps_association() {
    for list in [false, true] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.draft = if list { "1. item" } else { "plain" }.into();
        queue(&ctx, "list source", "literal body");
        app.collect_context_capture();
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        select(&ctx);
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
        );
        assert_eq!(app.context_chips.chips.is_empty(), list);
        assert!(app.draft.contains("literal body"));
    }
}
