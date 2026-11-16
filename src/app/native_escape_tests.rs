//! Observable native focus metadata: no custom focus prediction or event changes.
use super::*;
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
#[test]
fn current_owner_filter_keeps_native_forward_and_deferred_reverse_tab() {
    let ctx = egui::Context::default();
    let mut ids = [egui::Id::NULL; 2];
    let render = |events: Vec<egui::Event>, ids: &mut [egui::Id; 2]| {
        ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    for (index, label) in ["first", "second"].into_iter().enumerate() {
                        let response = ui.button(label);
                        ids[index] = response.id;
                        if response.has_focus() {
                            ctx.memory_mut(|memory| {
                                memory.set_focus_lock_filter_current(
                                    response.id,
                                    egui::EventFilter {
                                        escape: true,
                                        ..Default::default()
                                    },
                                );
                            });
                        }
                    }
                });
            },
        )
    };
    for _ in 0..3 {
        render(vec![], &mut ids);
    }
    ctx.memory_mut(|memory| memory.request_focus(ids[0]));
    render(vec![key(egui::Key::Tab, egui::Modifiers::NONE)], &mut ids);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(ids[1]));
    render(vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)], &mut ids);
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            ..Default::default()
        },
        |ctx| assert!(ctx.memory(|memory| memory.focus_lost_by_escape(ids[0]))),
    );
    render(vec![], &mut ids);
    ctx.memory_mut(|memory| {
        memory.request_focus(ids[0]);
        memory.set_focus_lock_filter_current(
            ids[1],
            egui::EventFilter {
                escape: true,
                ..Default::default()
            },
        );
    });
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            ..Default::default()
        },
        |ctx| assert!(ctx.memory(|memory| memory.focus_lost_by_escape(ids[0]))),
    );
    ctx.memory_mut(|memory| {
        memory.request_focus(ids[0]);
        memory.set_focus_lock_filter_current(
            ids[0],
            egui::EventFilter {
                escape: true,
                ..Default::default()
            },
        );
    });
    render(
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        &mut ids,
    );
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(ids[0]));
    assert!(!ctx.memory(|memory| memory.focus_lost_by_escape(ids[0])));
}

#[test]
fn native_escape_metadata_is_isolated_between_viewports() {
    let ctx = egui::Context::default();
    let owner = egui::Id::new("escape-viewport-owner");
    let child = egui::ViewportId::from_hash_of("escape-child");
    let _ = ctx.run(Default::default(), |ctx| {
        ctx.memory_mut(|memory| memory.request_focus(owner))
    });
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            ..Default::default()
        },
        |ctx| assert!(ctx.memory(|memory| memory.focus_lost_by_escape(owner))),
    );
    let _ = ctx.run(
        egui::RawInput {
            viewport_id: child,
            viewports: std::iter::once((child, egui::ViewportInfo::default())).collect(),
            ..Default::default()
        },
        |ctx| assert!(!ctx.memory(|memory| memory.focus_lost_by_escape(owner))),
    );
}

#[test]
fn exact_native_escape_owner_includes_deferred_reverse_target_and_resets_per_pass() {
    let ctx = egui::Context::default();
    let mut ids = [egui::Id::NULL; 2];
    {
        let mut native = |events| {
            ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ids[0] = ui.button("first").id;
                        ids[1] = ui.button("second").id;
                    });
                },
            )
        };
        for _ in 0..3 {
            native(vec![]);
        }
    }
    let render = |events: Vec<egui::Event>| {
        ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let _ = ui.button("first");
                    let _ = ui.button("second");
                });
            },
        )
    };
    ctx.memory_mut(|m| m.request_focus(ids[1]));
    render(vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)]);
    let mut observed = false;
    let _ = ctx.run(
        egui::RawInput {
            events: vec![
                key(egui::Key::Escape, egui::Modifiers::NONE),
                key(egui::Key::Escape, egui::Modifiers::NONE),
            ],
            ..Default::default()
        },
        |ctx| {
            observed =
                ctx.memory(|m| m.focus_lost_by_escape(ids[0]) && !m.focus_lost_by_escape(ids[1]));
        },
    );
    assert!(observed);
    render(vec![]);
    assert!(ctx.memory(|m| !m.focus_lost_by_escape(ids[0])));
    ctx.memory_mut(|m| {
        m.request_focus_with_filter(
            ids[0],
            egui::EventFilter {
                escape: true,
                ..Default::default()
            },
        )
    });
    render(vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    assert!(ctx.memory(|m| !m.focus_lost_by_escape(ids[0])));
}
