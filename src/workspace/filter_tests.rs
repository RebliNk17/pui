use super::*;
use std::fs;

fn configure(root: &Path, key: &str, value: &str) {
    let (_, exit) = read_only::run(
        read_only::command(root).args(["config", key, value]),
        &AtomicBool::new(false),
        Instant::now() + DEADLINE,
        1024,
        "fixture config",
        "too large",
    )
    .unwrap();
    assert!(exit.success());
}

#[test]
fn configured_clean_process_and_wildcard_filters_fail_closed_without_executing_or_mutating() {
    for (driver, kind) in [
        ("pui-clean", "clean"),
        ("pui-process", "process"),
        ("pui.*driver", "clean"),
    ] {
        let root = tests::fixture();
        // Capture status BEFORE enabling filters: the test must not invoke the
        // very helpers production refuses while taking its invariant snapshot.
        let before = tests::snapshot(&root);
        fs::write(
            root.join(".git/info/attributes"),
            format!("* filter={driver}\n"),
        )
        .unwrap();
        configure(
            &root,
            &format!("filter.{driver}.{kind}"),
            "printf ran > .git/pui-filter-ran; cat",
        );
        let workspace = inspect(&root);
        assert!(workspace.is_git && workspace.files.is_empty());
        assert!(
            workspace
                .error
                .as_deref()
                .unwrap()
                .contains("clean/process filters")
        );
        assert!(
            diff(&root, &tests::file("README.md"))
                .unwrap_err()
                .contains("clean/process filters")
        );
        let mut untracked = tests::file("untracked");
        untracked.status = "??".into();
        assert!(diff(&root, &untracked).unwrap().contains("Untracked"));
        assert!(!root.join(".git/pui-filter-ran").exists());
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), before.1);
        let (_, exit) = read_only::run(
            read_only::command(&root).args([
                "config",
                "--remove-section",
                &format!("filter.{driver}"),
            ]),
            &AtomicBool::new(false),
            Instant::now() + DEADLINE,
            1024,
            "fixture config",
            "too large",
        )
        .unwrap();
        assert!(exit.success());
        assert_eq!(tests::snapshot(&root), before);
        assert!(!root.join(".git/pui-filter-ran").exists());
    }
}

#[test]
fn effective_included_config_oversize_and_config_errors_never_silently_allow_reads() {
    let root = tests::fixture();
    let include = root.join(".git/pui-filter-config");
    fs::write(&include, "[filter \"included\"]\n clean = false\n").unwrap();
    configure(&root, "include.path", include.to_str().unwrap());
    assert!(
        inspect(&root)
            .error
            .as_deref()
            .unwrap()
            .contains("clean/process filters")
    );
    fs::write(&include, "not a valid config file\n").unwrap();
    let error = inspect(&root).error.unwrap();
    assert!(!error.contains("not a valid config")); // stderr and values are discarded
    assert!(diff(&root, &tests::file("README.md")).is_err());
    fs::write(
        &include,
        format!("[filter \"{}\"]\n process = false\n", "x".repeat(4096)),
    )
    .unwrap();
    assert!(inspect(&root).error.as_deref().unwrap().contains("4 KiB"));
    assert!(
        diff(&root, &tests::file("README.md"))
            .unwrap_err()
            .contains("4 KiB")
    );
}
