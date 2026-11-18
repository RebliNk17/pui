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
    app.prefs.cwd = std::env::temp_dir().join(format!("pui-model-cache-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(app.prefs.cwd.join(".pi")).unwrap();
    app.state = State::default();
    app
}
fn cache(app: &mut Pui) {
    app.seed_model_catalog();
    app.state.models = vec![protocol::Model {
        id: "cached".into(),
        name: "Cached model".into(),
        provider: "fixture".into(),
        context_window: 200000,
        input_cost: 1.0,
        output_cost: 2.0,
    }];
    app.state.model = "cached".into();
    app.state.provider = "fixture".into();
    app.state.model_name = "Cached model".into();
    app.state.thinking = "high".into();
    app.remember_model_catalog("get_available_models");
    app.remember_model_catalog("get_state");
}

#[test]
fn cached_catalog_and_recent_hint_are_immediate_but_never_effective_selection() {
    let mut app = app();
    cache(&mut app);
    // Serialize as on cold launch, not only in-memory reuse.
    let persisted = serde_json::to_string(&app.prefs).unwrap();
    app.prefs = serde_json::from_str(&persisted).unwrap();
    app.new_session();
    assert_eq!(app.state.models.len(), 1);
    assert!(app.state.models_cached);
    assert!(app.state.model.is_empty());
    assert!(app.state.provider.is_empty());
    assert!(app.state.levels.is_empty());
    assert_eq!(
        model_picker::selected_label(&app.state),
        "Cached model · High · recent"
    );
    assert!(app.model_change_blocked().is_some());
    app.state.apply(json!({"type":"response","command":"get_state","success":true,"data":{"model":{"id":"actual","name":"Actual model","provider":"real"}}}));
    assert_eq!(model_picker::selected_label(&app.state), "Actual model");
    assert!(app.state.model_hint.is_empty());
}

#[test]
fn cache_is_scoped_to_workspace_config_and_age() {
    let mut app = app();
    cache(&mut app);
    let cwd = app.prefs.cwd.clone();
    app.prefs.cwd = cwd.join("different");
    app.state = State::default();
    app.seed_model_catalog();
    assert!(app.state.models.is_empty());
    app.prefs.cwd = cwd.clone();
    std::fs::write(cwd.join(".pi/settings.json"), "{}").unwrap();
    app.seed_model_catalog();
    assert!(app.state.models.is_empty());
    cache(&mut app);
    app.prefs.model_catalogs[0].saved = now() - MAX_AGE - 1;
    app.state = State::default();
    app.seed_model_catalog();
    assert!(app.state.models.is_empty());
}

#[test]
fn failed_refresh_keeps_cache_but_successful_empty_catalog_replaces_it() {
    let mut app = app();
    cache(&mut app);
    app.state = State::default();
    app.seed_model_catalog();
    app.state.apply(json!({"type":"response","command":"get_available_models","success":false,"error":"offline"}));
    assert!(app.state.models_cached);
    assert_eq!(app.state.models.len(), 1);
    app.state.apply(json!({"type":"response","command":"get_available_models","success":true,"data":{"models":[]}}));
    app.remember_model_catalog("get_available_models");
    assert!(!app.state.models_cached);
    app.state = State::default();
    app.seed_model_catalog();
    assert!(app.state.models.is_empty());
    assert!(app.state.models_cached);
}

#[test]
fn old_process_cannot_store_metadata_under_new_configuration() {
    let mut app = app();
    cache(&mut app);
    std::fs::write(
        app.prefs.cwd.join(".pi/settings.json"),
        "{\"defaultModel\":\"changed\"}",
    )
    .unwrap();
    app.remember_model_catalog("get_available_models");
    app.state = State::default();
    app.seed_model_catalog();
    assert!(app.state.models.is_empty());
}

#[test]
fn worker_metadata_cannot_replace_the_root_chat_hint() {
    let mut app = app();
    cache(&mut app);
    let root = app.ensure_agent();
    app.agents.nodes.get_mut(&root).unwrap().access = "read-only".into();
    app.state.model = "worker".into();
    app.state.model_name = "Worker model".into();
    app.remember_model_catalog("get_state");
    app.state = State::default();
    app.seed_model_catalog();
    assert_eq!(app.state.model_hint, "Cached model · High");
}

#[test]
fn pui_models_status_requests_refreshed_catalog() {
    let mut app = app();
    let (client, events, commands) = rpc::Client::stub_with_commands();
    app.client = Some(client);
    events
        .send(rpc::Event::Record(json!({
            "type": "extension_ui_request",
            "method": "setStatus",
            "statusKey": "pui-models",
            "statusText": "{\"refreshed\":true}"
        })))
        .unwrap();
    app.poll_rpc();
    assert_eq!(
        commands
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()["type"],
        "get_available_models"
    );
}
