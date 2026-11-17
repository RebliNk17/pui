use super::*;

#[derive(Default)]
struct Storage(std::collections::HashMap<String, String>);
impl eframe::Storage for Storage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.into(), value);
    }
    fn flush(&mut self) {}
}

fn metadata(path: &Path) -> Metadata {
    Metadata {
        path: path.into(),
        cwd: std::env::temp_dir(),
        revision: uuid::Uuid::new_v4().to_string(),
        kind: Kind::Completed,
        at: now(),
        read: false,
    }
}
fn fixture() -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("pui-attention-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("שלום chat.jsonl");
    std::fs::write(&path, b"{\"type\":\"session\",\"cwd\":\"/tmp\"}\n").unwrap();
    (root, path)
}
pub(super) fn app(ctx: &egui::Context) -> Pui {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.state = State::default();
    app.agents = Default::default();
    app.attention = Default::default();
    app.attention_observed = Default::default();
    app.connected = true;
    app.loading_chat = false;
    app.prefs.cwd = std::env::temp_dir();
    app.client = Some(rpc::Client::stub());
    app
}
fn rendered(app: &mut Pui, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| app.attention_rendered(ui));
}
pub(super) fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
pub(super) fn frame(app: &mut Pui, events: Vec<egui::Event>, size: Vec2) -> egui::FullOutput {
    use eframe::App;
    let ctx = app.ctx.clone();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            app.update(ctx, &mut eframe::Frame::_new_kittest());
        },
    )
}

#[test]
fn lifecycle_is_observed_once_and_retries_stops_and_history_are_not_guessed() {
    let mut observed = Observed::default();
    assert_eq!(observed.event(&json!({"type":"agent_settled"})), None);
    observed.event(&json!({"type":"agent_start"}));
    observed
        .event(&json!({"type":"message_end","message":{"role":"assistant","stopReason":"error"}}));
    observed
        .event(&json!({"type":"message_end","message":{"role":"assistant","stopReason":"stop"}}));
    assert_eq!(
        observed.event(&json!({"type":"agent_settled"})),
        Some(Kind::Completed)
    );
    assert_eq!(observed.event(&json!({"type":"agent_settled"})), None);
    observed.event(&json!({"type":"agent_start"}));
    observed.cancel();
    assert!(!observed.failure());
    assert_eq!(observed.event(&json!({"type":"agent_settled"})), None);
    observed.event(&json!({"type":"agent_start"}));
    observed
        .event(&json!({"type":"message_end","message":{"role":"assistant","stopReason":"error"}}));
    assert_eq!(
        observed.event(&json!({"type":"agent_settled"})),
        Some(Kind::Failed)
    );
    observed.event(&json!({"type":"agent_start"}));
    assert!(observed.failure());
    assert!(!observed.failure());
    assert_eq!(observed.event(&json!({"type":"agent_settled"})), None);
}

#[test]
fn accepted_requests_are_deduplicated_without_retaining_payloads() {
    let mut observed = Observed::default();
    let request = json!({"type":"extension_ui_request","method":"input","id":"request-1","title":"SECRET","placeholder":"APPROVAL PAYLOAD"});
    assert!(observed.request(&request));
    assert!(!observed.request(&request));
    for n in 0..500 {
        observed.request(
            &json!({"type":"extension_ui_request","method":"confirm","id":format!("request-{n}")}),
        );
    }
    assert_eq!(observed.requests.len(), MAX_RECORDS);
    assert!(
        !observed
            .request(&json!({"type":"extension_ui_request","method":"setStatus","id":"status"}))
    );
    assert!(!observed.request(
        &json!({"type":"extension_ui_request","method":"input","id":"x".repeat(MAX_KEY_BYTES + 1)})
    ));
}

