use super::*;
#[test]
fn real_pathless_chats_share_global_cap_but_not_notes_or_draft_owner() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.state.session_file = None;
    let first_agent = app.ensure_agent();
    let owner_a = app.review_comment_owner();
    let root = app.prefs.cwd.clone();
    app.changes.review_comments.bind(&root, &owner_a, 1);
    let diff = "@@ -1 +1 @@\n-old\n+new\n";
    for _ in 0..MAX_COMMENTS {
        assert!(app.changes.review_comments.begin(
            "file.rs",
            diff,
            true,
            Some([1; 32]),
            super::tests::range(diff, "+new")
        ));
        app.changes.review_comments.editor.as_mut().unwrap().text = "note".into();
        assert!(app.changes.review_comments.save());
    }
    app.leave_session_layout();
    app.agents.active.clear();
    let second_agent = app.ensure_agent();
    let owner_b = app.review_comment_owner();
    assert_ne!(owner_a, owner_b);
    app.enter_session_layout();
    app.changes.review_comments.bind(&root, &owner_b, 1);
    assert_eq!(app.changes.review_comments.items.len(), MAX_COMMENTS);
    assert!(!app.changes.review_comments.begin(
        "file.rs",
        diff,
        true,
        Some([1; 32]),
        super::tests::range(diff, "+new")
    ));
    assert!(
        app.changes.review_comments.batch().is_err(),
        "Other chat cannot batch A notes"
    );
    app.draft = "keep b".into();
    context_capture::queue_review(&ctx, &root, &owner_a, "Local review", "A note");
    app.collect_context_capture();
    assert_eq!(app.draft, "keep b");
    app.leave_session_layout();
    app.agents.active = first_agent.clone();
    app.enter_session_layout();
    app.changes.review_comments.bind(&root, &owner_a, 1);
    assert_eq!(app.changes.review_comments.items.len(), MAX_COMMENTS);
    assert!(
        app.changes
            .review_comments
            .items
            .iter()
            .all(|item| item.stale)
    );
    // Same operation used only after the shared confirmation's explicit Confirm.
    app.changes.review_comments.items.clear();
    app.leave_session_layout();
    app.agents.active = second_agent;
    app.enter_session_layout();
    assert!(app.changes.review_comments.items.is_empty());
    app.leave_session_layout();
    app.agents.active = first_agent;
    app.enter_session_layout();
    assert!(
        app.changes.review_comments.items.is_empty(),
        "Clear cannot resurrect parked notes"
    );
}
