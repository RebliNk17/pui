use super::*;
use crate::app::files::tests as file_tests;
use std::time::{Duration, Instant};

fn chord(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
fn setup(ctx: &egui::Context) -> (Pui, Permit, Vec<u8>) {
    let root = crate::workspace::tests::fixture();
    let mut app = file_tests::app(ctx);
    app.files.close();
    app.inspector_panel = true;
    app.prefs.hide_sidebar = true;
    app.prefs.cwd = root.clone();
    app.workspace = Workspace {
        root: root.clone(),
        branch: "observed-main".into(),
        is_git: true,
        ..Default::default()
    };
    for _ in 0..3 {
        file_tests::frame(ctx, &mut app, vec![]);
    }
    let permit = permit(ctx).unwrap(); // deterministic first explicit open: busy, never an idle scan
    let trigger = ctx
        .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("branch-trigger")))
        .unwrap();
    file_tests::click(ctx, &mut app, trigger.1.center());
    for _ in 0..3 {
        file_tests::frame(ctx, &mut app, vec![]);
    }
    assert_eq!(app.changes.branches.requests, 1);
    assert!(app.changes.branches.open && app.changes.branches.error.is_some());
    let index = std::fs::read(root.join(".git/index")).unwrap();
    (app, permit, index)
}
#[test]
fn highlighted_branch_prepares_switch_draft_by_keyboard_without_git_write() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let (mut app, _permit, before) = setup(&ctx);
        mode.apply(&ctx);
        app.demo = false;
        app.changes.index_writable = true;
        app.changes.branches.error = None;
        app.changes.branches.snapshot = Some(workspace::branches::Snapshot {
            current: Current::Local("main".into()),
            local: vec!["main".into(), "feature/世界".into()],
        });
        app.changes.branches.project();
        app.changes.branches.selected = 1;
        for _ in 0..3 {
            file_tests::frame(&ctx, &mut app, vec![]);
        }
        let (id, _, enabled) = ctx
            .data(|d| {
                d.get_temp::<(egui::Id, egui::Rect, bool)>(popup_id().with("branch-switch-draft"))
            })
            .unwrap();
        assert!(enabled);
        ctx.memory_mut(|m| m.request_focus(id));
        file_tests::frame(&ctx, &mut app, vec![]);
        file_tests::frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        assert!(!app.changes.branches.open);
        assert_eq!(
            std::fs::read(app.prefs.cwd.join(".git/index")).unwrap(),
            before
        );
        assert!(!app.changes.prepare_branch_switch("bad..branch".into()));
    }
}
#[test]
fn real_header_click_explicit_refresh_pointer_keyboard_and_outside_close_only_read() {
    let ctx = egui::Context::default();
    let (mut app, permit, before) = setup(&ctx);
    for _ in 0..15 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    assert_eq!(app.changes.branches.requests, 1);
    drop(permit);
    let refresh = ctx
        .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("refresh-response")))
        .unwrap();
    ctx.memory_mut(|memory| memory.request_focus(refresh.0));
    file_tests::frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(app.changes.branches.requests, 2);
    let started = Instant::now();
    while app.changes.branches.pending.is_some() && started.elapsed() < Duration::from_secs(5) {
        file_tests::frame(&ctx, &mut app, vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(app.changes.branches.snapshot.is_some() && app.changes.branches.error.is_none());
    for _ in 0..4 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    let row = ctx
        .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(popup_id().with(("row", 0usize))))
        .unwrap();
    file_tests::click(&ctx, &mut app, row.1.center());
    assert!(app.changes.branches.open);
    assert_eq!(app.changes.branches.requests, 2);
    // Tab-focus row activation highlights only; it is not a switch action.
    ctx.memory_mut(|memory| memory.request_focus(row.0));
    file_tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Space), egui::Event::Text(" ".into())],
    );
    assert_eq!(app.changes.branches.selected, 0);
    assert!(app.changes.branches.open);
    let current = app
        .changes
        .branches
        .snapshot
        .as_ref()
        .unwrap()
        .current
        .clone();
    file_tests::frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(
        app.changes.branches.snapshot.as_ref().unwrap().current,
        current
    );
    file_tests::click(&ctx, &mut app, egui::pos2(8.0, 500.0));
    assert!(!app.changes.branches.open);
    assert!(app.changes.branches.snapshot.is_none());
    assert_eq!(
        std::fs::read(app.workspace.root.join(".git/index")).unwrap(),
        before
    );
}
#[test]
fn focused_trigger_modified_activation_and_pointer_open_chords_do_not_reach_rpc() {
    let ctx = egui::Context::default();
    let (mut app, permit, _) = setup(&ctx);
    app.state.busy = true;
    app.draft = "Keep draft".into();
    app.state.dialogs.push_back(json!({"id":"branch-rpc-approval","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo fixture\"}\n\nApprove this call only."}));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let count = app.state.messages.len();
    file_tests::frame(&ctx, &mut app, vec![key(egui::Key::Escape)]);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(trigger_id()));
    for event in [
        chord(egui::Key::Enter, egui::Modifiers::COMMAND),
        chord(egui::Key::Backspace, egui::Modifiers::COMMAND),
    ] {
        file_tests::frame(&ctx, &mut app, vec![event]);
        assert!(!app.changes.branches.open && app.changes.branches.pending.is_none());
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
    }
    file_tests::frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert!(app.changes.branches.open);
    assert_eq!(app.changes.branches.requests, 1);
    for extra in [egui::Key::Enter, egui::Key::Backspace] {
        file_tests::frame(&ctx, &mut app, vec![key(egui::Key::Escape)]);
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
        let trigger = ctx
            .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("branch-trigger")))
            .unwrap();
        for pressed in [true, false] {
            let mut events = vec![
                egui::Event::PointerMoved(trigger.1.center()),
                egui::Event::PointerButton {
                    pos: trigger.1.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            if !pressed {
                events.push(chord(extra, egui::Modifiers::COMMAND));
            }
            file_tests::frame(&ctx, &mut app, events);
        }
        assert!(app.changes.branches.open);
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.pending_dialog.is_none());
        assert!(commands.try_recv().is_err());
    }
    assert_eq!(app.state.messages.len(), count);
    assert_eq!(app.draft, "Keep draft");
    assert!(app.state.busy);
    drop(permit);
}
#[test]
fn hidden_and_parked_session_cancel_popup_worker_and_never_restore_it() {
    for parked in [false, true] {
        let ctx = egui::Context::default();
        let (mut app, permit, _) = setup(&ctx);
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        app.changes.branches.pending = Some(Pending {
            rx,
            cancel: cancel.clone(),
        });
        if parked {
            app.leave_session_layout();
        } else {
            app.inspector_panel = false;
            file_tests::frame(&ctx, &mut app, vec![]);
        }
        assert!(cancel.load(Ordering::Acquire));
        assert!(!egui::Popup::is_id_open(&ctx, popup_id()));
        assert!(
            tx.send(Ok(Snapshot {
                current: Current::Detached,
                local: vec!["late".into()]
            }))
            .is_err()
        );
        if parked {
            app.enter_session_layout();
        }
        assert!(!app.changes.branches.open && app.changes.branches.snapshot.is_none());
        assert!(self::permit(&ctx).is_none());
        drop(permit);
        assert!(self::permit(&ctx).is_some());
    }
}
#[test]
fn popup_does_not_rebind_recorded_review_source_or_owner() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    app.files.close();
    app.prefs.hide_sidebar = true;
    file_tests::frame(&ctx, &mut app, vec![]);
    let path = app.outputs.last().unwrap().path.clone();
    let diff = app.outputs.last().unwrap().diff.clone();
    app.open_diff_review((path.clone(), diff.clone()).into(), true);
    let before = app.changes.review.as_ref().unwrap().2.focus_id();
    app.workspace = Workspace {
        root: app.prefs.cwd.clone(),
        branch: "observed".into(),
        is_git: true,
        ..Default::default()
    };
    for _ in 0..3 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    let permit = permit(&ctx).unwrap();
    let trigger = ctx
        .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("branch-trigger")))
        .unwrap();
    file_tests::click(&ctx, &mut app, trigger.1.center());
    for event in [
        key(egui::Key::ArrowDown),
        key(egui::Key::Enter),
        key(egui::Key::Escape),
    ] {
        file_tests::frame(&ctx, &mut app, vec![event]);
    }
    let doc = app.changes.review.as_ref().unwrap();
    assert_eq!(
        (&doc.0, &doc.1, doc.2.focus_id(), doc.3),
        (&path, &diff, before, true)
    );
    assert!(
        app.changes
            .review_file_navigation(&ctx, &app.outputs, app.output_revision)
            .status
            .starts_with("Recorded file")
    );
    drop(permit);
}