#[test]
fn bounded_restart_metadata_validates_timestamps_keys_and_deletion() {
    let (root, path) = fixture();
    let mut attention = Attention::default();
    for n in 0..400 {
        attention.observe(
            Owner::Saved(root.join(format!("chat-{n}.jsonl"))),
            &root,
            Kind::Completed,
        );
    }
    assert_eq!(attention.entries.len(), MAX_RECORDS);
    attention.observe(Owner::Saved(path.clone()), &root, Kind::NeedsInput);
    let persisted = attention.persisted();
    assert_eq!(persisted.len(), MAX_RECORDS);
    assert!(serde_json::to_vec(&persisted).unwrap().len() < 256 * (2 * MAX_KEY_BYTES + 256));
    let encoded = serde_json::to_string(&persisted).unwrap();
    assert!(!encoded.contains("title"));
    assert!(!encoded.contains("message"));
    assert!(!encoded.contains("request"));
    let restored = Attention::restore(serde_json::from_str(&encoded).unwrap());
    assert_eq!(restored.unread(), MAX_RECORDS);
    assert_eq!(restored.entries[0].metadata.revision, persisted[0].revision);
    for invalid in [0, now() + 1, now() - MAX_AGE_SECONDS - 1] {
        let mut record = metadata(&path);
        record.at = invalid;
        assert_eq!(Attention::restore(vec![record]).unread(), 0);
    }
    let mut record = metadata(&path);
    record.path = PathBuf::from(format!("/{}", "x".repeat(MAX_KEY_BYTES)));
    let invalid: Metadata = serde_json::from_value(serde_json::to_value(record).unwrap()).unwrap();
    assert!(!valid_metadata(&invalid, now()));
    let mut restored = Attention::restore(vec![metadata(&path)]);
    restored.forget_path(&path);
    assert_eq!(restored.unread(), 0);
    // Restore never stats files; explicit deletion/Open removes stale owners.
    let stale = metadata(&root.join("missing.jsonl"));
    assert_eq!(Attention::restore(vec![stale]).unread(), 1);
    let records = json!((0..700).map(|_| metadata(&path)).collect::<Vec<_>>());
    let stored: Stored = serde_json::from_value(records).unwrap();
    assert_eq!(stored.0.len(), MAX_RECORDS);
}

#[test]
fn canonical_identity_and_runtime_promotion_keep_revision_and_read_watermark() {
    let (root, path) = fixture();
    let alias = root.join(".").join(path.file_name().unwrap());
    let mut attention = Attention::default();
    attention.observe(Owner::Runtime("agent-42".into()), &root, Kind::NeedsInput);
    assert!(attention.persisted().is_empty());
    let revision = attention.entries[0].metadata.revision.clone();
    attention.promote("agent-42", &alias);
    assert_eq!(attention.entries.len(), 1);
    assert_eq!(attention.entries[0].metadata.revision, revision);
    attention.mark_read(&Owner::Saved(lexical_identity(&path)));
    let mut snapshot = attention.metadata(&path).unwrap();
    snapshot.read = false;
    attention.import_for(&path, snapshot.clone());
    assert_eq!(attention.unread(), 0);
    attention.import_for(&root.join("other.jsonl"), metadata(&path));
    assert_eq!(attention.entries.len(), 1);
    snapshot.revision = uuid::Uuid::new_v4().to_string();
    attention.import_for(&alias, snapshot);
    assert_eq!(attention.entries.len(), 1);
    assert_eq!(attention.unread(), 1);
    attention.forget_path(&alias);
    assert!(attention.entries.is_empty());
}

