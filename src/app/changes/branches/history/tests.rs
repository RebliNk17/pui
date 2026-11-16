use super::*;
use crate::app::files::tests::{click, frame};
pub(super) fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
pub(super) fn fixture() -> (egui::Context, Pui) {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_changes: true,
            demo_branches: true,
            demo_git_history: true,
            ..Default::default()
        },
    );
    app.draft = "Kept draft".into();
    app.state.busy = true;
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app)
}
#[test]
fn explicit_history_demo_uses_existing_popup_without_worker_timer_or_provider() {
    let (ctx, mut app) = fixture();
    let output = frame(&ctx, &mut app, vec![]);
    assert!(app.changes.branches.history.active);
    assert!(app.changes.branches.history.snapshot.is_some());
    assert!(app.changes.branches.history.pending.is_none());
    assert_eq!(app.changes.branches.history.requests, 0);
    assert!(app.client.is_none());
    assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text.contains("Synthetic read-only history preview"))));
    let before = app
        .changes
        .branches
        .history
        .snapshot
        .as_ref()
        .unwrap()
        .head
        .clone();
    for _ in 0..50 {
        frame(&ctx, &mut app, vec![]);
    }
    assert_eq!(
        app.changes.branches.history.snapshot.as_ref().unwrap().head,
        before
    );
    assert_eq!(app.changes.branches.history.requests, 0);
    assert_eq!(app.draft, "Kept draft");
}
#[test]
fn real_explicit_tab_and_refresh_reads_only_and_has_no_idle_or_switch_execution() {
    let root = crate::workspace::tests::fixture();
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.prefs.cwd = root.clone();
    app.inspector_panel = true;
    app.prefs.hide_sidebar = true;
    app.workspace = Workspace {
        root: root.clone(),
        branch: "observed".into(),
        is_git: true,
        ..Default::default()
    };
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let gate = permit(&ctx).unwrap();
    let trigger = ctx
        .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("branch-trigger")))
        .unwrap();
    click(&ctx, &mut app, trigger.1.center());
    drop(gate);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(!app.changes.branches.history.loaded);
    let before = crate::workspace::tests::snapshot(&root);
    let (_, rect) = ctx
        .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("history-tab")))
        .unwrap();
    click(&ctx, &mut app, rect.center());
    assert!(app.changes.branches.history.active);
    assert_eq!(app.changes.branches.history.requests, 1);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while app.changes.branches.history.pending.is_some() {
        frame(&ctx, &mut app, vec![]);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        app.changes.branches.history.snapshot.is_some(),
        "{:?}",
        app.changes.branches.history.error
    );
    let head = app
        .changes
        .branches
        .history
        .snapshot
        .as_ref()
        .unwrap()
        .head
        .clone();
    for _ in 0..20 {
        frame(&ctx, &mut app, vec![]);
    }
    assert_eq!(app.changes.branches.history.requests, 1);
    assert_eq!(
        app.changes.branches.history.snapshot.as_ref().unwrap().head,
        head
    );
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    let refresh = ctx
        .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("refresh-response")))
        .unwrap();
    ctx.memory_mut(|memory| memory.request_focus(refresh.0));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.changes.branches.history.requests, 2);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while app.changes.branches.history.pending.is_some() {
        frame(&ctx, &mut app, vec![]);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!app.changes.branches.open);
    assert!(app.changes.branches.history.snapshot.is_none());
    assert!(app.changes.branches.history.pending.is_none());
}
#[test]
fn exact_owner_disconnected_delivery_hide_and_modal_cancel_stale_history() {
    let (ctx, mut app) = fixture();
    let owner = app.changes.branches.owner.clone().unwrap();
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    app.changes.branches.history.pending =
        Some(workspace::read_only::Request::new(rx, cancel.clone()));
    let mut other = owner.clone();
    other.identity.1 = "other".into();
    app.changes.branches.poll(Some(other));
    assert!(cancel.load(Ordering::Acquire));
    assert!(app.changes.branches.history.snapshot.is_none());
    assert!(tx.send(Err("late result".into())).is_err());
    let mut history = History {
        owner: Some(owner.clone()),
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    history.pending = Some(workspace::read_only::Request::new(rx, cancel));
    drop(tx);
    history.poll(Some(owner));
    assert!(history.error.as_ref().unwrap().contains("ended without"));
    app.changes.close_branches(&ctx);
    assert!(!app.changes.branches.history.active);
    assert!(app.changes.branches.history.snapshot.is_none());
}
#[test]
fn history_native_tab_space_enter_escape_chords_and_new_question_keep_draft_and_approvals() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app) = fixture();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let history = ctx
            .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("history-tab")))
            .unwrap()
            .0;
        // Native first Tab from the actual Branches control reaches History.
        let preceding = app.changes.branches.tabs[0].unwrap();
        ctx.memory_mut(|memory| memory.request_focus(preceding));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
        );
        assert_eq!(ctx.memory(|memory| memory.focused()), Some(history));
        app.state.dialogs.push_back(questions::demo_dialog());
        let mut events = vec![
            key(activation, egui::Modifiers::NONE),
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            egui::Event::Paste("not an answer".into()),
        ];
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        frame(&ctx, &mut app, events);
        assert!(app.changes.branches.history.active);
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
fn cancelled_history_shares_branch_permit_and_hidden_popup_drops_receiver_before_release() {
    let (ctx, mut app) = fixture();
    let gate = permit(&ctx).unwrap();
    let owner = app.changes.branches.owner.clone().unwrap();
    app.changes.branches.history.request(&ctx, owner);
    assert!(
        app.changes
            .branches
            .history
            .error
            .as_ref()
            .unwrap()
            .contains("still finishing")
    );
    assert!(permit(&ctx).is_none());
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    app.changes.branches.history.pending =
        Some(workspace::read_only::Request::new(rx, cancel.clone()));
    app.changes.branch_inputs(&ctx, false, &app.workspace);
    assert!(cancel.load(Ordering::Acquire));
    assert!(tx.send(Err("abandoned".into())).is_err());
    assert!(permit(&ctx).is_none());
    drop(gate);
    assert!(permit(&ctx).is_some());
}
#[test]
fn history_tab_pointer_phases_and_foreign_modal_do_not_approve_send_or_leave_pending_reads() {
    for phase in [true, false] {
        let (ctx, mut app) = fixture();
        app.changes.branches.history.active = false;
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let (_, rect) = ctx
            .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("history-tab")))
            .unwrap();
        let pos = rect.center();
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
            if pressed == phase {
                events.extend([
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                    key(egui::Key::Space, egui::Modifiers::NONE),
                    egui::Event::Text(" ".into()),
                    egui::Event::Paste("ignored".into()),
                ]);
            }
            frame(&ctx, &mut app, events);
        }
        assert!(app.changes.branches.history.active);
        assert_eq!(app.changes.branches.history.requests, 0);
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
        assert_eq!(app.draft, "Kept draft");
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            prompt_ui::modal(ctx, egui::Id::new("foreign-history-modal"), 220.0, |ui| {
                prompt_ui::heading(ui, "Information", "Foreign")
            });
        });
        app.changes.branch_inputs(&ctx, true, &app.workspace);
        assert!(!app.changes.branches.open);
        assert!(app.changes.branches.history.pending.is_none());
    }
}
#[test]
fn bounded_history_snapshot_projects_one_hundred_plain_rows_without_idle_rescan() {
    let (ctx, mut app) = fixture();
    let history = &mut app.changes.branches.history;
    history.snapshot = Some(workspace::history::Snapshot {
        head: "a".repeat(40),
        commits: (0..workspace::history::MAX_COMMITS)
            .map(|n| workspace::history::Commit {
                oid: format!("{n:040x}"),
                time: 1_700_000_000,
                subject: "x".repeat(1024),
                first_parent: None,
                parent_count: 0,
            })
            .collect(),
        limited: true,
    });
    let snapshot = history.snapshot.as_ref().unwrap();
    let bytes = snapshot.head.capacity()
        + snapshot.commits.capacity() * std::mem::size_of::<workspace::history::Commit>()
        + snapshot
            .commits
            .iter()
            .map(|commit| commit.oid.capacity() + commit.subject.capacity())
            .sum::<usize>();
    assert!(bytes < 128 * 1024);
    let start = std::time::Instant::now();
    for _ in 0..20 {
        frame(&ctx, &mut app, vec![]);
    }
    eprintln!(
        "20fullapp100historyrowframes {:?}; snapshotcapacity{bytes}B excludeseguilayout/allocator/process",
        start.elapsed()
    );
    assert_eq!(app.changes.branches.history.requests, 0);
    assert!(app.changes.branches.history.pending.is_none());
}
#[test]
fn history_popup_narrow_light_dark_zoom_stays_bounded_with_untrusted_subjects() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app) = fixture();
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            let history = &mut app.changes.branches.history;
            let snapshot = history.snapshot.as_mut().unwrap();
            snapshot.commits[0].subject = "Long literal <script> 世界 ".repeat(30);
            for _ in 0..4 {
                let mut native = eframe::Frame::_new_kittest();
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 780.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut native),
                );
            }
            let rect = ctx.memory(|memory| memory.area_rect(popup_id())).unwrap();
            assert!(ctx.screen_rect().contains_rect(rect));
            assert_eq!(app.changes.branches.history.requests, 0);
            assert!(app.client.is_none());
        }
    }
}
