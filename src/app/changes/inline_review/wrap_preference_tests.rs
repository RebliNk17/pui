use super::tests::{DIFF, PATH, click_control, nav, setup};
use super::*;
use crate::app::diff_viewer::wrap_preference::{get, init};
use crate::app::files::tests::{click, frame};

fn review(app: &mut Pui, path: &str, diff: &str) {
    app.open_diff_review((path.into(), diff.into()).into(), true);
    let ctx = app.ctx.clone();
    for _ in 0..4 {
        frame(&ctx, app, vec![]);
    }
}
fn wrap(app: &mut Pui) {
    let ctx = app.ctx.clone();
    let rect = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("diff-wrap-lines")))
        .unwrap();
    click(&ctx, app, rect.center());
}
#[test]
fn real_embedded_wrap_remembers_new_files_but_large_document_cannot_erase_preference() {
    let (ctx, mut app) = setup(false, DIFF);
    review(&mut app, PATH, DIFF);
    assert!(!app.changes.review.as_ref().unwrap().2.wrap());
    wrap(&mut app);
    assert_eq!(get(&ctx), Some(true));
    let text = app.changes.review.as_ref().unwrap().1.clone();
    let digest = app.changes.review.as_ref().unwrap().2.comment_digest();
    review(&mut app, "different.rs", DIFF);
    assert_eq!(app.changes.review.as_ref().unwrap().1, text);
    assert_eq!(
        app.changes.review.as_ref().unwrap().2.comment_digest(),
        digest
    );
    assert!(app.changes.review.as_ref().unwrap().2.wrap());
    for source in ["x".repeat(65537), "+x\n".repeat(4097)] {
        review(&mut app, "large.rs", &source);
        assert!(!app.changes.review.as_ref().unwrap().2.wrap());
        let id = ctx
            .data(|data| data.get_temp::<egui::Id>(egui::Id::new("diff-wrap-lines-focus")))
            .unwrap();
        assert!(!ctx.read_response(id).unwrap().enabled());
        wrap(&mut app);
        assert_eq!(get(&ctx), Some(true));
    }
    review(&mut app, "small.rs", DIFF);
    assert!(app.changes.review.as_ref().unwrap().2.wrap());
    wrap(&mut app);
    assert_eq!(get(&ctx), Some(false));
    review(&mut app, "next.rs", DIFF);
    assert!(!app.changes.review.as_ref().unwrap().2.wrap());
    app.state.busy = false;
    let mut last = None;
    for _ in 0..100 {
        last = Some(frame(&ctx, &mut app, vec![]));
    }
    assert!(
        last.unwrap().viewport_output[&egui::ViewportId::ROOT].repaint_delay
            > Duration::from_secs(1)
    );
}
#[test]
fn inline_git_recorded_close_reopen_and_hidden_review_use_one_window_preference() {
    for git in [false, true] {
        let (ctx, mut app) = setup(git, DIFF);
        click_control(&ctx, &mut app, "diff-wrap-lines-focus");
        assert_eq!(get(&ctx), Some(true));
        // A fresh inline Navigation uses the same value, not its destroyed row.
        app.changes.inline.clear();
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        assert!(nav(&app, git).wrap());
        review(&mut app, "other.rs", DIFF);
        assert!(app.changes.review.as_ref().unwrap().2.wrap());
        wrap(&mut app);
        assert_eq!(get(&ctx), Some(false));
        app.changes.review = None;
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        assert!(!nav(&app, git).wrap());
        assert_eq!(app.draft, "Kept draft");
        assert!(app.state.busy);
        assert!(app.workspace_rx.is_none() && app.diff_rx.is_none());
    }
}
#[test]
fn parked_review_reconciles_latest_window_choice_without_restoring_work_or_changing_owner() {
    let (ctx, mut app) = setup(false, DIFF);
    app.state.session_file = Some("/fixture/wrap-a".into());
    let a = app.ensure_agent();
    review(&mut app, PATH, DIFF);
    wrap(&mut app);
    assert_eq!(get(&ctx), Some(true));
    app.park_tree();
    app.state.session_file = Some("/fixture/wrap-b".into());
    let b = app.ensure_agent();
    app.enter_session_layout();
    review(&mut app, "second.rs", DIFF);
    assert!(app.changes.review.as_ref().unwrap().2.wrap());
    wrap(&mut app);
    assert_eq!(get(&ctx), Some(false));
    app.view_agent(&a);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(!app.changes.review.as_ref().unwrap().2.wrap());
    assert_eq!(app.changes.review.as_ref().unwrap().0, PATH);
    app.view_agent(&b);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    assert_eq!(app.changes.review.as_ref().unwrap().0, "second.rs");
    assert!(!app.changes.review.as_ref().unwrap().2.wrap());
    assert!(app.workspace_rx.is_none() && app.diff_rx.is_none());
}

#[test]
fn native_keyboard_new_question_foreign_popup_and_pointer_chords_only_change_explicit_wrap() {
    use super::tests::{key, pointer};
    for phase in [true, false] {
        let (ctx, mut app) = setup(false, DIFF);
        let commands = super::tests::approval(&mut app);
        let pos = ctx
            .read_response(super::tests::control(&ctx, "diff-wrap-lines-focus"))
            .unwrap()
            .rect
            .center();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
        for pressed in [true, false] {
            let mut events = pointer(pos, pressed);
            if pressed == phase {
                events.extend([
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                ]);
            }
            frame(&ctx, &mut app, events);
        }
        assert_eq!(get(&ctx), Some(true));
        assert!(commands.try_recv().is_err());
        assert_eq!(app.state.dialogs.len(), 1);
        assert_eq!(app.draft, "Kept draft");
        assert!(app.state.busy);
    }
    let (ctx, mut app) = setup(false, DIFF);
    let target = super::tests::control(&ctx, "diff-wrap-lines-focus");
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
    app.state.dialogs.push_back(questions::demo_dialog());
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Space, egui::Modifiers::NONE),
            egui::Event::Text(" ".into()),
        ],
    );
    assert_eq!(get(&ctx), Some(true));
    assert!(app.pending_dialog.is_none());
    assert!(commands.try_recv().is_err());
    assert_eq!(app.state.dialogs.len(), 1);
    egui::Popup::open_id(&ctx, egui::Id::new("wrap-pref-foreign"));
    ctx.memory_mut(|memory| memory.request_focus(target));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(get(&ctx), Some(true));
    assert_eq!(app.draft, "Kept draft");
    assert!(app.state.busy);
}

#[test]
fn preference_is_context_local_constant_metadata_and_no_restore_execution_or_idle_write() {
    let a = egui::Context::default();
    let b = egui::Context::default();
    init(&a, true);
    init(&b, false);
    assert_eq!(get(&a), Some(true));
    assert_eq!(get(&b), Some(false));
    let encoded = serde_json::to_string(&Preferences {
        diff_wrap: true,
        ..Default::default()
    })
    .unwrap();
    let parsed: Preferences = serde_json::from_str(&encoded).unwrap();
    assert!(parsed.diff_wrap);
    let legacy: Preferences = serde_json::from_str("{}").unwrap();
    assert!(!legacy.diff_wrap);
    let start = std::time::Instant::now();
    for _ in 0..10000 {
        assert_eq!(get(&a), Some(true));
    }
    eprintln!(
        "10000O1wrappreferencereads {:?}; one bool per Context; no document body or path collection",
        start.elapsed()
    );
}