#[test]
fn real_background_events_keep_exact_owner_and_never_scan_history_or_status() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (_root, path) = fixture();
    app.state.session_file = Some(path.display().to_string());
    let (client, events) = rpc::Client::stub_with_events();
    app.client = Some(client);
    let original = app.ensure_agent();
    events.send(rpc::Event::History(vec![])).unwrap();
    events
        .send(rpc::Event::Record(json!({"type":"agent_start"})))
        .unwrap();
    app.poll_rpc();
    assert_eq!(app.attention.unread(), 0);
    app.draft = "original draft".into();
    app.park_tree();
    app.state = State::default();
    app.ensure_agent();
    let active = app.agents.active.clone();
    events
        .send(rpc::Event::Record(json!({"type":"agent_settled"})))
        .unwrap();
    events
        .send(rpc::Event::Record(json!({"type":"agent_settled"})))
        .unwrap();
    app.poll_other_agents();
    assert_eq!(app.agents.active, active);
    assert_eq!(app.attention.unread(), 1);
    assert_eq!(
        app.attention.entries[0].owner,
        Owner::Saved(lexical_identity(&path))
    );
    let revision = app.attention.entries[0].metadata.revision.clone();
    app.poll_other_agents();
    assert_eq!(app.attention.entries[0].metadata.revision, revision);
    assert_eq!(app.agents.parked[&original].draft, "original draft");
    app.shutdown_agents();
}

#[test]
fn background_ask_internal_control_and_failure_events_are_observed_precisely() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (client, events) = rpc::Client::stub_with_events();
    app.client = Some(client);
    let original = app.ensure_agent();
    app.park_tree();
    app.ensure_agent();
    events.send(rpc::Event::Record(json!({"type":"extension_ui_request","method":"input","title":"Pui steering window","id":"internal"}))).unwrap();
    app.poll_other_agents();
    assert_eq!(app.attention.unread(), 0);
    let request = json!({"type":"extension_ui_request","method":"confirm","id":"ask","title":"SECRET","message":"APPROVAL DATA"});
    events.send(rpc::Event::Record(request.clone())).unwrap();
    app.poll_other_agents();
    assert_eq!(app.attention.entries[0].metadata.kind, Kind::NeedsInput);
    let revision = app.attention.entries[0].metadata.revision.clone();
    events.send(rpc::Event::Record(request)).unwrap();
    app.poll_other_agents();
    assert_eq!(app.attention.entries[0].metadata.revision, revision);
    events
        .send(rpc::Event::Record(json!({"type":"agent_start"})))
        .unwrap();
    events
        .send(rpc::Event::Error("secret error must not persist".into()))
        .unwrap();
    events.send(rpc::Event::Exited).unwrap();
    app.poll_other_agents();
    assert_eq!(app.attention.entries[0].owner, Owner::Runtime(original));
    assert_eq!(app.attention.entries[0].metadata.kind, Kind::Failed);
    assert_eq!(app.attention.entries.len(), 1);
    app.shutdown_agents();
}

#[test]
fn selected_and_unfocused_chats_are_unread_until_actually_rendered() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.ensure_agent();
    app.observe_attention(Kind::Completed);
    let original = app.agents.active.clone();
    let entry = app.attention.entries[0].clone();
    app.open_attention(&entry);
    assert_eq!(app.agents.active, original);
    assert_eq!(app.attention.unread(), 1);
    let _ = ctx.run(
        egui::RawInput {
            focused: false,
            ..Default::default()
        },
        |ctx| rendered(&mut app, ctx),
    );
    assert_eq!(app.attention.unread(), 1);
    let _ = ctx.run(
        egui::RawInput {
            focused: true,
            ..Default::default()
        },
        |ctx| rendered(&mut app, ctx),
    );
    assert_eq!(app.attention.unread(), 0);
    app.observe_attention(Kind::NeedsInput);
    app.attention.mark_all_read();
    assert_eq!(app.attention.unread(), 0);
}

#[test]
fn opening_exact_parked_owner_preserves_draft_context_and_never_answers_or_stops() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let owner = app.ensure_agent();
    app.state.busy = true;
    app.state.dialogs.push_back(json!({"type":"extension_ui_request","method":"confirm","id":"approve","title":"Allow bash?","message":"{}\n\nApprove this call only."}));
    app.observe_attention(Kind::NeedsInput);
    let entry = app.attention.entries[0].clone();
    app.draft = "owner draft".into();
    app.park_tree();
    app.ensure_agent();
    app.draft = "source draft".into();
    let source = app.agents.active.clone();
    app.open_attention(&entry);
    assert_eq!(app.agents.active, owner);
    assert_eq!(app.draft, "owner draft");
    assert_eq!(app.agents.parked[&source].draft, "source draft");
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy);
    assert!(commands.try_iter().next().is_none());
    assert_eq!(app.attention.unread(), 1);
    app.shutdown_agents();
}

