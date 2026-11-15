use super::*;
use crate::app::changes::review_comments::ui_tests::{key, pointer};
use crate::app::files::tests::frame;
const DIFF: &str = "@@ -1,2 +1,2 @@\n-old\n+chosen 世界\n context\n";
fn app() -> (egui::Context, Pui, u64) {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.prefs.hide_sidebar = true;
    app.focus_composer = false;
    app.draft = "Kept".into();
    app.state.busy = true;
    app.open_diff_review(("marker.rs".into(), DIFF.into()).into(), true);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let digest = app.changes.review.as_ref().unwrap().2.comment_digest();
    let notes = &mut app.changes.review_comments;
    assert!(notes.begin(
        "marker.rs",
        DIFF,
        true,
        digest,
        super::super::tests::range(DIFF, "+chosen 世界")
    ));
    notes.editor.as_mut().unwrap().text = "Exact local note".into();
    assert!(notes.save());
    let id = notes.items[0].id;
    notes.expanded = false;
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app, id)
}
fn marker(ctx: &egui::Context) -> (egui::Id, egui::Rect, usize) {
    ctx.data(|data| {
        data.get_temp::<(egui::Id, egui::Rect, usize)>(egui::Id::new((
            "review-note-marker",
            2usize,
        )))
    })
    .unwrap()
}
#[test]
fn exact_current_notes_group_one_marker_per_start_row_and_stale_foreign_mismatches_are_absent() {
    let (ctx, mut app, id) = app();
    let navigation = &app.changes.review.as_ref().unwrap().2;
    let notes = &app.changes.review_comments;
    assert_eq!(notes.markers("marker.rs", true, navigation).len(), 1);
    assert!(notes.markers("other.rs", true, navigation).is_empty());
    assert!(notes.markers("marker.rs", false, navigation).is_empty());
    let digest = navigation.comment_digest();
    let notes = &mut app.changes.review_comments;
    assert!(notes.begin(
        "marker.rs",
        DIFF,
        true,
        digest,
        super::super::tests::range(DIFF, "chosen")
    ));
    notes.editor.as_mut().unwrap().text = "Another note".into();
    assert!(notes.save());
    notes.items[0].resolved = true;
    let navigation = &app.changes.review.as_ref().unwrap().2;
    let grouped = notes.markers("marker.rs", true, navigation);
    assert_eq!(grouped.len(), 1);
    assert_eq!(grouped[0].count, 2);
    assert!(!grouped[0].resolved);
    assert_ne!(grouped[0].id, id);
    notes.items.iter_mut().for_each(|note| note.resolved = true);
    assert!(notes.markers("marker.rs", true, navigation)[0].resolved);
    let original = notes.source_root.clone();
    notes.source_root = Some("/foreign".into());
    assert!(notes.markers("marker.rs", true, navigation).is_empty());
    notes.source_root = original;
    let changed = diff_viewer::Navigation::new("@@ -1 +1 @@\n-different\n+body\n");
    assert!(notes.markers("marker.rs", true, &changed).is_empty());
    notes.stale(None);
    assert!(notes.markers("marker.rs", true, navigation).is_empty());
    assert!(
        notes
            .marker_reveal(navigation, "marker.rs", true, ctx.cumulative_pass_nr())
            .is_none()
    );
    let _ = ctx;
}
#[test]
fn actual_marker_click_opens_existing_comments_and_never_changes_draft_run_or_notes() {
    let (ctx, mut app, id) = app();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let count = app.state.messages.len();
    let saved = app.changes.review_comments.items[0]
        .anchor
        .selection
        .clone();
    let pos = marker(&ctx).1.center();
    for pressed in [true, false] {
        frame(&ctx, &mut app, pointer(pos, pressed));
    }
    assert!(!egui::text_selection::LabelSelectionState::load(&ctx).has_selection());
    for _ in 0..15 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(app.changes.review_comments.expanded);
    let (body, clip) = ctx
        .data(|data| {
            data.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new(("review-note-body", id)))
        })
        .unwrap();
    assert!(clip.intersects(body));
    assert_eq!(app.changes.review_comments.items[0].anchor.selection, saved);
    assert_eq!(
        app.changes.review_comments.items[0].text,
        "Exact local note"
    );
    assert_eq!(app.draft, "Kept");
    assert!(app.state.busy);
    assert_eq!(app.state.messages.len(), count);
    assert!(commands.try_recv().is_err());
    assert!(!egui::Popup::is_any_open(&ctx));
}
#[test]
fn marker_reveals_exact_scrolled_note_keeps_pending_editor_and_drops_hidden_or_deleted_intent() {
    let (ctx, mut app, _) = app();
    let mut diff = String::from("@@ -1,60 +1,60 @@\n-old\n+chosen 世界\n");
    for n in 0..50 {
        diff.push_str(&format!(" context {n}\n"));
    }
    app.open_diff_review(("many.rs".into(), diff.clone()).into(), true);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let digest = app.changes.review.as_ref().unwrap().2.comment_digest();
    let notes = &mut app.changes.review_comments;
    for n in 0..15 {
        assert!(notes.begin(
            "many.rs",
            &diff,
            true,
            digest,
            super::super::tests::range(&diff, &format!(" context {n}"))
        ));
        notes.editor.as_mut().unwrap().text = format!("note body {n}");
        assert!(notes.save());
    }
    assert!(notes.begin(
        "many.rs",
        &diff,
        true,
        digest,
        super::super::tests::range(&diff, "+chosen 世界")
    ));
    notes.editor.as_mut().unwrap().text = "target note at end of list".into();
    assert!(notes.save());
    let target = notes.items.last().unwrap().id;
    assert!(notes.begin_edit(target));
    notes.editor.as_mut().unwrap().text = "Unfinished saved text".into();
    notes.editor.as_mut().unwrap().first = false;
    notes.expanded = false;
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let pos = marker(&ctx).1.center();
    for pressed in [true, false] {
        frame(&ctx, &mut app, pointer(pos, pressed));
    }
    for _ in 0..100 {
        frame(&ctx, &mut app, vec![]);
    }
    let (body, clip) = ctx
        .data(|data| {
            data.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new(("review-note-body", target)))
        })
        .unwrap();
    assert!(clip.contains(body.center()), "{body:?} {clip:?}");
    assert_eq!(
        app.changes.review_comments.editor.as_ref().unwrap().text,
        "Unfinished saved text"
    );
    let document = app.changes.review.as_ref().unwrap().2.focus_id();
    let pass = ctx.cumulative_pass_nr();
    let notes = &mut app.changes.review_comments;
    notes.reveal_note = Some((target, document, pass));
    notes.items.retain(|note| note.id != target);
    assert!(
        notes
            .marker_reveal(
                &app.changes.review.as_ref().unwrap().2,
                "many.rs",
                true,
                pass + 1
            )
            .is_none()
    );
    notes.reveal_note = Some((1, document, pass));
    notes.clear_markers();
    assert!(notes.reveal_note.is_none());
    assert!(notes.marker_controls.is_empty());
}

