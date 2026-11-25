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
fn repositories_stay_grouped_with_open_prs_first_then_newest_updated() {
    let mut pulls = Pulls::default();
    for (repo, number, state, time) in [
        ("a/b", 90, "Merged", 500),
        ("c/b", 20, "Open", 700),
        ("a/b", 10, "Open", 200),
        ("a/b", 30, "Closed", 600),
        ("c/b", 50, "Merged", 800),
        ("a/b", 40, "Open", 100),
        ("a/b", 60, "Draft", 300),
    ] {
        let pull = pull(repo, number);
        pulls.results.insert(
            pull.url.clone(),
            Status {
                state: state.into(),
                ..status(Some(time))
            },
        );
        pulls.links.push(pull);
    }
    let ordered: Vec<_> = pulls
        .ordered_links()
        .iter()
        .map(|p| (p.repo.as_str(), p.number))
        .collect();
    assert_eq!(
        ordered,
        [
            ("a/b", 60),
            ("a/b", 10),
            ("a/b", 40),
            ("a/b", 30),
            ("a/b", 90),
            ("c/b", 20),
            ("c/b", 50),
        ]
    );
}

#[test]
fn open_prs_with_unknown_times_still_precede_finished_or_unknown_states() {
    let mut pulls = Pulls::default();
    for (number, state, time) in [
        (1, "Merged", Some(500)),
        (2, "Open", None),
        (3, "Draft", None),
        (4, "Unavailable", Some(600)),
        (5, "Closed", Some(700)),
        (6, "Open", Some(100)),
    ] {
        let pull = pull("a/b", number);
        pulls.results.insert(
            pull.url.clone(),
            Status {
                state: state.into(),
                ..status(time)
            },
        );
        pulls.links.push(pull);
    }
    pulls.links.push(pull("a/b", 7));
    assert_eq!(
        pulls
            .ordered_links()
            .iter()
            .map(|p| p.number)
            .collect::<Vec<_>>(),
        [6, 3, 2, 5, 4, 1, 7]
    );
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
    let cache = Arc::new(Mutex::new(Cache::default()));
    for (url, status) in &pulls.results {
        cache.lock().unwrap().record(url.clone(), status.clone(), 0);
    }
    // A completed background lookup is picked up without a prose revision.
    cache.lock().unwrap().entries.insert(
        pulls.links[1].url.clone(),
        CachedStatus {
            status: status(Some(300)),
            checked_at: 0,
        },
    );
    pulls.cache = Some(cache);
    pulls.update(&ctx, &[], 1, false);
    assert_eq!(pulls.ordered_links()[0].number, 60);
}

#[test]
fn only_non_merged_statuses_expire_after_thirty_seconds() {
    let url = pull("a/b", 1).url;
    let mut cache = Cache::default();
    assert!(cache.due(&url, 10_000));
    for state in ["Open", "Draft", "Closed", "Unavailable", "Merged"] {
        cache.entries.clear();
        let mut status = status(None);
        status.state = state.into();
        cache.record(url.clone(), status, 10_000);
        assert!(!cache.due(&url, 10_000));
        assert!(!cache.due(&url, 39_999));
        assert_eq!(cache.due(&url, 40_000), state != "Merged");
        assert_eq!(cache.due(&url, i64::MAX), state != "Merged");
        // A clock adjustment must not postpone refresh indefinitely.
        assert_eq!(cache.due(&url, 9_000), state != "Merged");
        cache.in_flight.insert(url.clone());
        assert!(!cache.due(&url, 50_000));
        cache.in_flight.clear();
    }
}

