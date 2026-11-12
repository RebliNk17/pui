use super::*;
use crate::app::files::tests::app;
#[test]
fn stale_child_assignment_cannot_escape_parent_and_deleted_id_metadata_is_released() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.sessions.clear();
    app.state = State::default();
    app.demo_agent_tree();
    let root = app.agents.active.clone();
    let child = app.agents.nodes[&root].children[0].clone();
    let section = Section::new("Root".into());
    let id = section.id.clone();
    app.prefs.sidebar_view.sections.push(section);
    app.section_view.unsaved.insert(child.clone(), id.clone());
    let (_, branches) = sidebar_tree::listing(&app);
    let mut visible = (0..branches.len()).collect::<Vec<_>>();
    assert!(
        partition(&app, &branches, &mut visible).is_empty(),
        "a surfaced child may never become a section root through stale metadata"
    );
    app.section_view.unsaved.insert(root.clone(), id);
    app.forget_deleted_agent_ids(&BTreeSet::from([child.clone()]));
    assert!(!app.section_view.unsaved.contains_key(&child));
    assert!(app.section_view.unsaved.contains_key(&root));
    app.forget_deleted_agent_ids(&BTreeSet::from([root.clone()]));
    assert!(!app.section_view.unsaved.contains_key(&root));
}
#[test]
fn assignment_survives_node_rename_and_project_change_then_promotes_once_to_real_path() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state = State::default();
    let root = app.ensure_agent();
    let section = Section::new("Working".into());
    let id = section.id.clone();
    app.prefs.sidebar_view.sections.push(section);
    app.section_view.unsaved.insert(root.clone(), id.clone());
    {
        let node = app.agents.nodes.get_mut(&root).unwrap();
        node.name = "Renamed".into();
        node.cwd = "/other/project".into();
    }
    assert_eq!(app.section_view.unsaved[&root], id);
    let path = app.prefs.cwd.join("saved.jsonl");
    app.agents.nodes.get_mut(&root).unwrap().path = Some(path.clone());
    app.promote_unsaved_section(&root);
    app.promote_unsaved_section(&root);
    assert_eq!(app.prefs.chats[&path].section.as_deref(), Some(id.as_str()));
    assert!(app.section_view.unsaved.is_empty());
    app.prefs.chats.get_mut(&path).unwrap().pinned = true;
    let (pins, branches) = sidebar_tree::listing(&app);
    assert!(
        pins.iter()
            .any(|branch| branch.path.as_ref() == Some(&path))
    );
    let mut visible = (0..branches.len()).collect::<Vec<_>>();
    assert!(!partition(&app, &branches, &mut visible).contains_key(&id));
}

#[test]
fn pathless_membership_cap_updates_existing_and_reclaims_removed_slots() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    for n in 0..256 {
        app.set_unsaved_section(&format!("bounded-{n}"), Some("first".into()));
    }
    assert_eq!(app.section_view.unsaved.len(), 256);
    app.set_unsaved_section("overflow", Some("second".into()));
    assert!(!app.section_view.unsaved.contains_key("overflow"));
    app.set_unsaved_section("bounded-0", Some("changed".into()));
    assert_eq!(app.section_view.unsaved["bounded-0"], "changed");
    app.set_unsaved_section("bounded-0", None);
    app.set_unsaved_section("overflow", Some("second".into()));
    assert_eq!(app.section_view.unsaved.len(), 256);
    assert_eq!(app.section_view.unsaved["overflow"], "second");
}
