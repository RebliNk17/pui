use super::*;

fn append(state: &mut Draft, draft: &mut String, label: &str, text: &str) -> u64 {
    state
        .append(
            draft,
            Snippet {
                label: label.into(),
                text: text.into(),
            },
        )
        .unwrap();
    state.chips.last().unwrap().id
}

#[test]
fn removes_only_explicit_capture_and_its_separator_in_any_order() {
    for original in ["", "Keep draft\n[Context: manual]\nuntouched\n[/Context]"] {
        for order in [[1, 0, 2], [0, 1, 2], [2, 1, 0]] {
            let mut state = Draft::default();
            let mut draft = original.to_owned();
            let ids: Vec<_> = (0..3)
                .map(|_| append(&mut state, &mut draft, "same", "same 世界"))
                .collect();
            assert_ne!(ids[0], ids[1]);
            assert_ne!(ids[1], ids[2]);
            for (removed, index) in order.into_iter().enumerate() {
                assert!(state.remove(&mut draft, ids[index]));
                assert_eq!(draft.matches("same 世界").count(), 2 - removed);
                if original.is_empty() {
                    assert!(!draft.starts_with('\n'));
                }
            }
            assert_eq!(draft, original);
            assert!(state.chips.is_empty());
            assert!(!state.remove(&mut draft, ids[0]));
        }
    }
}

#[test]
fn any_manual_change_invalidates_without_deleting_unrelated_or_retargeted_text() {
    for edit in [0, 1, 2, 3] {
        let mut state = Draft::default();
        let mut draft = "Keep draft".into();
        let id = append(&mut state, &mut draft, "source.rs:1", "chosen");
        match edit {
            0 => draft.insert_str(0, "manual "),
            1 => draft = draft.replace("chosen", "edited"), // same length
            2 => draft = draft.replace("Keep", "Swap"),     // outside capture, same length
            _ => draft.push_str("\n[Context: source.rs:1]\nchosen\n[/Context]"),
        }
        let edited = draft.clone();
        assert!(!state.remove(&mut draft, id));
        assert_eq!(draft, edited);
        assert!(state.chips.is_empty());
        let fresh = append(&mut state, &mut draft, "next", "new capture");
        assert!(state.remove(&mut draft, fresh));
        assert_eq!(draft, edited);
    }
}

#[test]
fn count_and_total_draft_budget_include_exact_wrappers_separators_and_unicode_labels() {
    let mut state = Draft::default();
    let mut draft = "original".into();
    for _ in 0..MAX_CAPTURES {
        append(&mut state, &mut draft, "same", "text");
    }
    let before = draft.clone();
    assert!(
        state
            .append(
                &mut draft,
                Snippet {
                    label: "extra".into(),
                    text: "body".into()
                }
            )
            .unwrap_err()
            .contains("16")
    );
    assert_eq!(draft, before);
    let first = state.chips[0].id;
    assert!(state.remove(&mut draft, first));
    append(&mut state, &mut draft, "new", "after removal");
    assert_eq!(state.chips.len(), MAX_CAPTURES);

    let ctx = egui::Context::default();
    let mut app = files::tests::app(&ctx);
    let label = "世界".repeat(1000);
    queue_parts(&ctx, &label, ["", "hello", "", "world"]);
    app.collect_context_capture();
    assert!(app.draft.contains("hello\n\nworld"));
    assert_eq!(app.context_chips.chips[0].label.chars().count(), 512);
    let addition = app.draft.len() + 2;
    app.context_chips.clear();
    app.draft = "x".repeat(MAX_DRAFT_BYTES - addition);
    queue_parts(&ctx, &label, ["hello", "world"]);
    app.collect_context_capture();
    assert_eq!(app.draft.len(), MAX_DRAFT_BYTES);
    let before = app.draft.clone();
    queue(&ctx, "next", "text");
    app.collect_context_capture();
    assert_eq!(app.draft, before);
    assert!(app.state.error.as_deref().unwrap().contains("too large"));
}

#[test]
fn chips_retain_only_small_metadata_and_do_not_rescan_or_copy_settled_draft() {
    let mut state = Draft::default();
    let mut draft = "original".into();
    let body = "界".repeat(5000);
    for i in 0..16 {
        append(&mut state, &mut draft, &format!("source{i}"), &body);
    }
    let address = draft.as_ptr();
    let capacity = draft.capacity();
    let metadata = state.chips.capacity() * std::mem::size_of::<Chip>()
        + state
            .chips
            .iter()
            .map(|c| c.label.capacity())
            .sum::<usize>();
    assert!(metadata < 4096, "{metadata}");
    let start = std::time::Instant::now();
    for _ in 0..100_000 {
        state.check_length(&draft);
    }
    eprintln!(
        "context chip metadata: {metadata} bytes for {} draft bytes; 100k O(1) settled checks {:?}",
        draft.len(),
        start.elapsed()
    );
    assert_eq!(draft.as_ptr(), address);
    assert_eq!(draft.capacity(), capacity);
    assert_eq!(state.chips.len(), 16);
}

#[test]
fn parked_chat_moves_chip_identity_and_new_chat_does_not_inherit_or_replay_it() {
    let ctx = egui::Context::default();
    let mut app = files::tests::app(&ctx);
    app.demo = false;
    app.draft = "parent draft".into();
    queue(&ctx, "parent.rs:1", "literal");
    app.collect_context_capture();
    let id = app.context_chips.chips[0].id;
    let original = app.draft.clone();
    let parent = app.ensure_agent();
    app.start_chat_in(app.prefs.cwd.clone());
    assert!(app.context_chips.chips.is_empty());
    assert!(app.draft.is_empty());
    app.view_agent(&parent);
    assert_eq!(app.context_chips.chips[0].id, id);
    assert_eq!(app.draft, original);
    assert!(app.context_chips.remove(&mut app.draft, id));
    assert_eq!(app.draft, "parent draft");
}

#[test]
fn stale_queued_capture_is_discarded_at_session_boundary_and_restored_text_is_not_parsed() {
    let ctx = egui::Context::default();
    let mut app = files::tests::app(&ctx);
    queue(&ctx, "old-owner", "old body");
    app.leave_session_layout();
    app.draft = "[Context: restored]\nnot a chip\n[/Context]".into();
    app.collect_context_capture();
    assert!(app.context_chips.chips.is_empty());
    assert!(!app.draft.contains("old body"));
    let draft = app.draft.clone();
    let parked = app.take_chat();
    app.put_chat(parked);
    assert_eq!(app.draft, draft);
    assert!(app.context_chips.chips.is_empty());
}

#[test]
fn explicit_queued_send_transfers_payload_once_then_restore_has_no_chip_retargeting() {
    let ctx = egui::Context::default();
    let mut app = files::tests::app(&ctx);
    app.demo = false;
    app.connected = true;
    app.state.busy = true;
    app.state.session_file = Some("/tmp/context-chip-queue.jsonl".into());
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.draft = "Keep original".into();
    queue(&ctx, "terminal", "literal command, not executed");
    app.collect_context_capture();
    let text = app.draft.clone();
    assert!(commands.try_recv().is_err());
    app.queue_draft_mode(settings::DeliveryMode::Queue);
    assert!(app.draft.is_empty());
    assert!(app.context_chips.chips.is_empty());
    let waiting = &app.waiting[&PathBuf::from("/tmp/context-chip-queue.jsonl")];
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].text, text);
    assert!(
        commands.try_recv().is_err(),
        "queue itself does not execute the text"
    );
    app.draft = text;
    assert!(app.context_chips.chips.is_empty());
}
