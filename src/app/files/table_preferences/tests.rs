use super::*;

#[test]
fn defaults_are_legacy_compatible_scoped_to_workspace_file_and_reversible() {
    let mut saved = TableHeaders::default();
    let (a, b, file) = (
        Path::new("/workspace/a"),
        Path::new("/workspace/b"),
        Path::new("/tmp/report.CSV"),
    );
    assert!(saved.header(a, file));
    assert!(saved.set(a, file, false));
    assert!(!saved.header(a, file));
    assert!(saved.header(b, file));
    assert!(saved.header(a, Path::new("/tmp/other.tsv")));
    let serialized = serde_json::to_string(&saved).unwrap();
    let mut loaded: TableHeaders = serde_json::from_str(&serialized).unwrap();
    assert!(!loaded.header(a, file));
    assert!(loaded.header(b, file));
    assert!(loaded.set(a, file, true));
    assert!(loaded.header(a, file));
    assert_eq!(serde_json::to_string(&loaded).unwrap(), "[]");
    let legacy: Preferences = serde_json::from_str("{}").unwrap();
    assert!(legacy.table_headers.header(a, file));
}
#[test]
fn key_validation_is_lexical_and_metadata_never_grants_read_access() {
    let mut saved = TableHeaders::default();
    for (root, file) in [
        ("relative", "/tmp/a.csv"),
        ("/work", "a.csv"),
        ("/work", "/tmp/../a.csv"),
        ("/work/../other", "/tmp/a.csv"),
        ("/work", "/tmp/control\n.csv"),
        ("/work", "/tmp/a.pdf"),
    ] {
        assert!(
            !saved.set(Path::new(root), Path::new(file), false),
            "{root}, {file}"
        );
    }
    assert!(saved.0.is_empty());
    assert!(!saved.set(
        Path::new("/work"),
        Path::new(&format!("/{}.csv", "a".repeat(PATH_LIMIT))),
        false
    ));
    // Nonexistent paths require no canonicalization/stat/read to store or load.
    let root = Path::new("/missing/metadata-only/workspace");
    let file = Path::new("/missing/metadata-only/./report.tsv");
    assert!(saved.set(root, file, false));
    assert!(!saved.header(root, Path::new("/missing/metadata-only/report.tsv")));
    let ctx = egui::Context::default();
    let app = crate::app::files::tests::app(&ctx);
    let workspace = app.prefs.cwd;
    assert!(saved.set(&workspace, &workspace.join(".env.csv"), false));
    assert!(read_content(&workspace, &workspace.join(".env.csv"), false).is_err());
}
#[test]
fn override_count_and_lru_write_order_are_bounded_on_insert_and_deserialize() {
    let root = Path::new("/work");
    let mut saved = TableHeaders::default();
    for n in 0..LIMIT {
        saved.set(root, &PathBuf::from(format!("/work/{n}.csv")), false);
    }
    saved.set(root, Path::new("/work/0.csv"), false);
    saved.set(root, Path::new("/work/new.csv"), false);
    assert_eq!(saved.0.len(), LIMIT);
    assert!(!saved.header(root, Path::new("/work/0.csv")));
    assert!(saved.header(root, Path::new("/work/1.csv")));
    let raw: Vec<_> = (0..1000)
        .map(|n| json!({"workspace":"/work", "path":format!("/work/{n}.csv")}))
        .collect();
    let mut raw = raw;
    raw.push(json!({"workspace":"/work","path":"/work/999.csv"}));
    raw.push(json!({"workspace":"/work","path":"../bad.csv"}));
    let restored: TableHeaders = serde_json::from_value(json!(raw)).unwrap();
    assert_eq!(restored.0.len(), LIMIT);
    assert!(restored.header(root, Path::new("/work/0.csv")));
    assert!(!restored.header(root, Path::new("/work/999.csv")));
    let before = serde_json::to_string(&restored).unwrap();
    for _ in 0..20 {
        restored.header(root, Path::new("/work/999.csv"));
    }
    assert_eq!(before, serde_json::to_string(&restored).unwrap());
    let started = std::time::Instant::now();
    for _ in 0..10_000 {
        assert!(!restored.header(root, Path::new("/work/999.csv")));
    }
    let capacities: usize = restored
        .0
        .iter()
        .map(|key| key.workspace.capacity() + key.path.capacity())
        .sum();
    eprintln!(
        "table header metadata: {} overrides / {} serialized bytes / {} path-capacity bytes; 10,000 last-key lookups {:?}; bounded at {} bytes per key path",
        restored.0.len(),
        before.len(),
        capacities,
        started.elapsed(),
        PATH_LIMIT
    );
}
