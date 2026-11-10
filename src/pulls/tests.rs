use super::*;

fn pull(repo: &str, number: u64) -> Pull {
    parse_url(&format!("https://github.com/{repo}/pull/{number}")).unwrap()
}

fn status(updated_at: Option<i64>) -> Status {
    Status {
        state: "Merged".into(),
        checks: "Checks passed".into(),
        updated_at,
        ..Default::default()
    }
}

#[test]
fn only_explicit_prose_prs_are_looked_up() {
    let messages = vec![Message {
        role: "assistant".into(),
        text: "Opened [PR #59](https://github.com/a/b/pull/59).".into(),
        thinking: "https://github.com/private/x/pull/1".into(),
        ..Default::default()
    }];
    assert_eq!(
        referenced(&messages)[0].url,
        "https://github.com/a/b/pull/59"
    );
}

#[test]
fn checks_are_not_guessed_from_chat_text() {
    let status = from_json(
        &serde_json::json!({"state":"OPEN","statusCheckRollup":[{"status":"IN_PROGRESS"}]}),
    );
    assert_eq!(status.checks, "Checks running");
}

#[test]
fn github_update_times_are_parsed_and_missing_or_invalid_times_stay_unknown() {
    let expected = chrono::DateTime::parse_from_rfc3339("2026-01-02T10:00:00Z")
        .unwrap()
        .timestamp_millis();
    for value in ["2026-01-02T10:00:00Z", "2026-01-02T12:00:00+02:00"] {
        assert_eq!(
            from_json(&serde_json::json!({"updatedAt": value})).updated_at,
            Some(expected)
        );
    }
    for raw in [
        serde_json::json!({}),
        serde_json::json!({"updatedAt": null}),
        serde_json::json!({"updatedAt": "invalid"}),
    ] {
        assert_eq!(from_json(&raw).updated_at, None);
    }
}

#[test]
fn newest_updated_prs_come_first_regardless_of_number_state_or_repository() {
    let mut pulls = Pulls {
        links: vec![pull("a/b", 90), pull("c/d", 20), pull("a/b", 10)],
        ..Default::default()
    };
    for (index, time) in [100, 300, 200].into_iter().enumerate() {
        pulls
            .results
            .insert(pulls.links[index].url.clone(), status(Some(time)));
    }
    pulls.results.get_mut(&pulls.links[1].url).unwrap().state = "Open".into();
    let ordered: Vec<_> = pulls
        .ordered_links()
        .iter()
        .map(|p| (p.repo.as_str(), p.number))
        .collect();
    assert_eq!(ordered, [("c/d", 20), ("a/b", 10), ("a/b", 90)]);
}

#[test]
fn unknown_times_sort_last_with_deterministic_ties() {
    let mut pulls = Pulls {
        links: vec![
            pull("a/b", 57),
            pull("a/b", 19),
            pull("a/b", 63),
            pull("a/b", 60),
            pull("a/b", 64),
        ],
        ..Default::default()
    };
    pulls
        .results
        .insert(pulls.links[0].url.clone(), status(Some(200)));
    pulls
        .results
        .insert(pulls.links[1].url.clone(), status(Some(300)));
    pulls
        .results
        .insert(pulls.links[2].url.clone(), status(None));
    pulls
        .results
        .insert(pulls.links[3].url.clone(), status(Some(200)));
    assert_eq!(
        pulls
            .ordered_links()
            .iter()
            .map(|p| p.number)
            .collect::<Vec<_>>(),
        [19, 60, 57, 64, 63]
    );
}

#[test]
fn receiving_a_refreshed_timestamp_reorders_without_a_chat_revision() {
    let ctx = egui::Context::default();
    let mut pulls = Pulls {
        links: vec![pull("a/b", 19), pull("a/b", 60)],
        revision: Some(1),
        ..Default::default()
    };
    for (index, time) in [200, 100].into_iter().enumerate() {
        pulls
            .results
            .insert(pulls.links[index].url.clone(), status(Some(time)));
    }
    assert_eq!(pulls.ordered_links()[0].number, 19);
    let (tx, rx) = mpsc::channel();
    pulls.rx = Some(rx);
    tx.send((pulls.links[1].url.clone(), status(Some(300))))
        .unwrap();
    pulls.update(&ctx, &[], 1, true);
    assert_eq!(pulls.ordered_links()[0].number, 60);
}

#[test]
fn rows_are_24_points_with_only_one_point_between_prs_in_both_themes() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        theme.apply(&ctx);
        let pull = pull("a/b", 57);
        let statuses = [
            Some(status(Some(100))),
            Some(Status {
                state: "Open".into(),
                checks: "Checks running".into(),
                ..Default::default()
            }),
            Some(Status {
                state: "Merged".into(),
                checks: "Checks failed".into(),
                ..Default::default()
            }),
            Some(Status {
                state: "Unavailable".into(),
                error: Some("Check gh auth status".into()),
                ..Default::default()
            }),
            None,
        ];
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let mut previous_bottom = None;
                for status in &statuses {
                    let response = pull_row(ui, &pull, status.as_ref());
                    assert_eq!(response.rect.height(), 24.0);
                    if let Some(bottom) = previous_bottom {
                        assert_eq!(response.rect.top() - bottom, 1.0);
                    }
                    previous_bottom = Some(response.rect.bottom());
                }
            });
        });
    }
}

#[test]
fn eight_prs_fit_in_a_compact_panel_without_changing_parent_spacing() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        theme.apply(&ctx);
        let mut pulls = Pulls {
            links: (1..=8).map(|number| pull("a/b", number)).collect(),
            ..Default::default()
        };
        for pull in &pulls.links {
            pulls
                .results
                .insert(pull.url.clone(), status(Some(pull.number as i64)));
        }
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let spacing = ui.spacing().item_spacing;
                    let top = ui.cursor().top();
                    pulls.show(ui);
                    let height = ui.cursor().top() - top;
                    assert!(height <= 290.0, "Eight PRs should stay compact: {height}");
                    assert_eq!(ui.spacing().item_spacing, spacing);
                });
            },
        );
    }
}

#[test]
fn pr_rows_open_their_own_url() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let pull = pull("a/b", 57);
    let frame = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    pull_row(ui, &pull, Some(&status(Some(100))));
                });
            },
        )
    };
    let _ = frame(vec![]);
    let _ = frame(vec![]);
    // The row starts at the central panel's 8pt margin.
    let pos = egui::pos2(40.0, 20.0);
    let _ = frame(vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
    ]);
    let output = frame(vec![egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    assert!(output.platform_output.commands.iter().any(|command| {
        matches!(command, egui::OutputCommand::OpenUrl(open) if open.url == pull.url)
    }));
}
