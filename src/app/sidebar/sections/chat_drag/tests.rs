use super::*;
use crate::app::files::tests::{app, frame};
#[test]
fn actual_saved_chat_pointer_drag_to_section_header_assigns_only_metadata() {
    for cancel in 0..3 {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.prefs.sidebar_view.empty_groups = true;
        app.prefs
            .sidebar_view
            .sections
            .push(Section::new("Drop here".into()));
        let section = app.prefs.sidebar_view.sections[0].id.clone();
        let path = app.sessions[1].path.clone();
        app.prefs.chats.entry(path.clone()).or_default().pinned = false;
        app.draft = "unchanged draft".into();
        app.state.busy = true;
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        let source = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("chat-section-source", &path))))
            .unwrap()
            .center();
        let target = ctx
            .data(|data| {
                data.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new((
                    "section-header-rects",
                    &section,
                )))
            })
            .unwrap()
            .0
            .center();
        let active = app.agents.active.clone();
        let pointer = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&ctx, &mut app, vec![egui::Event::PointerMoved(source)]);
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(source),
                pointer(source, true),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
                egui::Event::Text("blocked on press".into()),
            ],
        );
        assert_eq!(app.draft, "unchanged draft");
        assert!(app.section_view.chat_drag.is_some());
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(source + egui::vec2(12.0, 0.0))],
        );
        frame(&ctx, &mut app, vec![egui::Event::PointerMoved(target)]);
        if cancel == 1 {
            app.sessions.retain(|session| session.path != path);
        }
        if cancel == 2 {
            app.state.dialogs.push_back(questions::demo_dialog());
        }
        frame(
            &ctx,
            &mut app,
            vec![
                pointer(target, false),
                egui::Event::Text("must not type".into()),
            ],
        );
        assert_eq!(
            app.prefs.chats[&path].section.as_deref(),
            if cancel == 0 {
                Some(section.as_str())
            } else {
                None
            }
        );
        assert_eq!(app.agents.active, active);
        assert_eq!(app.draft, "unchanged draft");
        assert!(app.state.busy);
        if cancel == 2 {
            assert_eq!(app.state.dialogs.len(), 1);
        }
    }
}
#[test]
fn stale_source_render_pass_cancels_drag_and_cached_identity() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    for _ in 0..2 {
        frame(&ctx, &mut app, vec![]);
    }
    let path = app.sessions[1].path.clone();
    let widget = egui::Id::new("stale-source");
    app.section_view.chat_drag = Some(Drag {
        source: Source::Saved(path.clone()),
        root: app.prefs.cwd.clone(),
        owner: app.agents.active.clone(),
        widget,
        sections: app
            .prefs
            .sidebar_view
            .sections
            .iter()
            .map(|section| section.id.clone())
            .collect(),
        membership: app
            .prefs
            .chats
            .get(&path)
            .and_then(|meta| meta.section.clone()),
        active: false,
    });
    app.section_view.chat_drag_seen = Some((widget, ctx.cumulative_pass_nr().saturating_sub(10)));
    frame(&ctx, &mut app, vec![]);
    assert!(app.section_view.chat_drag.is_none());
    assert!(app.section_view.chat_drag_seen.is_none());
}
#[test]
fn explicit_release_assigns_saved_chat_without_opening_or_stopping() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let section = Section::new("Later".into());
    let target = section.id.clone();
    app.prefs.sidebar_view.sections.push(section);
    let path = app.sessions[0].path.clone();
    app.draft = "keep draft".into();
    app.state.busy = true;
    let active = app.agents.active.clone();
    app.section_view.chat_drag = Some(Drag {
        source: Source::Saved(path.clone()),
        root: app.prefs.cwd.clone(),
        owner: active.clone(),
        widget: egui::Id::new("fixture-chat"),
        sections: app
            .prefs
            .sidebar_view
            .sections
            .iter()
            .map(|section| section.id.clone())
            .collect(),
        membership: app
            .prefs
            .chats
            .get(&path)
            .and_then(|meta| meta.section.clone()),
        active: true,
    });
    app.section_view.chat_drop = Some(target.clone());
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::PointerButton {
                pos: egui::pos2(20.0, 20.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| {
            app.section_view.chat_drag_seen =
                Some((egui::Id::new("fixture-chat"), ctx.cumulative_pass_nr()));
            app.finish_chat_section_drag(ctx)
        },
    );
    assert_eq!(
        app.prefs.chats[&path].section.as_deref(),
        Some(target.as_str())
    );
    assert_eq!(app.agents.active, active);
    assert_eq!(app.draft, "keep draft");
    assert!(app.state.busy);
}
#[test]
fn release_without_same_pass_source_render_never_commits() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let path = app.sessions[0].path.clone();
    let section = Section::new("Target".into());
    let target = section.id.clone();
    app.prefs.sidebar_view.sections.push(section);
    app.section_view.chat_drag = Some(Drag {
        source: Source::Saved(path.clone()),
        root: app.prefs.cwd.clone(),
        owner: app.agents.active.clone(),
        widget: egui::Id::new("missing-source"),
        sections: app
            .prefs
            .sidebar_view
            .sections
            .iter()
            .map(|section| section.id.clone())
            .collect(),
        membership: app
            .prefs
            .chats
            .get(&path)
            .and_then(|meta| meta.section.clone()),
        active: true,
    });
    let before = app
        .prefs
        .chats
        .get(&path)
        .and_then(|meta| meta.section.clone());
    app.section_view.chat_drop = Some(target);
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::PointerButton {
                pos: egui::pos2(10.0, 10.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| app.finish_chat_section_drag(ctx),
    );
    assert_eq!(
        app.prefs
            .chats
            .get(&path)
            .and_then(|meta| meta.section.clone()),
        before
    );
    assert!(app.section_view.chat_drag.is_none());
}
#[test]
fn section_or_membership_changes_invalidate_press_bound_transaction() {
    for membership in [false, true] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let path = app.sessions[0].path.clone();
        let drag = Drag {
            source: Source::Saved(path.clone()),
            root: app.prefs.cwd.clone(),
            owner: app.agents.active.clone(),
            widget: egui::Id::new("stale-drag"),
            sections: app
                .prefs
                .sidebar_view
                .sections
                .iter()
                .map(|section| section.id.clone())
                .collect(),
            membership: app
                .prefs
                .chats
                .get(&path)
                .and_then(|meta| meta.section.clone()),
            active: true,
        };
        if membership {
            app.prefs.chats.entry(path).or_default().section = Some("changed".into());
        } else {
            app.prefs
                .sidebar_view
                .sections
                .push(Section::new("New section".into()));
        }
        assert!(!app.chat_move_current(&drag));
    }
}
#[test]
fn escape_cancels_drag_and_keeps_draft_and_run() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let path = app.sessions[0].path.clone();
    app.draft = "kept".into();
    app.state.busy = true;
    app.section_view.chat_drag = Some(Drag {
        source: Source::Saved(path.clone()),
        root: app.prefs.cwd.clone(),
        owner: app.agents.active.clone(),
        widget: egui::Id::new("escape-drag"),
        sections: app
            .prefs
            .sidebar_view
            .sections
            .iter()
            .map(|section| section.id.clone())
            .collect(),
        membership: app
            .prefs
            .chats
            .get(&path)
            .and_then(|meta| meta.section.clone()),
        active: true,
    });
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(app.section_view.chat_drag.is_none());
    assert_eq!(app.draft, "kept");
    assert!(app.state.busy);
}
#[test]
fn gesture_snapshots_do_not_retain_excess_path_or_string_capacity() {
    let mut path = PathBuf::with_capacity(1024 * 1024);
    path.push("/workspace/chat.jsonl");
    let copy = bounded_path(&path);
    assert_eq!(copy, path);
    assert!(copy.capacity() <= 4096);
    let mut text = String::with_capacity(1024 * 1024);
    text.push_str("section-id");
    let copy = bounded_text(&text);
    assert_eq!(copy, text);
    assert_eq!(copy.capacity(), copy.len());
    eprintln!(
        "drag fixed {}B; path capacity {}; identity capacity {}",
        std::mem::size_of::<Drag>(),
        bounded_path(&path).capacity(),
        copy.capacity()
    );
}
#[test]
fn runtime_identity_assigns_without_fake_path_and_promotion_cancels_old_drag() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.session_file = None;
    let id = app.ensure_agent();
    app.agents.nodes.get_mut(&id).unwrap().path = None;
    let target = Section::new("Runtime target".into());
    let section = target.id.clone();
    app.prefs.sidebar_view.sections.push(target);
    let source = Source::Runtime(id.clone());
    assert!(source.eligible(&app));
    app.set_unsaved_section(&id, Some(section.clone()));
    assert_eq!(source.membership(&app), Some(section));
    assert!(
        !app.prefs
            .chats
            .keys()
            .any(|path| path == &PathBuf::from(&id))
    );
    app.agents.nodes.get_mut(&id).unwrap().path = Some(app.prefs.cwd.join("promoted.jsonl"));
    assert!(!source.eligible(&app));
}
#[test]
fn runtime_limit_and_children_fail_closed_and_retained_identity_is_bounded() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.session_file = None;
    let id = app.ensure_agent();
    app.agents.nodes.get_mut(&id).unwrap().path = None;
    let source = Source::Runtime(id.clone());
    for index in 0..256 {
        app.section_view
            .unsaved
            .insert(format!("fixture-{index}"), "section".into());
    }
    assert!(!source.eligible(&app));
    app.section_view
        .unsaved
        .insert(id.clone(), "section".into());
    assert!(source.eligible(&app));
    app.agents.nodes.get_mut(&id).unwrap().parent = Some("parent".into());
    assert!(!source.eligible(&app));
    assert!(std::mem::size_of::<Hit>() <= 160);
    assert!(std::mem::size_of::<Drag>() <= 192);
}
#[test]
fn saved_agent_child_is_not_a_movable_root() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let id = app.ensure_agent();
    let path = app.sessions[0].path.clone();
    let node = app.agents.nodes.get_mut(&id).unwrap();
    node.path = Some(path.clone());
    node.parent = Some("parent-owner".into());
    let drag = Drag {
        source: Source::Saved(path.clone()),
        root: app.prefs.cwd.clone(),
        owner: id,
        widget: egui::Id::new("child-source"),
        sections: app
            .prefs
            .sidebar_view
            .sections
            .iter()
            .map(|section| section.id.clone())
            .collect(),
        membership: app
            .prefs
            .chats
            .get(&path)
            .and_then(|meta| meta.section.clone()),
        active: true,
    };
    assert!(!app.chat_move_current(&drag));
}
#[test]
fn owner_change_or_pinned_chat_refuses_assignment() {
    for pinned in [false, true] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let path = app.sessions[0].path.clone();
        let drag = Drag {
            source: Source::Saved(path.clone()),
            root: app.prefs.cwd.clone(),
            owner: app.agents.active.clone(),
            widget: egui::Id::new("fixture"),
            sections: app
                .prefs
                .sidebar_view
                .sections
                .iter()
                .map(|section| section.id.clone())
                .collect(),
            membership: app
                .prefs
                .chats
                .get(&path)
                .and_then(|meta| meta.section.clone()),
            active: true,
        };
        if pinned {
            app.prefs.chats.entry(path).or_default().pinned = true;
        } else {
            app.agents.active = "replacement".into();
        }
        assert!(!app.chat_move_current(&drag));
        frame(&ctx, &mut app, vec![]);
    }
}
