use super::*;
use crate::app::changes::review_comments::ui_tests::{key, pointer};
use crate::app::files::tests::frame;
fn source() -> String {
    (0..9)
        .map(|block| {
            format!(
                "@@ -{},12 +{},12 @@\n-old {block}\n+new {block} 世界\n{}",
                block * 50 + 1,
                block * 50 + 1,
                (0..11)
                    .map(|row| format!(" unchanged {block}-{row}\n"))
                    .collect::<String>()
            )
        })
        .collect()
}
fn store(diff: &str) -> (Comments, diff_viewer::Document) {
    let document: diff_viewer::Document = ("file.rs".into(), diff.into()).into();
    let mut notes = Comments::default();
    notes.bind(Path::new("/repo"), "owner", 1);
    notes.source_root = Some("/repo".into());
    let range = super::super::tests::range(diff, "+new 8 世界");
    assert!(notes.begin("file.rs", diff, true, document.2.comment_digest(), range));
    notes.editor.as_mut().unwrap().text = "Original note".into();
    assert!(notes.save());
    (notes, document)
}
#[test]
fn exact_id_owner_root_source_path_digest_and_stale_guards_never_retarget() {
    let ctx = egui::Context::default();
    let diff = source();
    let (mut notes, mut document) = store(&diff);
    let id = notes.items[0].id;
    assert!(notes.jump_available(id, "file.rs", true, &document.2));
    document.2.focus(&ctx);
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Home, egui::Modifiers::ALT)],
            ..Default::default()
        },
        |ctx| document.2.inputs(ctx, &diff),
    );
    assert_eq!(document.2.take_target(), Some(0));
    assert!(notes.jump(id, &ctx, "file.rs", &diff, true, &mut document.2));
    assert_eq!(
        document.2.take_target(),
        Some(notes.items[0].anchor.selection.rows.0 - 1)
    );
    assert!(document.2.take_target().is_none());
    // Jump does not pretend the containing hunk was a Previous/Next choice.
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::ArrowDown, egui::Modifiers::ALT)],
            ..Default::default()
        },
        |ctx| document.2.inputs(ctx, &diff),
    );
    let second_hunk = lines::rows(&diff)
        .filter(|row| row.header)
        .nth(1)
        .unwrap()
        .index;
    assert_eq!(document.2.take_target(), Some(second_hunk));
    assert!(!notes.jump(id + 1, &ctx, "file.rs", &diff, true, &mut document.2));
    assert!(!notes.jump(id, &ctx, "other.rs", &diff, true, &mut document.2));
    assert!(!notes.jump(id, &ctx, "file.rs", &diff, false, &mut document.2));
    let mut different = diff_viewer::Navigation::new(&diff.replace("+new 8", "+new x"));
    assert!(!notes.jump(id, &ctx, "file.rs", &diff, true, &mut different));
    notes.source_root = Some("/elsewhere".into());
    assert!(!notes.jump(id, &ctx, "file.rs", &diff, true, &mut document.2));
    notes.source_root = None;
    assert!(!notes.jump_available(id, "file.rs", true, &document.2));
    notes.source_root = Some("/repo".into());
    notes.items[0].resolved = true;
    assert!(notes.jump(id, &ctx, "file.rs", &diff, true, &mut document.2));
    assert!(notes.items[0].resolved);
    notes.items[0].stale = true;
    assert!(!notes.jump(id, &ctx, "file.rs", &diff, true, &mut document.2));
    notes.items[0].stale = false;
    notes.owner.as_mut().unwrap().session = "foreign".into();
    assert!(!notes.jump_available(id, "file.rs", true, &document.2));
}
#[test]
fn invalid_saved_range_confirmation_and_disappearing_id_refuse_without_note_editor_mutation() {
    let ctx = egui::Context::default();
    let diff = source();
    let (mut notes, mut document) = store(&diff);
    let id = notes.items[0].id;
    assert!(notes.begin_edit(id));
    let original = notes.items[0].anchor.selection.clone();
    notes.items[0].anchor.selection.rows = (1, 1);
    assert!(!notes.jump(id, &ctx, "file.rs", &diff, true, &mut document.2));
    notes.items[0].anchor.selection = original.clone();
    notes.clear_requested = true;
    assert!(!notes.jump(id, &ctx, "file.rs", &diff, true, &mut document.2));
    notes.clear_requested = false;
    assert!(notes.jump(id, &ctx, "file.rs", &diff, true, &mut document.2));
    assert_eq!(notes.editor.as_ref().unwrap().text, "Original note");
    assert_eq!(notes.items[0].anchor.selection, original);
    assert_eq!(notes.items[0].text, "Original note");
    let mut anchor = notes.items[0].anchor.clone();
    notes.items.clear();
    anchor.path = "replacement.rs".into();
    notes.items.push(Comment {
        id: id + 1,
        anchor,
        text: "same-index replacement".into(),
        resolved: false,
        stale: false,
    });
    assert!(!notes.jump(id, &ctx, "file.rs", &diff, true, &mut document.2));
    assert!(notes.editor.is_some());
}
fn app() -> (egui::Context, Pui, u64) {
    app_with(source())
}
fn app_with(diff: String) -> (egui::Context, Pui, u64) {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.prefs.hide_sidebar = true;
    app.focus_composer = false;
    app.draft = "Kept draft".into();
    app.state.busy = true;
    app.open_diff_review(("file.rs".into(), diff.clone()).into(), true);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let digest = app.changes.review.as_ref().unwrap().2.comment_digest();
    let notes = &mut app.changes.review_comments;
    assert!(notes.begin(
        "file.rs",
        &diff,
        true,
        digest,
        super::super::tests::range(&diff, "+new 8 世界")
    ));
    notes.editor.as_mut().unwrap().text = "Please review final block".into();
    assert!(notes.save());
    let id = notes.items[0].id;
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app, id)
}
fn jump_control(ctx: &egui::Context, id: u64) -> (egui::Id, egui::Rect) {
    ctx.data(|data| {
        data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new(("review-comment-jump", id)))
    })
    .unwrap()
}
#[test]
fn wrapped_narrow_themes_resize_reveal_original_row_and_leave_editor_text_untouched() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let diff = source().replace(
                "+new 8 世界",
                &format!("+new 8 世界 {}", "long Unicode e\u{301} שלום ".repeat(30)),
            );
            let (ctx, mut app, id) = app_with(diff);
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            app.changes.set_expanded(true); // explicit user presentation intent; never automatic on Jump
            assert!(app.changes.review_comments.begin_edit(id));
            app.changes.review_comments.editor.as_mut().unwrap().text = "Unfinished text".into();
            app.changes.review_comments.editor.as_mut().unwrap().first = false;
            // Reuse the real Wrap control, preserving the original numbered display range.
            for _ in 0..5 {
                frame(&ctx, &mut app, vec![]);
            }
            let pos = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("diff-wrap-lines")))
                .unwrap()
                .center();
            for pressed in [true, false] {
                frame(&ctx, &mut app, pointer(pos, pressed));
            }
            let pos = jump_control(&ctx, id).1.center();
            for pressed in [true, false] {
                frame(&ctx, &mut app, pointer(pos, pressed));
            }
            let saved = &app.changes.review_comments.items[0].anchor.selection;
            let row = saved.rows.0 - 1;
            let mut native = eframe::Frame::_new_kittest();
            let mut output = None;
            for turn in 0..110 {
                let time = ctx.input(|input| input.time) + 0.016;
                output = Some(ctx.run(
                    egui::RawInput {
                        time: Some(time),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(if turn < 3 { 740.0 } else { 620.0 }, 1100.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut native),
                ));
            }
            let output = output.unwrap();
            let (clip, text) = output
                .shapes
                .iter()
                .find_map(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape {
                        text.galley
                            .job
                            .text
                            .contains("+new 8 世界")
                            .then_some((shape.clip_rect, text))
                    } else {
                        None
                    }
                })
                .unwrap();
            assert!(
                text.galley.rows.len() > app.changes.review.as_ref().unwrap().1.lines().count() + 5,
                "fixture must actually wrap"
            );
            let mut logical = 0;
            let visual = text
                .galley
                .rows
                .iter()
                .find(|visual| {
                    let found = logical == row;
                    if visual.ends_with_newline {
                        logical += 1;
                    }
                    found
                })
                .unwrap();
            assert!(
                clip.contains(visual.rect().translate(text.pos.to_vec2()).center()),
                "{mode:?} zoom{zoom} clip{clip:?}"
            );
            assert_eq!(
                app.changes.review_comments.editor.as_ref().unwrap().text,
                "Unfinished text"
            );
            assert_eq!(app.draft, "Kept draft");
            assert!(app.state.busy);
            assert!(output.platform_output.commands.is_empty());
        }
    }
}