#[test]
fn marker_deleted_note_replacement_hidden_review_and_first_modal_cannot_replay_activation() {
    let (ctx, mut app, old) = app();
    let old_control = marker(&ctx).0;
    let digest = app.changes.review.as_ref().unwrap().2.comment_digest();
    let notes = &mut app.changes.review_comments;
    notes.items.clear();
    assert!(notes.begin(
        "marker.rs",
        DIFF,
        true,
        digest,
        super::super::tests::range(DIFF, "+chosen 世界")
    ));
    notes.editor.as_mut().unwrap().text = "replacement".into();
    assert!(notes.save());
    let current = notes.items[0].id;
    assert_ne!(old, current);
    notes.expanded = false;
    ctx.memory_mut(|memory| memory.request_focus(old_control));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(!app.changes.review_comments.expanded);
    assert_ne!(marker(&ctx).0, old_control);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let response = marker(&ctx);
    ctx.memory_mut(|memory| memory.request_focus(response.0));
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("marker-first-modal"), 220.0, |ui| {
            prompt_ui::heading(ui, "Information", "Foreign control")
        });
    });
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(!app.changes.review_comments.expanded);
    let nav = app.changes.review.as_ref().unwrap().2.focus_id();
    app.changes.review_comments.reveal_note = Some((current, nav, ctx.cumulative_pass_nr()));
    assert!(
        app.changes
            .review_comments
            .marker_reveal(
                &app.changes.review.as_ref().unwrap().2,
                "marker.rs",
                true,
                ctx.cumulative_pass_nr() + 3
            )
            .is_none(),
        "expired reveal cannot replay later"
    );
    app.files
        .open_workspace_source(
            &ctx,
            &app.prefs.cwd.clone(),
            &app.prefs.cwd.join("README.md"),
            None,
        )
        .unwrap();
    frame(&ctx, &mut app, vec![]);
    assert!(app.changes.review_comments.marker_controls.is_empty());
    assert!(app.changes.review_comments.reveal_note.is_none());
}

