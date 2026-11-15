use super::ui_tests::{key, pointer, setup};
use super::*;
use crate::app::files::tests::frame;
fn seeded() -> (egui::Context, Pui) {
    let (ctx, mut app) = setup();
    let document = app.changes.review.as_ref().unwrap();
    let (path, diff, digest) = (
        document.0.clone(),
        document.1.clone(),
        document.2.comment_digest(),
    );
    let notes = &mut app.changes.review_comments;
    for text in ["unresolved text", "resolved text"] {
        let start = diff.find("+chosen").unwrap();
        let chars = diff[..start].chars().count();
        assert!(notes.begin(&path, &diff, true, digest, chars..chars + 7));
        notes.editor.as_mut().unwrap().text = text.into();
        assert!(notes.save());
    }
    notes.items[1].resolved = true;
    notes.expanded = true;
    notes.storage_dirty = false;
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app)
}
fn filter(ctx: &egui::Context, only: bool) -> (egui::Id, egui::Rect) {
    ctx.data(|data| data.get_temp(egui::Id::new(("review-note-filter", only))))
        .unwrap()
}
fn choose(ctx: &egui::Context, app: &mut Pui, only: bool) {
    let pos = filter(ctx, only).1.center();
    for pressed in [true, false] {
        frame(ctx, app, pointer(pos, pressed));
    }
    frame(ctx, app, vec![]);
}
#[test]
fn unresolved_view_hides_only_resolved_notes_without_mutating_batch_storage_editor_or_draft() {
    let (ctx, mut app) = seeded();
    let before = app.changes.review_comments.batch().unwrap();
    let id = app.changes.review_comments.items[1].id;
    assert!(app.changes.review_comments.begin_edit(id));
    choose(&ctx, &mut app, true);
    let notes = &app.changes.review_comments;
    assert!(notes.unresolved_only);
    assert_eq!(notes.items.len(), 2);
    assert!(notes.items[1].resolved);
    assert_eq!(notes.batch().unwrap(), before);
    assert!(!notes.storage_dirty);
    assert!(notes.editor.is_some());
    assert_eq!(app.draft, "Kept draft");
    choose(&ctx, &mut app, false);
    assert!(!app.changes.review_comments.unresolved_only);
}
#[test]
fn filter_pointer_chords_native_tab_space_new_question_and_reverse_escape_are_local() {
    let (ctx, mut app) = seeded();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let pos = filter(&ctx, true).1.center();
    app.state.dialogs.push_back(questions::demo_dialog());
    for pressed in [true, false] {
        let mut events = pointer(pos, pressed);
        events.extend([
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
        ]);
        frame(&ctx, &mut app, events);
    }
    assert!(app.changes.review_comments.unresolved_only);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
    assert_eq!(app.draft, "Kept draft");
    let all = filter(&ctx, false).0;
    ctx.memory_mut(|memory| memory.request_focus(all));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
    );
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        Some(filter(&ctx, true).0)
    );
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Space, egui::Modifiers::NONE),
            egui::Event::Text(" ".into()),
        ],
    );
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
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
}
#[test]
fn resolved_marker_reveal_switches_to_all_and_owner_change_resets_view() {
    let (ctx, mut app) = seeded();
    let notes = &mut app.changes.review_comments;
    notes.unresolved_only = true;
    let id = notes.items[1].id;
    let document = app.changes.review.as_ref().unwrap();
    notes.reveal_note = Some((id, document.2.focus_id(), ctx.cumulative_pass_nr()));
    frame(&ctx, &mut app, vec![]);
    assert!(!app.changes.review_comments.unresolved_only);
    let notes = &mut app.changes.review_comments;
    notes.unresolved_only = true;
    notes.bind(Path::new("/other"), "other", 1);
    assert!(!notes.unresolved_only);
    assert_eq!(notes.items.len(), 2);
}
#[test]
fn filter_narrow_themes_zoom_and_settled_frames_keep_bounded_payload() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app) = seeded();
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            app.changes.review_comments.unresolved_only = true;
            let bytes = app.changes.review_comments.bytes();
            let start = std::time::Instant::now();
            for _ in 0..15 {
                let mut native = eframe::Frame::_new_kittest();
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(620.0, 850.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut native),
                );
                assert!(output.platform_output.commands.is_empty());
            }
            let (_, rect) = filter(&ctx, true);
            assert!(ctx.screen_rect().contains_rect(rect));
            assert_eq!(app.changes.review_comments.bytes(), bytes);
            assert!(!app.changes.review_comments.storage_dirty);
            eprintln!(
                "15filterednoteappframes {:?}, payload{}B",
                start.elapsed(),
                bytes
            );
        }
    }
}
#[test]
fn all_resolved_filter_is_empty_and_foreign_modal_cannot_toggle() {
    let (ctx, mut app) = seeded();
    app.changes
        .review_comments
        .items
        .iter_mut()
        .for_each(|note| note.resolved = true);
    choose(&ctx, &mut app, true);
    assert!(app.changes.review_comments.unresolved_only);
    assert!(app.changes.review_comments.batch().is_err());
    let id = filter(&ctx, false).0;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("foreign-note-filter"), 220.0, |ui| {
            prompt_ui::heading(ui, "Information", "Foreign")
        });
    });
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(app.changes.review_comments.unresolved_only);
}