#[test]
fn full_app_inbox_chords_never_approve_send_or_stop_in_light_dark_narrow_zoom() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for size in [egui::vec2(440.0, 520.0), egui::vec2(1040.0, 720.0)] {
            for (key_code, modifiers) in [
                (egui::Key::Enter, egui::Modifiers::COMMAND),
                (egui::Key::Backspace, egui::Modifiers::COMMAND),
                (egui::Key::Escape, egui::Modifiers::NONE),
            ] {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                app.prefs.theme = theme;
                ctx.set_zoom_factor(1.25);
                let (client, _events, commands) = rpc::Client::stub_with_records();
                app.client = Some(client);
                app.ensure_agent();
                app.state.busy = true;
                app.draft = "never send me".into();
                app.state.dialogs.push_back(json!({"type":"extension_ui_request","method":"confirm","id":"approve","title":"Allow bash?","message":"{}\n\nApprove this call only."}));
                app.observe_attention(Kind::NeedsInput);
                app.attention.open = true;
                app.attention.focus_close = true;
                frame(&mut app, vec![], size);
                frame(&mut app, vec![key(key_code, modifiers)], size);
                assert_eq!(app.state.dialogs.len(), 1);
                assert_eq!(app.draft, "never send me");
                assert!(app.state.busy);
                assert!(!commands.try_iter().any(|c| matches!(
                    protocol::string(&c, "type"),
                    "extension_ui_response" | "prompt" | "abort"
                )));
                if key_code == egui::Key::Escape {
                    assert!(!app.attention.open);
                }
                app.shutdown_agents();
            }
        }
    }
}

#[test]
fn ron_round_trip_and_corrupt_attention_do_not_reset_main_preferences() {
    let (_root, path) = fixture();
    let mut storage = Storage::default();
    let prefs = Preferences {
        theme: theme::Theme::Light,
        hide_sidebar: true,
        ..Default::default()
    };
    eframe::set_value(&mut storage, "preferences", &prefs);
    eframe::set_value(&mut storage, "attention-v1", &Stored(vec![metadata(&path)]));
    let records: Stored = eframe::get_value(&storage, "attention-v1").unwrap();
    assert_eq!(records.0.len(), 1);
    storage.0.insert("attention-v1".into(), "[broken]".into());
    let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    cc.storage = Some(&storage);
    let app = Pui::new(
        &cc,
        Options {
            demo: true,
            ..Default::default()
        },
    );
    assert!(matches!(app.prefs.theme, theme::Theme::Light));
    let persisted_preferences: Preferences = eframe::get_value(&storage, "preferences").unwrap();
    assert!(persisted_preferences.hide_sidebar);
    assert_eq!(app.attention.unread(), 0);
    let mut bad = serde_json::to_value(metadata(&path)).unwrap();
    bad["path"] = json!("x".repeat(MAX_KEY_BYTES + 1));
    let records: Stored = serde_json::from_value(json!([bad, metadata(&path)])).unwrap();
    assert_eq!(records.0.len(), 1);
    let mut unknown = serde_json::to_value(metadata(&path)).unwrap();
    unknown["unknown_body"] = json!({"secret": "never retained"});
    let record: Metadata = serde_json::from_value(unknown).unwrap();
    assert!(valid_metadata(&record, now()));
    assert!(!serde_json::to_string(&record).unwrap().contains("secret"));
}

