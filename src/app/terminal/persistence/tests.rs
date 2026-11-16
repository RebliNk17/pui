use super::*;

fn decoded(value: Value) -> SavedTabs {
    serde_json::from_value(value).unwrap()
}
fn snapshot(root: &Path, labels: &[u64], selected: u64) -> SavedTabs {
    decoded(json!({"projects":[{"root":root,"labels":labels,"selected":selected}]}))
}
fn inert(terminal: &Terminals) -> bool {
    terminal
        .projects
        .values()
        .flat_map(|project| &project.tabs)
        .all(|tab| {
            tab.restored
                && tab.session.is_none()
                && tab.starting.is_none()
                && tab.demo.is_none()
                && tab.error.is_none()
                && tab.selection.is_none()
                && tab.scroll == 0.0
        })
}

#[test]
fn numbered_tabs_hidden_roots_and_empty_final_close_round_trip_without_execution_state() {
    let ctx = egui::Context::default();
    let mut source = Terminals::default();
    let root = Path::new("/fixture/primary");
    source.toggle(&ctx, root, true);
    source.start(&ctx, true);
    source.start(&ctx, true);
    source.projects.get_mut(root).unwrap().selected = 1;
    let group = source.projects.get_mut(root).unwrap();
    group.tabs[1].selection = Some(((1, 0), (1, 3)));
    group.tabs[1].scroll = 2.0;
    group.tabs[1].error = Some("NOT_SAVED_ERROR".into());
    group.tabs[1]
        .demo
        .as_mut()
        .unwrap()
        .process(b"NOT_SAVED_OUTPUT command/approval");
    let hidden = Path::new("/fixture/hidden");
    source.workspace(&ctx, hidden, true);
    source.start(&ctx, true);
    let empty = Path::new("/fixture/empty");
    source.workspace(&ctx, empty, true);
    let id = source.projects[empty].tabs[0].id;
    source.close_tab(&ctx, empty, id);
    source.close(&ctx);
    let saved = serde_json::to_value(source.saved_tabs()).unwrap();
    assert_eq!(saved.as_object().unwrap().len(), 1);
    for project in saved["projects"].as_array().unwrap() {
        assert_eq!(project.as_object().unwrap().len(), 3);
    }
    let text = saved.to_string();
    for forbidden in [
        "NOT_SAVED",
        "command",
        "approval",
        "session",
        "starting",
        "output",
        "scroll",
        "selection",
        "cwd",
        "\"id\"",
    ] {
        assert!(!text.contains(forbidden), "{forbidden}: {text}");
    }
    let mut target = Terminals::default();
    target.restore_tabs(&decoded(saved));
    assert!(inert(&target));
    assert_eq!(target.projects[root].tabs.len(), 3);
    assert_eq!(target.projects[root].selected, 1);
    assert_eq!(target.projects[hidden].tabs.len(), 2);
    assert!(target.projects[empty].tabs.is_empty());
    let ids: BTreeSet<_> = source
        .projects
        .values()
        .flat_map(|group| group.tabs.iter().map(|tab| tab.id))
        .collect();
    assert!(
        target
            .projects
            .values()
            .flat_map(|group| &group.tabs)
            .all(|tab| !ids.contains(&tab.id))
    );
    for root in [root, hidden, empty] {
        target.restore_saved_layout(&ctx, root, (false, false));
        target.toggle(&ctx, root, true);
        target.workspace(&ctx, root, true);
        target.context_inputs(&ctx, true);
        target.close(&ctx);
    }
    assert!(inert(&target));
    assert_eq!(target.total_tabs(), 5);
    assert!(!target.running());
}

