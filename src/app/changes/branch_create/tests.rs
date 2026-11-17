use super::*;
#[test]
fn branch_list_handoff_only_fills_switch_draft() {
    let mut changes = Changes {
        index_writable: true,
        ..Default::default()
    };
    assert!(changes.prepare_branch_switch("feature/世界".into()));
    assert_eq!(changes.branch_create.name, "feature/世界");
    assert!(changes.branch_create.switch);
    assert!(changes.branch_create.start.is_empty());
    assert!(changes.branch_create.pending.is_none());
    assert!(changes.branch_create.review.is_none());
    assert!(changes.branch_create.applying.is_none());
    changes.index_writable = false;
    assert!(!changes.prepare_branch_switch("other".into()));
    assert_eq!(changes.branch_create.name, "feature/世界");
}

#[test]
fn history_handoff_is_draft_only_bound_to_owner_and_refuses_disabled_or_busy() {
    let mut changes = Changes {
        index_writable: true,
        ..Default::default()
    };
    let hash = "b".repeat(40);
    assert!(changes.prepare_history_branch(hash.clone()));
    assert_eq!(changes.branch_create.start, hash);
    assert!(!changes.branch_create.switch);
    assert!(changes.branch_create.name.is_empty());
    assert!(changes.branch_create.review.is_none());
    assert!(changes.branch_create.pending.is_none());
    assert!(changes.branch_create.applying.is_none());
    assert!(changes.branch_create.reveal);
    assert_eq!(changes.branch_create.owner, Some(changes.identity.clone()));
    changes.index_writable = false;
    assert!(!changes.prepare_history_branch("a".repeat(40)));
    assert_eq!(changes.branch_create.start, hash);
    changes.index_writable = true;
    assert!(!changes.prepare_history_branch("HEAD~1".into()));
    changes.identity.1 = "different session".into();
    changes.create_branch_inputs(&egui::Context::default(), true);
    assert!(changes.branch_create.start.is_empty());
}

#[test]
fn native_branch_confirmation_escape_themes_zoom_no_ref_write() {
    for switching in [false, true] {
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            for zoom in [0.8, 1.4] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut changes = Changes {
                    index_writable: true,
                    ..Default::default()
                };
                changes.branch_create.review = Some(workspace::branches::create::Review {
                    root: "/fixture".into(),
                    source: "refs/heads/main".into(),
                    head: "a".repeat(40),
                    name: "feature/世界".into(),
                    switch: switching,
                    target: if switching {
                        "a".repeat(40)
                    } else {
                        "b".repeat(40)
                    },
                    files: vec![workspace::history::files::Entry {
                        status: 'M',
                        path: "src/世界.rs".into(),
                    }],
                });
                for n in 0..4 {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(420.0, 620.0),
                            )),
                            events: if n == 3 {
                                vec![egui::Event::Key {
                                    key: egui::Key::Escape,
                                    physical_key: None,
                                    pressed: true,
                                    repeat: false,
                                    modifiers: egui::Modifiers::NONE,
                                }]
                            } else {
                                vec![]
                            },
                            ..Default::default()
                        },
                        |ctx| changes.create_branch_dialog(ctx),
                    );
                }
                assert!(changes.branch_create.review.is_none());
                assert!(changes.branch_create.applying.is_none());
            }
        }
    }
}