#[test]
fn explicit_maximum_snapshot_jump_cost_and_64_note_availability_do_not_retain_source() {
    let ctx = egui::Context::default();
    let diff = format!(
        "@@ -1,9999 +1,9999 @@\n{}+new 8 世界\n",
        format!(" {}\n", "x".repeat(180)).repeat(9998)
    );
    assert!(diff.len() < MAX_DIFF_BYTES);
    let (mut notes, mut document) = store(&diff);
    let id = notes.items[0].id;
    let start = std::time::Instant::now();
    assert!(notes.jump(id, &ctx, "file.rs", &diff, true, &mut document.2));
    let elapsed = start.elapsed();
    let bytes = notes.bytes();
    for _ in 1..MAX_COMMENTS {
        assert!(notes.begin(
            "file.rs",
            &diff,
            true,
            document.2.comment_digest(),
            notes.items[0].anchor.selection.chars.clone()
        ));
        notes.editor.as_mut().unwrap().text = "bounded".into();
        assert!(notes.save());
    }
    let before = notes.bytes();
    let checks = std::time::Instant::now();
    for _ in 0..100 {
        for note in &notes.items {
            assert!(notes.jump_available(note.id, "file.rs", true, &document.2));
        }
    }
    let checks = checks.elapsed();
    assert_eq!(notes.bytes(), before);
    assert!(bytes < MAX_METADATA + MAX_TEXT);
    assert!(before <= MAX_TOTAL);
    eprintln!(
        "{}B snapshot explicit note jump {elapsed:?}; 6400availabilitychecks {checks:?}; 64notes {}B payload; no retained source",
        diff.len(),
        before
    );
}