#[test]
fn completion_behind_confirmations_is_not_read_until_visible() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.ensure_agent();
    for blocker in 0..3 {
        app.observe_attention(Kind::Completed);
        app.agents.close_prompt = blocker == 0;
        app.menu.about = blocker == 1;
        app.updates.open = blocker == 2;
        let _ = ctx.run(egui::RawInput::default(), |ctx| rendered(&mut app, ctx));
        assert_eq!(app.attention.unread(), 1);
        app.agents.close_prompt = false;
        app.menu.about = false;
        app.updates.open = false;
        let _ = ctx.run(egui::RawInput::default(), |ctx| rendered(&mut app, ctx));
        assert_eq!(app.attention.unread(), 0);
    }
    app.observe_attention(Kind::Completed);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("obscures-chat"), 200.0, |ui| {
            prompt_ui::heading(ui, "Confirm", "")
        });
        rendered(&mut app, ctx);
    });
    assert_eq!(app.attention.unread(), 1);
}

#[test]
fn native_tab_trigger_activation_and_immediate_escape_are_atomic() {
    let size = egui::vec2(860.0, 650.0);
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.ensure_agent();
        app.state.busy = true;
        app.draft = "unchanged".into();
        app.focus_composer = false;
        let (client, _events, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        frame(&mut app, vec![], size);
        let id = ctx
            .data(|d| d.get_temp::<egui::Id>(egui::Id::new("attention-button-id")))
            .unwrap();
        let mut focused = false;
        for _ in 0..80 {
            frame(
                &mut app,
                vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
                size,
            );
            if ctx.memory(|m| m.has_focus(id)) {
                focused = true;
                break;
            }
        }
        assert!(focused, "native Tab must reach Catch up");
        let mut events = vec![key(activation, egui::Modifiers::NONE)];
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        frame(&mut app, events, size);
        assert!(
            app.attention.open,
            "opening Enter/Space must not also press Done"
        );
        assert_eq!(app.draft, "unchanged");
        frame(
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            size,
        );
        assert!(!app.attention.open);
        assert!(
            ctx.memory(|m| m.has_focus(id)),
            "Done/Escape returns trigger focus"
        );
        assert!(app.state.busy);
        assert!(!commands.try_iter().any(|c| matches!(
            protocol::string(&c, "type"),
            "prompt" | "abort" | "extension_ui_response"
        )));
        app.shutdown_agents();
    }
}

#[test]
fn missing_explicit_owner_is_cleared_without_retargeting_or_rpc() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let current = app.ensure_agent();
    let path =
        std::env::temp_dir().join(format!("missing-attention-{}.jsonl", uuid::Uuid::new_v4()));
    app.attention.import(metadata(&path));
    let entry = app.attention.entries[0].clone();
    app.open_attention(&entry);
    assert_eq!(app.agents.active, current);
    assert_eq!(app.attention.unread(), 0);
    assert!(
        app.state
            .notice
            .as_deref()
            .unwrap()
            .contains("no longer available")
    );
}