#[test]
fn invalid_serde_ids_roots_and_aggregate_counts_are_sanitized_without_io() {
    let long = format!("/{}", "x".repeat(4096));
    let mut projects = vec![
        json!({"root":"relative","labels":[1]}),
        json!({"root":"/fixture/../escape","labels":[1]}),
        json!({"root":"/fixture/\ncontrol","labels":[1]}),
        json!({"root":long,"labels":[1]}),
        json!({"root":"/valid","labels":[0,1,1,-2,"bad",999999999,null,8,9],"selected":999}),
        json!({"root":"/valid","labels":[7]}),
    ];
    projects.extend((0..100).map(
        |n| json!({"root":format!("/never-read-{n}"),"labels":[1,2,3,4,5,6,7,8,9],"selected":8}),
    ));
    let saved = decoded(json!({"projects": projects}));
    assert!(saved.projects.len() <= MAX_ROOTS);
    assert!(saved.projects.iter().all(|group| valid_root(&group.root)));
    assert_eq!(
        saved
            .projects
            .iter()
            .filter(|group| group.root == Path::new("/valid"))
            .count(),
        1
    );
    assert!(
        saved
            .projects
            .iter()
            .map(|group| group.labels.len())
            .sum::<usize>()
            <= tabs::MAX_TOTAL_TABS
    );
    let repaired = &saved.projects[0];
    assert_eq!(repaired.labels.len(), tabs::MAX_TABS);
    assert_eq!(
        repaired
            .labels
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len(),
        tabs::MAX_TABS
    );
    assert!(
        repaired
            .labels
            .iter()
            .all(|label| (1..=MAX_LABEL).contains(label))
    );
    assert_eq!(repaired.selected, repaired.labels.first().copied());
    for value in [
        json!(null),
        json!([]),
        json!({"projects":"bad"}),
        json!({"projects":[{"root":22}]}),
    ] {
        assert!(decoded(value).projects.is_empty());
    }
    let mut target = Terminals::default();
    target.restore_tabs(&saved);
    assert!(inert(&target));
    assert_eq!(target.total_tabs(), tabs::MAX_TOTAL_TABS);
}

