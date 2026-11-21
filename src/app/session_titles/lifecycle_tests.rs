use super::tests::{app, saved, state};
use super::*;

#[test]
fn initial_title_setup_precedes_the_exact_prompt_and_never_runs_for_background_inputs() {
    let mut app = app();
    app.demo = false;
    app.connected = true;
    app.new_chat = true;
    app.ultracode_ready = true;
    app.state = state("/fixture/first.jsonl", "Untitled session");
    app.state.session_id = Some("first-id".into());
    let (client, _, commands) = rpc::Client::stub_with_commands();
    app.client = Some(client);
    app.draft = "Fix Hebrew sidebar\n[Context: log]\nTool output\n[/Context]".into();
    let original = app.draft.clone();
    app.submit(&app.ctx.clone());
    let prepare = commands.recv_timeout(Duration::from_secs(1)).unwrap();
    let prefix = "/pui-initial-title ";
    let data: Value = serde_json::from_str(
        protocol::string(&prepare, "message")
            .strip_prefix(prefix)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(data["sessionFile"], "/fixture/first.jsonl");
    assert_eq!(data["sessionId"], "first-id");
    assert_eq!(data["title"], "Fix Hebrew sidebar");
    let prompt = commands.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(prompt["message"], original, "No prompt rewriting");
    assert_eq!(
        app.state.title_request.as_deref(),
        data["initialRequest"].as_str()
    );
    assert!(commands.try_recv().is_err());
    assert!(
        app.state.title.is_none(),
        "Enqueued setup is not assignment"
    );
    app.new_chat = false;
    let before = app.state.title_request.clone();
    app.prepare_initial_title(&run_status::WaitingMessage {
        text: "Scheduled unrelated work".into(),
        ..Default::default()
    });
    assert_eq!(app.state.title_request, before);
    assert!(commands.try_recv().is_err());
}

#[test]
fn late_assignment_cannot_overwrite_manual_rename_or_target_another_session() {
    let mut app = app();
    let path = PathBuf::from("/fixture/first.jsonl");
    app.state = state(path.to_str().unwrap(), "");
    app.state.session_id = Some("first-id".into());
    app.state.title_request = Some("a".repeat(64));
    app.sessions = vec![saved(path.to_str().unwrap(), "New conversation")];
    let manual = format!("My explicit {}", "界".repeat(100));
    app.prefs.chats.entry(path.clone()).or_default().title = Some(manual.clone());
    let data = json!({"version":1,"sessionFile":path,"sessionId":"first-id","title":"Fix Hebrew sidebar","initialRequest":"a".repeat(64)});
    let status = json!({"type":"extension_ui_request","method":"setStatus","statusKey":"pui-initial-title","statusText":data.to_string()});
    app.state.apply(status.clone());
    app.update_session_title();
    assert_eq!(
        app.session_title(&app.sessions[0]),
        manual,
        "Manual titles remain uncapped"
    );
    assert_eq!(app.state.assigned_title(), Some("Fix Hebrew sidebar"));
    let prefs: sidebar::ChatPrefs =
        serde_json::from_value(serde_json::to_value(&app.prefs.chats[&path]).unwrap()).unwrap();
    assert_eq!(prefs.title.as_deref(), Some(manual.as_str()));
    app.state = state("/fixture/second.jsonl", "Second assigned name");
    app.state.session_id = Some("second-id".into());
    app.state.preserve_title("Second assigned name");
    app.state.apply(status);
    assert_eq!(app.state.assigned_title(), Some("Second assigned name"));
}

#[test]
fn unknown_compacted_history_never_assigns_from_first_remaining_user_or_summary() {
    let mut app = app();
    app.state = state("/fixture/unknown.jsonl", "");
    app.state.messages = vec![
        Message {
            role: "compaction".into(),
            text: "Summary title".into(),
            ..Default::default()
        },
        Message {
            role: "user".into(),
            text: "Later unrelated request".into(),
            ..Default::default()
        },
    ];
    app.update_session_title();
    assert!(app.state.title.is_none());
    assert!(app.state.chat_title().is_none());
    assert!(
        !app.prefs
            .chats
            .contains_key(Path::new("/fixture/unknown.jsonl"))
    );
}

#[test]
fn failed_setup_with_discovered_prompt_never_pins_an_unconfirmed_automatic_title() {
    let mut app = app();
    app.state = state("/fixture/first.jsonl", "Untitled session");
    app.state.session_id = Some("first-id".into());
    app.state.title_request = Some("a".repeat(64));
    app.state.messages.push(Message {
        role: "user".into(),
        text: "Unconfirmed request".into(),
        ..Default::default()
    });
    app.sessions = vec![saved(
        "/fixture/first.jsonl",
        "Unconfirmed request flattened by discovery",
    )];
    app.refresh_session_titles();
    assert!(app.state.assigned_title().is_none());
    assert!(
        !app.prefs
            .chats
            .contains_key(Path::new("/fixture/first.jsonl"))
    );
    app.update_session_title();
    assert!(app.state.assigned_title().is_none());
}

#[test]
fn stale_discovery_never_replaces_assigned_live_titles_and_duplicate_roots_stay_isolated() {
    let mut app = app();
    let mut first = state("/fixture/one.jsonl", "Raw metadata");
    first.preserve_title("Fix Hebrew sidebar");
    let mut other = state("/fixture/two.jsonl", "Raw metadata");
    other.preserve_title("Research browser alternatives");
    let mut cached = saved("/fixture/one.jsonl", "Stale summary");
    let mut prefs = sidebar::ChatPrefs::default();
    reconcile(&mut first, Some(&mut cached), &mut prefs);
    reconcile(&mut first.clone(), Some(&mut cached), &mut prefs);
    assert_eq!(cached.title, "Fix Hebrew sidebar");
    let mut second = saved("/fixture/two.jsonl", "Same stale summary");
    reconcile(
        &mut other,
        Some(&mut second),
        &mut sidebar::ChatPrefs::default(),
    );
    assert_eq!(second.title, "Research browser alternatives");
    app.state = first;
    app.state.apply(json!({"type":"response","command":"get_state","data":{"sessionFile":"/fixture/one.jsonl","sessionId":"id","sessionName":"Restart/changed-provider metadata"}}));
    app.update_session_title();
    assert_eq!(app.state.assigned_title(), Some("Fix Hebrew sidebar"));
}

#[test]
fn reused_switch_clears_initial_request_and_id_before_late_metadata_and_reconnect_pins_name() {
    let mut app = app();
    app.demo = false;
    app.connected = true;
    app.loading_chat = false;
    app.agents = Default::default();
    app.sync = sync::WindowSync::new_at(&app.ctx, true, None);
    app.state = state("/fixture/source.jsonl", "Source assigned name");
    app.state.session_id = Some("source-id".into());
    app.state.title_request = Some("a".repeat(64));
    app.client = Some(rpc::Client::stub());
    let mut target = saved("/fixture/target.jsonl", "Target original title");
    target.cwd = app.prefs.cwd.clone();
    app.sessions = vec![target.clone()];
    app.open_chat(target.clone());
    assert_eq!(
        app.state.session_file.as_deref(),
        Some("/fixture/target.jsonl")
    );
    assert!(app.state.session_id.is_none());
    assert!(app.state.title_request.is_none());
    app.state
        .apply(json!({"type":"session_info_changed","name":"Late source name"}));
    assert_eq!(
        app.state.chat_title().as_deref(),
        Some("Target original title")
    );
    app.sessions = vec![target];
    app.connect(&app.ctx.clone(), Some("/fixture/target.jsonl".into()));
    app.state.apply(json!({"type":"response","command":"get_state","data":{"sessionFile":"/fixture/target.jsonl","sessionId":"target-id","sessionName":"Compacted summary"}}));
    assert_eq!(
        app.state.chat_title().as_deref(),
        Some("Target original title")
    );
}
