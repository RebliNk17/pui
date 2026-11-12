use super::*;
use crate::app::files::tests as file_tests;
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
fn pointer(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
fn at(output: &egui::FullOutput, index: usize) -> egui::Pos2 {
    let text = output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                text.galley
                    .job
                    .text
                    .contains("+new selected")
                    .then_some(text)
            } else {
                None
            }
        })
        .unwrap();
    text.pos
        + text
            .galley
            .pos_from_cursor(egui::text::CCursor::new(index))
            .center()
            .to_vec2()
}
#[test]
fn frozen_selection_popup_keeps_file_keys_then_explicit_navigation_clears_old_selection() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    app.prefs.hide_sidebar = true;
    app.draft = "Keep".into();
    app.state.busy = true;
    for _ in 0..2 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    let diff = "@@ -1 +1 @@\n-old\n+new selected\n";
    app.outputs.push(RecordedFile {
        path: "selected.rs".into(),
        diff: diff.into(),
        ..Default::default()
    });
    app.open_diff_review(("selected.rs".into(), diff.into()).into(), true);
    for _ in 0..4 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    let out = file_tests::frame(&ctx, &mut app, vec![]);
    let text = out
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(t) = &shape.shape {
                t.galley
                    .job
                    .text
                    .contains("+new selected")
                    .then_some(&t.galley.job.text)
            } else {
                None
            }
        })
        .unwrap();
    let start = text[..text.find("+new selected").unwrap()].chars().count();
    let a = at(&out, start);
    let b = at(&out, start + 13);
    file_tests::frame(
        &ctx,
        &mut app,
        pointer(a, egui::PointerButton::Primary, true),
    );
    file_tests::frame(&ctx, &mut app, vec![egui::Event::PointerMoved(b)]);
    file_tests::frame(
        &ctx,
        &mut app,
        pointer(b, egui::PointerButton::Primary, false),
    );
    assert!(egui::text_selection::LabelSelectionState::load(&ctx).has_selection());
    for pressed in [true, false] {
        file_tests::frame(
            &ctx,
            &mut app,
            pointer(a, egui::PointerButton::Secondary, pressed),
        );
    }
    for _ in 0..3 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    assert!(egui::Popup::is_any_open(&ctx));
    file_tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::ArrowRight, egui::Modifiers::ALT)],
    );
    assert_eq!(app.changes.review.as_ref().unwrap().0, "selected.rs");
    assert_eq!(app.draft, "Keep");
    file_tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!egui::Popup::is_any_open(&ctx));
    file_tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::ArrowRight, egui::Modifiers::ALT)],
    );
    assert_ne!(app.changes.review.as_ref().unwrap().0, "selected.rs");
    assert!(!egui::text_selection::LabelSelectionState::load(&ctx).has_selection());
    assert_eq!(app.draft, "Keep");
    assert!(app.state.busy);
}
