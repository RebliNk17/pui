use super::*;
const DIFF: &str =
    "@@ -10,2 +10,2 @@\n-old 界e\u{301}\n+new 界e\u{301}\n context\n@@ -20 +20 @@\n-old\n+new\n";
fn display(diff: &str) -> String {
    lines::rows(diff)
        .map(|row| {
            if row.header {
                lines::HUNK_CAPTION.into()
            } else {
                format!("{}{}\n", row.gutter(), row.text)
            }
        })
        .collect()
}
pub(super) fn range(diff: &str, chosen: &str) -> std::ops::Range<usize> {
    let display = display(diff);
    let start = display[..display.find(chosen).unwrap()].chars().count();
    start..start + chosen.chars().count()
}
fn notes() -> Comments {
    let mut notes = Comments::default();
    notes.bind(Path::new("/repo"), "session", 1);
    notes
}
fn add(notes: &mut Comments, text: &str) {
    assert!(notes.begin(
        "file.rs",
        DIFF,
        true,
        Some([7; 32]),
        range(DIFF, "+new 界e\u{301}")
    ));
    notes.editor.as_mut().unwrap().text = text.into();
    assert!(notes.save());
}
#[test]
fn unicode_wrapped_coordinates_and_duplicate_rows_are_exact_not_text_matched() {
    let selected = anchor::selected(DIFF, range(DIFF, "界e\u{301}")).unwrap();
    assert_eq!(selected.rows, (2, 2));
    assert_eq!(selected.old, Some((10, 10)));
    assert_eq!(selected.new, None);
    let shown = display(DIFF);
    let start = shown[..shown.rfind("+new").unwrap()].chars().count();
    let duplicate = anchor::selected(DIFF, start..start + 4).unwrap();
    assert_eq!(duplicate.rows, (7, 7));
    assert_eq!(duplicate.hunk, 2);
    assert_eq!(duplicate.new, Some((20, 20)));
}
#[test]
fn unsupported_cross_hunk_metadata_gutter_and_out_of_range_never_anchor() {
    assert!(anchor::selected(DIFF, 0..1).is_none());
    let start = range(DIFF, "+new 界e\u{301}").start;
    assert!(anchor::selected(DIFF, start..display(DIFF).chars().count()).is_none());
    let row_start = lines::HUNK_CAPTION.chars().count();
    assert!(anchor::selected(DIFF, row_start..row_start + 4).is_none());
    assert!(anchor::selected(DIFF, start..usize::MAX).is_none());
    assert!(anchor::selected("diff --git a/a b/a\n", 0..4).is_none());
    assert!(anchor::selected(DIFF, 3..3).is_none());
    let too_large = "+a".repeat(MAX_DIFF_BYTES / 2 + 1);
    assert!(anchor::selected(&too_large, 8..9).is_none());
}
#[test]
fn limits_include_pending_editor_all_owners_and_metadata() {
    let mut notes = notes();
    for _ in 0..MAX_COMMENTS {
        add(&mut notes, &"界".repeat(MAX_TEXT / 3));
    }
    assert!(notes.bytes() <= MAX_TOTAL);
    assert!(!notes.begin(
        "file.rs",
        DIFF,
        true,
        Some([7; 32]),
        range(DIFF, "+new 界e\u{301}")
    ));
    notes.bind(Path::new("/elsewhere"), "different", 1);
    assert_eq!(notes.items.len(), MAX_COMMENTS);
    assert!(!notes.begin(
        "file.rs",
        DIFF,
        true,
        Some([7; 32]),
        range(DIFF, "+new 界e\u{301}")
    ));
    let mut notes = Comments::default();
    notes.bind(
        Path::new(&format!("/{}", "a".repeat(4000))),
        &"s".repeat(2000),
        1,
    );
    for _ in 0..MAX_COMMENTS {
        if !notes.begin(
            "p",
            DIFF,
            false,
            Some([1; 32]),
            range(DIFF, "+new 界e\u{301}"),
        ) {
            break;
        }
        notes.editor.as_mut().unwrap().text = "x".repeat(MAX_TEXT);
        assert!(notes.save());
    }
    assert!(notes.items.len() < MAX_COMMENTS);
    assert!(notes.bytes() <= MAX_TOTAL);
    let mut bounded = super::tests::notes();
    assert!(!bounded.begin(
        &"p".repeat(MAX_METADATA),
        DIFF,
        true,
        Some([7; 32]),
        range(DIFF, "+new 界e\u{301}")
    ));
    assert!(bounded.editor.is_none());
}
#[test]
fn refresh_source_session_and_content_mismatch_stale_monotonically_without_retargeting() {
    let mut notes = notes();
    add(&mut notes, "first");
    let original = notes.items[0].anchor.selection.clone();
    notes.observe("file.rs", false, Some([1; 32]));
    assert!(!notes.items[0].stale, "Git is a different source");
    notes.observe("renamed.rs", true, Some([1; 32]));
    assert!(!notes.items[0].stale, "No rename retargeting");
    notes.observe("file.rs", true, Some([1; 32]));
    notes.observe("file.rs", true, Some([7; 32]));
    assert!(notes.items[0].stale);
    assert_eq!(notes.items[0].anchor.selection, original);
    notes.bind(Path::new("/repo"), "other-session", 2);
    assert!(notes.batch().is_err());
    assert_eq!(notes.items.len(), 1, "No silent owner-switch deletion");
    notes.bind(Path::new("/repo"), "session", 2);
    assert!(notes.batch().unwrap().contains("STALE"));
    assert!(notes.batch().unwrap().contains("SHA-256: 070707"));
    assert!(!notes.batch().unwrap().contains("base OID"));
}
#[test]
fn pending_text_and_resolved_notes_survive_stale_resolve_reopen_and_batch_is_complete_or_rejected()
{
    let mut notes = notes();
    add(&mut notes, "first local note");
    notes.items[0].resolved = true;
    assert!(notes.batch().is_err());
    notes.items[0].resolved = false;
    assert!(notes.batch().unwrap().contains("first local note"));
    for _ in 0..20 {
        add(&mut notes, &"a".repeat(MAX_TEXT));
    }
    assert!(notes.batch().unwrap_err().contains("24 KiB"));
    assert!(notes.begin(
        "file.rs",
        DIFF,
        false,
        Some([9; 32]),
        range(DIFF, "+new 界e\u{301}")
    ));
    notes.editor.as_mut().unwrap().text = "unsent".into();
    notes.stale(None);
    notes.bind(Path::new("/elsewhere"), "session", 3);
    assert_eq!(notes.editor.as_ref().unwrap().text, "unsent");
    assert!(notes.save());
    assert!(notes.items.last().unwrap().stale);
}
#[test]
fn queue_owner_is_checked_at_collection_without_draft_chat_permission_or_rpc_changes() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.draft = "keep".into();
    app.context_chips.clear_quotes();
    app.state.busy = true;
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let count = app.state.messages.len();
    context_capture::queue_review(
        &ctx,
        Path::new("/not-current"),
        "wrong-session",
        "Local review",
        "note",
    );
    app.collect_context_capture();
    assert_eq!(app.outgoing_text(), "keep");
    assert!(
        app.state
            .error
            .as_deref()
            .unwrap()
            .contains("repository/session changed")
    );
    let owner = app.review_comment_owner();
    context_capture::queue_review(&ctx, &app.prefs.cwd, &owner, "Local review", "note");
    app.collect_context_capture();
    assert!(
        app.outgoing_text()
            .starts_with("keep\n\n[Context: Local review]")
    );
    assert_eq!(app.state.messages.len(), count);
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
}
