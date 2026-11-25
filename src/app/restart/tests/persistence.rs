use super::*;

fn app() -> Pui {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(egui::Context::default()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.state = State::default();
    app.new_chat = true;
    app.prefs.cwd = "/fixture".into();
    app.draft.clear();
    app.draft_images.clear();
    app.history_drafts.clear();
    app
}

fn image() -> crate::media::Attachment {
    crate::media::Attachment::from_json(&json!({
        "type": "image", "data": "exact-image-payload", "mimeType": "image/png"
    }))
    .unwrap()
}

fn file() -> crate::media::Attachment {
    crate::media::Attachment::from_file(crate::media::FileAttachment {
        path: "/retained/immutable.txt".into(),
        name: "original.txt".into(),
        size: 123,
    })
}

fn capture(app: &mut Pui) -> Vec<Checkpoint> {
    app.checkpoint_chats();
    serde_json::from_str(&serde_json::to_string(&app.prefs.restart_checkpoints).unwrap()).unwrap()
}

#[test]
fn context_and_paste_cards_restore_exactly_separate_from_typed_text() {
    let mut source = app();
    source.draft = "  literal [Context: typed]\nnot a card\n[/Context]  \n".into();
    let card_payload = json!([
        {"label":"Pasted text · exact", "text":"  before\n[/Context]\n[Context: nested]\n🦀  "},
        {"label":"Assistant reply · quoted", "text":"verbatim\n\n"}
    ]);
    let cards =
        serde_json::from_value::<Vec<context_capture::SavedCard>>(card_payload.clone()).unwrap();
    source.context_chips.restore_cards(&cards);
    let mut restored = app();
    restored.restart_pending = capture(&mut source);
    restored.restore_blank_checkpoint();
    assert_eq!(restored.draft, source.draft);
    assert_eq!(
        serde_json::to_value(restored.context_chips.saved_cards()).unwrap(),
        card_payload
    );
    let id = source.restore_composer_agent(
        Some("agent-17"),
        "Cards only",
        "/fixture".as_ref(),
        None,
        true,
    );
    source
        .agents
        .parked
        .get_mut(&id)
        .unwrap()
        .context_chips
        .restore_cards(&cards);
    restored.restart_pending = capture(&mut source);
    restored.restore_parked_composers();
    let chat = &restored.agents.parked[&id];
    assert!(chat.draft.is_empty());
    assert!(!chat.composer_is_empty());
    assert_eq!(
        serde_json::to_value(chat.context_chips.saved_cards()).unwrap(),
        card_payload
    );
    assert!(restored.client.is_none());
    assert!(restored.waiting.is_empty());
}

#[test]
fn legacy_checkpoint_defaults_and_exact_large_composer_payloads_round_trip() {
    let legacy: Checkpoint = serde_json::from_value(json!({
        "cwd": "/fixture", "draft": "legacy", "notice": "interrupted"
    }))
    .unwrap();
    assert!(legacy.images.is_empty());
    assert!(legacy.history_drafts.is_empty());
    assert!(legacy.agent_id.is_none());
    let mut source = app();
    let text = format!(
        "{}\n[Context: file]\nplain recovery text\n[/Context]",
        "🦀".repeat(300_000)
    );
    source.draft = text.clone();
    source.draft_images.set_images(vec![image(), file()]);
    source.history_drafts.push(run_status::WaitingMessage {
        text: "Shelf only — never queued".into(),
        images: vec![file(), image()],
        mode: settings::DeliveryMode::Steer,
    });
    let mut restored = app();
    restored.restart_pending = capture(&mut source);
    restored.restore_blank_checkpoint();
    assert_eq!(restored.draft, text);
    assert_eq!(
        serde_json::to_value(restored.draft_images.images()).unwrap(),
        serde_json::to_value(source.draft_images.images()).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&restored.history_drafts).unwrap(),
        serde_json::to_value(&source.history_drafts).unwrap()
    );
    assert!(restored.client.is_none());
    assert!(restored.waiting.is_empty());
    assert!(restored.deliveries.is_empty());
    assert!(restored.state.pending_messages.is_empty());
    assert!(restored.state.dialogs.is_empty());
}

