use super::*;

fn painted(shape: &egui::Shape, labels: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(t) => labels.push((
            t.galley.job.text.clone(),
            t.galley.rect.translate(t.pos.to_vec2()),
        )),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| painted(s, labels)),
        _ => {}
    }
}
// Wrapping labels include leading space in the full galley bounds. Measure the
// actual glyph rows when asserting gaps beside another control.
fn painted_glyphs(shape: &egui::Shape, labels: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(t) => {
            let rect = t
                .galley
                .rows
                .iter()
                .map(|row| row.rect_without_leading_space().translate(t.pos.to_vec2()))
                .reduce(|a, b| a.union(b))
                .unwrap_or_else(|| t.galley.rect.translate(t.pos.to_vec2()));
            labels.push((t.galley.job.text.clone(), rect));
        }
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| painted_glyphs(s, labels)),
        _ => {}
    }
}
#[test]
fn native_manager_interactions_stage_review_discard_and_navigate_without_rpc() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut manager = Resources::default();
    let frame = |manager: &mut Resources, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1400.0, 1000.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| manager.show(ui, Path::new("/fixture"), true));
            },
        )
    };
    let click = |manager: &mut Resources, label: &str| {
        let output = frame(manager, vec![]);
        let mut labels = vec![];
        for shape in output.shapes {
            painted(&shape.shape, &mut labels);
        }
        let pos = labels
            .iter()
            .find(|(s, _)| s == label)
            .unwrap_or_else(|| panic!("Missing control {label}"))
            .1
            .center();
        for pressed in [true, false] {
            frame(
                manager,
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
    };
    frame(&mut manager, vec![]);
    frame(&mut manager, vec![]);
    click(&mut manager, "Disable here");
    assert_eq!(manager.drafts.len(), 1);
    assert!(manager.job.is_none());
    click(&mut manager, "Review changes →");
    assert!(manager.review_open);
    assert!(manager.review.is_some());
    assert!(manager.escape());
    click(&mut manager, "Discard drafts");
    assert!(manager.drafts.is_empty());
    click(&mut manager, "Workspace");
    assert_eq!(manager.mode, Mode::Workspace);
    click(&mut manager, "Packages & sources");
    assert_eq!(manager.mode, Mode::Packages);
    assert_eq!(manager.filtered().len(), 4);
    assert!(manager.job.is_none());
    click(&mut manager, "Library");
    let item = manager.selected.clone();
    frame(
        &mut manager,
        vec![egui::Event::Key {
            key: egui::Key::ArrowDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_ne!(manager.selected, item);
}
#[test]
fn all_resource_views_fit_narrow_and_wide_windows_in_both_themes() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [620.0, 1000.0, 1400.0] {
            for mode in [Mode::Library, Mode::Workspace, Mode::Packages] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                theme.apply(&ctx);
                let mut manager = Resources::default();
                manager.sync_workspace(Path::new("/fixture"), true);
                manager.mode = mode;
                for _ in 0..3 {
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(width, 1000.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default()
                                .show(ctx, |ui| manager.show(ui, Path::new("/fixture"), true));
                        },
                    );
                    let mut labels = vec![];
                    for shape in output.shapes {
                        painted(&shape.shape, &mut labels);
                    }
                    for (label, rect) in labels {
                        assert!(
                            rect.right() <= width + 1.0,
                            "{mode:?} at {width}: {label} {rect:?}"
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn inspector_and_toolbar_keep_gaps_even_in_edge_to_edge_panes() {
    fn rectangles(shape: &egui::Shape, out: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Rect(rect) => out.push(rect.rect),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| rectangles(shape, out)),
            _ => {}
        }
    }
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        mode.apply(&ctx);
        let mut manager = Resources {
            catalog: Some(demo_catalog()),
            mode: Mode::Packages,
            ..Default::default()
        };
        let item = demo_catalog().items.remove(0);
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1100.0, 1100.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        // Pane separators deliberately use zero spacing. Content
                        // must establish its own gaps instead of inheriting that.
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.allocate_ui_with_layout(
                            Vec2::new(700.0, 110.0),
                            Layout::top_down(Align::LEFT),
                            |ui| manager.toolbar(ui),
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(345.0, 850.0),
                            Layout::top_down(Align::LEFT),
                            |ui| manager.inspector(ui, &item, true),
                        );
                    });
                },
            ));
        }
        let mut labels = Vec::new();
        let mut rects = Vec::new();
        for shape in output.unwrap().shapes {
            painted_glyphs(&shape.shape, &mut labels);
            rectangles(&shape.shape, &mut rects);
        }
        let label = |text: &str| {
            labels
                .iter()
                .find(|(s, _)| s == text)
                .unwrap_or_else(|| panic!("Missing {text}"))
                .1
        };
        let title = label("code-review");
        let kind = label("Skills");
        let icon = rects
            .iter()
            .find(|r| {
                (r.width() - 30.0).abs() < 0.1
                    && (r.height() - 30.0).abs() < 0.1
                    && (r.top() - kind.top()).abs() < 8.0
            })
            .expect("Inspector icon");
        assert!(
            kind.left() - icon.right() >= 11.0,
            "Icon/type gap: {icon:?} {kind:?}"
        );
        assert!(
            title.top() - kind.bottom() >= 4.0,
            "Type/title gap: {kind:?} {title:?}"
        );
        let personal = label("Personal default");
        let switch = rects
            .iter()
            .find(|r| {
                (r.width() - 34.0).abs() < 0.1
                    && (r.height() - 20.0).abs() < 0.1
                    && (r.center().y - personal.center().y).abs() < 4.0
            })
            .expect("Personal toggle");
        assert!(
            personal.left() - switch.right() >= 10.0,
            "Toggle/label gap: {switch:?} {personal:?}"
        );
        let controls: Vec<_> = rects
            .iter()
            .filter(|r| {
                (r.height() - theme::CONTROL_HEIGHT).abs() < 1.0
                    && r.width() > 70.0
                    && r.top() < 110.0
            })
            .collect();
        let search = controls
            .iter()
            .filter(|r| r.width() > 300.0)
            .min_by(|a, b| a.width().total_cmp(&b.width()))
            .expect("Search field");
        let state = controls
            .iter()
            .find(|r| r.left() > search.right() && (r.top() - search.top()).abs() < 1.0)
            .expect("State dropdown beside search");
        assert!(
            state.left() - search.right() >= 9.0,
            "Search/state gap: {search:?} {state:?}"
        );
        let origins = label("All origins");
        let source = label("Developer tools");
        let count = label("4 resources");
        assert!(
            origins.left() - source.right() >= 20.0,
            "Filter gap: {source:?} {origins:?}"
        );
        assert!(
            count.left() - origins.right() >= 20.0,
            "Count gap: {origins:?} {count:?}"
        );
        assert!(
            origins.top() - label("All states").bottom() >= 10.0,
            "Toolbar row gap"
        );
        assert!(manager.drafts.is_empty());
    }
}
#[test]
fn selections_are_staged_and_defaults_remain_separate() {
    let mut manager = Resources {
        catalog: Some(demo_catalog()),
        ..Default::default()
    };
    let item = manager.catalog.as_ref().unwrap().items[0].clone();
    manager.select_state(&item, "project", "off");
    assert!(!manager.effective(&item));
    assert_eq!(manager.selection(&item, "user"), "on");
    assert!(manager.job.is_none());
    manager.select_state(&item, "project", "inherit");
    assert!(manager.drafts.is_empty());
    manager.select_state(&item, "user", "off");
    assert!(!manager.effective(&item));
    assert_eq!(manager.selection(&item, "project"), "inherit");
}
#[test]
fn filters_and_package_ownership_use_real_metadata() {
    let mut manager = Resources {
        catalog: Some(demo_catalog()),
        ..Default::default()
    };
    manager.query = "review".into();
    assert!(
        manager
            .filtered()
            .iter()
            .all(|r| r.name.contains("review") || r.description.contains("review"))
    );
    manager.query.clear();
    manager.mode = Mode::Packages;
    manager.source_group = "developer-tools".into();
    assert_eq!(manager.filtered().len(), 4);
    manager.mode = Mode::Library;
    manager.state_filter = 3;
    assert_eq!(manager.filtered().len(), 1);
}
#[test]
fn workspace_changes_clear_drafts_and_ignore_old_results() {
    let mut manager = Resources {
        cwd: Some("/old".into()),
        catalog: Some(demo_catalog()),
        ..Default::default()
    };
    let item = manager.catalog.as_ref().unwrap().items[0].clone();
    manager.select_state(&item, "project", "off");
    let (tx, rx) = mpsc::channel();
    manager.job = Some(Job {
        cwd: "/old".into(),
        kind: JobKind::Apply,
        rx,
    });
    manager.sync_workspace(Path::new("/new"), true);
    let _ = tx.send(Ok(json!({"results":[],"complete":true})));
    manager.poll();
    assert!(manager.drafts.is_empty());
    assert!(manager.notice.is_none());
}
#[test]
fn forms_validate_arguments_and_preserve_omitted_server_fields() {
    let manager = Resources {
        form: Form {
            kind: 2,
            name: "docs".into(),
            scope: "user".into(),
            edit_id: Some("fixture".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    let request = manager.form_request().unwrap();
    let definition: Value = serde_json::from_str(request["definition"].as_str().unwrap()).unwrap();
    assert!(definition["url"].is_null());
    assert!(definition["headers"].is_null());
    let mut manager = manager;
    manager.form.transport = 1;
    manager.form.args = "[1]".into();
    assert!(manager.form_request().is_err());
}
#[test]
fn resource_dialog_decisions_use_shared_actions_and_escape_never_applies() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [1.0, 1.4] {
            for kind in ["form", "engineering", "remove", "review"] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.style_mut(|style| style.animation_time = 0.0);
                ctx.set_zoom_factor(zoom);
                let mut manager = Resources {
                    catalog: Some(demo_catalog()),
                    ..Default::default()
                };
                let (id, cancel, confirm) = match kind {
                    "form" => {
                        manager.form.open = true;
                        ("resource-add", "Cancel", "Stage & review →")
                    }
                    "engineering" => {
                        manager.form.open = true;
                        manager.form.kind = 6;
                        manager.form.scope = "user".into();
                        ("resource-add", "Cancel", "Stage & review →")
                    }
                    "remove" => {
                        manager.remove = Some(demo_catalog().items.remove(0));
                        ("resource-remove", "Cancel", "Stage removal")
                    }
                    _ => {
                        manager.review_open = true;
                        manager.review = Some(Review::default());
                        ("resource-review", "Back", "Simulate apply")
                    }
                };
                let frame = |manager: &mut Resources, events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(1100.0, 1000.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| manager.dialogs(ctx, true),
                    )
                };
                for _ in 0..4 {
                    frame(&mut manager, vec![]);
                }
                let output = frame(&mut manager, vec![]);
                let rect = ctx.read_response(egui::Id::new(id)).unwrap().rect;
                assert!(
                    ctx.screen_rect().contains_rect(rect),
                    "{kind}/{zoom}: {rect:?}"
                );
                let mut labels = Vec::new();
                for shape in &output.shapes {
                    painted(&shape.shape, &mut labels);
                }
                let cancel = labels.iter().find(|(label, _)| label == cancel).unwrap().1;
                let confirm = labels.iter().find(|(label, _)| label == confirm).unwrap().1;
                assert!(cancel.right() < confirm.left(), "{kind}");
                assert!((cancel.center().y - confirm.center().y).abs() < 1.0);
                let colors = theme::palette(&ctx);
                let fill = if kind == "remove" {
                    colors.red
                } else {
                    colors.accent
                };
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Rect(button) if button.fill == fill && button.rect.contains_rect(confirm)
                )), "{kind}: action must use shared palette styling");
                frame(
                    &mut manager,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                assert!(!manager.form.open && manager.remove.is_none() && !manager.review_open);
                assert!(manager.drafts.is_empty());
                assert!(manager.job.is_none());
                assert!(manager.notice.is_none());
            }
        }
    }
}

