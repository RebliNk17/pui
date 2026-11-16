use super::*;

#[test]
fn index_uses_only_valid_unified_headers_not_literal_markers_or_unlocated_recorded_edits() {
    let source = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1,2 +3,2 @@ name\n--- literal removed\n+++ literal added\n context\n@@ malformed\n-old\n+new\n@@ -20 +30 @@\n-a\n+b\n";
    let nav = Navigation::new(source);
    assert_eq!(nav.rows, vec![3, 10]);
    assert!(nav.selected.is_none());
    let nav = Navigation::new("-recorded\n+edit\n");
    assert!(nav.rows.is_empty());
    assert_eq!(nav.summary(), "No unified hunks · read-only");
    for malformed in [
        "@@@ -1 +2 @@@",
        "@@ -0 +2 @@",
        "@@ -1,no +2 @@",
        "@@ -1 +2 missing",
    ] {
        assert!(Navigation::new(malformed).rows.is_empty());
    }
}
#[test]
fn index_and_capture_are_bounded_and_first_previous_next_last_do_not_wrap() {
    let source = "@@ -1 +1 @@\n-old\n+new\n".repeat(MAX_HUNKS);
    let start = std::time::Instant::now();
    let mut nav = Navigation::new(&source);
    assert_eq!(nav.rows.len(), MAX_HUNKS);
    assert!(nav.rows.capacity() * std::mem::size_of::<u32>() <= MAX_HUNKS * 4);
    eprintln!(
        "diff hunk index: {} hunks / {} bytes retained row capacity / {:?} debug",
        nav.rows.len(),
        nav.rows.capacity() * 4,
        start.elapsed()
    );
    assert_eq!(nav.previous(), None);
    assert_eq!(nav.next(), Some(0));
    let ctx = egui::Context::default();
    nav.choose(0, &source, &ctx);
    assert_eq!(nav.previous(), None);
    assert_eq!(nav.next(), Some(1));
    nav.choose(MAX_HUNKS - 1, &source, &ctx);
    assert_eq!(nav.next(), None);
    assert_eq!(nav.previous(), Some(MAX_HUNKS - 2));
    assert_eq!(nav.take_target(), Some((MAX_HUNKS - 1) * 3));
    assert!(nav.take_target().is_none());
    assert!(nav.header.chars().count() <= 160);
    let over = Navigation::new(&(source + "@@ -1 +1 @@\n"));
    assert!(over.rows.is_empty() && over.limited);
    let over = Navigation::new(&"x".repeat(MAX_BYTES + 1));
    assert!(over.rows.is_empty() && over.limited);
}
#[test]
fn document_identity_reopen_or_same_path_changed_diff_gets_fresh_navigation_and_no_extra_body_copy()
{
    let ctx = egui::Context::default();
    let source = "@@ -1 +1 @@\n-a\n+b\n".to_owned();
    let ptr = source.as_ptr();
    let mut doc: Document = ("file.rs".to_owned(), source).into();
    assert_eq!(ptr, doc.1.as_ptr());
    doc.2.choose(0, &doc.1, &ctx);
    let fresh: Document = ("file.rs".to_owned(), "@@ -5 +5 @@\n-a\n+c\n".to_owned()).into();
    assert!(fresh.2.selected.is_none());
    assert_ne!(doc.1, fresh.1);
    assert!(fresh.2.target.is_none());
}
