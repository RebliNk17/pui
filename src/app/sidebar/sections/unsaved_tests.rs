use super::*;
use crate::app::files::tests::{app, click, frame, text};
fn setup() -> (egui::Context, Pui, String, String) {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.sessions.clear();
    app.state = State::default();
    let id = app.ensure_agent();
    let section = Section::new("Focused".into());
    let key = section.id.clone();
    app.prefs.sidebar_view.sections.push(section);
    app.prefs.hide_inspector = true;
    (ctx, app, id, key)
}
fn member(app: &Pui, id: &str, section: &str) -> bool {
    app.section_view
        .unsaved
        .get(id)
        .is_some_and(|saved| saved == section)
}

#[test]
fn actual_pathless_root_menu_moves_without_changing_chat_draft_or_run() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let (ctx, mut app, id, section) = setup();
        app.prefs.theme = mode;
        app.draft = "keep pathless draft".into();
        app.state.busy = true;
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        let key = format!("agent:{id}");
        let menu = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("sidebar-unsaved-menu", &key))))
            .unwrap();
        click(&ctx, &mut app, menu.center());
        let out = frame(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, text(&out, "Move to section"));
        let out = frame(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, text(&out, "Focused"));
        assert!(member(&app, &id, &section));
        assert_eq!(app.agents.active, id);
        assert_eq!(app.draft, "keep pathless draft");
        assert!(app.state.busy);
        let (_, branches) = sidebar_tree::listing(&app);
        let mut visible = (0..branches.len()).collect::<Vec<_>>();
        let groups = partition(&app, &branches, &mut visible);
        assert_eq!(groups[&section].len(), 1);
        assert!(visible.is_empty());
        let path = app.prefs.cwd.join("saved.jsonl");
        app.agents.nodes.get_mut(&id).unwrap().path = Some(path.clone());
        app.promote_unsaved_section(&id);
        assert!(!app.section_view.unsaved.contains_key(&id));
        assert_eq!(
            app.prefs.chats[&path].section.as_deref(),
            Some(section.as_str())
        );
    }
}

#[test]
fn pathless_membership_is_exact_id_child_ownership_and_stale_sections_stay_safe() {
    let (_, mut app, id, section) = setup();
    app.demo_agent_tree();
    let root = app.agents.active.clone();
    let child = app.agents.nodes[&root].children[0].clone();
    app.section_view
        .unsaved
        .insert(root.clone(), section.clone());
    app.section_view
        .unsaved
        .insert(child.clone(), section.clone());
    let (_, branches) = sidebar_tree::listing(&app);
    let mut visible = (0..branches.len()).collect::<Vec<_>>();
    let groups = partition(&app, &branches, &mut visible);
    let grouped = &branches[groups[&section][0]];
    assert_eq!(grouped.key, format!("agent:{root}"));
    assert!(!grouped.children.is_empty());
    app.promote_unsaved_section(&child);
    assert!(!app.section_view.unsaved.contains_key(&child));
    app.remove_sidebar_section(&section);
    assert!(!app.section_view.unsaved.contains_key(&root));
    let (_, branches) = sidebar_tree::listing(&app);
    let mut visible = (0..branches.len()).collect::<Vec<_>>();
    assert!(partition(&app, &branches, &mut visible).is_empty());
    assert_eq!(id, root);
}

#[test]
fn saved_assignment_wins_and_unsaved_metadata_does_not_serialize_fake_session_paths() {
    let (_, mut app, id, section) = setup();
    app.section_view.unsaved.insert(id.clone(), section.clone());
    let before = serde_json::to_string(&app.prefs).unwrap();
    assert!(!before.contains(&format!("agent:{id}")));
    assert!(
        !app.prefs
            .chats
            .keys()
            .any(|path| path.to_string_lossy().contains(&id))
    );
    let path = app.prefs.cwd.join("known.jsonl");
    app.agents.nodes.get_mut(&id).unwrap().path = Some(path.clone());
    app.prefs.chats.entry(path.clone()).or_default().section = Some("explicit-existing".into());
    app.promote_unsaved_section(&id);
    assert_eq!(
        app.prefs.chats[&path].section.as_deref(),
        Some("explicit-existing")
    );
    assert!(app.section_view.unsaved.is_empty());
    let path = app.prefs.cwd.join("next.jsonl");
    app.agents.nodes.get_mut(&id).unwrap().path = Some(path.clone());
    app.section_view
        .unsaved
        .insert(id.clone(), "missing".into());
    app.promote_unsaved_section(&id);
    assert!(!app.prefs.chats.contains_key(&path));
}

#[test]
fn pathless_menu_escape_and_approval_shortcuts_cannot_send_or_stop() {
    let (ctx, mut app, id, _) = setup();
    app.draft = "keep".into();
    app.state.busy = true;
    app.state.dialogs.push_back(json!({"id":"pathless-approval","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo safe\"}\n\nApprove this call only."}));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let key = format!("agent:{id}");
    let menu = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("sidebar-unsaved-menu", &key))))
        .unwrap();
    click(&ctx, &mut app, menu.center());
    frame(&ctx, &mut app, vec![]);
    for (code, mods) in [
        (egui::Key::Enter, egui::Modifiers::COMMAND),
        (egui::Key::Backspace, egui::Modifiers::COMMAND),
        (egui::Key::Escape, egui::Modifiers::NONE),
    ] {
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Key {
                key: code,
                modifiers: mods,
                physical_key: None,
                pressed: true,
                repeat: false,
            }],
        );
    }
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy && commands.try_recv().is_err());
    assert_eq!(app.draft, "keep");
}