#[test]
fn streaming_decoder_discards_excess_labels_projects_and_unknown_payloads() {
    let many_labels = std::iter::repeat_n("1", 1000).collect::<Vec<_>>().join(",");
    let projects = (0..100)
        .map(|index| {
            format!("{{\"root\":\"/large-{index}\",\"labels\":[{many_labels}],\"selected\":1}}")
        })
        .collect::<Vec<_>>()
        .join(",");
    let text = format!("{{\"projects\":[{projects}],\"output\":[{many_labels}]}}");
    let saved: SavedTabs = serde_json::from_str(&text).unwrap();
    assert_eq!(saved.projects.len(), MAX_ROOTS);
    assert_eq!(
        saved
            .projects
            .iter()
            .map(|project| project.labels.len())
            .sum::<usize>(),
        tabs::MAX_TOTAL_TABS
    );
    assert!(
        saved
            .projects
            .iter()
            .all(|project| project.labels.len() <= tabs::MAX_TABS)
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_root_is_dropped_instead_of_silently_retargeted() {
    use std::os::unix::ffi::OsStringExt;
    let root = PathBuf::from(std::ffi::OsString::from_vec(
        b"/fixture/invalid-\xff".to_vec(),
    ));
    assert!(!valid_root(&root));
    let mut terminal = Terminals::default();
    terminal.projects.insert(root, Project::default());
    assert!(terminal.saved_tabs().projects.is_empty());
}

#[test]
fn global_cap_includes_placeholders_and_bounds_empty_groups_across_checkpoints() {
    let ctx = egui::Context::default();
    let mut target = Terminals::default();
    let running = Path::new("/running");
    target.toggle(&ctx, running, true);
    for _ in 1..tabs::MAX_TABS {
        target.start(&ctx, true);
    }
    for n in 0..100 {
        let root = PathBuf::from(format!("/checkpoint-{n}"));
        target.restore_tabs(&snapshot(&root, &[1, 2, 3, 4, 5, 6, 7, 8], 8));
        assert!(target.total_tabs() <= tabs::MAX_TOTAL_TABS);
        assert!(target.projects.len() <= MAX_ROOTS);
    }
    assert_eq!(target.total_tabs(), tabs::MAX_TOTAL_TABS);
    target.root = "/checkpoint-0".into();
    let count = target.total_tabs();
    target.activate_restored(&ctx, true);
    assert_eq!(
        target.total_tabs(),
        count,
        "activation reuses a placeholder, even at the cap"
    );
    target.start(&ctx, true);
    assert_eq!(target.total_tabs(), count);
    target.workspace(&ctx, Path::new("/unopened-at-cap"), true);
    assert!(!target.projects.contains_key(Path::new("/unopened-at-cap")));
    let saved = target.saved_tabs();
    assert!(saved.projects.len() <= MAX_ROOTS);
    // Free both tab/group budget. A dropped checkpoint root is still inert,
    // rather than acquiring the clean-window fresh-project auto-start behavior.
    target.projects.remove(Path::new("/checkpoint-0"));
    assert!(target.total_tabs() < tabs::MAX_TOTAL_TABS);
    target.workspace(&ctx, Path::new("/checkpoint-99"), true);
    assert!(!target.projects.contains_key(Path::new("/checkpoint-99")));
    target.workspace(&ctx, Path::new("/never-seen-after-restore"), true);
    assert!(
        !target
            .projects
            .contains_key(Path::new("/never-seen-after-restore"))
    );
}

#[test]
fn legacy_layout_only_placeholders_reserve_global_capacity_without_spawning() {
    let ctx = egui::Context::default();
    let mut target = Terminals::default();
    for index in 0..100 {
        target.restore_saved_layout(
            &ctx,
            &PathBuf::from(format!("/legacy-{index}")),
            (true, false),
        );
    }
    assert_eq!(target.total_tabs(), tabs::MAX_TOTAL_TABS);
    assert_eq!(target.restored_roots.len(), tabs::MAX_TOTAL_TABS);
    target.root = "/legacy-0".into();
    target.start(&ctx, true);
    assert_eq!(target.total_tabs(), tabs::MAX_TOTAL_TABS);
    assert_eq!(target.restored_roots.len(), tabs::MAX_TOTAL_TABS - 1);
    assert!(
        target.projects[Path::new("/legacy-0")].tabs[0]
            .demo
            .is_some()
    );
    target.restore_tabs(&snapshot(Path::new("/too-many"), &[1, 2], 2));
    assert_eq!(target.total_tabs(), tabs::MAX_TOTAL_TABS);
}

#[test]
fn rehydration_never_overwrites_running_or_deliberately_closed_in_memory_tabs() {
    let ctx = egui::Context::default();
    let mut terminal = Terminals::default();
    let root = Path::new("/live");
    terminal.toggle(&ctx, root, true);
    terminal.start(&ctx, true);
    let saved = terminal.saved_tabs();
    let id = terminal.projects[root].tabs[1].id;
    terminal.projects.get_mut(root).unwrap().tabs[1]
        .demo
        .as_mut()
        .unwrap()
        .process(b"LIVE_MARKER");
    terminal.restore_tabs(&snapshot(root, &[99], 99));
    terminal.restore_saved_layout(&ctx, root, (true, false));
    assert_eq!(terminal.projects[root].tabs[1].id, id);
    assert_eq!(terminal.projects[root].selected, 1);
    assert!(
        terminal.projects[root].tabs[1]
            .demo
            .as_ref()
            .unwrap()
            .screen()
            .contents()
            .contains("LIVE_MARKER")
    );
    let ids = terminal.projects[root]
        .tabs
        .iter()
        .map(|tab| tab.id)
        .collect::<Vec<_>>();
    for id in ids {
        terminal.close_tab(&ctx, root, id);
    }
    terminal.restore_tabs(&saved);
    terminal.workspace(&ctx, root, true);
    terminal.close(&ctx);
    terminal.toggle(&ctx, root, true);
    assert!(terminal.projects[root].tabs.is_empty());
    let mut reopened = Terminals::default();
    reopened.restore_tabs(&terminal.saved_tabs());
    reopened.restore_saved_layout(&ctx, root, (true, false));
    reopened.workspace(&ctx, root, true);
    assert!(reopened.projects[root].tabs.is_empty());
}
