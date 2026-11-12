use super::*;
use crate::app::tasks::work_item::Tab;

fn tab(app: &mut Pui, label: &str) {
    let rect = app
        .ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("work-item-tab-rect", label))))
        .unwrap();
    click(app, rect.center(), false);
}

#[test]
fn activity_is_in_place_with_status_composer_and_draft_intact() {
    let mut app = app();
    proposal(&mut app);
    app.demo_task();
    app.draft = "Keep this draft".into();
    let id = app.tasks.selected.clone().unwrap();
    app.open_task_from_board(id, Some("demo-Builder".into()));
    let before = serde_json::to_value(app.tasks.current()).unwrap();
    for _ in 0..3 {
        render(&mut app, vec![], false);
    }
    let text = labels(&render(&mut app, vec![], false));
    for expected in [
        "WORK ITEM",
        "Independent review in progress",
        "Stage activity",
    ] {
        assert!(
            text.iter().any(|(s, _)| s == expected),
            "{expected}: {text:?}"
        );
    }
    assert!(text.iter().any(|(s, _)| s.starts_with("Chat with Pi")));
    tab(&mut app, "Evidence");
    assert_eq!(
        app.tasks.views[app.tasks.selected.as_ref().unwrap()].tab,
        Tab::Evidence
    );
    assert!(app.tasks.selected_run.is_none());
    tab(&mut app, "Changes");
    assert_eq!(app.draft, "Keep this draft");
    assert_eq!(serde_json::to_value(app.tasks.current()).unwrap(), before);
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.job.is_none());
}

#[test]
fn banner_actions_navigate_evidence_without_retry_or_lifecycle_changes() {
    for (phase, action, expected_tab) in [
        (Phase::Blocked, "Review evidence", Tab::Evidence),
        (Phase::Stale, "Review evidence", Tab::Evidence),
        (Phase::Ready, "Review changes", Tab::Changes),
        (Phase::Interrupted, "Inspect retained work", Tab::Activity),
    ] {
        let mut app = app();
        proposal(&mut app);
        app.tasks.current_mut().unwrap().phase = phase;
        let before = serde_json::to_value(app.tasks.current()).unwrap();
        for _ in 0..3 {
            render(&mut app, vec![], false);
        }
        let text = labels(&render(&mut app, vec![], false));
        let pos = text.iter().find(|(s, _)| s == action).unwrap().1.center();
        click(&mut app, pos, false);
        assert_eq!(
            app.tasks.views[app.tasks.selected.as_ref().unwrap()].tab,
            expected_tab
        );
        assert_eq!(serde_json::to_value(app.tasks.current()).unwrap(), before);
        assert!(app.tasks.processes.is_empty());
        assert!(app.tasks.job.is_none());
    }
}

#[test]
fn work_item_geometry_is_bounded_in_both_themes_and_zoom() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        for zoom in [0.8, 1.0, 1.4] {
            for width in [260.0, 736.0] {
                let mut app = app();
                proposal(&mut app);
                theme.apply(&app.ctx);
                app.ctx.set_zoom_factor(zoom);
                app.tasks.current_mut().unwrap().objective = "VeryLongUnbrokenObjective".repeat(20);
                for selected in [Tab::Overview, Tab::Evidence, Tab::Changes, Tab::Activity] {
                    let id = app.tasks.selected.clone().unwrap();
                    app.tasks.views.entry(id).or_default().tab = selected;
                    let ctx = app.ctx.clone();
                    for _ in 0..3 {
                        let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1200.0, 1600.0))), ..Default::default() }, |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                ui.scope_builder(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(ui.cursor().min, Vec2::new(width, 1400.0))), |ui| {
                                    ui.set_width(width); let available = ui.available_rect_before_wrap();
                                    app.task_content(ui);
                                    assert!(ui.min_rect().right() <= available.right() + 1.0, "{theme:?}/{zoom}/{width}/{selected:?}: {:?} vs {available:?}", ui.min_rect());
                                });
                            });
                        });
                    }
                }
            }
        }
    }
}