#[test]
fn marker_pointer_press_release_chords_and_paired_space_new_question_are_owned() {
    for phase in [true, false] {
        let (ctx, mut app, _) = app();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.dialogs.push_back(questions::demo_dialog());
        let pos = marker(&ctx).1.center();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
        for pressed in [true, false] {
            let mut events = pointer(pos, pressed);
            if pressed == phase {
                events.extend([
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                    key(egui::Key::Space, egui::Modifiers::NONE),
                    egui::Event::Text(" ".into()),
                ]);
            }
            frame(&ctx, &mut app, events);
        }
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.pending_dialog.is_none());
        assert_eq!(app.draft, "Kept");
        assert!(app.state.busy);
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn wrapped_marker_uses_first_visual_row_and_does_not_change_copy_characters_in_themes_zoom() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app, _) = app();
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            app.changes.set_expanded(true);
            let diff = format!(
                "@@ -1 +1 @@\n-old\n+chosen 世界 {}\n",
                "wrap e\u{301} שלום ".repeat(40)
            );
            app.open_diff_review(("wrap-marker.rs".into(), diff.clone()).into(), true);
            diff_viewer::wrap_preference::init(&ctx, true);
            for _ in 0..4 {
                frame(&ctx, &mut app, vec![]);
            }
            let digest = app.changes.review.as_ref().unwrap().2.comment_digest();
            let notes = &mut app.changes.review_comments;
            assert!(notes.begin(
                "wrap-marker.rs",
                &diff,
                true,
                digest,
                super::super::tests::range(&diff, "+chosen 世界")
            ));
            notes.editor.as_mut().unwrap().text = "wrapped anchor".into();
            assert!(notes.save());
            notes.expanded = false;
            app.changes.set_expanded(true); // explicitly requested, never marker-driven
            let mut native = eframe::Frame::_new_kittest();
            let mut output = None;
            for _ in 0..8 {
                output = Some(ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(620.0, 1000.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut native),
                ));
            }
            let output = output.unwrap();
            let (id, rect, count) = marker(&ctx);
            assert_eq!(count, 1);
            assert!(ctx.read_response(id).unwrap().enabled());
            let text = output
                .shapes
                .iter()
                .find_map(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape {
                        text.galley
                            .job
                            .text
                            .contains("+chosen 世界")
                            .then_some(text)
                    } else {
                        None
                    }
                })
                .unwrap();
            assert!(text.galley.rows.len() > diff.lines().count());
            let display = lines::rows(&diff)
                .map(|row| {
                    if row.header {
                        lines::HUNK_CAPTION.to_owned()
                    } else {
                        format!("{}{}\n", row.gutter(), row.text)
                    }
                })
                .collect::<String>();
            assert_eq!(text.galley.job.text, display);
            let logical_row = &text.galley.rows[2];
            assert!((rect.center().y - (text.pos.y + logical_row.rect().center().y)).abs() < 2.0);
            assert!(rect.left() >= 0.0 && rect.right() <= ctx.screen_rect().right());
        }
    }
}

