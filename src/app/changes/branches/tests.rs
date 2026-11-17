use super::*;
use std::time::{Duration, Instant};

fn workspace() -> Workspace {
    Workspace {
        root: "/branch-fixture".into(),
        branch: "main".into(),
        is_git: true,
        ..Default::default()
    }
}
fn loaded(changes: &mut Changes, ctx: &egui::Context) {
    changes.branches.pending = None;
    changes.branches.owner = Owner::new(&changes.identity, &workspace());
    changes.branches.open = true;
    changes.branches.snapshot = Some(Snapshot {
        current: Current::Local("main".into()),
        local: vec!["main".into(), "topic/日本語".into(), "z-last".into()],
    });
    changes.branches.project();
    egui::Popup::open_id(ctx, popup_id());
    ctx.memory_mut(|memory| memory.request_focus(search_id()));
}
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
fn render(
    ctx: &egui::Context,
    changes: &mut Changes,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            changes.review_inputs(ctx, true, &[], 0, &workspace());
            changes.show(
                ctx,
                Path::new("/branch-fixture"),
                "chat",
                &workspace(),
                (&[], 0),
                false,
            );
            egui::CentralPanel::default().show(ctx, |_| {});
        },
    )
}
use std::path::Path;
fn text(output: &egui::FullOutput, needle: &str) -> bool {
    output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains(needle)))
}
#[test]
fn closed_native_trigger_does_not_capture_a_foreign_popup_or_modal() {
    let ctx = egui::Context::default();
    let mut changes = Changes::default();
    changes.bind(Path::new("/branch-fixture"), "chat");
    for modal in [false, true] {
        let foreign = egui::Id::new("branch-foreign-owner");
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.interact(ui.max_rect(), trigger_id(), Sense::click());
            });
            if modal {
                prompt_ui::modal(ctx, foreign, 280.0, |ui| {
                    prompt_ui::heading(ui, "Information", "Foreign owner");
                });
            }
        });
        if !modal {
            egui::Popup::open_id(&ctx, foreign);
        }
        ctx.memory_mut(|memory| {
            memory.request_focus_with_filter(
                trigger_id(),
                egui::EventFilter {
                    escape: true,
                    ..Default::default()
                },
            )
        });
        let events = vec![
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Escape, egui::Modifiers::NONE),
        ];
        let _ = ctx.run(
            egui::RawInput {
                events: events.clone(),
                ..Default::default()
            },
            |ctx| {
                let before = ctx.input(|input| input.events.clone());
                changes.branch_inputs(ctx, true, &workspace());
                assert_eq!(ctx.input(|input| input.events.clone()), before);
                assert_eq!(ctx.memory(|memory| memory.focused()), Some(trigger_id()));
                if !modal {
                    assert!(egui::Popup::is_id_open(ctx, foreign));
                }
            },
        );
        egui::Popup::close_all(&ctx);
    }
}
#[test]
fn explicit_demo_uses_actual_popup_with_synthetic_notice_and_no_git_worker() {
    use crate::app::files::tests::frame;
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_changes: true,
            demo_branches: true,
            ..Default::default()
        },
    );
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let output = frame(&ctx, &mut app, vec![]);
    assert!(text(
        &output,
        "Demo fixture · synthetic local/current names"
    ));
    assert!(app.changes.branches.demo && app.changes.branches.open);
    assert!(app.changes.branches.pending.is_none() && app.changes.branches.requests == 0);
    assert!(app.client.is_none());
    assert!(
        ctx.data(|data| data.get_temp::<Arc<AtomicBool>>(popup_id().with("worker")))
            .is_none()
    );
}
#[test]
fn passive_changes_frames_do_not_acquire_branch_worker_or_poll_git() {
    let ctx = egui::Context::default();
    let mut changes = Changes::default();
    for _ in 0..30 {
        render(&ctx, &mut changes, 1134.0, vec![]);
    }
    assert!(!changes.branches.open && changes.branches.pending.is_none());
    assert!(
        ctx.data(|data| data.get_temp::<Arc<AtomicBool>>(popup_id().with("worker")))
            .is_none()
    );
    assert!(permit(&ctx).is_some());
    assert!(changes.branches.owner.is_none());
}
#[test]
fn loading_error_disconnect_stale_root_and_session_cannot_deliver_old_lists() {
    let ctx = egui::Context::default();
    let mut changes = Changes::default();
    render(&ctx, &mut changes, 1134.0, vec![]);
    loaded(&mut changes, &ctx);
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    changes.branches.snapshot = None;
    changes.branches.pending = Some(Pending {
        rx,
        cancel: cancel.clone(),
    });
    for _ in 0..3 {
        render(&ctx, &mut changes, 1134.0, vec![]);
    }
    let output = render(&ctx, &mut changes, 1134.0, vec![]);
    assert!(text(&output, "Reading local branches"));
    drop(tx);
    let output = render(&ctx, &mut changes, 1134.0, vec![]);
    assert!(text(&output, "ended without a result"));
    assert!(cancel.load(Ordering::Acquire));
    changes.branches.error =
        Some("Local branch list exceeds 256 branches. No partial list is shown.".into());
    let output = render(&ctx, &mut changes, 1134.0, vec![]);
    assert!(text(&output, "exceeds 256 branches"));
    assert!(changes.branches.snapshot.is_none());
    for mutation in [0, 1, 2] {
        loaded(&mut changes, &ctx);
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        changes.branches.pending = Some(Pending {
            rx,
            cancel: cancel.clone(),
        });
        let mut owner = changes.branches.owner.clone().unwrap();
        match mutation {
            0 => owner.root = "/other-root".into(),
            1 => owner.identity.1 = "other-chat".into(),
            _ => owner.observed = "external-checkout".into(),
        }
        changes.branches.poll(Some(owner));
        assert!(changes.branches.stale && changes.branches.snapshot.is_none());
        assert!(changes.branches.pending.is_none() && cancel.load(Ordering::Acquire));
        assert!(
            tx.send(Ok(Snapshot {
                current: Current::Detached,
                local: vec!["stale".into()]
            }))
            .is_err()
        );
    }
}
#[test]
fn popup_is_quiet_bounded_in_themes_zoom_and_narrow_with_truthful_selection() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [420.0, 1134.0] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(1.25);
            let mut changes = Changes::default();
            render(&ctx, &mut changes, width, vec![]);
            loaded(&mut changes, &ctx);
            for _ in 0..4 {
                render(&ctx, &mut changes, width, vec![]);
            }
            let output = render(&ctx, &mut changes, width, vec![]);
            assert!(text(
                &output,
                "Inspect names · prepare a reviewed switch"
            ));
            assert!(text(&output, "Current local: main"));
            let rect = ctx.memory(|memory| memory.area_rect(popup_id())).unwrap();
            assert!(
                rect.left() >= 0.0 && rect.right() <= ctx.screen_rect().right() + 1.0,
                "{rect:?}"
            );
            render(
                &ctx,
                &mut changes,
                width,
                vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
            );
            assert_eq!(changes.branches.selected, 1);
            assert_eq!(
                changes.branches.snapshot.as_ref().unwrap().current,
                Current::Local("main".into())
            );
            let output = render(&ctx, &mut changes, width, vec![]);
            assert!(text(
                &output,
                "Highlighted local: topic/日本語 · not current"
            ));
            changes.branches.snapshot.as_mut().unwrap().current = Current::Detached;
            let output = render(&ctx, &mut changes, width, vec![]);
            assert!(text(&output, "detached HEAD · no local branch selected"));
            render(
                &ctx,
                &mut changes,
                width,
                vec![egui::Event::Text("日本語".into())],
            );
            assert_eq!(changes.branches.hits, vec![1]);
            render(
                &ctx,
                &mut changes,
                width,
                vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            );
            assert!(!changes.branches.open && !egui::Popup::is_id_open(&ctx, popup_id()));
        }
    }
}
#[test]
fn unsupported_or_oversized_workspace_metadata_never_starts_discovery() {
    assert!(
        Owner::new(
            &(PathBuf::from("/root"), "chat".into()),
            &Workspace::default()
        )
        .is_none()
    );
    let mut workspace = workspace();
    workspace.branch = "x".repeat(1025);
    assert!(Owner::new(&(PathBuf::from("/root"), "chat".into()), &workspace).is_none());
    workspace.branch = "bad\nname".into();
    assert!(Owner::new(&(PathBuf::from("/root"), "chat".into()), &workspace).is_none());
    assert!(
        Owner::new(
            &(PathBuf::from("/root"), "x".repeat(8193)),
            &self::workspace()
        )
        .is_none()
    );
}
#[test]
fn cancelled_owner_retains_one_window_permit_until_worker_releases_it() {
    let ctx = egui::Context::default();
    let permit1 = permit(&ctx).unwrap();
    assert!(permit(&ctx).is_none());
    let mut changes = Changes::default();
    changes.branches.open = true;
    changes.branches.request(
        &ctx,
        Owner::new(
            &(PathBuf::from("/branch-fixture"), "chat".into()),
            &workspace(),
        )
        .unwrap(),
    );
    assert!(
        changes
            .branches
            .error
            .as_ref()
            .unwrap()
            .contains("still finishing")
    );
    changes.close_branches(&ctx);
    assert!(permit(&ctx).is_none());
    drop(permit1);
    assert!(permit(&ctx).is_some());
}
#[test]
fn explicit_real_local_worker_completion_and_close_are_bounded() {
    let ctx = egui::Context::default();
    let root = crate::workspace::tests::fixture();
    let workspace = Workspace {
        root: root.clone(),
        branch: "observed".into(),
        is_git: true,
        ..Default::default()
    };
    let before = crate::workspace::tests::snapshot(&root);
    let mut branches = Branches {
        open: true,
        ..Default::default()
    };
    let owner = Owner::new(&(root, "chat".into()), &workspace).unwrap();
    branches.request(&ctx, owner.clone());
    let started = Instant::now();
    while branches.pending.is_some() && started.elapsed() < Duration::from_secs(5) {
        branches.poll(Some(owner.clone()));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(branches.pending.is_none() && branches.error.is_none());
    assert!(branches.snapshot.is_some());
    assert_eq!(crate::workspace::tests::snapshot(&workspace.root), before);
    branches.close(&ctx, false);
    assert!(branches.snapshot.is_none() && branches.hits.is_empty());
    assert!(permit(&ctx).is_some());
}
#[test]
fn full_app_popup_keys_cannot_stop_send_answer_approval_or_agent_entry_presets() {
    use crate::app::files::tests::{app, frame};
    for method in ["confirm", "input", "select"] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.files.close();
        app.inspector_panel = true;
        app.workspace = workspace();
        app.state.busy = true;
        app.draft = "Keep draft".into();
        frame(&ctx, &mut app, vec![]);
        loaded(&mut app.changes, &ctx);
        app.state.dialogs.push_back(json!({"id":"branch-prompt", "method":method, "title":"Fixture", "message":"Keep pending", "options":["first", "second"]}));
        let messages = app.state.messages.len();
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let refresh = app.changes.branches.refresh_id.unwrap();
        for focus in [search_id(), refresh] {
            ctx.memory_mut(|memory| memory.request_focus(focus));
            for event in [
                key(egui::Key::Enter, egui::Modifiers::COMMAND),
                key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                key(egui::Key::Num1, egui::Modifiers::NONE),
            ] {
                frame(&ctx, &mut app, vec![event]);
                assert_eq!(app.state.dialogs.len(), 1);
                assert_eq!(app.state.messages.len(), messages);
                assert!(app.state.busy);
                assert!(egui::Popup::is_id_open(&ctx, popup_id()));
            }
        }
        // Newly arrived input prompt may steal focus; branch hook reclaims it
        // before any following text/selection shortcut can reach that entry.
        frame(&ctx, &mut app, vec![egui::Event::Text("filter".into())]);
        assert_ne!(app.dialog_text, "filter");
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert_eq!(app.state.dialogs.len(), 1);
        assert_eq!(app.draft, "Keep draft");
        assert!(app.state.busy);
        assert!(!egui::Popup::is_id_open(&ctx, popup_id()));
    }
}
#[test]
fn popup_input_staging_bounds_events_and_paste_payload() {
    let event = bounded_key(&egui::Event::Paste("日本語".repeat(1000)));
    let egui::Event::Paste(text) = event else {
        panic!("expected paste");
    };
    assert_eq!(text.chars().count(), 128);
    let ctx = egui::Context::default();
    let mut changes = Changes::default();
    render(&ctx, &mut changes, 1134.0, vec![]);
    loaded(&mut changes, &ctx);
    render(
        &ctx,
        &mut changes,
        1134.0,
        (0..300)
            .map(|_| egui::Event::Text("x".repeat(256)))
            .collect(),
    );
    assert!(changes.branches.query.len() <= 128);
    assert!(changes.branches.keys.is_empty());
}
#[test]
fn bounded_search_projection_settles_without_workers_and_releases_payload() {
    let mut branches = Branches {
        snapshot: Some(Snapshot {
            current: Current::Detached,
            local: (0..256)
                .map(|n| format!("topic/{n:04}/{}", "x".repeat(240)))
                .collect(),
        }),
        query: "topic".into(),
        ..Default::default()
    };
    branches.project();
    assert_eq!(branches.hits.len(), 256);
    let capacity: usize = branches
        .snapshot
        .as_ref()
        .unwrap()
        .local
        .iter()
        .map(String::capacity)
        .sum();
    let started = Instant::now();
    for _ in 0..100 {
        branches.project();
    }
    eprintln!(
        "branch projection 256 bounded refs/100 searches {:?}, retained name capacity {capacity}B (excludes Git/OS/font/thread overhead)",
        started.elapsed()
    );
    assert!(capacity < 128 * 1024);
    assert!(branches.pending.is_none());
    branches.cancel();
    assert!(branches.snapshot.is_none() && branches.query.is_empty() && branches.hits.is_empty());
}
