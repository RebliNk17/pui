use super::tests::fixture;
use super::tests::rendered;
use super::*;

#[test]
fn rendered_frames_never_collect_or_hash_and_changed_inputs_require_explicit_comparison() {
    let (mut app, home) = fixture();
    if let Trigger::Monitor { command, .. } = &mut app.automation.jobs[0].trigger {
        *command = "x".repeat(16000);
    }
    let job = &app.automation.jobs[0];
    let historical = Provenance::capture(job, Some(&home), Origin::Scheduled, 100).unwrap();
    let immutable = serde_json::to_value(&historical).unwrap();
    COLLECTIONS.with(|counts| counts.set((0, 0)));
    let comparison = Comparison::collect(job, Some(&home), false).unwrap();
    assert_eq!(COLLECTIONS.with(|counts| counts.get()), (3, 4));
    COLLECTIONS.with(|counts| counts.set((0, 0)));
    let ctx = egui::Context::default();
    for _ in 0..6 {
        let text = rendered(&ctx, true, |ui| {
            assert!(!historical.show(ui, job, Some(&home), false, Some(&comparison)));
        })
        .join("\n");
        assert!(text.contains("matched the last explicit comparison"));
    }
    assert_eq!(COLLECTIONS.with(|counts| counts.get()), (0, 0));
    let mut history = job.clone();
    history.record_captured_check(&runner::Check::default(), 101, Some(historical.clone()));
    for _ in 0..6 {
        rendered(&ctx, true, |ui| {
            ui.push_id(("latest-check", &history.id), |ui| {
                let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    ui.make_persistent_id("execution-provenance"),
                    true,
                );
                state.set_open(true);
                state.store(ui.ctx());
            });
            assert!(!history.show_check_history(ui, Some(&home), false, Some(&comparison)));
        });
    }
    assert_eq!(COLLECTIONS.with(|counts| counts.get()), (0, 0));
    for changed in [
        "command",
        "timeout",
        "job",
        "workspace",
        "session",
        "creator",
        "store",
        "owner",
    ] {
        let mut edited = job.clone();
        let moved = home.join("moved");
        let mut store = Some(home.as_path());
        let mut owner = false;
        match changed {
            "command" => {
                if let Trigger::Monitor { command, .. } = &mut edited.trigger {
                    command.push(' ');
                }
            }
            "timeout" => {
                if let Trigger::Monitor {
                    timeout_seconds, ..
                } = &mut edited.trigger
                {
                    *timeout_seconds += 1;
                }
            }
            "job" => edited.id = uuid::Uuid::new_v4().to_string(),
            "workspace" => edited.creator.cwd.push("changed"),
            "session" => edited.creator.path.push("changed"),
            "creator" => edited.creator.id = "agent-999".into(),
            "store" => store = Some(moved.as_path()),
            "owner" => owner = true,
            _ => unreachable!(),
        }
        assert!(!comparison.applies(&edited, store, owner), "{changed}");
        for _ in 0..3 {
            let text = rendered(&ctx, true, |ui| {
                historical.show(ui, &edited, store, owner, Some(&comparison));
            })
            .join("\n");
            assert!(
                text.contains("Current comparison unavailable or invalidated"),
                "{changed}"
            );
            assert!(
                !text.contains("matched the last explicit comparison"),
                "{changed}"
            );
            assert!(!text.contains("Matching current command"), "{changed}");
        }
        assert_eq!(COLLECTIONS.with(|counts| counts.get()), (0, 0), "{changed}");
        // Only the explicit comparison action is allowed to collect again.
        let refreshed = Comparison::collect(&edited, store, owner).unwrap();
        assert!(refreshed.applies(&edited, store, owner));
        assert_eq!(COLLECTIONS.with(|counts| counts.get()), (3, 4));
        COLLECTIONS.with(|counts| counts.set((0, 0)));
        rendered(&ctx, true, |ui| {
            historical.show(ui, &edited, store, owner, Some(&refreshed));
        });
        assert_eq!(COLLECTIONS.with(|counts| counts.get()), (0, 0));
    }
    assert_eq!(serde_json::to_value(&historical).unwrap(), immutable);
    assert!(app.automation.running.is_empty());
    assert!(app.automation.mail.is_empty());
}
