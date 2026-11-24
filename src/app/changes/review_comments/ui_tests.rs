use super::*;
use crate::app::files::tests::{app, frame};
const DIFF: &str = "@@ -1 +1 @@\n-old value\n+chosen 界e\u{301} value\n";
pub(super) fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
pub(super) fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
pub(super) fn click(ctx: &egui::Context, app: &mut Pui, name: &str) {
    let rect = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(name)))
        .unwrap();
    for pressed in [true, false] {
        frame(ctx, app, pointer(rect.center(), pressed));
    }
}
pub(super) fn setup() -> (egui::Context, Pui) {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.prefs.hide_sidebar = true;
    app.prefs.hide_inspector = true;
    app.draft = "Kept draft".into();
    app.context_chips.clear_quotes();
    app.state.busy = true;
    app.open_diff_review(("src/local-review.rs".into(), DIFF.into()).into(), true);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app)
}
pub(super) fn select(ctx: &egui::Context, app: &mut Pui, chosen: &str) {
    let out = frame(ctx, app, vec![]);
    let t = out
        .shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                t.galley.job.text.contains("+chosen").then_some(t)
            } else {
                None
            }
        })
        .unwrap();
    let raw = &t.galley.job.text;
    let start = raw[..raw.find(chosen).unwrap()].chars().count();
    let at = |i| {
        t.pos
            + t.galley
                .pos_from_cursor(egui::text::CCursor::new(i))
                .center()
                .to_vec2()
    };
    let a = at(start);
    let b = at(start + chosen.chars().count());
    frame(ctx, app, pointer(a, true));
    frame(ctx, app, vec![egui::Event::PointerMoved(b)]);
    frame(ctx, app, pointer(b, false));
}
#[test]
fn actual_range_comment_resolve_reopen_batch_is_local_and_never_approves_or_sends() {
    let (ctx, mut app) = setup();
    let permission = json!({"id":"review-permission","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."});
    app.state.dialogs.push_back(permission);
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let messages = app.state.messages.len();
    select(&ctx, &mut app, "chosen 界e\u{301}");
    click(&ctx, &mut app, "review-comment-add");
    assert!(app.changes.review_comments.editor.is_some());
    assert!(!egui::Popup::is_any_open(&ctx));
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("Please clarify this branch".into())],
    );
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
    );
    assert_eq!(app.outgoing_text(), "Kept draft");
    assert!(app.pending_dialog.is_none());
    click(&ctx, &mut app, "review-comment-save");
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.changes.review_comments.items.len(), 1);
    assert_eq!(
        app.changes.review_comments.items[0].anchor.selection.rows,
        (3, 3)
    );
    assert_eq!(app.changes.review.as_ref().unwrap().1, DIFF);
    let resolve = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("review-comment-resolve", 0usize))))
        .unwrap();
    for pressed in [true, false] {
        frame(&ctx, &mut app, pointer(resolve.center(), pressed));
    }
    assert!(app.changes.review_comments.items[0].resolved);
    for pressed in [true, false] {
        frame(&ctx, &mut app, pointer(resolve.center(), pressed));
    }
    assert!(!app.changes.review_comments.items[0].resolved);
    click(&ctx, &mut app, "review-comment-batch");
    assert!(
        app.outgoing_text()
            .starts_with("Kept draft\n\n[Context: Local review comments")
    );
    assert!(app.outgoing_text().contains("Please clarify this branch"));
    assert!(app.outgoing_text().contains("SHA-256:"));
    assert!(
        !app.outgoing_text().contains("+chosen"),
        "No duplicate source body in notes"
    );
    assert_eq!(app.state.messages.len(), messages);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.pending_dialog.is_none());
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
}
#[test]
fn chord_entry_first_tab_escape_and_disabled_modal_preserve_review_and_draft() {
    let (ctx, mut app) = setup();
    select(&ctx, &mut app, "chosen");
    frame(
        &ctx,
        &mut app,
        vec![key(
            egui::Key::M,
            egui::Modifiers {
                alt: true,
                ..egui::Modifiers::COMMAND
            },
        )],
    );
    assert!(app.changes.review_comments.editor.is_some());
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
    );
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(app.changes.review_comments.editor.is_none());
    assert!(app.changes.review.is_some());
    assert_eq!(app.outgoing_text(), "Kept draft");
    // A modal owns the chord; it cannot create a review comment underneath it.
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("test-review-blocker"), 200.0, |ui| {
            ui.label("Permission scope");
        });
    });
    let document = app.changes.review.as_ref().unwrap();
    assert!(!app.changes.review_comments.inputs(&ctx, document));
    assert!(!app.changes.review_comments.request_add);
}
#[test]
fn clear_requires_confirmation_escape_cancels_and_explicit_confirm_clears_all_owners() {
    let (ctx, mut app) = setup();
    select(&ctx, &mut app, "chosen");
    click(&ctx, &mut app, "review-comment-add");
    frame(&ctx, &mut app, vec![egui::Event::Text("Local note".into())]);
    click(&ctx, &mut app, "review-comment-save");
    frame(&ctx, &mut app, vec![]);
    click(&ctx, &mut app, "review-comment-clear");
    assert!(app.changes.review_comments.clear_requested);
    assert_eq!(app.changes.review_comments.items.len(), 1);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!app.changes.review_comments.clear_requested);
    assert_eq!(app.changes.review_comments.items.len(), 1);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    click(&ctx, &mut app, "review-comment-clear");
    assert!(
        app.changes.review_comments.clear_requested,
        "reopen confirmation"
    );
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let out = frame(&ctx, &mut app, vec![]);
    let confirm = out
        .shapes
        .iter()
        .rev()
        .find_map(|shape| {
            if let egui::Shape::Text(t) = &shape.shape {
                (t.galley.job.text == "Clear all").then_some(t.pos + t.galley.size() / 2.0)
            } else {
                None
            }
        })
        .unwrap();
    for pressed in [true, false] {
        frame(&ctx, &mut app, pointer(confirm, pressed));
    }
    assert!(app.changes.review_comments.items.is_empty());
    assert!(!app.changes.review_comments.clear_requested);
    assert_eq!(app.outgoing_text(), "Kept draft");
}
#[test]
fn narrow_theme_zoom_review_form_is_inline_bounded_and_does_not_duplicate_filename() {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            let ctx = egui::Context::default();
            ctx.set_theme(theme);
            ctx.set_zoom_factor(zoom);
            let mut document: diff_viewer::Document = ("src/local.rs".into(), DIFF.into()).into();
            let mut notes = Comments {
                source_root: Some(PathBuf::from("/repo")),
                ..Default::default()
            };
            notes.bind(Path::new("/repo"), "session", 1);
            let range = super::tests::range(DIFF, "+chosen 界e\u{301} value");
            assert!(notes.begin(
                "src/local.rs",
                DIFF,
                false,
                document.2.comment_digest(),
                range
            ));
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(320.0, 720.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        diff_viewer::contents_with_review(
                            ui,
                            &mut document,
                            None,
                            Some(&mut notes),
                        );
                    });
                },
            );
            let texts = out
                .shapes
                .iter()
                .filter_map(|shape| {
                    if let egui::Shape::Text(t) = &shape.shape {
                        Some(t)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(
                texts
                    .iter()
                    .filter(|t| t.galley.job.text == "local.rs")
                    .count(),
                1
            );
            assert!(texts.iter().any(|t| t.galley.job.text == "Local comment"));
            assert!(!egui::Popup::is_any_open(&ctx));
            assert!(ctx.memory(|memory| memory.top_modal_layer().is_none()));
        }
    }
}
