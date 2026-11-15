use super::*;

fn render(app: &mut Pui, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1000.0, 850.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    )
}
fn click(app: &mut Pui, pos: egui::Pos2) {
    for pressed in [true, false] {
        render(
            app,
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
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
fn fixture() -> (Pui, String, PathBuf) {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    let section = Section::new("Review".into());
    let id = section.id.clone();
    let path = app.sessions[0].path.clone();
    app.prefs.sidebar_view.sections.push(section);
    app.prefs.chats.entry(path.clone()).or_default().section = Some(id.clone());
    (app, id, path)
}
fn label(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
    crate::app::files::tests::text(output, text)
}
fn header(app: &Pui, id: &str) -> (egui::Rect, egui::Rect) {
    app.ctx
        .data(|data| {
            data.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new(("section-header-rects", id)))
        })
        .unwrap()
}

#[test]
fn inline_section_rename_saves_only_on_enter_and_escape_never_stops_chat() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        let (mut app, id, _) = fixture();
        app.prefs.theme = theme;
        app.draft = "Keep draft".into();
        app.state.busy = true;
        for _ in 0..3 {
            render(&mut app, vec![]);
        }
        app.begin_section_rename(&id);
        render(&mut app, vec![]);
        app.section_view.editing.as_mut().unwrap().1 = "Renamed review".into();
        render(&mut app, vec![key(egui::Key::Enter)]);
        assert_eq!(app.prefs.sidebar_view.sections[0].name, "Renamed review");
        assert!(!app.section_view.editing());
        app.begin_section_rename(&id);
        render(&mut app, vec![]);
        app.section_view.editing.as_mut().unwrap().1 = "Discard me".into();
        render(&mut app, vec![key(egui::Key::Escape)]);
        assert_eq!(app.prefs.sidebar_view.sections[0].name, "Renamed review");
        assert!(!app.section_view.editing());
        assert!(app.state.busy);
        assert_eq!(app.draft, "Keep draft");
        app.begin_section_rename(&id);
        render(&mut app, vec![]);
        app.section_view.editing.as_mut().unwrap().1 = "  ".into();
        render(&mut app, vec![key(egui::Key::Enter)]);
        assert!(app.section_view.editing());
        render(&mut app, vec![key(egui::Key::Escape)]);
    }
}

#[test]
fn header_toggle_and_menu_reorder_use_stable_ids_without_opening_chats() {
    let (mut app, id, path) = fixture();
    app.prefs
        .sidebar_view
        .sections
        .push(Section::new("Later".into()));
    app.draft = "unchanged".into();
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let active = app.agents.active.clone();
    let (heading, more) = header(&app, &id);
    assert!(heading.right() < more.left());
    click(&mut app, heading.center());
    assert!(app.prefs.sidebar_view.sections[0].collapsed);
    let heading = header(&app, &id).0.center();
    click(&mut app, heading);
    assert!(!app.prefs.sidebar_view.sections[0].collapsed);
    let menu = header(&app, &id).1.center();
    click(&mut app, menu);
    let output = render(&mut app, vec![]);
    click(&mut app, label(&output, "Move section down"));
    assert_eq!(app.prefs.sidebar_view.sections[1].id, id);
    assert_eq!(app.prefs.chats[&path].section.as_deref(), Some(id.as_str()));
    assert_eq!(app.agents.active, active);
    assert_eq!(app.draft, "unchanged");
}

