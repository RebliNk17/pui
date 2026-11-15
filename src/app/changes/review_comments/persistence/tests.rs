use super::*;
const DIFF: &str = "@@ -1 +1 @@\n-old\n+new 世界\n";
#[derive(Default)]
pub(super) struct Storage(pub(super) std::collections::HashMap<String, String>);
impl eframe::Storage for Storage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.into(), value);
    }
    fn flush(&mut self) {}
}
pub(super) fn notes() -> Comments {
    let mut notes = Comments::default();
    notes.bind(Path::new("/repo"), "/sessions/current.jsonl", 1);
    notes.source_root = Some("/actual/git".into());
    add(&mut notes, "Saved note 世界");
    notes
}
fn add(notes: &mut Comments, text: &str) {
    assert!(notes.begin(
        "file.rs",
        DIFF,
        true,
        Some([7; 32]),
        super::super::tests::range(DIFF, "+new 世界")
    ));
    notes.editor.as_mut().unwrap().text = text.into();
    assert!(notes.save());
}
fn encode(storage: &mut Storage, envelope: Envelope) {
    storage
        .0
        .insert(KEY.into(), serde_json::to_string(&envelope).unwrap());
}
fn read(storage: &Storage) -> Envelope {
    serde_json::from_str(&storage.0[KEY]).unwrap()
}
pub(super) fn restore(storage: &Storage) -> Comments {
    let mut notes = Comments::default();
    notes.restore_checkpoint(Some(storage));
    notes
}
#[test]
fn saved_notes_roundtrip_exact_original_metadata_and_resolved_but_are_always_stale() {
    let mut notes = notes();
    add(&mut notes, "Resolved");
    notes.items[1].resolved = true;
    let original = serde_json::to_value(&notes.items[0].anchor).unwrap();
    let mut storage = Storage::default();
    notes.save_checkpoint(&mut storage);
    let mut restored = restore(&storage);
    assert_eq!(restored.items.len(), 2);
    assert!(restored.items.iter().all(|item| item.stale));
    assert_eq!(restored.items[0].text, "Saved note 世界");
    assert_eq!(
        serde_json::to_value(&restored.items[0].anchor).unwrap(),
        original
    );
    assert!(restored.items[1].resolved);
    assert_eq!(restored.next_id, notes.next_id);
    assert!(restored.owner.is_none() && restored.source_root.is_none());
    assert!(restored.editor.is_none() && !restored.expanded && !restored.confirming());
    assert!(
        restored.marker_controls.is_empty()
            && restored.controls.is_empty()
            && restored.reveal_note.is_none()
    );
    restored.bind(Path::new("/repo"), "/sessions/foreign.jsonl", 1);
    assert!(restored.batch().is_err());
    restored.bind(Path::new("/repo"), "/sessions/current.jsonl", 1);
    let batch = restored.batch().unwrap();
    assert!(
        batch.contains("STALE")
            && batch.contains("Root: /actual/git")
            && !batch.contains("Resolved")
    );
    let next = restored.next_id;
    add(&mut restored, "next");
    assert_eq!(restored.items.last().unwrap().id, next + 1);
}
#[test]
fn unfinished_edits_and_pathless_notes_are_never_written_or_promoted_by_restore() {
    let mut notes = notes();
    let id = notes.items[0].id;
    assert!(notes.begin_edit(id));
    notes.editor.as_mut().unwrap().text = "Unfinished replacement must not persist".into();
    let mut storage = Storage::default();
    notes.save_checkpoint(&mut storage);
    let restored = restore(&storage);
    assert_eq!(restored.items[0].text, "Saved note 世界");
    assert!(restored.editor.is_none());
    assert!(!storage.0[KEY].contains("Unfinished replacement"));
    notes.editor = None;
    notes.bind(Path::new("/repo"), "agent:runtime-identity", 1);
    add(&mut notes, "Pathless excluded");
    notes.save_checkpoint(&mut storage);
    assert_eq!(restore(&storage).items.len(), 1);
    assert!(!storage.0[KEY].contains("Pathless excluded"));
    notes.items.clear();
    notes.storage_dirty = true;
    notes.save_checkpoint(&mut storage);
    let mut cleared = restore(&storage);
    assert!(cleared.items.is_empty());
    let watermark = cleared.next_id;
    cleared.bind(Path::new("/repo"), "/sessions/current.jsonl", 1);
    add(&mut cleared, "After clear");
    assert_eq!(cleared.items[0].id, watermark + 1);
}
#[test]
fn corrupt_envelope_future_version_huge_payload_and_wrong_types_do_not_poison_preferences() {
    for raw in [
        "{broken".into(),
        "[]".into(),
        serde_json::json!({"version":2,"next_id":9,"records":[]}).to_string(),
        serde_json::json!({"version":1,"next_id":0,"records":[{}]}).to_string(),
        "x".repeat(MAX_STORAGE + 1),
    ] {
        let mut storage = Storage::default();
        storage.0.insert(KEY.into(), raw);
        storage
            .0
            .insert("preferences".into(), "unchanged preferences".into());
        let restored = restore(&storage);
        assert!(restored.items.is_empty());
        assert_eq!(restored.storage_notice, Some(INVALID));
        assert_eq!(storage.0["preferences"], "unchanged preferences");
    }
}
#[test]
fn malformed_individual_metadata_duplicate_id_and_excess_records_are_isolated_bounded() {
    let mut notes = notes();
    let mut storage = Storage::default();
    notes.save_checkpoint(&mut storage);
    let record = read(&storage).records.remove(0);
    let mut invalid = serde_json::from_str::<serde_json::Value>(&record).unwrap();
    invalid["anchor"]["selection"]["chars"]["start"] = serde_json::json!(u64::MAX);
    encode(
        &mut storage,
        Envelope {
            version: 1,
            next_id: 20,
            records: vec![
                "broken".into(),
                serde_json::to_string(&invalid).unwrap(),
                record.clone(),
                record.clone(),
            ],
        },
    );
    let restored = restore(&storage);
    assert_eq!(restored.items.len(), 1);
    assert_eq!(restored.storage_notice, Some(PARTIAL));
    assert_eq!(restored.next_id, 20);
    let mut flood = Record::from_comment(&notes.items[0]);
    flood.anchor.owner.root = PathBuf::from(format!("/{}", "x".repeat(1700)));
    flood.anchor.root = PathBuf::from(format!("/{}", "y".repeat(1700)));
    flood.anchor.owner.session = format!("/{}", "s".repeat(1700));
    flood.text = "a".repeat(MAX_TEXT);
    let records = (1..=MAX_COMMENTS)
        .map(|id| {
            flood.id = id as u64;
            serde_json::to_string(&flood).unwrap()
        })
        .collect();
    encode(
        &mut storage,
        Envelope {
            version: 1,
            next_id: 64,
            records,
        },
    );
    let bounded = restore(&storage);
    assert!(bounded.items.len() < MAX_COMMENTS);
    assert!(bounded.bytes() <= MAX_TOTAL);
    assert_eq!(bounded.storage_notice, Some(PARTIAL));
    encode(
        &mut storage,
        Envelope {
            version: 1,
            next_id: 0,
            records: vec![record; MAX_COMMENTS + 1],
        },
    );
    let restored = restore(&storage);
    assert!(restored.items.is_empty());
    assert_eq!(restored.storage_notice, Some(INVALID));
}
#[test]
fn valid_serde_with_invalid_owner_paths_coordinates_digest_or_text_is_not_adopted() {
    let mut notes = notes();
    let mut storage = Storage::default();
    notes.save_checkpoint(&mut storage);
    let record = serde_json::from_str::<serde_json::Value>(&read(&storage).records[0]).unwrap();
    for n in 0..12 {
        let mut invalid = record.clone();
        match n {
            0 => invalid["id"] = serde_json::json!(0),
            1 => invalid["anchor"]["owner"]["session"] = serde_json::json!("agent:other"),
            2 => invalid["anchor"]["owner"]["root"] = serde_json::json!("relative"),
            3 => invalid["anchor"]["root"] = serde_json::json!("/bad\nroot"),
            4 => invalid["anchor"]["path"] = serde_json::json!(""),
            5 => invalid["anchor"]["digest"] = serde_json::json!([1, 2]),
            6 => invalid["anchor"]["selection"]["rows"] = serde_json::json!([5, 1]),
            7 => invalid["anchor"]["selection"]["hunk"] = serde_json::json!(usize::MAX),
            8 => invalid["anchor"]["selection"]["old"] = serde_json::json!([0, 2]),
            9 => invalid["text"] = serde_json::json!(" "),
            10 => invalid["text"] = serde_json::json!("x".repeat(MAX_TEXT + 1)),
            _ => invalid["anchor"]["path"] = serde_json::json!("x".repeat(MAX_METADATA + 1)),
        }
        encode(
            &mut storage,
            Envelope {
                version: 1,
                next_id: 0,
                records: vec![serde_json::to_string(&invalid).unwrap()],
            },
        );
        assert!(restore(&storage).items.is_empty(), "case {n}");
    }
}
#[test]
fn writer_budget_and_invalid_live_record_leave_previous_checkpoint_untouched() {
    let mut notes = notes();
    let mut storage = Storage::default();
    notes.save_checkpoint(&mut storage);
    let old = storage.0[KEY].clone();
    notes.items[0].text = "bad".repeat(MAX_TEXT);
    notes.storage_dirty = true;
    notes.save_checkpoint(&mut storage);
    assert_eq!(storage.0[KEY], old);
    assert_eq!(notes.storage_notice, Some(SAVE_LIMIT));
    assert!(notes.storage_dirty);
    let mut writer = Limited(Vec::new());
    assert!(writer.write_all(&vec![0; MAX_STORAGE]).is_ok());
    assert!(writer.write_all(&[1]).is_err());
    assert_eq!(writer.0.len(), MAX_STORAGE);
    let mut envelope = read(&storage);
    envelope
        .records
        .insert(0, "x".repeat(MAX_RECORD_STORAGE + 1));
    encode(&mut storage, envelope);
    let restored = restore(&storage);
    assert_eq!(restored.items.len(), 1);
    assert_eq!(restored.storage_notice, Some(PARTIAL));
}

