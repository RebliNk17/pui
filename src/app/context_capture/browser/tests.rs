use super::*;
fn app() -> Pui {
    let ctx = egui::Context::default();
    crate::app::files::tests::app(&ctx)
}
fn snapshot(app: &mut Pui) -> u64 {
    let ctx = app.ctx.clone();
    let snap = Snapshot {
        root: app.prefs.cwd.clone(),
        session: app.review_comment_owner(),
        draft: app.draft.clone(),
        images: app.draft_images.revision(),
        token: 0,
        pass: ctx.cumulative_pass_nr(),
    };
    ctx.data_mut(|d| d.insert_temp(id().with("live"), Arc::new(snap)));
    freeze(&ctx).unwrap()
}
#[test]
fn browser_tokens_are_unique_across_clear_and_bind_exact_draft_images() {
    let mut app = app();
    app.draft = "keep 世界".into();
    let first = snapshot(&mut app);
    clear(&app.ctx);
    let second = snapshot(&mut app);
    assert_ne!(first, second);
    assert_eq!(token(&app.ctx), Some(second));
    let before = app.draft.clone();
    queue(
        &app.ctx,
        Handoff::Text {
            token: first,
            page: "http://localhost:3000".into(),
            epoch: 0,
            label: "old".into(),
            body: "old".into(),
        },
    );
    collect(&mut app);
    assert_eq!(app.draft, before);
    assert_eq!(
        token(&app.ctx),
        Some(second),
        "late old result must not consume the newer review"
    );
    cancel(&app.ctx, first);
    assert_eq!(token(&app.ctx), Some(second));
    cancel(&app.ctx, second);
    assert_eq!(token(&app.ctx), None);
}
#[test]
fn hidden_or_changed_page_and_attachment_or_owner_mutation_never_append() {
    for mode in ["hidden", "draft", "image", "root", "session"] {
        let mut app = app();
        app.draft = "original".into();
        let epoch = app.files.seed_browser_capture("http://localhost:3000");
        let t = snapshot(&mut app);
        let original = app.draft.clone();
        match mode {
            "hidden" => app.files = Default::default(),
            "draft" => app.draft = "changed!".into(),
            "image" => app.draft_images.clear(),
            "root" => app.prefs.cwd = PathBuf::from("/different"),
            "session" => app.state.session_file = Some("/different-session".into()),
            _ => {}
        }
        queue(
            &app.ctx,
            Handoff::Text {
                token: t,
                page: "http://localhost:3000".into(),
                epoch,
                label: "context".into(),
                body: "content".into(),
            },
        );
        collect(&mut app);
        assert!(!app.draft.contains("content"));
        if mode == "hidden" {
            assert_eq!(app.draft, original);
        }
        assert!(token(&app.ctx).is_none());
    }
}
#[test]
fn fresh_attachment_revision_changes_on_replacement_and_reviewed_append_is_bounded() {
    let mut draft = attachments::DraftImages::default();
    let before = draft.revision();
    draft.clear();
    assert_ne!(before, draft.revision());
    let source =
        crate::media::Attachment::from_bytes(include_bytes!("../../../../assets/icon.png"))
            .unwrap();
    let before = draft.revision();
    draft
        .append_reviewed("browser".into(), source.clone())
        .unwrap();
    assert_ne!(before, draft.revision());
    assert_eq!(draft.images().len(), 1);
    for _ in 1..8 {
        draft
            .append_reviewed("browser".into(), source.clone())
            .unwrap();
    }
    let before = draft.revision();
    assert!(draft.append_reviewed("extra".into(), source).is_err());
    assert_eq!(draft.revision(), before);
    assert_eq!(draft.images().len(), 8);
}
