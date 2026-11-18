use super::*;
#[test]
fn native_cancel_requests_signal_discards_late_success_and_never_retries() {
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
            let (tx, rx) = mpsc::channel();
            let cancel = Arc::new(AtomicBool::new(false));
            changes.push.applying = Some(workspace::read_only::Request::new(rx, cancel.clone()));
            let ws = Workspace {
                is_git: true,
                ..Default::default()
            };
            let draw = |changes: &mut Changes, events| {
                ctx.run(
                    egui::RawInput {
                        events,
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        changes.push_inputs(ctx, true);
                        egui::CentralPanel::default()
                            .show(ctx, |ui| changes.push_review(ui, &ws, false));
                    },
                )
            };
            for _ in 0..3 {
                draw(&mut changes, vec![]);
            }
            let (_, rect, enabled) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new("cancel-push-control"))
                })
                .unwrap();
            assert!(enabled);
            for pressed in [true, false] {
                draw(
                    &mut changes,
                    vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            pressed,
                            button: egui::PointerButton::Primary,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(cancel.load(Ordering::Acquire));
            assert!(changes.push.applying.is_none());
            assert!(changes.push.notice.as_ref().unwrap().contains("uncertain"));
            assert!(tx.send(Ok(workspace::push::Outcome::Updated)).is_err());
            draw(&mut changes, vec![]);
            assert!(changes.push.pending.is_none());
            assert!(changes.push.snapshot.is_none());
            assert!(
                changes
                    .push
                    .notice
                    .as_ref()
                    .unwrap()
                    .contains("cancellation requested")
            );
        }
    }
}
#[test]
fn hidden_push_requests_cancel_and_reports_uncertainty() {
    let ctx = egui::Context::default();
    let mut changes = Changes {
        index_writable: true,
        ..Default::default()
    };
    let (_tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    changes.push.applying = Some(workspace::read_only::Request::new(rx, cancel.clone()));
    changes.push_inputs(&ctx, false);
    assert!(cancel.load(Ordering::Acquire));
    assert!(changes.push.notice.as_ref().unwrap().contains("uncertain"));
}
#[test]
fn explicit_target_fields_render_without_worker_or_saved_configuration() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut changes = Changes::default();
            changes.push.explicit = true;
            changes.push.remote = "origin".into();
            changes.push.destination = "feature/世界".into();
            let ws = Workspace {
                is_git: true,
                ..Default::default()
            };
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(420.0, 620.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| changes.push_review(ui, &ws, false));
                },
            );
            assert!(output.platform_output.commands.is_empty());
            assert!(changes.push.pending.is_none());
            assert!(changes.push.applying.is_none());
            assert!(changes.push.controls.len() >= 4);
            assert_eq!(changes.push.destination, "feature/世界");
        }
    }
}
#[test]
fn remote_inspect_confirmation_escape_never_contacts_remote() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        mode.apply(&ctx);
        let mut changes = Changes {
            index_writable: true,
            ..Default::default()
        };
        changes.push.snapshot = Some(workspace::push::Snapshot {
            root: "/fixture".into(),
            branch: "refs/heads/main".into(),
            head: "a".repeat(40),
            remote: "origin".into(),
            destination: "refs/heads/main".into(),
            endpoint: "https://example.invalid/repo".into(),
            explicit_target: false,
            signing_config: Vec::new(),
        });
        changes.push.remote_confirm = true;
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
                |ctx| changes.push_dialog(ctx),
            );
        }
        assert!(!changes.push.remote_confirm);
        assert!(changes.push.remote_pending.is_none());
    }
}
#[test]
fn observed_divergence_label_is_read_only_themes_zoom() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let s = workspace::push::Snapshot {
                root: "/fixture".into(),
                branch: "refs/heads/main".into(),
                head: "a".repeat(40),
                remote: "origin".into(),
                destination: "refs/heads/main".into(),
                endpoint: "https://example.invalid/repo".into(),
                explicit_target: false,
                signing_config: Vec::new(),
            };
            let mut changes = Changes::default();
            changes.push.snapshot = Some(s.clone());
            changes.push.remote_observed = Some((
                s,
                Ok((
                    Some("b".repeat(40)),
                    workspace::push::Relation::Counts {
                        ahead: 2,
                        behind: 1,
                    },
                )),
            ));
            let ws = Workspace {
                is_git: true,
                ..Default::default()
            };
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(420.0, 620.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| changes.push_review(ui, &ws, false));
                },
            );
            assert!(out.platform_output.commands.is_empty());
            assert!(out.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.job.text.contains("2 ahead · 1 behind · diverged"))));
            assert!(changes.push.pending.is_none());
            assert!(changes.push.remote_pending.is_none());
        }
    }
}
#[test]
fn upstream_confirmation_escape_never_changes_configuration() {
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
            changes.push.snapshot = Some(workspace::push::Snapshot {
                root: "/fixture".into(),
                branch: "refs/heads/main".into(),
                head: "a".repeat(40),
                remote: "origin".into(),
                destination: "refs/heads/main".into(),
                endpoint: "https://example.invalid/repo".into(),
                explicit_target: true,
                signing_config: Vec::new(),
            });
            changes.push.upstream.confirm = true;
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
                    |ctx| changes.push_dialog(ctx),
                );
            }
            assert!(!changes.push.upstream.confirm);
            assert!(changes.push.upstream.pending.is_none());
        }
    }
}
#[test]
fn push_confirmation_escape_never_starts_network_worker() {
    for details in [false, true] {
        for hooks in [false, true] {
            for sign in [false, true] {
                for mode in [theme::Theme::Light, theme::Theme::Dark] {
                    let ctx = egui::Context::default();
                    theme::install(&ctx);
                    mode.apply(&ctx);
                    ctx.set_zoom_factor(if hooks || sign { 1.4 } else { 0.8 });
                    let mut changes = Changes {
                        index_writable: true,
                        ..Default::default()
                    };
                    changes.push.snapshot = Some(workspace::push::Snapshot {
                        root: "/fixture".into(),
                        branch: "refs/heads/main".into(),
                        head: "a".repeat(40),
                        remote: "origin".into(),
                        destination: "refs/heads/master".into(),
                        endpoint: "https://example.invalid/repo".into(),
                        explicit_target: false,
                        signing_config: Vec::new(),
                    });
                    changes.push.confirm = true;
                    changes.push.hooks = hooks;
                    changes.push.diagnostics = details;
                    changes.push.timeout = Some(300);
                    changes.push.sign = sign;
                    changes.push.sign_timeout = Some(120);
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
                            |ctx| changes.push_dialog(ctx),
                        );
                    }
                    assert!(!changes.push.confirm);
                    assert_eq!(changes.push.timeout, Some(300));
                    assert!(changes.push.applying.is_none());
                }
            }
        }
    }
}
#[test]
fn native_review_narrow_themes_displays_exact_target_and_never_starts_worker() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut changes = Changes::default();
            changes.push.snapshot = Some(workspace::push::Snapshot {
                root: "/fixture".into(),
                branch: "refs/heads/main".into(),
                head: "a".repeat(40),
                remote: "origin".into(),
                destination: "refs/heads/master".into(),
                endpoint: "git@example.invalid:team/repo".into(),
                explicit_target: false,
                signing_config: Vec::new(),
            });
            let workspace = Workspace {
                root: "/fixture".into(),
                is_git: true,
                ..Default::default()
            };
            for _ in 0..3 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .show(ctx, |ui| changes.push_review(ui, &workspace, false));
                    },
                );
                assert!(output.platform_output.commands.is_empty());
                assert!(output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.job.text.contains("Destination: refs/heads/master")&&t.galley.job.text.contains("No push performed"))));
            }
            assert!(changes.push.pending.is_none());
        }
    }
}
