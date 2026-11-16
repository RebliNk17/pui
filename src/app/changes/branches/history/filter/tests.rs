use super::super::tests::{fixture, key};
use super::*;
use crate::app::files::tests::frame;
#[test]
fn changed_snapshot_clear_does_not_retarget_and_narrow_theme_zoom_is_bounded() {
    for dark in [false, true] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app) = fixture();
            if dark {
                ctx.set_visuals(egui::Visuals::dark());
            } else {
                ctx.set_visuals(egui::Visuals::light());
            }
            ctx.set_zoom_factor(zoom);
            ctx.memory_mut(|memory| memory.request_focus(id()));
            frame(&ctx, &mut app, vec![egui::Event::Text("approvals".into())]);
            let old = app.changes.branches.history.filter.clear_control.unwrap();
            app.changes.branches.history.filter.key(Some(old));
            app.changes.branches.history.snapshot.as_mut().unwrap().head = "c".repeat(40);
            let mut native = eframe::Frame::_new_kittest();
            for _ in 0..3 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(420.0, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut native),
                );
            }
            assert_eq!(app.changes.branches.history.filter.query, "approvals");
            let clear = app.changes.branches.history.filter.clear_control.unwrap();
            assert_ne!(clear, old);
            assert!(
                ctx.screen_rect()
                    .contains_rect(ctx.read_response(clear).unwrap().interact_rect)
            );
            assert_eq!(app.draft, "Kept draft");
        }
    }
}
#[test]
fn clear_pointer_space_and_reverse_escape_preserve_questions_and_chat() {
    for pointer in [false, true] {
        let (ctx, mut app) = fixture();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        ctx.memory_mut(|memory| memory.request_focus(id()));
        frame(&ctx, &mut app, vec![egui::Event::Text("approvals".into())]);
        for _ in 0..2 {
            frame(&ctx, &mut app, vec![]);
        }
        app.state.dialogs.push_back(questions::demo_dialog());
        let clear = app.changes.branches.history.filter.clear_control.unwrap();
        if pointer {
            let pos = ctx.read_response(clear).unwrap().interact_rect.center();
            crate::app::files::tests::click(&ctx, &mut app, pos);
        } else {
            ctx.memory_mut(|memory| memory.request_focus(clear));
            frame(
                &ctx,
                &mut app,
                vec![
                    key(egui::Key::Space, egui::Modifiers::NONE),
                    egui::Event::Text(" ".into()),
                ],
            );
        }
        assert!(app.changes.branches.history.filter.query.is_empty());
        assert_eq!(app.draft, "Kept draft");
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.state.busy);
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
        assert!(!app.changes.branches.open);
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn staged_clear_cannot_adopt_changed_query_or_replay_after_hidden_control() {
    let (ctx, mut app) = fixture();
    ctx.memory_mut(|memory| memory.request_focus(id()));
    frame(&ctx, &mut app, vec![egui::Event::Text("approvals".into())]);
    let old = app.changes.branches.history.filter.clear_control.unwrap();
    app.changes.branches.history.filter.key(Some(old));
    let history = &mut app.changes.branches.history;
    history.filter.query = "synthetic".into();
    history.filter.project(history.snapshot.as_ref().unwrap());
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.changes.branches.history.filter.query, "synthetic");
    assert!(app.changes.branches.history.filter.clear_key.is_none());
    app.changes.branches.history.filter.key(Some(old));
    let history = &mut app.changes.branches.history;
    history.filter.query.clear();
    history.filter.project(history.snapshot.as_ref().unwrap());
    frame(&ctx, &mut app, vec![]);
    assert!(app.changes.branches.history.filter.clear_key.is_none());
    assert!(app.changes.branches.history.filter.clear_control.is_none());
}
#[test]
fn clear_filter_restores_loaded_list_without_git_or_chat_actions() {
    let (ctx, mut app) = fixture();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    ctx.memory_mut(|memory| memory.request_focus(id()));
    frame(&ctx, &mut app, vec![egui::Event::Text("no matches".into())]);
    let clear = ctx
        .data(|data| data.get_temp::<egui::Id>(id().with("clear-control")))
        .unwrap();
    ctx.memory_mut(|memory| memory.request_focus(clear));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(app.changes.branches.history.filter.query.is_empty());
    assert!(app.changes.branches.history.filter.hits.is_none());
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.changes.branches.history.commit_controls.len(), 2);
    assert_eq!(app.changes.branches.history.requests, 0);
    assert_eq!(app.draft, "Kept draft");
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
}
#[test]
fn literal_unicode_subject_oid_empty_and_no_match_project_original_indices() {
    let (_, app) = fixture();
    let snapshot = app.changes.branches.history.snapshot.as_ref().unwrap();
    let mut filter = Filter::default();
    assert!(filter.includes(0));
    for (query, expected) in [
        ("SYNTHETIC", vec![0]),
        ("approvals", vec![1]),
        ("bbbb", vec![1]),
        ("no match", vec![]),
        (" ", vec![0, 1]),
    ] {
        filter.query = query.into();
        filter.project(snapshot);
        assert_eq!(
            (0..2)
                .filter(|&index| filter.includes(index))
                .collect::<Vec<_>>(),
            expected
        );
    }
    filter.query = "界".repeat(300);
    filter.project(snapshot);
    assert_eq!(filter.query.chars().count(), 128);
    assert_eq!(filter.query.capacity(), 384);
}
#[test]
fn actual_filter_typing_spaces_paste_native_focus_and_activation_never_send_or_read_git() {
    let (ctx, mut app) = fixture();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    ctx.memory_mut(|memory| memory.request_focus(id()));
    frame(&ctx, &mut app, vec![egui::Event::Text("Keep chat".into())]);
    assert_eq!(app.changes.branches.history.filter.query, "Keep chat");
    assert_eq!(app.changes.branches.history.commit_controls.len(), 1);
    assert!(app.changes.branches.history.filter.includes(1));
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Space, egui::Modifiers::NONE),
            egui::Event::Text(" ".into()),
            egui::Event::Paste("drafts".into()),
        ],
    );
    assert_eq!(
        app.changes.branches.history.filter.query,
        "Keep chat drafts"
    );
    assert!(commands.try_recv().is_err());
    assert_eq!(app.draft, "Kept draft");
    assert!(app.state.busy);
    assert_eq!(app.changes.branches.history.requests, 0);
    let row = app.changes.branches.history.commit_controls[0].0;
    for _ in 0..10 {
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
        );
        if ctx.memory(|memory| memory.has_focus(row)) {
            break;
        }
    }
    assert!(ctx.memory(|memory| memory.has_focus(row)));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(app.changes.branches.history.has_detail());
}
#[test]
fn old_filtered_row_cannot_open_hidden_commit_back_retains_filter_close_releases_it() {
    let (ctx, mut app) = fixture();
    let old = app.changes.branches.history.commit_controls[0].0;
    app.changes.branches.history.filter.query = "approvals".into();
    let history = &mut app.changes.branches.history;
    history.filter.project(history.snapshot.as_ref().unwrap());
    ctx.memory_mut(|memory| memory.request_focus(old));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(!app.changes.branches.history.has_detail());
    let history = &mut app.changes.branches.history;
    assert!(history.open_detail(&ctx, &"b".repeat(40), true));
    history.close_detail(&ctx);
    assert_eq!(history.filter.query, "approvals");
    app.changes.close_branches(&ctx);
    assert!(app.changes.branches.history.filter.query.is_empty());
}
#[test]
fn filter_new_question_chords_and_reverse_escape_preserve_draft_and_approval() {
    let (ctx, mut app) = fixture();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    ctx.memory_mut(|memory| memory.request_focus(id()));
    frame(&ctx, &mut app, vec![egui::Event::Text("approvals".into())]);
    app.state.dialogs.push_back(questions::demo_dialog());
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
        ],
    );
    assert_eq!(app.state.dialogs.len(), 1);
    assert_eq!(app.draft, "Kept draft");
    assert!(commands.try_recv().is_err());
    assert!(!app.changes.branches.history.has_detail());
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
    assert!(!app.changes.branches.open);
    assert_eq!(app.state.dialogs.len(), 1);
}
#[test]
fn detail_view_drops_hidden_clear_intent_but_keeps_filter_for_back() {
    let (ctx, mut app) = fixture();
    ctx.memory_mut(|memory| memory.request_focus(id()));
    frame(&ctx, &mut app, vec![egui::Event::Text("approvals".into())]);
    let history = &mut app.changes.branches.history;
    let clear = history.filter.clear_control.unwrap();
    history.filter.key(Some(clear));
    assert!(history.open_detail(&ctx, &"b".repeat(40), true));
    frame(&ctx, &mut app, vec![]);
    let history = &mut app.changes.branches.history;
    assert!(history.filter.clear_control.is_none());
    assert!(history.filter.clear_key.is_none());
    assert_eq!(history.filter.query, "approvals");
    history.close_detail(&ctx);
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.changes.branches.history.filter.query, "approvals");
}
#[test]
fn owner_change_drops_clear_control_and_staged_activation_without_dispatch() {
    let (ctx, mut app) = fixture();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    ctx.memory_mut(|memory| memory.request_focus(id()));
    frame(&ctx, &mut app, vec![egui::Event::Text("approvals".into())]);
    let history = &mut app.changes.branches.history;
    let old = history.filter.clear_control.unwrap();
    history.filter.key(Some(old));
    assert_eq!(history.filter.clear_key, Some(old));
    history.poll(None);
    assert!(history.filter.query.is_empty());
    assert!(history.filter.hits.is_none());
    assert!(history.filter.clear_control.is_none());
    assert!(history.filter.clear_key.is_none());
    assert!(history.snapshot.is_none());
    assert_eq!(history.requests, 0);
    history.filter_key(Some(old));
    assert!(history.filter.clear_key.is_none());
    assert_eq!(app.draft, "Kept draft");
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
}
#[test]
fn filter_bounded_projection_has_no_idle_work_and_resets_on_owner_change() {
    let (ctx, mut app) = fixture();
    let history = &mut app.changes.branches.history;
    let snapshot = history.snapshot.as_mut().unwrap();
    snapshot.commits = (0..100)
        .map(|index| workspace::history::Commit {
            oid: format!("{index:040x}"),
            time: 1,
            subject: format!("{index} {}", "界".repeat(330)),
            first_parent: None,
            parent_count: 0,
        })
        .collect();
    let start = std::time::Instant::now();
    for _ in 0..100 {
        history.filter.query = "界".repeat(128);
        history.filter.project(history.snapshot.as_ref().unwrap());
    }
    eprintln!(
        "100x100subject filter {:?}; query{}B map{}B",
        start.elapsed(),
        history.filter.query.capacity(),
        history.filter.hits.as_ref().unwrap().capacity() * std::mem::size_of::<usize>()
    );
    assert_eq!(history.filter.hits.as_ref().unwrap().len(), 100);
    assert!(history.filter.hits.as_ref().unwrap().capacity() <= 128);
    history.poll(None);
    assert!(history.filter.query.is_empty());
    assert!(history.snapshot.is_none());
    assert!(app.client.is_none());
    let _ = ctx;
}