#[test]
fn confirmation_escape_and_explicit_remove_keep_sessions_and_live_approvals() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        let (mut app, id, path) = fixture();
        app.prefs.theme = theme;
        app.state.busy = true;
        app.draft = "Do not submit".into();
        app.state.dialogs.push_back(json!({"type":"extension_ui_request", "method":"confirm", "id":"approval", "title":"Allow bash?", "message":"{\"command\":\"test\"}\n\nApprove this call only."}));
        let sessions = app.sessions.len();
        app.section_view.remove = Some(id.clone());
        render(&mut app, vec![]);
        render(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
        );
        assert_eq!(app.state.dialogs.len(), 1);
        render(&mut app, vec![key(egui::Key::Escape)]);
        assert!(!app.section_view.confirming());
        assert!(app.prefs.chats[&path].section.is_some());
        app.section_view.remove = Some(id.clone());
        for _ in 0..3 {
            render(&mut app, vec![]);
        }
        let output = render(&mut app, vec![]);
        click(&mut app, label(&output, "Remove section"));
        assert!(app.prefs.sidebar_view.sections.is_empty());
        assert!(app.prefs.chats[&path].section.is_none());
        assert_eq!(app.sessions.len(), sessions);
        assert!(app.state.busy);
        assert_eq!(app.state.dialogs.len(), 1);
        assert_eq!(app.draft, "Do not submit");
    }
}