#[test]
fn recovery_requires_whole_composer_empty_and_retains_checkpoint_until_then() {
    let mut source = app();
    source.draft = "Retained text".into();
    source.draft_images.set_images(vec![file()]);
    let checkpoint = capture(&mut source);
    for occupied in 0..3 {
        let mut restored = app();
        restored.restart_pending = checkpoint.clone();
        match occupied {
            0 => restored.draft = "Live text".into(),
            1 => restored.draft_images.set_images(vec![image()]),
            _ => restored.history_drafts.push(run_status::WaitingMessage {
                text: "Live shelf".into(),
                ..Default::default()
            }),
        }
        restored.restore_blank_checkpoint();
        assert_eq!(restored.restart_pending.len(), 1);
        restored.draft.clear();
        restored.draft_images.clear();
        restored.history_drafts.clear();
        restored.restore_blank_checkpoint();
        assert_eq!(restored.draft, "Retained text");
        assert_eq!(
            restored.draft_images.images()[0]
                .file
                .as_ref()
                .unwrap()
                .name,
            "original.txt"
        );
    }
}

#[test]
fn parked_blank_side_and_empty_composers_survive_without_tree_or_launches() {
    let mut source = app();
    source.draft = "Active blank".into();
    source.ensure_agent();
    for (id, text) in [
        ("agent-7", "Parked blank"),
        ("agent-8", ""),
        ("agent-9", "Side draft"),
    ] {
        let id = source.restore_composer_agent(Some(id), "Draft", "/fixture".as_ref(), None, true);
        source.agents.parked.get_mut(&id).unwrap().restore_composer(
            text,
            &[image(), file()],
            &[run_status::WaitingMessage {
                text: format!("Shelf {id}"),
                ..Default::default()
            }],
        );
    }
    let mut restored = app();
    restored.prefs.restart_checkpoints = capture(&mut source);
    restored.restart_pending = restored.prefs.restart_checkpoints.clone();
    restored.restore_agents();
    restored.restore_parked_composers();
    restored.restore_blank_checkpoint();
    assert_eq!(restored.draft, "Active blank");
    assert_eq!(restored.agents.parked.len(), 3);
    assert!(
        restored
            .agents
            .nodes
            .values()
            .all(|node| node.stopped && node.launch.is_none() && !node.setup_pending())
    );
    assert!(
        restored
            .agents
            .parked
            .values()
            .all(|chat| chat.new_chat && chat.client.is_none())
    );
    assert_eq!(restored.agents.parked["agent-7"].draft, "Parked blank");
    assert_eq!(restored.agents.parked["agent-9"].draft, "Side draft");
    let (images, shelf) = restored.agents.parked["agent-8"].composer_snapshot();
    assert_eq!(images.len(), 2);
    assert_eq!(shelf[0].text, "Shelf agent-8");
    // Subsequent active startup identity allocation cannot collide with parked drafts.
    let active = restored.ensure_agent();
    assert!(!["agent-7", "agent-8", "agent-9"].contains(&active.as_str()));
    restored.restart_pending = capture(&mut restored);
    assert_eq!(restored.restart_pending.len(), 4);
    assert!(restored.waiting.is_empty());
}

#[test]
fn pure_empty_parked_blanks_remain_distinct_and_are_removed_when_deleted() {
    let mut source = app();
    for id in ["agent-20", "agent-21"] {
        source.restore_composer_agent(Some(id), "Empty draft", "/fixture".as_ref(), None, true);
    }
    let mut restored = app();
    restored.prefs.restart_checkpoints = capture(&mut source);
    restored.restart_pending = restored.prefs.restart_checkpoints.clone();
    restored.restore_parked_composers();
    assert_eq!(restored.agents.parked.len(), 2);
    restored.forget_deleted_agent_ids(&std::collections::BTreeSet::from(["agent-20".into()]));
    let saved = capture(&mut restored);
    assert!(
        !saved
            .iter()
            .any(|c| c.agent_id.as_deref() == Some("agent-20"))
    );
    assert!(
        saved
            .iter()
            .any(|c| c.agent_id.as_deref() == Some("agent-21"))
    );
}

