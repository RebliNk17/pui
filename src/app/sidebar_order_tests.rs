use super::*;
use crate::app::sidebar_tree::Target;
use std::time::{Duration, SystemTime};

fn row(title: &str, created: Option<u64>, modified: u64, pinned: bool) -> Branch {
    Branch {
        key: title.into(),
        target: Target::Session(0),
        path: None,
        title: title.into(),
        status: String::new(),
        cwd: "/fixture".into(),
        project: "/fixture".into(),
        created: created.map(|s| SystemTime::UNIX_EPOCH + Duration::from_secs(s)),
        modified: SystemTime::UNIX_EPOCH + Duration::from_secs(modified),
        activity: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(modified)),
        children: vec![],
        pinned,
        archived: false,
        hidden: false,
    }
}
fn titles(rows: &[Branch], options: Options) -> Vec<String> {
    let mut rows = rows.to_vec();
    rows.sort_by(|a, b| options.compare(a, b));
    rows.into_iter().map(|r| r.title).collect()
}

#[test]
fn sorting_is_independent_of_grouping_and_supports_both_directions() {
    let rows = [
        row("zebra", Some(30), 10, false),
        row("Alpha", Some(10), 30, false),
        row("beta", Some(20), 20, false),
    ];
    for (sort, expected) in [
        (Sort::Activity, vec!["Alpha", "beta", "zebra"]),
        (Sort::Created, vec!["zebra", "beta", "Alpha"]),
        (Sort::Name, vec!["Alpha", "beta", "zebra"]),
    ] {
        let options = Options {
            sort,
            ..Default::default()
        };
        assert_eq!(titles(&rows, options), expected);
        let mut reversed = expected;
        reversed.reverse();
        assert_eq!(
            titles(
                &rows,
                Options {
                    reverse: true,
                    ..options
                }
            ),
            reversed
        );
    }
}

#[test]
fn sorting_inside_sections_does_not_prioritize_pins() {
    let rows = [
        row("Alpha", Some(30), 30, false),
        row("zebra", Some(10), 10, true),
    ];
    for sort in [Sort::Activity, Sort::Created, Sort::Name] {
        for reverse in [false, true] {
            let options = Options { sort, reverse };
            assert_eq!(
                titles(&rows, options)[0],
                if reverse { "zebra" } else { "Alpha" }
            );
        }
    }
    assert_eq!(
        titles(
            &rows,
            Options {
                sort: Sort::Name,
                ..Default::default()
            }
        ),
        ["Alpha", "zebra"]
    );
}

#[test]
fn unknown_dates_stay_last_and_equal_values_preserve_discovery_order() {
    let rows = [
        row("Unknown", None, 10, false),
        row("Second", Some(20), 10, false),
        row("First", Some(20), 10, false),
    ];
    for reverse in [false, true] {
        assert_eq!(
            titles(
                &rows,
                Options {
                    sort: Sort::Created,
                    reverse,
                }
            ),
            ["Second", "First", "Unknown"]
        );
    }
    assert_eq!(
        titles(&rows, Options::default()),
        ["Unknown", "Second", "First"]
    );
}

#[test]
fn project_headings_sort_by_project_name_or_newest_timestamp_not_first_pinned_chat() {
    let rows = [
        row("old pin", Some(10), 10, true),
        row("new", Some(50), 50, false),
        row("other", Some(30), 30, false),
    ];
    let a = (Path::new("/z/Alpha"), &[0, 1][..]);
    let b = (Path::new("/a/Zebra"), &[2][..]);
    for sort in [Sort::Activity, Sort::Created, Sort::Name] {
        let options = Options {
            sort,
            ..Default::default()
        };
        assert_eq!(options.compare_groups(a, b, &rows), Ordering::Less);
        assert_eq!(
            Options {
                reverse: true,
                ..options
            }
            .compare_groups(a, b, &rows),
            Ordering::Greater
        );
    }
    let options = Options {
        reverse: true,
        ..Default::default()
    };
    assert_eq!(options.compare_groups(a, b, &rows), Ordering::Greater);
}

fn running_titles(rows: &mut [Branch], order: &mut RunningOrder, options: Options) -> Vec<String> {
    order.update(&[], rows, |row| {
        crate::theme::status::is_working(&row.status)
    });
    rows.sort_by(|a, b| options.compare_running(a, b, order));
    rows.iter().map(|row| row.title.clone()).collect()
}