#[test]
fn new_section_is_transactional_and_empty_groups_follow_visibility_setting() {
    let (mut app, _, _) = fixture();
    app.new_sidebar_section();
    let pending = app.prefs.sidebar_view.sections.last().unwrap().id.clone();
    assert!(app.section_view.editing());
    render(&mut app, vec![]);
    render(&mut app, vec![key(egui::Key::Escape)]);
    assert!(
        !app.prefs
            .sidebar_view
            .sections
            .iter()
            .any(|section| section.id == pending)
    );
    let empty = Section::new("Empty collection".into());
    app.prefs.sidebar_view.sections.push(empty);
    let output = render(&mut app, vec![]);
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Empty collection")));
    app.prefs.sidebar_view.empty_groups = true;
    label(&render(&mut app, vec![]), "Empty collection");
    app.new_sidebar_section();
    let id = app.prefs.sidebar_view.sections.last().unwrap().id.clone();
    render(&mut app, vec![]);
    app.section_view.editing.as_mut().unwrap().1 = "Saved section".into();
    render(&mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(
        app.prefs
            .sidebar_view
            .sections
            .iter()
            .find(|section| section.id == id)
            .unwrap()
            .name,
        "Saved section"
    );
}

#[test]
fn root_assignment_menu_relocates_only_the_intended_session() {
    let (mut app, id, original) = fixture();
    let target = app.sessions[1].path.clone();
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let output = render(&mut app, vec![]);
    let target_title = app.session_title(&app.sessions[1]);
    let title = label(&output, &target_title);
    let active = app.agents.active.clone();
    for pressed in [true, false] {
        render(
            &mut app,
            vec![
                egui::Event::PointerMoved(title),
                egui::Event::PointerButton {
                    pos: title,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    let output = render(&mut app, vec![]);
    click(&mut app, label(&output, "Move to section"));
    let output = render(&mut app, vec![]);
    let choice = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Review" => {
                Some(text.pos + text.galley.size() / 2.0)
            }
            _ => None,
        })
        .max_by(|a, b| a.x.total_cmp(&b.x))
        .expect("Section submenu choice");
    click(&mut app, choice);
    assert_eq!(
        app.prefs.chats[&target].section.as_deref(),
        Some(id.as_str())
    );
    assert_eq!(
        app.prefs.chats[&original].section.as_deref(),
        Some(id.as_str())
    );
    assert_eq!(app.agents.active, active);
}

#[test]
fn roots_keep_children_once_and_pinned_descendants_keep_pin_precedence() {
    let (mut app, id, _) = fixture();
    app.demo_agent_tree();
    let root = app.agents.active.clone();
    let parent_path = PathBuf::from("/fixture/sections-parent");
    app.agents.nodes.get_mut(&root).unwrap().path = Some(parent_path.clone());
    app.state.session_file = Some(parent_path.display().to_string());
    app.prefs
        .chats
        .entry(parent_path.clone())
        .or_default()
        .section = Some(id.clone());
    let child = app.agents.nodes[&root].children[0].clone();
    let child_path = PathBuf::from("/fixture/sections-child");
    app.agents.nodes.get_mut(&child).unwrap().path = Some(child_path.clone());
    app.agents
        .parked
        .get_mut(&child)
        .unwrap()
        .state
        .session_file = Some(child_path.display().to_string());
    let meta = app.prefs.chats.entry(child_path.clone()).or_default();
    meta.section = Some(id.clone());
    meta.pinned = true;
    let meta = app.prefs.chats.entry(child_path.clone()).or_default();
    meta.pinned = false;
    let (pins, branches) = sidebar_tree::listing(&app);
    assert!(pins.is_empty());
    let mut visible: Vec<_> = (0..branches.len()).collect();
    let grouped = partition(&app, &branches, &mut visible);
    let root = grouped[&id]
        .iter()
        .map(|index| &branches[*index])
        .find(|branch| branch.path.as_ref() == Some(&parent_path))
        .unwrap();
    assert_eq!(
        root.children
            .iter()
            .filter(|child| child.path.as_ref() == Some(&child_path))
            .count(),
        1
    );
    app.prefs.chats.get_mut(&child_path).unwrap().pinned = true;
    let (pins, branches) = sidebar_tree::listing(&app);
    assert_eq!(
        pins.iter()
            .filter(|branch| branch.path.as_ref() == Some(&child_path))
            .count(),
        1
    );
    let mut visible: Vec<_> = (0..branches.len()).collect();
    let groups = partition(&app, &branches, &mut visible);
    let parent = groups[&id]
        .iter()
        .map(|index| &branches[*index])
        .find(|branch| branch.path.as_ref() == Some(&parent_path))
        .unwrap();
    assert!(
        !parent
            .children
            .iter()
            .any(|branch| branch.path.as_ref() == Some(&child_path))
    );
    assert!(!parent.children.is_empty());
    assert!(
        visible
            .iter()
            .all(|index| branches[*index].path.as_ref() != Some(&parent_path))
    );
}

#[test]
fn collapsed_section_keyboard_and_show_more_preserve_chat_identity() {
    let (mut app, id, _) = fixture();
    for session in &app.sessions {
        app.prefs
            .chats
            .entry(session.path.clone())
            .or_default()
            .section = Some(id.clone());
    }
    app.prefs.sidebar_view.sections[0].collapsed = true;
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let heading_rect = header(&app, &id).0;
    click(&mut app, heading_rect.center());
    assert!(!app.prefs.sidebar_view.sections[0].collapsed);
    let output = render(&mut app, vec![]);
    click(&mut app, label(&output, "Show 2 more"));
    assert!(app.prefs.sidebar_view.sections[0].expanded);
    let output = render(&mut app, vec![]);
    click(&mut app, label(&output, "Show less"));
    assert!(!app.prefs.sidebar_view.sections[0].expanded);
    assert!(!app.prefs.sidebar_view.view_all);
    let heading_id = app
        .ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("section-header-id", &id))))
        .unwrap();
    app.ctx
        .memory_mut(|memory| memory.request_focus(heading_id));
    render(&mut app, vec![key(egui::Key::ArrowLeft)]);
    assert!(app.prefs.sidebar_view.sections[0].collapsed);
    render(&mut app, vec![key(egui::Key::ArrowRight)]);
    assert!(!app.prefs.sidebar_view.sections[0].collapsed);
}

#[test]
fn section_confirmation_fits_themes_zoom_and_safe_enter_cancel() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        for zoom in [0.8, 1.4] {
            let (mut app, id, path) = fixture();
            app.prefs.theme = mode;
            app.prefs.sidebar_view.sections[0].name = "Long section name ".repeat(4);
            app.ctx.set_zoom_factor(zoom);
            app.section_view.remove = Some(id);
            let ctx = app.ctx.clone();
            let draw = |app: &mut Pui, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(620.0, 700.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| app.sidebar_section_confirmation(ctx),
                )
            };
            for _ in 0..3 {
                draw(&mut app, vec![]);
            }
            let output = draw(&mut app, vec![]);
            for needle in ["Remove this section?", "Keep section", "Remove section"] {
                let position = label(&output, needle);
                assert!(
                    ctx.screen_rect().contains(position),
                    "{needle} outside {:?}",
                    ctx.screen_rect()
                );
            }
            draw(&mut app, vec![key(egui::Key::Enter)]);
            assert!(
                !app.section_view.confirming(),
                "Safe initial Enter must cancel, never remove"
            );
            assert!(app.prefs.chats[&path].section.is_some());
        }
    }
}