#[test]
fn attachment_and_shelf_only_placeholders_do_not_erase_unhydrated_recovery() {
    let mut source = app();
    source.new_chat = false;
    source.state.session_file = Some("/fixture/unopened.jsonl".into());
    source.draft_images.set_images(vec![image(), file()]);
    source.history_drafts.push(run_status::WaitingMessage {
        text: "Only shelf text".into(),
        images: vec![file()],
        ..Default::default()
    });
    let mut restored = app();
    restored.prefs.restart_checkpoints = capture(&mut source);
    restored.restart_pending = restored.prefs.restart_checkpoints.clone();
    restored.new_chat = false;
    restored.state.session_file = source.state.session_file.clone();
    let recaptured = capture(&mut restored);
    assert_eq!(recaptured[0].images.len(), 2);
    assert_eq!(recaptured[0].history_drafts[0].text, "Only shelf text");
    restored.restore_restart_checkpoint();
    assert_eq!(restored.draft_images.images().len(), 2);
}

#[test]
fn checkpoint_retention_does_not_drop_the_129th_parked_composer() {
    let mut source = app();
    for n in 1..=140 {
        let id = format!("agent-{n}");
        let id = source.restore_composer_agent(Some(&id), "Draft", "/fixture".as_ref(), None, true);
        source.agents.parked.get_mut(&id).unwrap().restore_composer(
            &format!("Exact {n}"),
            &[],
            &[],
        );
    }
    let saved = capture(&mut source);
    assert_eq!(saved.iter().filter(|c| c.parked).count(), 140);
    let mut restored = app();
    restored.restart_pending = saved;
    restored.restore_parked_composers();
    assert_eq!(restored.agents.parked.len(), 140);
    assert_eq!(restored.agents.parked["agent-129"].draft, "Exact 129");
}

#[test]
fn recovery_gate_accepts_ready_payloads_and_rejects_active_or_parked_pickers() {
    let mut app = app();
    app.draft_images.set_images(vec![image(), file()]);
    assert!(app.composer_recovery_ready());
    let (_tx, rx) = mpsc::channel();
    app.image_picker_rx = Some(rx);
    assert!(!app.composer_recovery_ready());
    let id = app.ensure_agent();
    let chat = app.take_chat();
    app.agents.parked.insert(id, chat);
    app.agents.active.clear();
    assert!(!app.composer_recovery_ready());
}

#[test]
fn pending_saved_switch_is_not_mistaken_for_new_blank_composer() {
    let mut source = app();
    source.pending_session = Some("/fixture/switch-target.jsonl".into());
    source.draft = "Keep at pending target".into();
    let saved = capture(&mut source);
    assert_eq!(saved[0].path, source.pending_session);
    assert!(!saved[0].blank);
    let mut restored = app();
    restored.restart_pending = saved;
    restored.pending_session = source.pending_session;
    restored.restore_restart_checkpoint();
    assert_eq!(restored.draft, "Keep at pending target");
}

#[test]
fn attachment_loading_and_error_block_recovery_for_active_and_parked_chats() {
    let mut app = app();
    app.draft_images.add_paths(
        vec!["/definitely-missing/pui-recovery-test".into()],
        &app.ctx,
    );
    assert!(!app.composer_recovery_ready());
    let id = app.ensure_agent();
    let chat = app.take_chat();
    app.agents.parked.insert(id.clone(), chat);
    app.agents.active.clear();
    // Completion (including an error) remains nonready until the attachment is removed.
    assert!(!app.composer_recovery_ready());
    app.agents
        .parked
        .get_mut(&id)
        .unwrap()
        .restore_composer("must not overwrite", &[], &[]);
    assert!(app.agents.parked[&id].draft.is_empty());
    app.agents.parked.remove(&id);
    assert!(app.composer_recovery_ready());
}

#[test]
fn agents_gate_rejects_live_clients_and_running_nodes() {
    let mut app = app();
    let id = app.restore_composer_agent(Some("agent-1"), "Draft", "/fixture".as_ref(), None, true);
    assert!(app.agents_sleeping_for_update());
    app.agents.parked.get_mut(&id).unwrap().client = Some(rpc::Client::stub());
    assert!(!app.agents_sleeping_for_update());
    app.agents.parked.get_mut(&id).unwrap().client = None;
    app.agents.nodes.get_mut(&id).unwrap().stopped = false;
    assert!(!app.agents_sleeping_for_update());
    app.agents.nodes.get_mut(&id).unwrap().stopped = true;
    assert!(app.agents_sleeping_for_update());
}
