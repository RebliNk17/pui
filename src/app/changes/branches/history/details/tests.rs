use super::super::tests::{fixture, key};
use super::*;
use crate::app::files::tests::{click, frame};
fn first(ctx: &egui::Context) -> (egui::Id, egui::Rect) {
    ctx.data(|data| {
        data.get_temp::<(egui::Id, egui::Rect)>(
            popup_id().with(("commit-detail-row", &"a".repeat(40))),
        )
    })
    .unwrap()
}
#[test]
fn explicit_commit_click_opens_plain_exact_detail_and_back_reuses_pinned_list_without_worker() {
    let (ctx, mut app) = fixture();
    let before = app
        .changes
        .branches
        .history
        .snapshot
        .as_ref()
        .unwrap()
        .head
        .clone();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    click(&ctx, &mut app, first(&ctx).1.center());
    assert!(app.changes.branches.history.has_detail());
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(app.changes.branches.history.detail.pending.is_none());
    assert_eq!(app.changes.branches.history.detail.requests, 0);
    assert!(
        app.changes
            .branches
            .history
            .detail
            .result
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .message
            .contains("Synthetic")
    );
    assert_eq!(app.draft, "Kept draft");
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
    let back = ctx
        .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("detail-back")))
        .unwrap();
    click(&ctx, &mut app, back.1.center());
    assert!(!app.changes.branches.history.has_detail());
    assert_eq!(
        app.changes.branches.history.snapshot.as_ref().unwrap().head,
        before
    );
}
#[test]
fn real_local_detail_request_shared_permit_completion_exact_membership_and_back_cancel() {
    let root = crate::workspace::tests::fixture();
    let ctx = egui::Context::default();
    let workspace = Workspace {
        root: root.clone(),
        branch: "observed".into(),
        is_git: true,
        ..Default::default()
    };
    let owner = Owner::new(&(root.clone(), "owner".into()), &workspace).unwrap();
    let snapshot = workspace::history::read(&root, &AtomicBool::new(false)).unwrap();
    let hash = snapshot.head.clone();
    let mut history = History {
        active: true,
        loaded: true,
        owner: Some(owner),
        snapshot: Some(snapshot),
        ..Default::default()
    };
    let before = crate::workspace::tests::snapshot(&root);
    assert!(!history.open_detail(&ctx, &"0".repeat(40), false));
    assert!(history.open_detail(&ctx, &hash, false));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while history.detail.pending.is_some() {
        history.poll(history.owner.clone());
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        history
            .detail
            .result
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .oid,
        hash
    );
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    history.close_detail(&ctx);
    let gate = permit(&ctx).unwrap();
    assert!(history.open_detail(&ctx, &hash, false));
    assert!(
        history
            .detail
            .result
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap_err()
            .contains("still finishing")
    );
    assert!(permit(&ctx).is_none());
    history.close_detail(&ctx);
    drop(gate);
    assert!(permit(&ctx).is_some());
}
#[test]
fn detail_owner_change_receiver_drop_disconnect_and_mismatched_delivery_never_adopt() {
    let (ctx, mut app) = fixture();
    let hash = "a".repeat(40);
    assert!(app.changes.branches.history.open_detail(&ctx, &hash, true));
    let history = &mut app.changes.branches.history;
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    history.detail.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
    let mut other = history.owner.clone().unwrap();
    other.root = "/other".into();
    history.poll(Some(other));
    assert!(cancel.load(Ordering::Acquire));
    assert!(tx.send(Err("late".into())).is_err());
    assert!(!history.has_detail());
    let mut details = Details {
        selected: Some(hash),
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel();
    details.pending = Some(workspace::read_only::Request::new(
        rx,
        Arc::new(AtomicBool::new(false)),
    ));
    tx.send(Ok(workspace::history::details::Detail {
        oid: "b".repeat(40),
        time: 1,
        parents: vec![],
        message: "wrong".into(),
        signature: String::new(),
    }))
    .unwrap();
    details.poll();
    assert!(details.result.as_ref().unwrap().is_err());
    let (tx, rx) = mpsc::channel();
    details.pending = Some(workspace::read_only::Request::new(
        rx,
        Arc::new(AtomicBool::new(false)),
    ));
    drop(tx);
    details.poll();
    assert!(
        details
            .result
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap_err()
            .contains("ended without")
    );
}
#[test]
fn actual_first_tab_commit_enter_space_reverse_escape_and_new_questions_keep_scope() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app) = fixture();
        let row = first(&ctx).0;
        let history_tab = app.changes.branches.tabs[1].unwrap();
        ctx.memory_mut(|memory| memory.request_focus(history_tab));
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
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.dialogs.push_back(questions::demo_dialog());
        let mut events = vec![key(activation, egui::Modifiers::NONE)];
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        frame(&ctx, &mut app, events);
        assert!(app.changes.branches.history.has_detail());
        for _ in 0..2 {
            frame(&ctx, &mut app, vec![]);
        }
        assert_eq!(app.state.dialogs.len(), 1);
        assert_eq!(app.draft, "Kept draft");
        assert!(commands.try_recv().is_err());
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
        assert!(app.state.busy);
        assert_eq!(app.state.dialogs.len(), 1);
    }
}
#[test]
fn commit_pointer_phases_chords_and_foreign_modal_do_not_send_answer_or_leave_job() {
    for phase in [true, false] {
        let (ctx, mut app) = fixture();
        let pos = first(&ctx).1.center();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.dialogs.push_back(questions::demo_dialog());
        for pressed in [true, false] {
            let mut events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            if phase == pressed {
                events.extend([
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                    egui::Event::Text(" ".into()),
                    egui::Event::Paste("no answer".into()),
                ]);
            }
            frame(&ctx, &mut app, events);
        }
        assert!(app.changes.branches.history.has_detail());
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
        assert_eq!(app.draft, "Kept draft");
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            prompt_ui::modal(ctx, egui::Id::new("foreign-commit-detail"), 220.0, |ui| {
                prompt_ui::heading(ui, "Information", "Foreign")
            });
        });
        app.changes.branch_inputs(&ctx, true, &app.workspace);
        assert!(!app.changes.branches.open);
    }
}
#[test]
fn back_and_hidden_tab_drop_actual_detail_delivery_and_restore_exact_row_focus() {
    let (ctx, mut app) = fixture();
    let (row, rect) = first(&ctx);
    click(&ctx, &mut app, rect.center());
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let (tx, rx) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    app.changes.branches.history.detail.pending =
        Some(workspace::read_only::Request::new(rx, cancelled.clone()));
    let back = ctx
        .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("detail-back")))
        .unwrap();
    click(&ctx, &mut app, back.1.center());
    assert!(cancelled.load(Ordering::Acquire));
    assert!(tx.send(Err("late".into())).is_err());
    for _ in 0..2 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(ctx.memory(|memory| memory.has_focus(row)));
    assert!(!app.changes.branches.history.has_detail());
    click(&ctx, &mut app, first(&ctx).1.center());
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let (tx, rx) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    app.changes.branches.history.detail.pending =
        Some(workspace::read_only::Request::new(rx, cancelled.clone()));
    app.inspector_panel = false;
    frame(&ctx, &mut app, vec![]);
    assert!(cancelled.load(Ordering::Acquire));
    assert!(tx.send(Err("late".into())).is_err());
    assert!(!app.changes.branches.open);
}
#[test]
fn old_commit_widget_cannot_activate_replacement_snapshot_or_refresh_detail_view() {
    let (ctx, mut app) = fixture();
    let (old, _) = first(&ctx);
    let snapshot = app.changes.branches.history.snapshot.as_mut().unwrap();
    snapshot.head = "c".repeat(40);
    snapshot.commits[0].oid = snapshot.head.clone();
    ctx.memory_mut(|memory| memory.request_focus(old));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(!app.changes.branches.history.has_detail());
    assert!(
        app.changes
            .branches
            .history
            .open_detail(&ctx, &"c".repeat(40), true)
    );
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let refresh = ctx
        .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("refresh-response")))
        .unwrap()
        .0;
    assert!(app.changes.branches.refresh_id.is_none());
    assert!(ctx.read_response(refresh).is_none());
    ctx.memory_mut(|memory| memory.request_focus(refresh));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(app.changes.branches.history.has_detail());
    assert_eq!(app.changes.branches.history.requests, 0);
}
#[test]
fn explicit_detail_demo_and_max_message_retention_do_not_create_idle_work_or_capture() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_changes: true,
            demo_branches: true,
            demo_git_history: true,
            demo_commit_detail: true,
            ..Default::default()
        },
    );
    assert!(app.changes.branches.history.has_detail());
    app.changes
        .branches
        .history
        .detail
        .result
        .as_mut()
        .unwrap()
        .as_mut()
        .unwrap()
        .message = "x".repeat(workspace::history::details::MAX_MESSAGE);
    app.state.busy = false;
    let start = std::time::Instant::now();
    for _ in 0..30 {
        let output = frame(&ctx, &mut app, vec![]);
        assert!(output.platform_output.commands.is_empty());
    }
    let detail = app
        .changes
        .branches
        .history
        .detail
        .result
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(detail.message.capacity(), 24 * 1024);
    assert!(app.changes.branches.history.detail.pending.is_none());
    assert!(app.client.is_none() && app.workspace_rx.is_none() && app.diff_rx.is_none());
    assert!(app.draft.is_empty());
    eprintln!(
        "30fullapp24KiBdetailframes {:?}; messagecapacity{}B, noIO/capture",
        start.elapsed(),
        detail.message.capacity()
    );
}
#[test]
fn parent_metadata_stays_plain_noninteractive_and_never_changes_selected_commit() {
    for count in [0, 1, 2, 16] {
        let (ctx, mut app) = fixture();
        assert!(
            app.changes
                .branches
                .history
                .open_detail(&ctx, &"a".repeat(40), true)
        );
        let parents = (0..count)
            .map(|index| format!("{index:040x}"))
            .collect::<Vec<_>>();
        app.changes
            .branches
            .history
            .detail
            .result
            .as_mut()
            .unwrap()
            .as_mut()
            .unwrap()
            .parents = parents.clone();
        let start = std::time::Instant::now();
        for _ in 0..12 {
            let output = frame(&ctx, &mut app, vec![]);
            assert!(output.platform_output.commands.is_empty());
        }
        let detail = app
            .changes
            .branches
            .history
            .detail
            .result
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap();
        assert_eq!(detail.parents, parents);
        assert_eq!(detail.oid, "a".repeat(40));
        assert!(app.changes.branches.history.detail.pending.is_none());
        assert_eq!(app.draft, "Kept draft");
        eprintln!(
            "12parentdetailframes/{count}parents {:?}; parentstrings{}B",
            start.elapsed(),
            parents.iter().map(String::capacity).sum::<usize>()
        );
    }
}
#[test]
fn detail_narrow_themes_zoom_large_untrusted_message_is_bounded_and_settled_is_inert() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app) = fixture();
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            assert!(
                app.changes
                    .branches
                    .history
                    .open_detail(&ctx, &"a".repeat(40), true)
            );
            app.changes
                .branches
                .history
                .detail
                .result
                .as_mut()
                .unwrap()
                .as_mut()
                .unwrap()
                .message = "literal <script> 世界\n".repeat(200);
            let mut native = eframe::Frame::_new_kittest();
            for _ in 0..5 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 780.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut native),
                );
                assert!(output.platform_output.commands.is_empty());
            }
            let rect = ctx.memory(|memory| memory.area_rect(popup_id())).unwrap();
            assert!(ctx.screen_rect().contains_rect(rect));
            assert_eq!(app.changes.branches.history.detail.requests, 0);
            assert!(app.client.is_none());
        }
    }
}
