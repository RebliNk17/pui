use super::*;

pub(super) fn fixture() -> (Pui, PathBuf) {
    let mut app = crate::app::automation::tests::app(&egui::Context::default());
    crate::app::automation::tests::create(&mut app, "monitor");
    let home = std::env::temp_dir().join(format!("pui-provenance-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&home).unwrap();
    (app, home)
}
pub(super) fn rendered(
    ctx: &egui::Context,
    enabled: bool,
    mut draw: impl FnMut(&mut egui::Ui),
) -> Vec<String> {
    fn labels(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.job.text.clone()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| labels(s, out)),
            _ => {}
        }
    }
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(320.0, 1600.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let width = ui.available_width();
                ui.add_enabled_ui(enabled, &mut draw);
                assert!(ui.min_rect().width() <= width + 1.0);
            });
        },
    );
    assert!(output.platform_output.commands.is_empty());
    let mut text = Vec::new();
    for shape in output.shapes {
        labels(&shape.shape, &mut text);
    }
    text
}

#[test]
fn launch_snapshot_survives_edit_reschedule_store_restart_and_creator_id_remap() {
    let (mut app, home) = fixture();
    let job = &mut app.automation.jobs[0];
    let captured = Provenance::capture(job, Some(&home), Origin::Scheduled, 100).unwrap();
    let snapshot = serde_json::to_value(&captured).unwrap();
    let check = runner::Check {
        exit_code: Some(1),
        ..Default::default()
    };
    job.record_captured_check(&check, 101, Some(captured.clone()));
    job.record_captured_check(
        &runner::Check {
            exit_code: Some(1),
            ..Default::default()
        },
        102,
        Some(captured),
    );
    job.name = "renamed after check".into();
    job.message = "edited follow-up".into();
    job.creator.id = "agent-999".into();
    if let Trigger::Monitor {
        timeout_seconds,
        interval_seconds,
        command,
    } = &mut job.trigger
    {
        *interval_seconds = 80;
        *timeout_seconds = 50;
        *command = "different command".into();
    }
    store::save(&home, &app.automation.jobs).unwrap();
    let restored = store::load(&home).unwrap();
    let evidence = restored[0].evidence.as_ref().unwrap();
    assert_eq!(
        serde_json::to_value(evidence.provenance.as_ref().unwrap()).unwrap(),
        snapshot
    );
    assert_eq!(evidence.origin_label(), "Scheduled execution");
    assert_eq!(
        serde_json::to_value(restored[0].previous_checks[0].provenance.as_ref().unwrap()).unwrap(),
        snapshot
    );
    let comparison = Comparison::collect(&restored[0], Some(&home), false);
    let text = rendered(&egui::Context::default(), true, |ui| {
        evidence.provenance.as_ref().unwrap().show(
            ui,
            &restored[0],
            Some(&home),
            false,
            comparison.as_ref(),
        );
    })
    .join("\n");
    assert!(text.contains("Current execution identity differs"));
    assert!(text.contains("Historical command text not retained"));
    assert!(!text.contains("different command"));
    assert!(app.automation.running.is_empty());
    assert!(restored[0].pending.is_none());
    assert_eq!(restored[0].deliveries, 0);
}

#[test]
fn exact_command_bytes_job_and_store_must_match_before_current_text_is_shown() {
    let (mut app, home) = fixture();
    let job = &mut app.automation.jobs[0];
    let captured = Provenance::capture(job, Some(&home), Origin::Scheduled, 100).unwrap();
    let command = "exit 0";
    let ctx = egui::Context::default();
    let comparison = Comparison::collect(job, Some(&home), false);
    let text = rendered(&ctx, true, |ui| {
        captured.show(ui, job, Some(&home), false, comparison.as_ref());
    });
    assert!(text.iter().any(|s| s == command));
    let moved = home.join("moved-store");
    let comparison = Comparison::collect(job, Some(&moved), false);
    let text = rendered(&ctx, true, |ui| {
        captured.show(ui, job, Some(&moved), false, comparison.as_ref());
    });
    assert!(!text.iter().any(|s| s == command));
    assert!(text.iter().any(|s| s.contains("Store identity differs")));
    job.id = uuid::Uuid::new_v4().to_string();
    let comparison = Comparison::collect(job, Some(&home), false);
    let text = rendered(&ctx, true, |ui| {
        captured.show(ui, job, Some(&home), false, comparison.as_ref());
    });
    assert!(!text.iter().any(|s| s == command));
    assert_ne!(digest(b"exit 0"), digest(b"exit 0 "));
}

