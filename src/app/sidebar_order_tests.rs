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
fn pinned_first_is_optional_and_not_reversed_with_the_sort() {
    let rows = [
        row("Alpha", Some(30), 30, false),
        row("zebra", Some(10), 10, true),
    ];
    for sort in [Sort::Activity, Sort::Created, Sort::Name] {
        for reverse in [false, true] {
            let options = Options {
                sort,
                reverse,
                pinned_first: true,
            };
            assert_eq!(titles(&rows, options)[0], "zebra");
        }
    }
    assert_eq!(
        titles(
            &rows,
            Options {
                sort: Sort::Name,
                pinned_first: false,
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
                    ..Default::default()
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
            pinned_first: false,
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
    assert_eq!(options.compare_groups(a, b, &rows), Ordering::Less);
}

#[test]
fn legacy_preferences_keep_existing_defaults() {
    let options: Options = serde_json::from_str("{}").unwrap();
    assert!(options.sort == Sort::Activity);
    assert!(!options.reverse);
    assert!(options.pinned_first);
}
