use super::*;
#[test]
fn evidence_is_bounded_unicode_roundtrip_and_legacy_defaults_without_execution() {
    let check = runner::Check {
        success: false,
        output: "世".repeat(2000),
        error: Some("e".repeat(600)),
        ..Default::default()
    };
    let evidence = Evidence::new(&check, 123);
    assert!(evidence.valid());
    assert!(evidence.output.len() <= 4000);
    assert_eq!(evidence.error.as_ref().unwrap().len(), 512);
    let restored: Evidence =
        serde_json::from_slice(&serde_json::to_vec(&evidence).unwrap()).unwrap();
    assert_eq!(restored.output, evidence.output);
    let ctx = egui::Context::default();
    let mut app = super::super::tests::app(&ctx);
    super::super::tests::create(&mut app, "monitor");
    let mut value = serde_json::to_value(&app.automation.jobs[0]).unwrap();
    value.as_object_mut().unwrap().remove("evidence");
    let legacy: Job = serde_json::from_value(value).unwrap();
    assert!(legacy.evidence.is_none());
    assert!(app.automation.running.is_empty());
}
#[test]
fn observed_check_persists_latest_excerpt_without_delivery_or_reexecution() {
    let ctx = egui::Context::default();
    let mut app = super::super::tests::app(&ctx);
    let id = super::super::tests::create(&mut app, "monitor");
    app.loading_chat = true;
    let (tx, rx) = mpsc::channel();
    tx.send(runner::Check {
        success: false,
        output: "exit-one observed".into(),
        error: None,
        ..Default::default()
    })
    .unwrap();
    app.automation.running.insert(
        id,
        Running {
            cancel: Arc::new(AtomicBool::new(false)),
            rx,
        },
    );
    app.poll_automation();
    let job = &app.automation.jobs[0];
    assert_eq!(job.evidence.as_ref().unwrap().output, "exit-one observed");
    assert!(job.pending.is_none());
    assert_eq!(job.deliveries, 0);
    let home = std::env::temp_dir().join(format!("pui-check-evidence-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&home).unwrap();
    store::save(&home, &app.automation.jobs).unwrap();
    let restored = store::load(&home).unwrap();
    assert_eq!(
        restored[0].evidence.as_ref().unwrap().output,
        "exit-one observed"
    );
    assert!(app.automation.running.is_empty());
}
#[test]
fn legacy_result_metadata_remains_unknown_and_new_fields_roundtrip() {
    let legacy: Evidence = serde_json::from_value(
        json!({"observed":123,"triggered":false,"output":"old excerpt","error":null}),
    )
    .unwrap();
    assert_eq!(legacy.exit_code, None);
    assert_eq!(legacy.elapsed_ms, None);
    assert_eq!(legacy.truncated, None);
    let current = Evidence::new(
        &runner::Check {
            success: false,
            output: "waiting".into(),
            error: None,
            exit_code: Some(1),
            elapsed_ms: Some(42),
            truncated: false,
        },
        123,
    );
    let restored: Evidence =
        serde_json::from_slice(&serde_json::to_vec(&current).unwrap()).unwrap();
    assert_eq!(restored.exit_code, Some(1));
    assert_eq!(restored.elapsed_ms, Some(42));
    assert_eq!(restored.truncated, Some(false));
}
#[test]
fn check_history_is_four_total_evicts_oldest_and_persists_without_replay() {
    let ctx = egui::Context::default();
    let mut app = super::super::tests::app(&ctx);
    super::super::tests::create(&mut app, "monitor");
    let job = &mut app.automation.jobs[0];
    for n in 0..9 {
        job.record_check(
            &runner::Check {
                output: format!("check {n}"),
                exit_code: Some(1),
                ..Default::default()
            },
            100 + n,
        );
    }
    assert_eq!(job.evidence.as_ref().unwrap().output, "check 8");
    assert_eq!(job.previous_checks.len(), 3);
    assert_eq!(job.previous_checks[0].output, "check 5");
    assert_eq!(job.previous_checks[2].output, "check 7");
    let restored: Job = serde_json::from_slice(&serde_json::to_vec(job).unwrap()).unwrap();
    assert_eq!(restored.previous_checks[1].output, "check 6");
    restored.validate().unwrap();
    assert_eq!(restored.deliveries, 0);
    assert!(restored.pending.is_none());
    let mut invalid = restored.clone();
    invalid
        .previous_checks
        .push(invalid.evidence.clone().unwrap());
    assert!(invalid.validate().is_err());
    let mut legacy = serde_json::to_value(&restored).unwrap();
    legacy.as_object_mut().unwrap().remove("previous_checks");
    assert!(
        serde_json::from_value::<Job>(legacy)
            .unwrap()
            .previous_checks
            .is_empty()
    );
}
#[test]
fn history_ui_themes_zoom_keeps_job_state_and_dispatch_unchanged() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut app = super::super::tests::app(&ctx);
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            super::super::tests::create(&mut app, "monitor");
            let job = &mut app.automation.jobs[0];
            for n in 0..4 {
                job.record_check(
                    &runner::Check {
                        output: format!("literal previous {n}"),
                        ..Default::default()
                    },
                    123 + n,
                );
            }
            let before = serde_json::to_value(&*job).unwrap();
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
                        egui::CentralPanel::default().show(ctx, |ui| job.show_check_history(ui));
                    },
                );
                assert!(output.platform_output.commands.is_empty());
            }
            assert_eq!(before, serde_json::to_value(job).unwrap());
        }
    }
}
#[test]
fn literal_evidence_theme_zoom_narrow_has_no_clipboard_or_execution() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let evidence = Evidence::new(
                &runner::Check {
                    success: true,
                    output: "untrusted <script>\n\x1b[31m =1+2 世界".into(),
                    error: None,
                    ..Default::default()
                },
                123,
            );
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(420.0, 620.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| evidence.show(ui));
                },
            );
            assert!(output.platform_output.commands.is_empty());
            assert_eq!(evidence.output, "untrusted <script>\n\x1b[31m =1+2 世界");
        }
    }
}