pub(super) fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
#[test]
fn pointer_open_with_approval_chord_cannot_answer_the_selected_owner() {
    let size = egui::vec2(860.0, 650.0);
    for chord in [egui::Key::Enter, egui::Key::Backspace, egui::Key::Escape] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let (client, _events, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let owner = app.ensure_agent();
        app.state.busy = true;
        app.draft = "owner draft".into();
        app.state.dialogs.push_back(json!({"type":"extension_ui_request","method":"confirm","id":"approval","title":"Allow bash?","message":"{}\n\nApprove this call only."}));
        app.observe_attention(Kind::NeedsInput);
        app.park_tree();
        app.ensure_agent();
        app.focus_composer = false;
        app.draft = "source draft".into();
        app.attention.open = true;
        app.attention.focus_close = true;
        frame(&mut app, vec![], size);
        frame(&mut app, vec![], size);
        let rect = ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("attention-first-row")))
            .unwrap();
        frame(&mut app, pointer(rect.center(), true), size);
        let mut events = pointer(rect.center(), false);
        events.push(key(
            chord,
            if chord == egui::Key::Escape {
                egui::Modifiers::NONE
            } else {
                egui::Modifiers::COMMAND
            },
        ));
        frame(&mut app, events, size);
        if chord != egui::Key::Escape {
            assert_eq!(app.agents.active, owner);
        }
        if app.agents.active == owner {
            assert_eq!(app.state.dialogs.len(), 1);
            assert_eq!(app.draft, "owner draft");
            assert!(app.state.busy);
        }
        assert!(!commands.try_iter().any(|c| matches!(
            protocol::string(&c, "type"),
            "extension_ui_response" | "prompt" | "abort"
        )));
        app.shutdown_agents();
    }
}
#[test]
fn closed_tab_focused_trigger_escape_is_not_stop_or_deny() {
    let size = egui::vec2(860.0, 650.0);
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.ensure_agent();
    app.state.busy = true;
    app.focus_composer = false;
    app.draft = "retained".into();
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    frame(&mut app, vec![], size);
    let id = ctx
        .data(|d| d.get_temp::<egui::Id>(egui::Id::new("attention-button-id")))
        .unwrap();
    for _ in 0..80 {
        frame(
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
            size,
        );
        if ctx.memory(|m| m.has_focus(id)) {
            break;
        }
    }
    assert!(ctx.memory(|m| m.has_focus(id)));
    frame(
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        size,
    );
    assert!(app.state.busy);
    assert_eq!(app.draft, "retained");
    assert!(
        !commands
            .try_iter()
            .any(|c| protocol::string(&c, "type") == "abort")
    );
    app.shutdown_agents();
}

#[test]
fn reverse_tab_target_escape_is_reserved_before_deferred_focus_is_cleared() {
    let size = egui::vec2(860.0, 650.0);
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.ensure_agent();
    app.state.busy = true;
    app.focus_composer = false;
    app.state.dialogs.push_back(json!({"type":"extension_ui_request","method":"confirm","id":"approval","title":"Allow bash?","message":"{}\n\nApprove this call only."}));
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    frame(&mut app, vec![], size);
    let next = ctx
        .data(|d| d.get_temp::<egui::Id>(egui::Id::new("attention-next-button")))
        .unwrap();
    let trigger = ctx
        .data(|d| d.get_temp::<egui::Id>(egui::Id::new("attention-button-id")))
        .unwrap();
    for _ in 0..80 {
        frame(
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
            size,
        );
        if ctx.memory(|m| m.has_focus(next)) {
            break;
        }
    }
    assert!(ctx.memory(|m| m.has_focus(next)));
    frame(
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
        size,
    );
    frame(
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        size,
    );
    assert!(ctx.memory(|m| m.has_focus(trigger)));
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy);
    assert!(!commands.try_iter().any(|c| matches!(
        protocol::string(&c, "type"),
        "abort" | "extension_ui_response"
    )));
    app.shutdown_agents();
}

#[test]
fn closed_trigger_press_and_release_chords_do_not_send_answer_or_stop() {
    let size = egui::vec2(860.0, 650.0);
    for pressed in [true, false] {
        for chord in [egui::Key::Escape, egui::Key::Enter, egui::Key::Backspace] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.ensure_agent();
            app.state.busy = true;
            app.draft = "keep draft".into();
            app.focus_composer = true;
            app.state.dialogs.push_back(json!({"type":"extension_ui_request","method":"confirm","id":"approval","title":"Allow bash?","message":"{}\n\nApprove this call only."}));
            let (client, _events, commands) = rpc::Client::stub_with_records();
            app.client = Some(client);
            frame(&mut app, vec![], size);
            frame(&mut app, vec![], size);
            let rect = ctx
                .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("attention-button")))
                .unwrap();
            if !pressed {
                frame(&mut app, pointer(rect.center(), true), size);
            }
            let mut events = pointer(rect.center(), pressed);
            events.push(key(
                chord,
                if chord == egui::Key::Escape {
                    egui::Modifiers::NONE
                } else {
                    egui::Modifiers::COMMAND
                },
            ));
            frame(&mut app, events, size);
            assert_eq!(app.state.dialogs.len(), 1);
            assert_eq!(app.draft, "keep draft");
            assert!(app.state.busy);
            assert!(!commands.try_iter().any(|c| matches!(
                protocol::string(&c, "type"),
                "prompt" | "abort" | "extension_ui_response"
            )));
            app.shutdown_agents();
        }
    }
}

