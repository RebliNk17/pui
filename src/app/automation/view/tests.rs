use super::*;

fn app(ctx: &egui::Context) -> Pui {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo_automation();
    app.open_automation();
    ctx.style_mut(|style| style.animation_time = 0.0);
    app
}
fn frame(
    app: &mut Pui,
    ctx: &egui::Context,
    size: Vec2,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    fn labels(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(text) => out.push((
                text.galley.job.text.clone(),
                egui::Rect::from_min_size(text.pos, text.galley.size()),
            )),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| labels(shape, out)),
            _ => {}
        }
    }
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| app.automation_panel(ctx),
    );
    let mut text = Vec::new();
    for shape in output.shapes {
        labels(&shape.shape, &mut text);
    }
    text
}
fn click(app: &mut Pui, ctx: &egui::Context, size: Vec2, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            size,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}
fn position(text: &[(String, egui::Rect)], label: &str) -> egui::Pos2 {
    text.iter()
        .find(|(text, _)| text == label)
        .unwrap_or_else(|| panic!("Missing {label:?}: {text:?}"))
        .1
        .center()
}

#[test]
fn quiet_rows_are_bounded_in_both_themes_at_narrow_widths_and_zoom() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        for width in [270.0, 420.0, 1200.0] {
            for zoom in [0.8, 1.0, 1.4] {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let job = &mut app.automation.jobs[0];
                job.name = "Very long task title ".repeat(16);
                job.creator.name =
                    "https://example.com/issues?filters=long-creator-name".repeat(20);
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let available = ui.available_rect_before_wrap();
                            let response = job_row(ui, job, false, false);
                            assert!(response.rect.height() <= 88.0);
                            assert!(available.contains_rect(response.rect));
                            assert!(ui.min_rect().width() <= available.width() + 1.0);
                        });
                    },
                );
            }
        }
    }
}

#[test]
fn list_starts_collapsed_then_exposes_full_prompt_command_and_creator_on_demand() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let size = Vec2::new(1000.0, 1400.0);
    let name = app.automation.jobs[0].name.clone();
    let message = app.automation.jobs[0].message.clone();
    let text = frame(&mut app, &ctx, size, vec![]);
    assert!(
        text.iter()
            .any(|(s, _)| s == "2 active · 1 paused · 1 completed")
    );
    assert!(!text.iter().any(|(s, _)| s == &message));
    assert!(
        !text
            .iter()
            .any(|(s, _)| s.contains("Delivered events/prompts"))
    );
    assert!(!text.iter().any(|(s, _)| s == "Back to chat"));
    click(&mut app, &ctx, size, position(&text, &name));
    let text = frame(&mut app, &ctx, size, vec![]);
    assert!(text.iter().any(|(s, _)| s == &message));
    assert!(text.iter().any(|(s, _)| s == "Release readiness"));
    assert!(!text.iter().any(|(s, _)| s == "Cron · 0 9 * * 1-5 · UTC"));
    click(&mut app, &ctx, size, position(&text, "Execution details"));
    let text = frame(&mut app, &ctx, size, vec![]);
    assert!(text.iter().any(|(s, _)| s == "Cron · 0 9 * * 1-5 · UTC"));
    assert!(
        text.iter()
            .any(|(s, _)| s == "Workspace: /workspace/payments")
    );
    click(&mut app, &ctx, size, position(&text, &name));
    let text = frame(&mut app, &ctx, size, vec![]);
    assert!(!text.iter().any(|(s, _)| s == &message));
    assert_eq!(app.automation.jobs.len(), 4);
}

#[test]
fn filters_and_collapsed_errors_do_not_hide_delivery_problems() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let size = Vec2::new(1000.0, 1100.0);
    app.automation.jobs[1].error = Some("Creator is open in another window".into());
    let text = frame(&mut app, &ctx, size, vec![]);
    assert!(
        text.iter()
            .any(|(s, _)| s == "Creator is open in another window")
    );
    assert!(text.iter().any(|(s, _)| s == "Needs review"));
    click(&mut app, &ctx, size, position(&text, "Active  2"));
    let text = frame(&mut app, &ctx, size, vec![]);
    assert!(text.iter().any(|(s, _)| s == "Wait for CI to pass"));
    assert!(!text.iter().any(|(s, _)| s == "Weekly dependency review"));
    assert!(
        !text
            .iter()
            .any(|(s, _)| s == "Harbor PR 71 validation completion")
    );
    click(&mut app, &ctx, size, position(&text, "History  1"));
    let text = frame(&mut app, &ctx, size, vec![]);
    assert!(
        text.iter()
            .any(|(s, _)| s == "Harbor PR 71 validation completion")
    );
    assert!(!text.iter().any(|(s, _)| s == "Wait for CI to pass"));
    click(
        &mut app,
        &ctx,
        size,
        position(&text, "Harbor PR 71 validation completion"),
    );
    let text = frame(&mut app, &ctx, size, vec![]);
    assert!(text.iter().any(|(s, _)| s == "Run again"));
    assert!(
        text.iter()
            .any(|(s, _)| s.contains("follow-up work is not verified"))
    );
}