#[test]
fn unlocated_but_exact_original_anchor_roundtrips_without_inventing_coordinates() {
    let mut notes = notes();
    let selection = &mut notes.items[0].anchor.selection;
    selection.located = false;
    selection.hunk = 0;
    selection.old = Some((1, 1));
    selection.new = None;
    let mut storage = Storage::default();
    notes.save_checkpoint(&mut storage);
    let restored = restore(&storage);
    assert_eq!(restored.items.len(), 1);
    assert!(!restored.items[0].anchor.selection.located);
    assert!(restored.items[0].stale);
}

#[test]
fn restore_once_and_current_live_store_win_without_resurrecting_cleared_notes() {
    let mut notes = notes();
    let mut storage = Storage::default();
    notes.save_checkpoint(&mut storage);
    let mut restored = restore(&storage);
    restored.items.clear();
    restored.restore_checkpoint(Some(&storage));
    assert!(restored.items.is_empty());
    let mut live = self::notes();
    live.items[0].text = "Newer live text".into();
    live.restore_checkpoint(Some(&storage));
    assert_eq!(live.items[0].text, "Newer live text");
}
#[test]
fn max_notes_storage_budgets_capacity_expansion_and_identity_exhaustion_stay_bounded() {
    let mut notes = notes();
    notes.items[0].text = "\u{1}".repeat(MAX_TEXT);
    for _ in 1..MAX_COMMENTS {
        add(&mut notes, &"\u{1}".repeat(MAX_TEXT));
    }
    let mut storage = Storage::default();
    let start = std::time::Instant::now();
    notes.save_checkpoint(&mut storage);
    let mut restored = restore(&storage);
    assert_eq!(restored.items.len(), MAX_COMMENTS);
    assert!(restored.bytes() <= MAX_TOTAL);
    assert!(storage.0[KEY].len() <= MAX_STORAGE);
    eprintln!(
        "64notecheckpoint+restore {:?}; encoded{}B retained{}B",
        start.elapsed(),
        storage.0[KEY].len(),
        restored.bytes()
    );
    let mut envelope = read(&storage);
    envelope.next_id = u64::MAX;
    encode(&mut storage, envelope);
    restored = restore(&storage);
    restored.items.truncate(1);
    restored.bind(Path::new("/repo"), "/sessions/current.jsonl", 1);
    assert!(restored.begin(
        "file.rs",
        DIFF,
        true,
        Some([7; 32]),
        super::super::tests::range(DIFF, "+new 世界")
    ));
    restored.editor.as_mut().unwrap().text = "Cannot reuse identity".into();
    assert!(!restored.save());
    assert_eq!(restored.items.len(), 1);
    assert!(restored.editor.is_some());
}