#[test]
fn provenance_bounds_unknown_canonicalization_and_store_overflow_fail_closed() {
    let (mut app, home) = fixture();
    let job = &mut app.automation.jobs[0];
    job.creator.cwd = home.join("missing").join("x".repeat(400));
    let captured = Provenance::capture(job, Some(&home), Origin::TestOnce, 100).unwrap();
    assert!(!captured.workspace.canonical);
    assert!(captured.workspace.shortened);
    assert_eq!(captured.workspace.excerpt.len(), 128);
    assert_eq!(
        captured.workspace.sha256,
        digest(job.creator.cwd.as_os_str().as_encoded_bytes())
    );
    assert!(captured.valid(101));
    // A wall-clock adjustment can put completion before launch time; both
    // recorded timestamps remain claims, not a monotonic duration.
    assert!(captured.valid(99));
    // Even pathological control-byte excerpts stay under the explicit 4 KiB bound.
    let mut worst = captured.clone();
    for identity in [
        &mut worst.workspace,
        &mut worst.session,
        worst.store.as_mut().unwrap(),
    ] {
        identity.excerpt = "\0".repeat(128);
    }
    assert!(serde_json::to_vec(&worst).unwrap().len() <= 4096);
    let mut invalid = captured.clone();
    invalid.command_sha256 = "bad digest".into();
    assert!(!invalid.valid(101));
    invalid = captured.clone();
    invalid.workspace.excerpt = "x".repeat(129);
    assert!(!invalid.valid(101));
    for n in 0..4 {
        job.record_captured_check(
            &runner::Check {
                output: "x".repeat(4000),
                ..Default::default()
            },
            101 + n,
            Some(worst.clone()),
        );
    }
    store::save(&home, &app.automation.jobs).unwrap();
    let before = std::fs::read(home.join("jobs.json")).unwrap();
    app.automation.jobs[0].message = "m".repeat(6000);
    if let Trigger::Monitor { command, .. } = &mut app.automation.jobs[0].trigger {
        *command = "x".repeat(16000);
    }
    let mut oversized = Vec::new();
    for _ in 0..128 {
        let mut copy = app.automation.jobs[0].clone();
        copy.id = uuid::Uuid::new_v4().to_string();
        oversized.push(copy);
    }
    assert!(
        store::save(&home, &oversized)
            .unwrap_err()
            .contains("4 MiB")
    );
    assert_eq!(std::fs::read(home.join("jobs.json")).unwrap(), before);
    assert_eq!(store::load(&home).unwrap().len(), 1);
}

#[test]
fn legacy_origin_is_unknown_and_malformed_imported_provenance_is_rejected() {
    let (mut app, home) = fixture();
    let mut legacy: Evidence = serde_json::from_value(
        json!({"observed":123,"triggered":true,"output":"old","error":null,"diagnostic":false}),
    )
    .unwrap();
    assert!(legacy.origin_label().contains("unknown execution origin"));
    legacy.mark_test();
    assert!(legacy.origin_label().contains("Legacy diagnostic flag"));
    let job = &mut app.automation.jobs[0];
    job.record_captured_check(
        &runner::Check::default(),
        101,
        Provenance::capture(job, Some(&home), Origin::Scheduled, 100),
    );
    let mut imported = serde_json::to_value(&app.automation.jobs).unwrap();
    imported[0]["evidence"]["provenance"]["workspace"]["sha256"] = json!("untrusted bad hash");
    std::fs::write(
        home.join("jobs.json"),
        serde_json::to_vec(&imported).unwrap(),
    )
    .unwrap();
    assert!(store::load(&home).is_err());
    assert!(app.automation.running.is_empty());
}

#[test]
fn observer_inspection_is_literal_bounded_in_light_dark_narrow_zoom_and_disabled_states() {
    let (mut app, home) = fixture();
    let captured =
        Provenance::capture(&app.automation.jobs[0], Some(&home), Origin::TestOnce, 100).unwrap();
    app.automation.jobs[0].record_captured_check(
        &runner::Check {
            success: true,
            exit_code: Some(0),
            output: "untrusted \x1b[31m <script>".into(),
            ..Default::default()
        },
        101,
        Some(captured),
    );
    store::save(&home, &app.automation.jobs).unwrap();
    let lock = store::lock(&home).unwrap();
    let bytes = std::fs::read(home.join("jobs.json")).unwrap();
    let mut observer = Automation {
        home: Some(home.clone()),
        ..Default::default()
    };
    observer.refresh();
    assert!(observer.lock.is_none());
    assert!(observer.running.is_empty());
    let job = &observer.jobs[0];
    let before = serde_json::to_value(job).unwrap();
    let comparison = Comparison::collect(job, Some(&home), false);
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            for enabled in [true, false] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let text = rendered(&ctx, enabled, |ui| {
                    job.evidence
                        .as_ref()
                        .unwrap()
                        .provenance
                        .as_ref()
                        .unwrap()
                        .show(ui, job, Some(&home), false, comparison.as_ref());
                })
                .join("\n");
                assert!(text.contains("Test once · no follow-up delivery"));
                assert!(text.contains("not authenticated proof"));
                let text = rendered(&ctx, enabled, |ui| {
                    job.show_check_history(ui, Some(&home), false, None);
                })
                .join("\n");
                assert!(text.contains("Task verification is not recorded"));
                assert!(text.contains("does not prove prompt delivery"));
                assert!(!text.contains("Triggered · exit 0"));
            }
        }
    }
    assert_eq!(before, serde_json::to_value(job).unwrap());
    assert_eq!(std::fs::read(home.join("jobs.json")).unwrap(), bytes);
    assert!(observer.precheck.is_none());
    assert!(observer.running.is_empty());
    assert!(observer.mail.is_empty());
    drop(lock);
}
