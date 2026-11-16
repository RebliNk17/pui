use super::*;
use std::path::Path;
fn outputs() -> Vec<RecordedFile> {
    ["third.rs", "second.rs", "first.rs"]
        .into_iter()
        .map(|path| RecordedFile {
            path: path.into(),
            diff: format!("@@ -1 +1 @@\n-old {path}\n+new {path}\n"),
            ..Default::default()
        })
        .collect()
}
fn doc(file: &RecordedFile) -> diff_viewer::Document {
    let mut doc: diff_viewer::Document = (file.path.clone(), file.diff.clone()).into();
    doc.3 = true;
    doc
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers: egui::Modifiers::ALT,
        physical_key: None,
        pressed: true,
        repeat: true,
    }
}

#[test]
fn recorded_order_exact_membership_stale_revision_and_bounds_never_invent_file_position() {
    let outputs = outputs();
    let document = doc(&outputs[1]);
    let mut files = Files::default();
    files.bind(&document, &outputs, 42);
    assert_eq!(files.position, Some(1));
    assert_eq!(files.previous(42), Some(0));
    assert_eq!(files.next(42), Some(2));
    assert_eq!(files.label(42), "Recorded file 2 of 3");
    assert!(!files.available(43));
    assert!(files.label(43).contains("changed"));
    let mut changed = doc(&outputs[1]);
    changed.1.push_str("changed snapshot");
    let mut files = Files::default();
    files.bind(&changed, &outputs, 1);
    assert!(!files.available(1));
    let mut lots = outputs.clone();
    lots.resize(MAX_FILES + 1, outputs[0].clone());
    let mut files = Files::default();
    files.bind(&document, &lots, 0);
    assert!(files.limited);
    let mut huge = outputs.clone();
    huge[0].diff = "x".repeat(MAX_CURRENT_DIFF + 1);
    let mut files = Files::default();
    files.bind(&document, &huge, 0);
    assert!(files.limited);
    let mut long = outputs.clone();
    long[0].path = "x".repeat(MAX_PATH + 1);
    let mut files = Files::default();
    files.bind(&document, &long, 0);
    assert!(files.limited);
}

#[test]
fn ordered_repeated_keys_choose_event_time_file_without_boundary_wrap_or_snapshot_list() {
    let ctx = egui::Context::default();
    let outputs = outputs();
    let mut changes = Changes::default();
    changes.open_review(&ctx, doc(&outputs[2]), true, Path::new("/fixture"), "chat");
    let draw = |changes: &mut Changes, events| {
        ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                changes.review_file_inputs(ctx, &outputs, 1);
                egui::CentralPanel::default().show(ctx, |ui| {
                    diff_viewer::contents(ui, changes.review.as_mut().unwrap());
                });
            },
        )
    };
    draw(&mut changes, vec![]);
    draw(
        &mut changes,
        vec![
            key(egui::Key::ArrowRight),
            key(egui::Key::ArrowRight),
            key(egui::Key::ArrowRight),
            key(egui::Key::ArrowLeft),
        ],
    );
    assert_eq!(changes.review.as_ref().unwrap().0, "second.rs");
    draw(
        &mut changes,
        vec![key(egui::Key::ArrowLeft), key(egui::Key::ArrowLeft)],
    );
    assert_eq!(changes.review.as_ref().unwrap().0, "first.rs");
    assert!(std::mem::size_of::<Files>() <= 64);
    assert!(changes.review.as_ref().unwrap().3);
}

#[test]
fn bounded_membership_is_once_per_open_and_navigation_retains_only_constant_metadata() {
    let outputs: Vec<_> = (0..MAX_FILES)
        .map(|n| RecordedFile {
            path: format!("file{n}.rs"),
            diff: "+x\n".repeat(10_000),
            ..Default::default()
        })
        .collect();
    let document = doc(&outputs[127]);
    let start = std::time::Instant::now();
    let mut files = Files::default();
    files.bind(&document, &outputs, 10);
    for _ in 0..10_000 {
        files.bind(&document, &outputs, 10);
        assert!(files.available(10));
    }
    assert!(std::mem::size_of::<Files>() <= 64);
    eprintln!(
        "256 recorded files, {} bytes of bodies borrowed, one binding + 10k settled calls: {:?}; {}B navigation metadata",
        outputs.iter().map(|f| f.diff.len()).sum::<usize>(),
        start.elapsed(),
        std::mem::size_of::<Files>()
    );
}
