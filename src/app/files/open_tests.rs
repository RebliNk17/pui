use super::*;

fn request(payload: Value) -> Value {
    json!({"type":"extension_ui_request","method":"input","title":"Pui open","id":"preview","placeholder":payload.to_string()})
}

fn response(commands: &Receiver<Value>) -> Value {
    let record = commands.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(record["type"], "extension_ui_response");
    assert_eq!(record["id"], "preview");
    serde_json::from_str(record["value"].as_str().unwrap()).unwrap()
}

#[test]
fn background_parent_and_child_previews_follow_only_the_owning_chat() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for child in [false, true] {
            let ctx = egui::Context::default();
            let mut app = tests::app(&ctx);
            app.demo = false; // exercise the real parked RPC polling path
            app.prefs.theme = theme;
            let owner_root = app.prefs.cwd.clone();
            fs::write(owner_root.join("preview.html"), "<h1>Preview</h1>").unwrap();
            let owner_path = owner_root.join("owner.jsonl");
            app.state.session_file = Some(owner_path.display().to_string());
            app.connected = true;
            let (client, events, commands) = rpc::Client::stub_with_commands();
            app.client = Some(client);
            let owner = app.ensure_agent();
            app.park_tree();

            let visible_root = tests::fixture();
            app.prefs.cwd = visible_root.clone();
            app.state.session_file = Some(visible_root.join("visible.jsonl").display().to_string());
            app.connected = true;
            app.client = Some(rpc::Client::stub());
            app.draft = "Keep visible draft".into();
            let visible = app.ensure_agent();
            if child {
                app.agents.nodes.get_mut(&owner).unwrap().parent = Some(visible.clone());
                app.agents
                    .nodes
                    .get_mut(&visible)
                    .unwrap()
                    .children
                    .push(owner.clone());
            } else {
                // Reproduce the report: parent requests while its child is selected.
                app.agents.nodes.get_mut(&visible).unwrap().parent = Some(owner.clone());
                app.agents
                    .nodes
                    .get_mut(&owner)
                    .unwrap()
                    .children
                    .push(visible.clone());
            }
            app.enter_session_layout();
            app.files
                .open_target(&ctx, &visible_root, "README.md")
                .unwrap();
            app.settings_open = true;
            app.inspector_panel = true;
            app.focus_composer = false;
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
            app.workspace.branch = "visible branch".into();

            // Paths are resolved before parking, and HTML/URLs cannot start CEF
            // or surrender the visible composer's focus during background polling.
            let mut targets = vec!["README.md", "preview.html"];
            if cfg!(target_os = "macos") {
                targets.push("http://localhost:3000/preview");
            }
            for target in &targets {
                events
                    .send(rpc::Event::Record(request(
                        json!({"version":1,"target":target}),
                    )))
                    .unwrap();
            }
            app.poll_other_agents();
            for _ in &targets {
                assert_eq!(response(&commands), json!({"queued":true}));
            }
            assert_eq!(app.agents.active, visible);
            assert_eq!(app.prefs.cwd, visible_root);
            assert_eq!(app.workspace.branch, "visible branch");
            assert_eq!(app.draft, "Keep visible draft");
            assert!(app.settings_open && app.inspector_panel);
            assert!(!app.focus_composer);
            assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new("composer"))));
            assert_eq!(app.files.selected, Some(visible_root.join("README.md")));
            assert_eq!(app.files.tabs.len(), 1);
            assert!(app.files.urls.is_empty());
            assert!(app.preview_opens.is_empty());

            // Other background callbacks must not present the queued preview.
            app.with_live_chat(&owner_path, |app| {
                app.open_queued_previews();
                assert_eq!(app.preview_opens.len(), targets.len());
            })
            .unwrap();
            app.poll_other_agents();
            assert_eq!(app.files.tabs.len(), 1);
            app.view_agent(&owner);
            assert!(app.preview_opens.is_empty());
            assert!(app.files.open);
            assert_eq!(app.files.root, owner_root);
            assert!(
                app.files
                    .tabs
                    .iter()
                    .any(|tab| tab.path == owner_root.join("README.md"))
            );
            assert!(
                app.files
                    .tabs
                    .iter()
                    .any(|tab| tab.path == owner_root.join("preview.html"))
            );
            if cfg!(target_os = "macos") {
                assert_eq!(
                    app.files.selected_url.as_deref(),
                    Some("http://localhost:3000/preview")
                );
            }
            assert!(!app.settings_open && !app.inspector_panel);
            assert!(commands.try_recv().is_err()); // one acknowledgment, not a second tool response
            // Closing the preview and switching away/back never replays the queue.
            app.files.open = false;
            app.view_agent(&visible);
            assert_eq!(app.files.selected, Some(visible_root.join("README.md")));
            assert_eq!(app.draft, "Keep visible draft");
            app.view_agent(&owner);
            assert!(!app.files.open);
        }
    }
}

#[test]
fn background_preview_validation_is_immediate_and_queue_is_bounded() {
    let ctx = egui::Context::default();
    let mut app = tests::app(&ctx);
    let (client, _, commands) = rpc::Client::stub_with_commands();
    app.client = Some(client);
    app.agents.background = true;
    for payload in [
        json!({"version":2,"target":"README.md"}),
        json!({"version":1}),
        json!({"version":1,"target":17}),
        json!({"version":1,"target":""}),
        json!({"version":1,"target":"../secret.md"}),
        json!({"version":1,"target":".env"}),
        json!({"version":1,"target":"https://user:pass@example.com"}),
        json!({"version":1,"target":"javascript:alert(1)"}),
        json!({"version":1,"target":"bad\npath"}),
        json!({"version":1,"target":"x".repeat(8193)}),
    ] {
        assert!(app.open_request(&request(payload)));
        assert!(response(&commands)["error"].is_string());
        assert!(app.preview_opens.is_empty());
    }
    let mut malformed = request(json!({}));
    malformed["placeholder"] = json!("not JSON");
    assert!(app.open_request(&malformed));
    assert_eq!(response(&commands)["error"], "Invalid open request.");
    for i in 0..MAX_TABS {
        assert!(app.open_request(&request(
            json!({"version":1,"target":format!("report-{i}.md")})
        )));
        assert_eq!(response(&commands), json!({"queued":true}));
    }
    assert!(app.open_request(&request(json!({"version":1,"target":"report-0.md"}))));
    assert_eq!(response(&commands), json!({"queued":true}));
    assert_eq!(app.preview_opens.len(), MAX_TABS);
    assert!(app.open_request(&request(json!({"version":1,"target":"overflow.md"}))));
    assert!(
        response(&commands)["error"]
            .as_str()
            .unwrap()
            .contains("16 queued")
    );
    assert_eq!(app.preview_opens.len(), MAX_TABS);
    assert!(!app.files.open);
    assert!(app.state.dialogs.is_empty());
}

#[test]
fn foreground_preview_still_acknowledges_opening_not_queued() {
    let ctx = egui::Context::default();
    let mut app = tests::app(&ctx);
    let (client, _, commands) = rpc::Client::stub_with_commands();
    app.client = Some(client);
    assert!(app.open_request(&request(json!({"version":1,"target":"README.md"}))));
    assert_eq!(response(&commands), json!({"opening":true}));
    assert!(app.preview_opens.is_empty());
    assert!(app.files.open);
}