#[test]
fn merged_ci_keeps_polling_until_checks_finish_including_after_restart() {
    let home = std::env::temp_dir().join(format!("pui-pulls-{}", uuid::Uuid::new_v4()));
    let links = vec![pull("a/b", 210)];
    let url = &links[0].url;
    let mut cache = Cache {
        home: Some(home.clone()),
        ..Default::default()
    };
    let mut running = status(Some(100));
    running.checks = "Checks running".into();
    cache.record(url.clone(), running, 10_000);
    let mut restarted = Cache {
        home: Some(home.clone()),
        ..Default::default()
    };
    restarted.load(url);
    assert!(!restarted.due(url, 39_999));
    assert!(restarted.due(url, 40_000));
    assert_eq!(
        restarted.next_refresh(&links, 39_000),
        Some(Duration::from_secs(1))
    );
    assert_eq!(
        restarted.schedule(links.iter().collect(), 40_000),
        [url.clone()]
    );
    restarted.in_flight.remove(url);
    restarted.record(
        url.clone(),
        Status {
            state: "Unavailable".into(),
            error: Some("Offline".into()),
            ..Default::default()
        },
        40_000,
    );
    assert_eq!(restarted.entries[url].status.state, "Merged");
    assert_eq!(restarted.entries[url].status.checks, "Checks running");
    assert!(restarted.due(url, 70_000));
    restarted.record(url.clone(), status(Some(200)), 70_000);
    assert_eq!(restarted.entries[url].status.checks, "Checks passed");
    assert!(restarted.entries[url].status.error.is_none());
    assert!(!restarted.due(url, i64::MAX));
    assert_eq!(restarted.next_refresh(&links, 70_000), None);
    assert_eq!(
        store::load(&home, url).unwrap().status.checks,
        "Checks passed"
    );
    // The pre-completion window cannot overwrite the completed disk snapshot.
    cache.record(
        url.clone(),
        Status {
            checks: "Checks running".into(),
            ..status(Some(100))
        },
        60_000,
    );
    assert_eq!(cache.entries[url].status.checks, "Checks passed");
    restarted.refresh(&links);
    assert!(restarted.due(url, 70_001));
    restarted.record(
        url.clone(),
        Status {
            checks: "Checks running".into(),
            ..status(Some(300))
        },
        80_000,
    );
    assert_eq!(restarted.entries[url].status.checks, "Checks running");
    assert!(restarted.due(url, 110_000));
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn merged_unavailable_checks_keep_refreshing_but_terminal_checks_can_rest() {
    for checks in [
        "",
        "Checks unavailable",
        "Checks running",
        "Checks passed",
        "Checks failed",
        "No checks",
    ] {
        let mut cached = status(None);
        cached.checks = checks.into();
        let expected = matches!(checks, "" | "Checks unavailable" | "Checks running");
        assert_eq!(cached.needs_refresh(), expected, "{checks}");
        cached.error = Some("Offline".into());
        assert!(cached.needs_refresh());
    }
}

#[test]
fn merged_does_not_imply_checks_passed_and_mixed_jobs_keep_refreshing() {
    for state in ["OPEN", "MERGED"] {
        let raw = serde_json::json!({
            "state": state,
            "statusCheckRollup": [
                {"status": "COMPLETED", "conclusion": "FAILURE"},
                {"status": "IN_PROGRESS", "conclusion": null}
            ]
        });
        let running = from_json(&raw);
        assert_eq!(running.checks, "Checks running");
        assert!(running.needs_refresh());
        let mut completed = raw;
        completed["statusCheckRollup"][1] =
            serde_json::json!({"status": "COMPLETED", "conclusion": "SUCCESS"});
        assert_eq!(from_json(&completed).checks, "Checks failed");
    }
}

#[test]
fn polling_is_bounded_deduplicated_and_prioritizes_unseen_then_oldest() {
    let links: Vec<_> = (1..=9).map(|number| pull("a/b", number)).collect();
    let mut cache = Cache::default();
    for (index, pull) in links.iter().take(7).enumerate() {
        let mut open = status(None);
        open.state = "Open".into();
        cache.record(pull.url.clone(), open, 10_000 - index as i64);
    }
    cache.record(links[6].url.clone(), status(None), 20_000);
    let scheduled = cache.schedule(links.iter().collect(), 50_000);
    assert_eq!(
        scheduled,
        [
            links[7].url.clone(),
            links[8].url.clone(),
            links[5].url.clone(),
            links[4].url.clone()
        ]
    );
    assert_eq!(cache.in_flight.len(), MAX_IN_FLIGHT);
    assert!(cache.schedule(links.iter().collect(), 50_000).is_empty());
    cache.in_flight.remove(&links[7].url);
    cache.record(links[7].url.clone(), status(None), 50_000);
    assert_eq!(
        cache.schedule(links.iter().collect(), 50_000),
        [links[3].url.clone()]
    );
    assert!(!cache.in_flight.contains(&links[6].url));
}

#[test]
fn repaint_deadline_tracks_the_next_expiry_instead_of_delaying_each_frame() {
    let links = vec![pull("a/b", 1), pull("a/b", 2)];
    let mut cache = Cache::default();
    cache.record(links[0].url.clone(), status(None), 10_000);
    let mut open = status(None);
    open.state = "Open".into();
    cache.record(links[1].url.clone(), open, 10_000);
    assert_eq!(
        cache.next_refresh(&links, 10_000),
        Some(Duration::from_secs(30))
    );
    assert_eq!(
        cache.next_refresh(&links, 39_999),
        Some(Duration::from_millis(1))
    );
    cache.in_flight.insert(links[1].url.clone());
    assert_eq!(cache.next_refresh(&links, 40_000), None);
}

#[test]
fn manual_refresh_preserves_visible_status_and_rechecks_merged_prs() {
    let links = vec![pull("a/b", 1), pull("a/b", 2)];
    let mut cache = Cache::default();
    cache.record(links[0].url.clone(), status(None), 10_000);
    let mut open = status(None);
    open.state = "Open".into();
    cache.record(links[1].url.clone(), open, 10_000);
    cache.refresh(&links);
    assert!(cache.due(&links[0].url, 10_001));
    assert!(cache.due(&links[1].url, 10_001));
    assert_eq!(cache.entries[&links[0].url].status.state, "Merged");
    assert_eq!(cache.entries[&links[1].url].status.state, "Open");
}

#[test]
fn failed_refresh_keeps_last_known_status_and_merged_is_immutable() {
    let url = pull("a/b", 1).url;
    let mut cache = Cache::default();
    let mut open = status(Some(100));
    open.state = "Open".into();
    cache.record(url.clone(), open, 10_000);
    cache.record(
        url.clone(),
        Status {
            state: "Unavailable".into(),
            error: Some("Offline".into()),
            ..Default::default()
        },
        20_000,
    );
    let cached = &cache.entries[&url];
    assert_eq!(cached.status.state, "Open");
    assert_eq!(cached.status.checks, "Checks passed");
    assert_eq!(cached.status.updated_at, Some(100));
    assert_eq!(cached.status.error.as_deref(), Some("Offline"));
    assert_eq!(cached.checked_at, 20_000);
    cache.record(url.clone(), status(Some(200)), 30_000);
    cache.record(url.clone(), Status::default(), 40_000);
    assert_eq!(cache.entries[&url].status.state, "Merged");
    assert!(cache.entries[&url].status.error.is_none());
    assert_eq!(cache.entries[&url].checked_at, 30_000);
}

#[test]
fn switching_chats_reuses_statuses_and_in_flight_requests() {
    let ctx = egui::Context::default();
    let pull = pull("a/b", 1);
    let messages = vec![Message {
        role: "assistant".into(),
        text: pull.url.clone(),
        ..Default::default()
    }];
    let cache = shared_cache(&ctx);
    {
        let mut cache = cache.lock().unwrap();
        cache.home = None;
        let mut open = status(None);
        open.state = "Open".into();
        cache.record(pull.url.clone(), open, 0);
        cache.in_flight.insert(pull.url.clone());
    }
    let mut first_chat = Pulls::default();
    first_chat.update(&ctx, &messages, 1, false);
    assert_eq!(first_chat.results[&pull.url].state, "Open");
    let mut second_chat = Pulls::default();
    second_chat.update(&ctx, &messages, 1, false);
    assert_eq!(second_chat.results[&pull.url].state, "Open");
    assert_eq!(cache.lock().unwrap().in_flight.len(), 1);
    assert!(Arc::ptr_eq(
        first_chat.cache.as_ref().unwrap(),
        second_chat.cache.as_ref().unwrap()
    ));
    // The worker can finish after the original chat has gone away.
    drop(first_chat);
    cache
        .lock()
        .unwrap()
        .record(pull.url.clone(), status(None), 10_000);
    cache.lock().unwrap().in_flight.remove(&pull.url);
    second_chat.update(&ctx, &messages, 1, false);
    assert_eq!(second_chat.results[&pull.url].state, "Merged");
    assert!(cache.lock().unwrap().in_flight.is_empty());
    assert_eq!(
        inline(&ctx, &pull.url).as_deref(),
        Some(" · Merged · Checks passed")
    );
}

#[test]
fn disk_cache_survives_restart_and_stale_writers_cannot_undo_a_merge() {
    let home = std::env::temp_dir().join(format!("pui-pulls-{}", uuid::Uuid::new_v4()));
    let url = pull("a/b", 1).url;
    let mut cache = Cache {
        home: Some(home.clone()),
        ..Default::default()
    };
    cache.record(url.clone(), status(Some(100)), 10_000);
    let mut restarted = Cache {
        home: Some(home.clone()),
        ..Default::default()
    };
    restarted.load(&url);
    assert_eq!(restarted.entries[&url].status.state, "Merged");
    assert_eq!(restarted.entries[&url].status.updated_at, Some(100));
    assert!(!restarted.due(&url, i64::MAX));
    let mut stale_window = Cache {
        home: Some(home.clone()),
        ..Default::default()
    };
    stale_window.record(url.clone(), Status::default(), 20_000);
    assert_eq!(stale_window.entries[&url].status.state, "Merged");
    assert_eq!(store::load(&home, &url).unwrap().status.state, "Merged");

    let open_url = pull("a/b", 2).url;
    let mut open = status(Some(200));
    open.state = "Open".into();
    cache.record(open_url.clone(), open, 20_000);
    restarted.load(&open_url);
    assert_eq!(restarted.entries[&open_url].status.state, "Open");
    assert!(!restarted.due(&open_url, 49_999));
    assert!(restarted.due(&open_url, 50_000));
    let mut stale = status(Some(50));
    stale.state = "Closed".into();
    stale_window.record(open_url.clone(), stale, 15_000);
    assert_eq!(stale_window.entries[&open_url].status.state, "Open");
    assert!(store::load(&home, &pull("other/repo", 1).url).is_none());
    // Merged wins even when clocks differ between windows.
    stale_window.record(open_url.clone(), status(Some(300)), 1);
    assert_eq!(
        store::load(&home, &open_url).unwrap().status.state,
        "Merged"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&home).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for file in std::fs::read_dir(&home).unwrap() {
            assert_eq!(
                file.unwrap().metadata().unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn demo_does_not_initialize_or_fetch_the_real_cache() {
    let ctx = egui::Context::default();
    let mut pulls = Pulls::default();
    pulls.update(
        &ctx,
        &[Message {
            role: "assistant".into(),
            text: pull("a/b", 1).url,
            ..Default::default()
        }],
        1,
        true,
    );
    assert!(pulls.cache.is_none());
    assert!(pulls.results.is_empty());
}

#[test]
fn merged_state_is_recognized_even_without_a_merge_timestamp() {
    assert_eq!(
        from_json(&serde_json::json!({"state": "MERGED"})).state,
        "Merged"
    );
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
