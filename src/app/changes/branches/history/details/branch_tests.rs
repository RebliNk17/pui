use super::*;
#[test]
fn history_branch_action_exact_loaded_oid_only_and_disabled_demo_no_handoff() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            for enabled in [false, true] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                theme.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut history = History {
                    branch_enabled: enabled,
                    ..Default::default()
                };
                let hash = "b".repeat(40);
                history.detail.selected = Some(hash.clone());
                history.detail.result = Some(Ok(workspace::history::details::Detail {
                    oid: hash.clone(),
                    time: 1700000000,
                    parents: vec![],
                    message: "selected commit".into(),
                    signature: String::new(),
                }));
                let draw = |history: &mut History, demo: bool| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(420.0, 620.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                history.show_detail(ui, demo);
                            });
                        },
                    )
                };
                for _ in 0..3 {
                    draw(&mut history, false);
                }
                let (id, _, actual) = ctx
                    .data(|d| {
                        d.get_temp::<(egui::Id, egui::Rect, bool)>(
                            popup_id().with("history-branch-draft"),
                        )
                    })
                    .unwrap();
                assert_eq!(actual, enabled);
                history.detail.branch_key = Some(id);
                draw(&mut history, false);
                assert_eq!(history.branch_draft, enabled.then_some(hash.clone()));
                history.branch_draft = None;
                history.detail.branch_key = Some(id);
                draw(&mut history, true);
                assert!(history.branch_draft.is_none());
                history.detail.result = Some(Ok(workspace::history::details::Detail {
                    oid: "a".repeat(40),
                    time: 0,
                    parents: vec![],
                    message: "mismatch".into(),
                    signature: String::new(),
                }));
                history.detail.branch_key = Some(id);
                draw(&mut history, false);
                assert!(history.branch_draft.is_none());
            }
        }
    }
}
