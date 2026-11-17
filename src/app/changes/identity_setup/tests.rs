use super::*;
#[test]
fn loaded_values_are_draft_only_and_late_results_cannot_cross_owner_or_disabled_state() {
    let ctx = egui::Context::default();
    for guard in 0..4 {
        let mut changes = Changes {
            index_writable: true,
            ..Default::default()
        };
        changes.identity_setup.name = "Existing draft".into();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        changes.identity_setup.loading = Some(workspace::read_only::Request::new(rx, cancel));
        changes.identity_setup.owner = Some(if guard == 1 {
            (PathBuf::from("/other-owner"), "other-session".into())
        } else {
            changes.identity.clone()
        });
        tx.send(Ok(("Loaded".into(), "loaded@example.invalid".into())))
            .unwrap();
        if guard == 2 {
            changes.index_writable = false;
        }
        changes.identity_setup_inputs(&ctx, guard != 3);
        assert!(changes.identity_setup.loading.is_none());
        assert!(changes.identity_setup.review.is_none());
        assert!(changes.identity_setup.applying.is_none());
        if guard == 0 {
            assert_eq!(changes.identity_setup.name, "Loaded");
            assert_eq!(changes.identity_setup.email, "loaded@example.invalid");
            assert!(changes.identity_setup.edit_local);
            assert!(!changes.identity_setup.override_inherited);
        } else {
            assert!(changes.identity_setup.name.is_empty());
        }
    }
}

#[test]
fn local_identity_confirmation_escape_themes_zoom_never_writes() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            for edit in [false, true] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let root = workspace::commit::identity_setup::tests::fixture();
                let mut changes = Changes {
                    index_writable: true,
                    ..Default::default()
                };
                let inherited = root.join(".git/inherited");
                std::fs::write(
                    &inherited,
                    "[user]\n name = Inherited\n email = inherited@example.invalid\n",
                )
                .unwrap();
                workspace::tests::git(
                    &root,
                    &["config", "include.path", inherited.to_str().unwrap()],
                );
                if edit {
                    workspace::tests::git(&root, &["config", "--unset", "include.path"]);
                    workspace::commit::identity_setup::apply(
                        workspace::commit::identity_setup::inspect(
                            &root,
                            "Old",
                            "old@example.invalid",
                            &AtomicBool::new(false),
                        )
                        .unwrap(),
                        &AtomicBool::new(false),
                    )
                    .unwrap();
                }
                changes.identity_setup.review = Some(
                    workspace::commit::identity_setup::inspect_mode(
                        &root,
                        "Fixture",
                        "fixture@example.invalid",
                        !edit,
                        edit,
                        &AtomicBool::new(false),
                    )
                    .unwrap(),
                );
                let before = std::fs::read(root.join(".git/config")).unwrap();
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
                        |ctx| changes.identity_setup_dialog(ctx),
                    );
                }
                assert!(changes.identity_setup.review.is_none());
                assert!(changes.identity_setup.applying.is_none());
                assert_eq!(std::fs::read(root.join(".git/config")).unwrap(), before);
            }
        }
    }
}
