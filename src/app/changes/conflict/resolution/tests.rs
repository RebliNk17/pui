use super::*;
fn key(key: egui::Key, pressed: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
fn labels(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(t) => out.push((
            t.galley.job.text.clone(),
            egui::Rect::from_min_size(t.pos, t.galley.size()),
        )),
        egui::Shape::Vec(v) => {
            for s in v {
                labels(s, out)
            }
        }
        _ => {}
    }
}
fn frame(ctx: &egui::Context, s: &mut Resolution, events: Vec<egui::Event>) -> egui::FullOutput {
    frame_size(ctx, s, events, Vec2::new(860.0, 620.0))
}
fn frame_size(
    ctx: &egui::Context,
    s: &mut Resolution,
    events: Vec<egui::Event>,
    size: Vec2,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            let mut notice = None;
            s.inputs(ctx, &mut notice);
            s.dialog(ctx, true, true, &mut notice);
        },
    )
}
#[test]
fn editor_and_stage_prompts_bounded_themes_zoom_escape_and_key_release_never_write() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            for edit in [true, false] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                theme.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut s = Resolution::default();
                let review = backend::Review::fixture_for_ui("resolved\n");
                if edit {
                    s.editor = Some((review, "draft edited\n".into()));
                } else {
                    s.inspected = Some(review);
                }
                for _ in 0..4 {
                    frame(&ctx, &mut s, vec![]);
                }
                let output = frame(&ctx, &mut s, vec![]);
                let mut text = Vec::new();
                for shape in output.shapes {
                    labels(&shape.shape, &mut text);
                }
                for label in [
                    "Cancel",
                    if edit {
                        "Save working file"
                    } else {
                        "Mark resolved"
                    },
                ] {
                    let rect = text.iter().find(|(s, _)| s == label).unwrap().1;
                    assert!(ctx.screen_rect().contains_rect(rect), "{label}: {rect:?}");
                }
                frame(&ctx, &mut s, vec![key(egui::Key::Enter, true)]);
                assert!(s.applying.is_none());
                frame(
                    &ctx,
                    &mut s,
                    vec![key(egui::Key::Enter, false), key(egui::Key::Escape, true)],
                );
                assert!(s.editor.is_none() && s.inspected.is_none() && s.applying.is_none());
                let _ = ctx.run(
                    egui::RawInput {
                        events: vec![
                            key(egui::Key::Escape, false),
                            egui::Event::Text("leak".into()),
                        ],
                        ..Default::default()
                    },
                    |ctx| {
                        s.inputs(ctx, &mut None);
                        assert!(ctx.input(|i| i.events.is_empty()));
                    },
                );
            }
        }
    }
}
#[test]
fn narrow_stress_prompt_actions_remain_visible() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            for edit in [true, false] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                theme.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut s = Resolution::default();
                let review = backend::Review::fixture_for_ui("resolved\n");
                if edit {
                    s.editor = Some((review, "edited\n".into()));
                } else {
                    s.inspected = Some(review);
                }
                for _ in 0..4 {
                    frame_size(&ctx, &mut s, vec![], Vec2::new(420.0, 620.0));
                }
                let out = frame_size(&ctx, &mut s, vec![], Vec2::new(420.0, 620.0));
                let mut text = Vec::new();
                for shape in out.shapes {
                    labels(&shape.shape, &mut text);
                }
                for label in [
                    "Cancel",
                    if edit {
                        "Save working file"
                    } else {
                        "Mark resolved"
                    },
                ] {
                    let rect = text.iter().find(|(t, _)| t == label).unwrap().1;
                    assert!(
                        ctx.screen_rect().contains_rect(rect),
                        "{label} zoom{zoom}: {rect:?}"
                    );
                }
            }
        }
    }
}
#[test]
fn editor_typing_is_isolated_and_escape_works_with_text_focus() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut s = Resolution {
        editor: Some((
            backend::Review::fixture_for_ui("original\n"),
            "draft\n".into(),
        )),
        ..Default::default()
    };
    for _ in 0..4 {
        frame(&ctx, &mut s, vec![]);
    }
    let id = ctx
        .data(|d| d.get_temp::<egui::Id>(egui::Id::new("conflict-editor-text")))
        .unwrap();
    ctx.memory_mut(|m| m.request_focus(id));
    frame(&ctx, &mut s, vec![]);
    frame(&ctx, &mut s, vec![egui::Event::Text("typed".into())]);
    assert!(s.editor.as_ref().unwrap().1.contains("typed"));
    assert!(ctx.input(|i| i.events.is_empty()));
    frame(&ctx, &mut s, vec![key(egui::Key::Escape, true)]);
    assert!(s.editor.is_none());
    assert!(s.applying.is_none());
}
#[test]
fn hidden_owner_permission_foreign_modal_cancel_requests_and_discard_drafts() {
    for case in 0..3 {
        let ctx = egui::Context::default();
        let mut changes = Changes {
            index_writable: true,
            ..Default::default()
        };
        changes.conflict.owner = Some(changes.identity.clone());
        changes.conflict.resolution.editor =
            Some((backend::Review::fixture_for_ui("file\n"), "draft\n".into()));
        let (_tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        changes.conflict.resolution.pending =
            Some(workspace::read_only::Request::new(rx, cancel.clone()));
        if case == 1 {
            changes.index_writable = false;
        }
        if case == 2 {
            changes.identity.1 = "different".into();
        }
        changes.conflict_inputs(&ctx, case != 0);
        assert!(cancel.load(Ordering::Acquire));
        assert!(changes.conflict.resolution.editor.is_none());
        assert!(changes.conflict.resolution.pending.is_none());
    }
    let ctx = egui::Context::default();
    let mut s = Resolution {
        inspected: Some(backend::Review::fixture_for_ui("resolved\n")),
        ..Default::default()
    };
    let _ = ctx.run(Default::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("foreign"), 200.0, |ui| {
            prompt_ui::heading(ui, "Foreign prompt", "");
        });
        s.dialog(ctx, true, true, &mut None);
    });
    assert!(s.inspected.is_none());
    assert!(s.applying.is_none());
}
#[test]
fn actual_row_and_dialog_save_then_fresh_stage_updates_only_exact_index_entry() {
    let original = backend::Review::real_fixture_for_ui();
    let root = original.root.clone();
    let index = std::fs::read(root.join(".git/index")).unwrap();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut s = Resolution::default();
    let mut notice = None;
    for stage in [false, true] {
        let row = |ctx: &egui::Context,
                   s: &mut Resolution,
                   events: Vec<egui::Event>,
                   notice: &mut Option<String>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(860.0, 620.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    s.inputs(ctx, notice);
                    s.begin_rows();
                    egui::CentralPanel::default().show(ctx, |ui| {
                        s.row(ui, &root, "file.txt", true, notice);
                    });
                },
            )
        };
        for _ in 0..3 {
            row(&ctx, &mut s, vec![], &mut notice);
        }
        let (id, _, enabled) = ctx
            .data(|d| {
                d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new((
                    "conflict-resolution-row",
                    stage,
                    "file.txt",
                )))
            })
            .unwrap();
        assert!(enabled);
        ctx.memory_mut(|m| m.request_focus(id));
        row(&ctx, &mut s, vec![], &mut notice);
        row(&ctx, &mut s, vec![key(egui::Key::Enter, true)], &mut notice);
        assert!(s.pending.is_some());
        let result = s
            .pending
            .as_ref()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap();
        s.pending = None;
        if stage {
            s.inspected = Some(result);
        } else {
            s.editor = Some((result, "native reviewed resolution\n".into()));
        }
        frame(&ctx, &mut s, vec![key(egui::Key::Enter, false)]);
        for _ in 0..4 {
            frame(&ctx, &mut s, vec![]);
        }
        let output = frame(&ctx, &mut s, vec![]);
        let mut text = Vec::new();
        for shape in output.shapes {
            labels(&shape.shape, &mut text);
        }
        let label = if stage {
            "Mark resolved"
        } else {
            "Save working file"
        };
        let pos = text.iter().find(|(t, _)| t == label).unwrap().1.center();
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut s,
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
        let result = s
            .applying
            .as_ref()
            .expect("explicit confirm starts action")
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        result.unwrap();
        s.applying = None;
        assert_eq!(
            std::fs::read(root.join("file.txt")).unwrap(),
            b"native reviewed resolution\n"
        );
        if !stage {
            assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
        }
    }
    assert_eq!(
        crate::workspace::tests::git(&root, &["show", ":file.txt"]),
        b"native reviewed resolution\n"
    );
    assert!(
        !crate::workspace::tests::git(&root, &["ls-files", "-u", "--", "second.txt"]).is_empty()
    );
    assert!(root.join(".git/MERGE_HEAD").exists());
}
#[test]
fn marker_stage_disabled_and_owner_mismatch_cannot_start_worker() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut s = Resolution {
        inspected: Some(backend::Review::fixture_for_ui(
            "<<<<<<< ours\n=======\n>>>>>>> theirs\n",
        )),
        ..Default::default()
    };
    for _ in 0..4 {
        frame(&ctx, &mut s, vec![]);
    }
    let output = frame(&ctx, &mut s, vec![]);
    let mut text = Vec::new();
    for shape in output.shapes {
        labels(&shape.shape, &mut text);
    }
    let pos = text
        .iter()
        .find(|(t, _)| t == "Mark resolved")
        .unwrap()
        .1
        .center();
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut s,
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
    assert!(s.inspected.is_some());
    assert!(s.applying.is_none());
    let _ = ctx.run(Default::default(), |ctx| {
        s.dialog(ctx, true, false, &mut None)
    });
    assert!(s.inspected.is_none());
    assert!(s.applying.is_none());
}