#[test]
fn foreign_popup_and_modal_keep_trigger_keys_and_pointer_chords() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.ensure_agent();
    let size = egui::vec2(860.0, 650.0);
    app.focus_composer = false;
    app.draft = "retained".into();
    frame(&mut app, vec![], size);
    let id = ctx
        .data(|d| d.get_temp::<egui::Id>(egui::Id::new("attention-button-id")))
        .unwrap();
    let rect = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("attention-button")))
        .unwrap();
    for _ in 0..80 {
        frame(
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
            size,
        );
        if ctx.memory(|m| m.has_focus(id)) {
            break;
        }
    }
    assert!(ctx.memory(|m| m.has_focus(id)));
    let popup = egui::Id::new("foreign-popup");
    egui::Popup::open_id(&ctx, popup);
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            ..Default::default()
        },
        |ctx| {
            app.attention_inputs(ctx);
            assert!(
                ctx.input(|i| i.key_pressed(egui::Key::Escape)),
                "Catch up must not consume foreign popup cancellation"
            );
            egui::Popup::close_id(ctx, popup);
        },
    );
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
            ..Default::default()
        },
        |ctx| {
            egui::Popup::open_id(ctx, popup);
            egui::CentralPanel::default().show(ctx, |ui| app.attention_button(ui));
            assert!(!app.attention.open);
            egui::Popup::close_id(ctx, popup);
        },
    );
    app.menu.about = true;
    let mut events = pointer(rect.center(), true);
    events.push(key(egui::Key::Enter, egui::Modifiers::COMMAND));
    let _ = ctx.run(
        egui::RawInput {
            events,
            ..Default::default()
        },
        |ctx| {
            app.attention_inputs(ctx);
            assert!(
                ctx.input(|i| i.key_pressed(egui::Key::Enter)),
                "hidden trigger must not steal foreign-modal keys"
            );
        },
    );
    assert!(!app.attention.open);
}

#[test]
fn attention_adds_no_idle_repaint_or_polling_work() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.ensure_agent();
    app.observe_attention(Kind::Completed);
    for _ in 0..4 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            app.attention_inputs(ctx);
            app.attention_panel(ctx);
        });
    }
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        app.attention_inputs(ctx);
        app.attention_panel(ctx);
    });
    assert_eq!(
        output.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
        Duration::MAX
    );
}

#[test]
fn demo_attention_opens_production_modal_with_only_runtime_synthetic_records() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_attention: true,
            ..Default::default()
        },
    );
    assert!(app.demo && app.attention.open);
    assert_eq!(app.attention.entries.len(), 2);
    assert!(app.attention.entries.iter().all(|e| matches!(e.owner, Owner::Runtime(_)) && e.metadata.path.as_os_str().is_empty()));
    assert!(app.attention.persisted().is_empty());
    assert!(app.state.session_file.is_none() && app.state.dialogs.is_empty());
    assert!(
        app.client.is_none()
            && app
                .agents
                .parked
                .values()
                .all(|c| c.client.is_none() && c.state.dialogs.is_empty())
    );
    frame(&mut app, vec![], egui::vec2(860.0, 650.0));
    frame(
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
        egui::vec2(860.0, 650.0),
    );
    assert!(app.attention.open && app.state.dialogs.is_empty());
    frame(
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        egui::vec2(860.0, 650.0),
    );
    assert!(!app.attention.open);
}