#[test]
fn wide_line_number_marker_does_not_cover_number_or_source_and_modal_cancels_pending_reveal() {
    let (ctx, mut app, _) = app();
    let diff = "@@ -1000000 +1000000 @@\n-old\n+chosen 世界\n";
    app.open_diff_review(("wide.rs".into(), diff.into()).into(), true);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let digest = app.changes.review.as_ref().unwrap().2.comment_digest();
    let notes = &mut app.changes.review_comments;
    assert!(notes.begin(
        "wide.rs",
        diff,
        true,
        digest,
        super::super::tests::range(diff, "+chosen 世界")
    ));
    notes.editor.as_mut().unwrap().text = "wide coordinate".into();
    assert!(notes.save());
    let target = notes.items.last().unwrap().id;
    notes.expanded = false;
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let output = frame(&ctx, &mut app, vec![]);
    let (id, rect, _) = marker(&ctx);
    assert!(ctx.read_response(id).is_some());
    let text = output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                text.galley
                    .job
                    .text
                    .contains("+chosen 世界")
                    .then_some(text)
            } else {
                None
            }
        })
        .unwrap();
    let row = &text.galley.rows[2];
    assert!(rect.left() >= text.pos.x + row.pos.x + row.glyphs[6].max_x());
    assert!(rect.right() <= text.pos.x + row.pos.x + row.glyphs[10].pos.x);
    let nav = app.changes.review.as_ref().unwrap().2.focus_id();
    app.changes.review_comments.expanded = true;
    app.changes.review_comments.reveal_note = Some((target, nav, ctx.cumulative_pass_nr()));
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("cancel-marker-reveal"), 220.0, |ui| {
            prompt_ui::heading(ui, "Information", "Foreign")
        });
    });
    frame(&ctx, &mut app, vec![]);
    assert!(app.changes.review_comments.reveal_note.is_none());
}

#[test]
fn bounded_64_note_projection_and_settled_marker_frames_add_no_source_retention_or_timer() {
    let (ctx, mut app, _) = app();
    let digest = app.changes.review.as_ref().unwrap().2.comment_digest();
    let notes = &mut app.changes.review_comments;
    for _ in 1..MAX_COMMENTS {
        assert!(notes.begin(
            "marker.rs",
            DIFF,
            true,
            digest,
            super::super::tests::range(DIFF, "+chosen 世界")
        ));
        notes.editor.as_mut().unwrap().text = "bounded".into();
        assert!(notes.save());
    }
    notes.expanded = false;
    let bytes = notes.bytes();
    let navigation = &app.changes.review.as_ref().unwrap().2;
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let markers = notes.markers("marker.rs", true, navigation);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].count, 64);
    }
    eprintln!(
        "1000x64note markerprojection {:?}; note payload{}B; marker tuple{}B",
        start.elapsed(),
        bytes,
        std::mem::size_of::<Marker>()
    );
    app.state.busy = false;
    let mut output = None;
    for _ in 0..100 {
        output = Some(frame(&ctx, &mut app, vec![]));
    }
    assert!(
        output.unwrap().viewport_output[&egui::ViewportId::ROOT].repaint_delay
            > Duration::from_secs(1)
    );
    assert_eq!(app.changes.review_comments.bytes(), bytes);
    assert!(app.changes.review_comments.marker_controls.capacity() <= MAX_COMMENTS * 2);
    assert!(app.workspace_rx.is_none() && app.diff_rx.is_none());
}

#[test]
fn native_marker_tab_enter_space_reverse_escape_and_foreign_popup_do_not_escape_owner() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app, _) = app();
        let target = marker(&ctx).0;
        for _ in 0..120 {
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
            );
            if ctx.memory(|memory| memory.has_focus(target)) {
                break;
            }
        }
        assert!(ctx.memory(|memory| memory.has_focus(target)));
        app.state.dialogs.push_back(questions::demo_dialog());
        let mut events = vec![key(activation, egui::Modifiers::NONE)];
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        frame(&ctx, &mut app, events);
        frame(&ctx, &mut app, vec![]);
        assert!(app.changes.review_comments.expanded);
        assert_eq!(app.state.dialogs.len(), 1);
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
        assert!(app.changes.review.is_some());
        egui::Popup::open_id(&ctx, egui::Id::new("foreign-marker"));
        ctx.memory_mut(|memory| memory.request_focus(target));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert!(egui::Popup::is_any_open(&ctx));
        assert!(app.state.busy);
    }
}