#[test]
fn many_section_projection_is_bounded_and_keeps_sorted_members() {
    let (mut app, id, _) = fixture();
    let sample = app.sessions[0].clone();
    app.sessions = (0..2000)
        .map(|index| Session {
            path: PathBuf::from(format!("/fixture/section-load-{index}")),
            title: format!("Chat {index:04}"),
            ..sample.clone()
        })
        .collect();
    for session in &app.sessions {
        app.prefs
            .chats
            .entry(session.path.clone())
            .or_default()
            .section = Some(id.clone());
    }
    let (_, branches) = sidebar_tree::listing(&app);
    let mut visible: Vec<_> = (0..branches.len()).rev().collect();
    let expected = visible.clone();
    let start = std::time::Instant::now();
    let groups = partition(&app, &branches, &mut visible);
    assert!(visible.is_empty());
    assert_eq!(groups[&id], expected);
    eprintln!(
        "2,000 sorted section members partitioned in {:?}; {} stored index bytes",
        start.elapsed(),
        groups[&id].capacity() * std::mem::size_of::<usize>()
    );
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let output = render(&mut app, vec![]);
    label(&output, "Show 1995 more");
}

#[test]
fn stale_assignment_falls_back_to_grouping_and_archive_hidden_filters_still_apply() {
    let (mut app, id, path) = fixture();
    app.prefs.chats.get_mut(&path).unwrap().section = Some("missing-section".into());
    let (_, branches) = sidebar_tree::listing(&app);
    let mut visible: Vec<_> = (0..branches.len()).collect();
    assert!(partition(&app, &branches, &mut visible).is_empty());
    assert!(
        visible
            .iter()
            .any(|index| branches[*index].path.as_ref() == Some(&path))
    );
    let meta = app.prefs.chats.get_mut(&path).unwrap();
    meta.section = Some(id);
    meta.archived = true;
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let output = render(&mut app, vec![]);
    assert!(!output.shapes.iter().any(
        |shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Review")
    ));
    app.show_archived = true;
    label(&render(&mut app, vec![]), "Review");
    app.prefs.chats.get_mut(&path).unwrap().hidden = true;
    let output = render(&mut app, vec![]);
    assert!(!output.shapes.iter().any(
        |shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Review")
    ));
    app.show_hidden = true;
    label(&render(&mut app, vec![]), "Review");
}