#[test]
fn expanded_actions_preserve_demo_and_read_only_guards() {
    for disabled in ["demo", "read-only", "enabled"] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        if disabled != "demo" {
            app.demo = false;
        }
        if disabled == "read-only" {
            app.automation.failed = true;
        }
        let size = Vec2::new(1000.0, 1200.0);
        let text = frame(&mut app, &ctx, size, vec![]);
        click(
            &mut app,
            &ctx,
            size,
            position(&text, "Morning release review"),
        );
        let text = frame(&mut app, &ctx, size, vec![]);
        click(&mut app, &ctx, size, position(&text, "Pause"));
        assert_eq!(
            app.automation.jobs[0].enabled,
            disabled != "enabled",
            "{disabled}"
        );
        assert!(app.automation.delete.is_none());
    }
}

#[test]
fn orphan_delete_is_available_in_observer_window_and_confirmation_cancel_sends_nothing() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        mode.apply(&ctx);
        ctx.set_zoom_factor(1.25);
        app.demo = false;
        app.agents.nodes.clear();
        app.client = None;
        let home = std::env::temp_dir().join(format!("pui-delete-ui-{}", uuid::Uuid::new_v4()));
        let _owner = store::lock(&home).unwrap();
        store::save(&home, &app.automation.jobs).unwrap();
        app.automation.home = Some(home.clone());
        app.sync = sync::WindowSync::new_at(&ctx, false, Some(home.join("registry")));
        app.automation.jobs[0].error = Some("Creator agent is unavailable".into());
        assert!(app.can_delete_scheduled_job());
        assert!(!app.automation.writable());
        let size = Vec2::new(900.0, 1000.0);
        let text = frame(&mut app, &ctx, size, vec![]);
        click(
            &mut app,
            &ctx,
            size,
            position(&text, "Morning release review"),
        );
        let text = frame(&mut app, &ctx, size, vec![]);
        click(&mut app, &ctx, size, position(&text, "•••"));
        let text = frame(&mut app, &ctx, size, vec![]);
        click(&mut app, &ctx, size, position(&text, "Delete…"));
        assert!(app.automation.delete.is_some());
        let text = frame(&mut app, &ctx, size, vec![]);
        assert!(text.iter().any(|(label, _)| label == "Delete job"));
        click(&mut app, &ctx, size, position(&text, "Cancel"));
        assert!(app.automation.delete.is_none());
        assert!(app.automation.deleting.is_none());
        assert_eq!(store::load(&home).unwrap().len(), 4);
        assert!(app.agents.nodes.is_empty());
        // Confirming in an observer queues exactly one request, with no direct write.
        app.automation.delete = Some(app.automation.jobs[0].id.clone());
        for _ in 0..3 {
            frame(&mut app, &ctx, size, vec![]);
        }
        let text = frame(&mut app, &ctx, size, vec![]);
        click(&mut app, &ctx, size, position(&text, "Delete job"));
        assert!(app.automation.delete.is_none());
        assert!(app.automation.deleting.is_some());
        assert_eq!(store::load(&home).unwrap().len(), 4);
    }
}

#[test]
fn scheduled_tasks_use_the_shared_breadcrumb_to_return_to_chat() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "Keep this draft".into();
    let mut chat = egui::Rect::NOTHING;
    let mut render = |events| {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1200.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::TopBottomPanel::top("breadcrumb-test").show(ctx, |ui| {
                    app.navigation_header(ui, ui.available_width());
                });
                if app.automation.open {
                    app.automation_panel(ctx);
                }
            },
        );
    };
    render(vec![]);
    // The first breadcrumb button after history is the shared Chat link.
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1200.0, 900.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::TopBottomPanel::top("breadcrumb-test").show(ctx, |ui| {
                app.navigation_header(ui, ui.available_width());
            });
            app.automation_panel(ctx);
        },
    );
    for shape in output.shapes {
        if let egui::Shape::Text(text) = shape.shape {
            assert_ne!(text.galley.text(), "Back to chat");
            if text.galley.text() == "Chat" {
                chat = egui::Rect::from_min_size(text.pos, text.galley.size());
            }
        }
    }
    assert!(chat.is_positive());
    for pressed in [true, false] {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1200.0, 900.0),
                )),
                events: vec![
                    egui::Event::PointerMoved(chat.center()),
                    egui::Event::PointerButton {
                        pos: chat.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ctx| {
                egui::TopBottomPanel::top("breadcrumb-test").show(ctx, |ui| {
                    app.navigation_header(ui, ui.available_width());
                });
            },
        );
    }
    assert!(!app.automation.open);
    assert_eq!(app.draft, "Keep this draft");
}

