use super::*;
fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-quick-ranking-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    root
}
fn file(root: &Path, path: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "NOT_INDEXED").unwrap();
}
fn names(root: &Path, query: &str) -> (Vec<String>, bool) {
    let (paths, limited) = discover(root, query, &AtomicBool::new(false)).unwrap();
    (
        paths
            .iter()
            .map(|path| path.strip_prefix(root).unwrap().to_str().unwrap().into())
            .collect(),
        limited,
    )
}
#[test]
fn classes_prefer_exact_relative_filename_prefix_substring_and_fuzzy_deterministically() {
    let needle = Needle::new("app").unwrap();
    let ordered = [
        "app",
        "src/app",
        "application.rs",
        "app/src.rs",
        "src/myapp.rs",
        "src/app/main.rs",
        "a_p_p.rs",
        "a/path/p.rs",
    ];
    let mut ranked = ordered
        .iter()
        .filter_map(|name| needle.score(Path::new(name)).map(|score| (score, *name)))
        .collect::<Vec<_>>();
    ranked.sort();
    assert_eq!(
        ranked.iter().map(|(_, name)| *name).collect::<Vec<_>>(),
        ordered
    );
    let n = Needle::new("sac").unwrap();
    assert!(n.score(Path::new("src/app/changes.rs")).is_some());
    assert!(n.score(Path::new("src/only.rs")).is_none());
    assert!(
        Needle::new("界e\u{301}")
            .unwrap()
            .score(Path::new("src/世界e\u{301}.rs"))
            .is_some()
    );
    assert!(
        Needle::new("שלום")
            .unwrap()
            .score(Path::new("src/שלום.rs"))
            .is_some()
    );
}
#[test]
fn top_k_continues_past_first_200_to_keep_late_exact_best_not_traversal_first() {
    let root = fixture();
    for n in 0..240 {
        file(&root, &format!("z/needle.rs-{n:03}"));
    }
    // Sorted directories are visited LIFO: a/ is discovered after z/'s >200
    // substring matches. Returning the first 200 would miss this exact name.
    file(&root, "a/needle.rs");
    file(&root, "needle.rs");
    let (paths, limited) = names(&root, "needle.rs");
    assert!(limited);
    assert_eq!(paths.len(), MAX_MATCHES);
    assert_eq!(paths[0], "needle.rs");
    assert_eq!(paths[1], "a/needle.rs");
    assert_eq!(names(&root, "needle.rs").0, paths);
}
#[test]
fn sensitive_generated_fifo_and_symlink_names_never_bypass_discovery_policy() {
    let root = fixture();
    for path in [
        "src/app/changes.rs",
        "target/changes.rs",
        "node_modules/changes.rs",
        ".env",
        "secrets.json",
    ] {
        file(&root, path);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(root.join("src/app/changes.rs"), root.join("shortcut.rs")).unwrap();
        let path =
            std::ffi::CString::new(root.join("pipe.rs").as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    }
    let (paths, limited) = names(&root, "");
    assert!(limited);
    assert_eq!(paths, ["src/app/changes.rs"]);
    file(&root, "bad\nname.rs");
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        // APFS rejects invalid UTF-8 filenames; exercise the scorer directly
        // rather than assuming the host filesystem can create one.
        let invalid = PathBuf::from(std::ffi::OsString::from_vec(vec![b'x', 0xff]));
        assert!(Needle::new("x").unwrap().score(&invalid).is_none());
    }
    assert_eq!(names(&root, "").0, ["src/app/changes.rs"]);
    let (paths, _) = names(&root, "NOT_INDEXED");
    assert!(paths.is_empty());
    fs::write(root.join("binary.bin"), [0, 1, 2, 255]).unwrap();
    assert_eq!(
        names(&root, "binary").0,
        ["binary.bin"],
        "filename discovery never tries to decode bodies"
    );
    let (paths, _) = discover(&root, "", &AtomicBool::new(true)).unwrap();
    assert!(paths.is_empty());
}
#[test]
fn long_ranked_results_have_an_aggregate_path_byte_cap_and_directory_limit_is_truthful() {
    let root = fixture();
    let parent = format!(
        "{}/{}/{}",
        "a".repeat(180),
        "b".repeat(180),
        "c".repeat(180)
    );
    for n in 0..200 {
        file(&root, &format!("{parent}/{n:03}{}", "x".repeat(175)));
    }
    let (paths, limited) = discover(&root, "", &AtomicBool::new(false)).unwrap();
    assert!(limited);
    assert!(paths.len() < MAX_MATCHES);
    let bytes: usize = paths.iter().map(|path| path.as_os_str().len()).sum();
    assert!(bytes <= MAX_RESULT_BYTES);
    let retained: usize = paths.iter().map(|path| path.capacity()).sum();
    assert!(retained <= MAX_RESULT_BYTES);
    eprintln!(
        "longpathresults {} / {}B logical / {retained}B capacity",
        paths.len(),
        bytes
    );
    let root = fixture();
    for n in 0..2001 {
        file(&root, &format!("file-{n:04}"));
    }
    assert!(names(&root, "absent").1);
}

#[test]
fn bounded_frontier_path_results_query_unicode_and_deterministic_20000_score_cost() {
    assert!(Needle::new(&"x".repeat(1025)).is_err());
    assert!(Needle::new("bad\nquery").is_err());
    let needle = Needle::new("sac").unwrap();
    let start = std::time::Instant::now();
    for n in 0..MAX_SEARCH {
        assert!(
            needle
                .score(Path::new(&format!("src/app/changes_{n:05}.rs")))
                .is_some()
        );
    }
    eprintln!(
        "20,000filenamefuzzyscores {:?}; heapcap200/frontier256/path4096B/results128KiB",
        start.elapsed()
    );
    let root = fixture();
    for n in 0..270 {
        fs::create_dir(root.join(format!("dir{n:03}"))).unwrap();
    }
    assert!(names(&root, "x").1);
}