#[test]
fn engineering_form_stages_only_on_enabled_confirmation_and_fits_small_windows() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for (width, zoom) in [(620.0, 1.0), (850.0, 1.4)] {
            for (selection, count) in [(0, 5), (3, 1)] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                ctx.style_mut(|style| style.animation_time = 0.0);
                let mut manager = Resources {
                    catalog: Some(Catalog {
                        trusted: false,
                        ..demo_catalog()
                    }),
                    form: Form {
                        open: true,
                        kind: 6,
                        scope: "user".into(),
                        engineering_selection: selection,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let frame = |manager: &mut Resources, events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(width, 1000.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| manager.dialogs(ctx, true),
                    )
                };
                for _ in 0..4 {
                    frame(&mut manager, vec![]);
                }
                let output = frame(&mut manager, vec![]);
                let rect = ctx
                    .read_response(egui::Id::new("resource-add"))
                    .unwrap()
                    .rect;
                assert!(
                    ctx.screen_rect().contains_rect(rect),
                    "{width}/{zoom}: {rect:?}"
                );
                let mut labels = Vec::new();
                for shape in output.shapes {
                    painted(&shape.shape, &mut labels);
                }
                assert!(labels.iter().any(|(label, _)| label == "Stage & review →"));
                assert!(manager.drafts.is_empty());
                let click = |manager: &mut Resources| {
                    let output = frame(manager, vec![]);
                    let mut labels = Vec::new();
                    for shape in output.shapes {
                        painted(&shape.shape, &mut labels);
                    }
                    let pos = labels
                        .iter()
                        .find(|(label, _)| label == "Stage & review →")
                        .unwrap()
                        .1
                        .center();
                    for pressed in [true, false] {
                        frame(
                            manager,
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
                };
                // Pending discovery/review work disables staging.
                let (_tx, rx) = mpsc::channel();
                manager.job = Some(Job {
                    cwd: PathBuf::new(),
                    kind: JobKind::List,
                    rx,
                });
                click(&mut manager);
                assert!(manager.drafts.is_empty());
                assert!(manager.form.open);
                manager.job = None;
                click(&mut manager);
                assert_eq!(manager.drafts.len(), count);
                assert!(!manager.form.open);
                assert!(manager.review_open);
                assert!(manager.review.is_some());
                assert!(manager.job.is_none()); // Demo does not invoke Node or modify config.
                for draft in &manager.drafts {
                    assert_eq!(draft.request["kind"], "skills");
                    assert_eq!(draft.request["scope"], "user");
                    if count == 1 {
                        assert!(draft.request["source"].as_str().unwrap().contains("/tdd/"));
                    }
                }
            }
        }
    }
}

#[test]
fn escape_closes_resource_dialog_before_settings() {
    let mut manager = Resources::default();
    manager.form.open = true;
    assert!(manager.escape());
    assert!(!manager.escape());
    manager.review_open = true;
    assert!(manager.escape());
}