#[test]
fn status_counts_filters_and_next_activity_distinguish_pending_and_completed() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let colors = theme::palette(&ctx);
    assert_eq!(job_counts(&app.automation.jobs), [2, 1, 1]);
    assert_eq!(
        cadence(&app.automation.jobs[0].trigger),
        "Cron · Weekdays at 09:00 UTC"
    );
    assert_eq!(
        cadence(&app.automation.jobs[1].trigger),
        "Monitor · Every minute"
    );
    assert_eq!(
        cadence(&Trigger::Cron {
            schedule: "*/5 * * * *".into()
        }),
        "Cron · */5 * * * * · UTC"
    );
    let job = &mut app.automation.jobs[1];
    job.next_run = 140;
    assert_eq!(next_activity(job, 100), "Check in 40s");
    assert_eq!(next_activity(job, 140), "Check due");
    job.pending = Some("condition met".into());
    assert_eq!(job_status(job, false, colors).0, "Waiting to deliver");
    assert_eq!(next_activity(job, 100), "Waiting for creator chat");
    job.dispatching = true;
    assert_eq!(job_status(job, false, colors).0, "Delivering");
    job.dispatching = false;
    job.enabled = false;
    job.error = Some("Review delivery before retrying".into());
    assert_eq!(job_status(job, false, colors).0, "Needs review");
    assert_eq!(next_activity(job, 100), "Not scheduled");
    assert!(!JobFilter::Active.includes(job));
    job.error = None;
    job.completed = true;
    assert_eq!(job_status(job, false, colors).0, "Completed");
    assert!(JobFilter::History.includes(job));
}

#[test]
fn empty_page_explains_scheduling_without_empty_filters() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.automation.jobs.clear();
    let text = frame(&mut app, &ctx, Vec2::new(1000.0, 900.0), vec![]);
    assert!(text.iter().any(|(s, _)| s == "No scheduled tasks yet."));
    assert!(text.iter().any(|(s, _)| s == "New recurring prompt…"));
    assert!(!text.iter().any(|(s, _)| s.starts_with("All tasks")));
    assert!(!text.iter().any(|(s, _)| s.contains("0 active")));
    // Help is expanded when there is nothing else to show.
    assert!(
        text.iter()
            .any(|(s, _)| s.contains("Delivered events/prompts"))
    );
}

#[test]
fn load_failure_offers_reload_that_recovers_after_repair() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        mode.apply(&ctx);
        app.demo = false;
        let jobs = std::mem::take(&mut app.automation.jobs);
        let home = std::env::temp_dir().join(format!("pui-reload-ui-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("jobs.json"), b"not json").unwrap();
        app.automation.home = Some(home.clone());
        app.automation.refresh();
        assert!(app.automation.failed && app.automation.load_failed);
        let size = Vec2::new(420.0, 900.0);
        let text = frame(&mut app, &ctx, size, vec![]);
        assert!(
            text.iter()
                .any(|(s, _)| s == "Couldn’t load scheduled tasks")
        );
        assert!(text.iter().any(|(s, _)| s == "Scheduled tasks unavailable"));
        assert!(!text.iter().any(|(s, _)| s == "No scheduled tasks yet."));
        // Reloading a still-broken store keeps failing closed.
        click(&mut app, &ctx, size, position(&text, "Reload"));
        assert!(app.automation.failed && app.automation.jobs.is_empty());
        store::save(&home, &jobs).unwrap();
        let text = frame(&mut app, &ctx, size, vec![]);
        click(&mut app, &ctx, size, position(&text, "Reload"));
        assert!(!app.automation.failed && app.automation.error.is_none());
        assert_eq!(app.automation.jobs.len(), 4);
        let _ = std::fs::remove_dir_all(&home);
    }
}

#[test]
fn write_failures_are_not_reloadable_and_action_errors_dismiss() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo = false;
    app.automation.failed = true;
    app.automation.error = Some("disk full".into());
    let size = Vec2::new(1000.0, 900.0);
    let text = frame(&mut app, &ctx, size, vec![]);
    assert!(text.iter().any(|(s, _)| s == "Scheduling stopped"));
    assert!(!text.iter().any(|(s, _)| s == "Reload"));
    app.automation.reload();
    assert!(app.automation.failed);
    app.automation.failed = false;
    app.automation.error = Some("Unknown job".into());
    let text = frame(&mut app, &ctx, size, vec![]);
    click(&mut app, &ctx, size, position(&text, "Dismiss"));
    assert!(app.automation.error.is_none());
}