#[test]
fn assignment_choices_remain_bounded_with_long_names_and_many_sections() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        mode.apply(&ctx);
        let sections: Vec<_> = (0..64)
            .map(|index| Section::new(format!("{index} {}", "Long section name ".repeat(4))))
            .collect();
        let mut meta = ChatPrefs::default();
        let mut draw = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(390.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| assignment_menu(ui, &sections, &mut meta));
                },
            )
        };
        for _ in 0..3 {
            draw(vec![]);
        }
        let position = label(&draw(vec![]), "Move to section");
        for pressed in [true, false] {
            draw(vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        let output = draw(vec![]);
        label(&output, "No section · use grouping");
        for layer in ctx.memory(|memory| memory.areas().visible_layer_ids()) {
            if layer.order >= egui::Order::Foreground {
                if let Some(rect) = ctx.memory(|memory| memory.area_rect(layer.id)) {
                    assert!(rect.width() < 300.0, "Assignment menu too wide: {rect:?}");
                    assert!(
                        rect.height() < 420.0,
                        "Assignment menu must scroll: {rect:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn view_all_reopens_sections_and_reset_keeps_user_organization() {
    let (mut app, id, path) = fixture();
    app.prefs.sidebar_view.sections[0].collapsed = true;
    app.closed_projects.insert("/fixture/project".into());
    let ctx = app.ctx.clone();
    let mut trigger = egui::Rect::NOTHING;
    let mut draw = |app: &mut Pui, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 700.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| trigger = app.sidebar_view_options(ui).rect);
            },
        )
    };
    for _ in 0..3 {
        draw(&mut app, vec![]);
    }
    let trigger = trigger.center();
    let draw = |app: &mut Pui, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 700.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.sidebar_view_options(ui);
                });
            },
        )
    };
    let click = |app: &mut Pui, position| {
        for pressed in [true, false] {
            draw(
                app,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    };
    click(&mut app, trigger);
    let output = draw(&mut app, vec![]);
    click(&mut app, label(&output, "View all"));
    assert!(app.prefs.sidebar_view.view_all);
    assert!(app.closed_projects.is_empty());
    assert!(!app.prefs.sidebar_view.sections[0].collapsed);
    click(&mut app, trigger);
    let output = draw(&mut app, vec![]);
    click(&mut app, label(&output, "Reset to default"));
    assert!(!app.prefs.sidebar_view.view_all);
    assert_eq!(app.prefs.sidebar_view.sections[0].id, id);
    assert_eq!(app.prefs.chats[&path].section.as_deref(), Some(id.as_str()));
}

#[test]
fn legacy_names_migrate_to_stable_bounded_sections_and_roundtrip() {
    let mut options: sidebar_view::Options =
        serde_json::from_value(json!({"sections":["Review", "Later"]})).unwrap();
    normalize(&mut options);
    let ids: Vec<_> = options
        .sections
        .iter()
        .map(|section| section.id.clone())
        .collect();
    assert_ne!(ids[0], ids[1]);
    let restored: sidebar_view::Options =
        serde_json::from_value(serde_json::to_value(&options).unwrap()).unwrap();
    assert_eq!(restored.sections[0].id, ids[0]);
    assert_eq!(restored.sections[0].name, "Review");
    options.sections = (0..100)
        .map(|_| Section {
            id: "duplicate".into(),
            name: "\n".repeat(100),
            collapsed: false,
            expanded: false,
        })
        .collect();
    normalize(&mut options);
    assert_eq!(options.sections.len(), LIMIT);
    assert_eq!(
        options
            .sections
            .iter()
            .map(|s| &s.id)
            .collect::<BTreeSet<_>>()
            .len(),
        LIMIT
    );
    assert!(
        options
            .sections
            .iter()
            .all(|s| s.name == "Untitled section")
    );
}

#[test]
fn partition_moves_only_roots_and_preserves_sort_and_pinned_ancestry() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.prefs.sidebar_view.sections =
        vec![Section::new("Review".into()), Section::new("Later".into())];
    let id = app.prefs.sidebar_view.sections[0].id.clone();
    let parent = app.sessions[0].path.clone();
    app.prefs.chats.entry(parent.clone()).or_default().section = Some(id.clone());
    let pinned = app.sessions[1].path.clone();
    let meta = app.prefs.chats.entry(pinned.clone()).or_default();
    meta.pinned = true;
    meta.section = Some(id.clone());
    let (pins, branches) = sidebar_tree::listing(&app);
    assert_eq!(pins.len(), 1);
    let mut visible: Vec<_> = (0..branches.len()).rev().collect();
    let groups = partition(&app, &branches, &mut visible);
    assert_eq!(groups[&id].len(), 1);
    assert_eq!(branches[groups[&id][0]].path.as_ref(), Some(&parent));
    assert!(
        visible
            .iter()
            .all(|index| branches[*index].path.as_ref() != Some(&parent))
    );
    app.remove_sidebar_section(&id);
    assert!(app.prefs.chats[&parent].section.is_none());
    assert!(app.prefs.chats[&pinned].pinned);
    assert_eq!(app.sessions.len(), 7);
}
