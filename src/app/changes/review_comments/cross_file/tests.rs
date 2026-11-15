use super::*;
const TARGET: &str = "@@ -1 +1 @@\n-old\n+chosen 世界\n";
fn outputs() -> Vec<RecordedFile> {
    vec![
        RecordedFile {
            path: "target.rs".into(),
            diff: TARGET.into(),
            ..Default::default()
        },
        RecordedFile {
            path: "current.rs".into(),
            diff: "@@ -1 +1 @@\n-a\n+b\n".into(),
            ..Default::default()
        },
    ]
}
fn fixture() -> (
    egui::Context,
    Comments,
    diff_viewer::Document,
    Vec<RecordedFile>,
    u64,
) {
    let ctx = egui::Context::default();
    let outputs = outputs();
    let mut notes = Comments::default();
    notes.bind(Path::new("/repo"), "owner", 1);
    notes.source_root = Some("/repo".into());
    let nav = diff_viewer::Navigation::new(TARGET);
    assert!(notes.begin(
        "target.rs",
        TARGET,
        true,
        nav.comment_digest(),
        super::super::tests::range(TARGET, "+chosen 世界")
    ));
    notes.editor.as_mut().unwrap().text = "Exact target note".into();
    assert!(notes.save());
    let id = notes.items[0].id;
    let mut doc: diff_viewer::Document = (outputs[1].path.clone(), outputs[1].diff.clone()).into();
    doc.3 = true;
    (ctx, notes, doc, outputs, id)
}
#[test]
fn exact_current_recorded_target_opens_once_and_preserves_note_editor_status_and_payload() {
    let (ctx, mut notes, doc, outputs, id) = fixture();
    assert!(notes.begin_edit(id));
    notes.editor.as_mut().unwrap().text = "Unfinished editing".into();
    let bytes = notes.bytes();
    let selection = notes.items[0].anchor.selection.clone();
    notes.items[0].resolved = true;
    notes.prepare_recorded_targets(&outputs, 1, &doc, true);
    assert_eq!(notes.recorded_targets, [(id, 1)]);
    assert!(notes.request_recorded_open(id, doc.2.focus_id(), ctx.cumulative_pass_nr()));
    let (position, mut opened) = notes.take_recorded_open(&ctx, &outputs, 1, &doc).unwrap();
    assert_eq!(position, 1);
    assert_eq!(opened.0, "target.rs");
    assert_eq!(opened.1, TARGET);
    assert!(opened.3);
    assert_eq!(opened.2.take_target(), Some(selection.rows.0 - 1));
    assert!(notes.take_recorded_open(&ctx, &outputs, 1, &doc).is_none());
    assert_eq!(notes.items[0].anchor.selection, selection);
    assert_eq!(notes.editor.as_ref().unwrap().text, "Unfinished editing");
    assert!(notes.items[0].resolved && !notes.items[0].stale);
    assert_eq!(notes.bytes(), bytes);
}
#[test]
fn stale_foreign_source_root_duplicate_missing_paths_and_invalid_list_never_offer_target() {
    for invalid in 0..8 {
        let (_, mut notes, mut doc, mut outputs, _) = fixture();
        match invalid {
            0 => notes.items[0].stale = true,
            1 => notes.items[0].anchor.owner.session = "other".into(),
            2 => notes.items[0].anchor.recorded = false,
            3 => notes.source_root = Some("/other".into()),
            4 => outputs.push(outputs[0].clone()),
            5 => {
                outputs.remove(0);
            }
            6 => doc.3 = false,
            _ => notes.items[0].anchor.root = "/other".into(),
        };
        notes.prepare_recorded_targets(&outputs, 1, &doc, true);
        assert!(notes.recorded_targets.is_empty(), "case{invalid}");
    }
    let (_, mut notes, doc, outputs, _) = fixture();
    notes.prepare_recorded_targets(&outputs, 1, &doc, false);
    assert!(notes.recorded_targets.is_empty());
}
#[test]
fn explicit_open_revalidates_digest_range_revision_id_document_and_confirmation_without_mutation() {
    for invalid in 0..8 {
        let (ctx, mut notes, mut doc, mut outputs, id) = fixture();
        notes.prepare_recorded_targets(&outputs, 1, &doc, true);
        assert!(notes.request_recorded_open(id, doc.2.focus_id(), ctx.cumulative_pass_nr()));
        let revision = if invalid == 0 { 2 } else { 1 };
        match invalid {
            1 => outputs[0].diff.push_str("+different\n"),
            2 => notes.items[0].anchor.selection.rows = (1, 1),
            3 => notes.items.clear(),
            4 => doc.2 = diff_viewer::Navigation::new(&doc.1),
            5 => notes.clear_requested = true,
            6 => notes.items[0].id += 1,
            7 => outputs.swap(0, 1),
            _ => {}
        };
        assert!(
            notes
                .take_recorded_open(&ctx, &outputs, revision, &doc)
                .is_none(),
            "case{invalid}"
        );
        assert_eq!(doc.0, "current.rs");
        assert!(notes.recorded_open.is_none());
    }
}
#[test]
fn explicit_mismatch_marks_original_note_and_pending_edit_stale_without_reanchoring() {
    let (ctx, mut notes, doc, mut outputs, id) = fixture();
    assert!(notes.begin_edit(id));
    let original = notes.items[0].anchor.selection.clone();
    notes.prepare_recorded_targets(&outputs, 1, &doc, true);
    notes.request_recorded_open(id, doc.2.focus_id(), ctx.cumulative_pass_nr());
    outputs[0].diff.push_str("+changed\n");
    assert!(notes.take_recorded_open(&ctx, &outputs, 1, &doc).is_none());
    assert!(notes.items[0].stale && notes.editor.as_ref().unwrap().stale);
    assert_eq!(notes.items[0].anchor.selection, original);
    outputs[0].diff = TARGET.into();
    notes.prepare_recorded_targets(&outputs, 1, &doc, true);
    assert!(notes.recorded_targets.is_empty());
}
#[test]
fn maximum_bounded_recorded_open_validates_only_explicit_target_then_reveals_distant_row() {
    let (ctx, mut notes, doc, mut outputs, id) = fixture();
    let large = format!(
        "@@ -1,50001 +1,50001 @@\n{}+chosen 世界\n",
        " unchanged literal data in immutable snapshot\n".repeat(40_000)
    );
    assert!(large.len() < MAX_DIFF_BYTES);
    let nav = diff_viewer::Navigation::new(&large);
    let item = &mut notes.items[0];
    item.anchor.digest = nav.comment_digest().unwrap();
    item.anchor.selection =
        anchor::selected(&large, super::super::tests::range(&large, "+chosen 世界")).unwrap();
    outputs[0].diff = large;
    notes.prepare_recorded_targets(&outputs, 1, &doc, true);
    notes.request_recorded_open(id, doc.2.focus_id(), ctx.cumulative_pass_nr());
    let start = std::time::Instant::now();
    let (_, mut opened) = notes.take_recorded_open(&ctx, &outputs, 1, &doc).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(opened.2.take_target(), Some(40_001));
    assert!(!notes.items[0].stale);
    eprintln!(
        "explicitrecordednoteopen {}B {elapsed:?}; only target cloned after exactdigest/rangevalidation",
        opened.1.len()
    );
}
#[test]
fn oversize_file_or_list_never_retains_targets_or_opens_unbounded_source() {
    let (_, mut notes, doc, mut outputs, _) = fixture();
    outputs[0].diff = "x".repeat(MAX_DIFF_BYTES + 1);
    notes.prepare_recorded_targets(&outputs, 1, &doc, true);
    assert!(notes.recorded_targets.is_empty());
    outputs[0].diff = TARGET.into();
    outputs.resize(257, RecordedFile::default());
    notes.prepare_recorded_targets(&outputs, 1, &doc, true);
    assert!(notes.recorded_targets.is_empty());
}
#[test]
fn hidden_expired_and_foreign_modal_requests_do_not_replay_and_metadata_stays_bounded() {
    let (ctx, mut notes, doc, outputs, id) = fixture();
    notes.prepare_recorded_targets(&outputs, 1, &doc, true);
    assert!(notes.request_recorded_open(id, doc.2.focus_id(), ctx.cumulative_pass_nr()));
    notes.clear_recorded_targets();
    assert!(notes.take_recorded_open(&ctx, &outputs, 1, &doc).is_none());
    notes.prepare_recorded_targets(&outputs, 1, &doc, true);
    notes.request_recorded_open(id, doc.2.focus_id(), ctx.cumulative_pass_nr());
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    assert!(notes.take_recorded_open(&ctx, &outputs, 1, &doc).is_none());
    notes.prepare_recorded_targets(&outputs, 1, &doc, true);
    notes.request_recorded_open(id, doc.2.focus_id(), ctx.cumulative_pass_nr());
    egui::Popup::open_id(&ctx, egui::Id::new("foreign-recorded-open"));
    assert!(notes.take_recorded_open(&ctx, &outputs, 1, &doc).is_none());
    assert!(egui::Popup::is_any_open(&ctx));
    for _ in 1..MAX_COMMENTS {
        assert!(notes.begin(
            "target.rs",
            TARGET,
            true,
            Some(notes.items[0].anchor.digest),
            super::super::tests::range(TARGET, "+chosen 世界")
        ));
        notes.editor.as_mut().unwrap().text = "bounded".into();
        assert!(notes.save());
    }
    let bytes = notes.bytes();
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        notes.prepare_recorded_targets(&outputs, 1, &doc, true);
        assert!(notes.recorded_targets.len() <= MAX_COMMENTS);
    }
    eprintln!(
        "1000x64recordednotetargetprojections {:?}; retainedpair{}B; payload{bytes}B",
        start.elapsed(),
        std::mem::size_of::<(u64, usize)>()
    );
    assert_eq!(notes.bytes(), bytes);
    assert!(notes.recorded_targets.capacity() <= MAX_COMMENTS * 2);
}