#[test]
fn actual_anchor_click_reveals_saved_lines_and_preserves_run_draft_rpc_and_notes() {
    let (ctx, mut app, id) = app();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.dialogs.push_back(json!({"id":"jump-approval","method":"confirm","title":"Allow bash?","message":"controlled"}));
    let messages = app.state.messages.len();
    let original = app.changes.review_comments.items[0]
        .anchor
        .selection
        .clone();
    let pos = jump_control(&ctx, id).1.center();
    for pressed in [true, false] {
        frame(&ctx, &mut app, pointer(pos, pressed));
    }
    for _ in 0..100 {
        frame(&ctx, &mut app, vec![]);
    }
    let out = frame(&ctx, &mut app, vec![]);
    let (clip, text) = out
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                text.galley
                    .job
                    .text
                    .contains("+new 8 世界")
                    .then_some((shape.clip_rect, text))
            } else {
                None
            }
        })
        .unwrap();
    let row = original.rows.0 - 1;
    assert!(
        clip.contains(
            text.galley.rows[row]
                .rect()
                .translate(text.pos.to_vec2())
                .center()
        )
    );
    assert_eq!(
        app.changes.review_comments.items[0].anchor.selection,
        original
    );
    assert_eq!(app.draft, "Kept draft");
    assert!(app.state.busy);
    assert_eq!(app.state.messages.len(), messages);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
    assert!(!egui::text_selection::LabelSelectionState::load(&ctx).has_selection());
    assert!(app.workspace_rx.is_none() && app.diff_rx.is_none());
    app.state.busy = false;
    let mut settled = None;
    for _ in 0..100 {
        settled = Some(frame(&ctx, &mut app, vec![]));
    }
    assert!(
        settled.unwrap().viewport_output[&egui::ViewportId::ROOT].repaint_delay
            > Duration::from_secs(1)
    );
}
#[test]
fn anchor_pointer_press_release_chords_and_new_question_never_send_approve_stop_or_answer() {
    for phase in [true, false] {
        let (ctx, mut app, id) = app();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.dialogs.push_back(questions::demo_dialog());
        let pos = jump_control(&ctx, id).1.center();
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
        assert_eq!(app.draft, "Kept draft");
        assert!(app.state.busy);
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn native_tab_enter_space_reverse_escape_and_stale_foreign_popup_are_scoped() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app, id) = app();
        let target = jump_control(&ctx, id).0;
        for _ in 0..100 {
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
        let mut events = vec![key(activation, egui::Modifiers::NONE)];
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        frame(&ctx, &mut app, events);
        let row = app.changes.review_comments.items[0].anchor.selection.rows.0 - 1;
        assert_eq!(ctx.data(|data|data.get_temp::<(usize,egui::Rect)>(egui::Id::new("diff-hunk-target"))).unwrap().0,row);
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
        app.changes.review_comments.stale(None);
        app.changes.review_comments.expanded = true;
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let (disabled, _) = jump_control(&ctx, id);
        assert!(!ctx.read_response(disabled).unwrap().enabled());
        egui::Popup::open_id(&ctx, egui::Id::new("jump-foreign"));
        ctx.memory_mut(|memory| memory.request_focus(disabled));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert!(egui::Popup::is_any_open(&ctx));
        assert!(app.state.busy);
    }
}