#[test]
fn running_agents_hold_positions_until_completion_and_new_runs_join_last() {
    let mut rows = [
        row("idle", None, 100, false),
        row("first", None, 30, false),
        row("second", None, 20, false),
        row("third", None, 10, false),
    ];
    for row in &mut rows[1..] {
        row.status = "Working…".into();
    }
    let mut order = RunningOrder::default();
    let options = Options::default();
    assert_eq!(
        running_titles(&mut rows, &mut order, options),
        ["first", "second", "third", "idle"]
    );
    // Simulate new filesystem discovery order and alternating streamed updates.
    rows.reverse();
    for title in ["third", "second", "first", "third"] {
        rows.iter_mut().find(|r| r.title == title).unwrap().modified += Duration::from_secs(200);
        assert_eq!(
            running_titles(&mut rows, &mut order, options),
            ["first", "second", "third", "idle"]
        );
    }
    rows[1].status = "Finished".into();
    assert_eq!(
        running_titles(&mut rows, &mut order, options),
        ["first", "third", "second", "idle"]
    );
    rows[3].status = "Connecting…".into();
    assert_eq!(
        running_titles(&mut rows, &mut order, options),
        ["first", "third", "idle", "second"]
    );
    rows[0].status = "Idle".into();
    assert_eq!(
        running_titles(&mut rows, &mut order, options),
        ["third", "idle", "first", "second"]
    );
    rows[2].status = "Working…".into();
    assert_eq!(
        running_titles(&mut rows, &mut order, options),
        ["third", "idle", "first", "second"]
    );
}

#[test]
fn active_states_keep_ranks_but_waiting_stopped_and_removed_agents_release_them() {
    let mut rows = [
        row("first", None, 10, false),
        row("second", None, 20, false),
    ];
    for row in &mut rows {
        row.status = "Working…".into();
    }
    let mut order = RunningOrder::default();
    running_titles(&mut rows, &mut order, Options::default());
    for status in ["Compacting context…", "Retrying provider…", "Interrupting…"] {
        rows[0].status = status.into();
        rows.reverse();
        assert_eq!(
            running_titles(&mut rows, &mut order, Options::default()),
            ["first", "second"]
        );
    }
    for status in ["Waiting for input", "Stopped", "Queued", "Failed", "Idle"] {
        rows.iter_mut()
            .find(|row| row.title == "first")
            .unwrap()
            .status = status.into();
        assert_eq!(
            running_titles(&mut rows, &mut order, Options::default()),
            ["second", "first"]
        );
        rows[1].status = "Working…".into();
        assert_eq!(
            running_titles(&mut rows, &mut order, Options::default()),
            ["second", "first"]
        );
    }
    let mut removed = rows[0].clone();
    order.update(&[], &rows[1..], |_| true);
    removed.status = "Working…".into();
    rows[0] = removed;
    assert_eq!(
        running_titles(&mut rows, &mut order, Options::default()),
        ["first", "second"]
    );
}

#[test]
fn running_priority_is_not_reversed_and_idle_rows_keep_selected_sort() {
    for sort in [Sort::Activity, Sort::Created, Sort::Name] {
        for reverse in [false, true] {
            let mut rows = [
                row("idle A", Some(20), 20, false),
                row("runner B", Some(10), 10, false),
                row("runner A", Some(30), 30, false),
                row("idle Z", Some(10), 10, false),
            ];
            rows[1].status = "Working".into();
            rows[2].status = "Running".into();
            let options = Options { sort, reverse };
            let mut order = RunningOrder::default();
            let mut expected = titles(&[rows[0].clone(), rows[3].clone()], options);
            expected.splice(0..0, ["runner B".into(), "runner A".into()]);
            assert_eq!(running_titles(&mut rows, &mut order, options), expected);
        }
    }
}

#[test]
fn running_descendants_promote_their_tree_and_children_without_reparenting() {
    let mut rows = [
        row("idle root", None, 100, false),
        row("parent", None, 10, false),
    ];
    let mut worker = row("worker", None, 1, false);
    worker.status = "Working…".into();
    rows[1].children = vec![row("idle child", None, 100, false), worker];
    let mut order = RunningOrder::default();
    assert_eq!(
        running_titles(&mut rows, &mut order, Options::default()),
        ["parent", "idle root"]
    );
    order.sort_children(&mut rows);
    assert_eq!(rows[0].children[0].title, "worker");
    assert_eq!(rows[0].children[1].title, "idle child");
    assert_eq!(order.compare_groups(&[0], &[1], &rows), Ordering::Less);
    rows[0].children[0].status = "Finished".into();
    assert_eq!(
        running_titles(&mut rows, &mut order, Options::default()),
        ["idle root", "parent"]
    );
}

#[test]
fn legacy_preferences_keep_existing_defaults() {
    let options: Options = serde_json::from_str("{}").unwrap();
    assert!(options.sort == Sort::Activity);
    assert!(!options.reverse);
    let legacy: Options = serde_json::from_str(r#"{"pinned_first":false}"#).unwrap();
    assert!(legacy.sort == Sort::Activity);
}
