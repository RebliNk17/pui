use super::*;

fn saved(path: &str, revision: &str) -> EventKey {
    EventKey {
        owner: Owner::Saved(path.into()),
        revision: revision.into(),
    }
}
fn update(registry: &mut Registry, id: uuid::Uuid, single: bool, unread: Vec<EventKey>) {
    *registry.windows.get_mut(&id).unwrap() = WindowState { single, unread };
}
fn register(registry: &mut Registry) -> uuid::Uuid {
    let id = uuid::Uuid::new_v4();
    assert!(registry.register(id));
    id
}

#[test]
fn dock_is_exact_transition_only_and_zero_or_unknown_clears_once() {
    let mut registry = Registry::default();
    let window = register(&mut registry);
    assert_eq!(registry.transition(), None, "Unknown is initially blank");
    let event = saved("/fixture/chat.jsonl", "observed-revision");
    update(&mut registry, window, true, vec![event.clone()]);
    assert_eq!(registry.transition(), Some(Some(1)));
    for _ in 0..10 {
        assert_eq!(registry.transition(), None);
    }
    update(&mut registry, window, true, vec![]);
    assert_eq!(registry.transition(), Some(None));
    assert_eq!(registry.transition(), None);
    update(&mut registry, window, true, vec![event]);
    assert_eq!(registry.transition(), Some(Some(1)));
    registry.windows.get_mut(&window).unwrap().single = false;
    assert_eq!(registry.transition(), Some(None));
    assert_eq!(registry.transition(), None);
}

#[test]
fn mirrors_deduplicate_revision_zero_and_release_never_erase_another_window() {
    let mut registry = Registry::default();
    let a = register(&mut registry);
    let b = register(&mut registry);
    let event = saved("/fixture/chat.jsonl", "revision-1");
    update(&mut registry, a, true, vec![event.clone()]);
    update(&mut registry, b, true, vec![event]);
    assert_eq!(registry.transition(), Some(Some(1)));
    update(&mut registry, a, true, vec![]);
    assert_eq!(
        registry.transition(),
        None,
        "Window-local read is not a shared read"
    );
    registry.release(a, true);
    assert_eq!(registry.transition(), None);
    let c = register(&mut registry);
    update(
        &mut registry,
        c,
        true,
        vec![saved("/fixture/chat.jsonl", "revision-2")],
    );
    assert_eq!(
        registry.transition(),
        Some(Some(2)),
        "Distinct observed revisions are not summed mirrors"
    );
    registry.release(b, true);
    assert_eq!(registry.transition(), Some(Some(1)));
    registry.release(c, true);
    assert_eq!(registry.transition(), Some(None));
    assert!(registry.windows.is_empty());
}

#[test]
fn runtime_owners_are_window_scoped_and_remote_presence_suppresses_the_union() {
    let mut registry = Registry::default();
    let a = register(&mut registry);
    let b = register(&mut registry);
    let event = EventKey {
        owner: Owner::Runtime("agent-1".into()),
        revision: "same".into(),
    };
    update(&mut registry, a, true, vec![event.clone()]);
    update(&mut registry, b, true, vec![event]);
    assert_eq!(registry.transition(), Some(Some(2)));
    registry.windows.get_mut(&b).unwrap().single = false;
    assert_eq!(registry.transition(), Some(None));
    registry.release(b, true);
    assert_eq!(registry.transition(), Some(Some(1)));
}

#[test]
fn window_and_exact_label_limits_fail_closed_and_recover_after_release() {
    let mut registry = Registry::default();
    let a = register(&mut registry);
    update(
        &mut registry,
        a,
        true,
        (0..MAX_RECORDS)
            .map(|n| saved(&format!("/fixture/{n}.jsonl"), "one"))
            .collect(),
    );
    assert_eq!(registry.transition(), Some(Some(MAX_RECORDS)));
    let b = register(&mut registry);
    update(
        &mut registry,
        b,
        true,
        vec![saved("/fixture/overflow.jsonl", "one")],
    );
    assert_eq!(
        registry.transition(),
        Some(None),
        "Never claim a truncated exact total"
    );
    registry.release(b, true);
    assert_eq!(registry.transition(), Some(Some(MAX_RECORDS)));
    for _ in 1..MAX_WINDOWS {
        let id = register(&mut registry);
        update(&mut registry, id, true, vec![]);
    }
    let overflow = uuid::Uuid::new_v4();
    assert!(!registry.register(overflow));
    assert_eq!(registry.windows.len(), MAX_WINDOWS);
    assert_eq!(registry.transition(), Some(None));
    registry.release(overflow, false);
    assert_eq!(registry.transition(), Some(Some(MAX_RECORDS)));
}

#[test]
fn projection_uses_existing_unread_watermarks_and_drop_unregisters_once() {
    let mut attention = Attention::default();
    attention.observe(
        Owner::Runtime("agent-fixture".into()),
        Path::new("/fixture"),
        Kind::Completed,
    );
    let mut window = Window::default();
    window.update(&attention, true);
    let (id, registered) = window.registration.unwrap();
    assert!(registered);
    REGISTRY.with_borrow(|registry| assert_eq!(registry.windows[&id].unread.len(), 1));
    attention.mark_all_read();
    window.update(&attention, true);
    REGISTRY.with_borrow(|registry| assert!(registry.windows[&id].unread.is_empty()));
    window.release();
    window.release();
    REGISTRY.with_borrow(|registry| assert!(!registry.windows.contains_key(&id)));
    let mut window = Window::default();
    window.update(&attention, true);
    let (id, _) = window.registration.unwrap();
    drop(window);
    REGISTRY.with_borrow(|registry| assert!(!registry.windows.contains_key(&id)));
}

#[test]
fn updating_dock_never_schedules_an_idle_repaint() {
    let ctx = egui::Context::default();
    let mut attention = Attention::default();
    attention.observe(
        Owner::Runtime("fixture".into()),
        Path::new("/fixture"),
        Kind::NeedsInput,
    );
    let mut window = Window::default();
    for _ in 0..4 {
        let _ = ctx.run(egui::RawInput::default(), |_| {
            window.update(&attention, true)
        });
    }
    let out = ctx.run(egui::RawInput::default(), |_| {
        window.update(&attention, true)
    });
    assert_eq!(
        out.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
        Duration::MAX
    );
}
