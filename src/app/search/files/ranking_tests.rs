use super::tests::{app, search};
use super::ui_tests::frame;
use super::*;
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        pressed: true,
        physical_key: None,
        repeat: false,
    }
}
fn fixture() -> Pui {
    let mut app = app();
    let root = app.prefs.cwd.clone();
    for path in [
        "src/app/changes.rs",
        "src/another/candidate.rs",
        "special_anchor_code.txt",
    ] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "line one\nline two\n").unwrap();
    }
    app.draft = "Keep draft".into();
    app.state.busy = true;
    app.open_quick_files();
    app.search = "sac:2".into();
    search(&mut app);
    app
}
#[test]
fn ranked_fuzzy_worker_unicode_case_line_edits_and_safe_original_path_activation() {
    let mut app = fixture();
    assert_eq!(
        app.spotlight.hits[0].candidate.title,
        "special_anchor_code.txt:2"
    );
    assert!(
        app.spotlight
            .hits
            .iter()
            .any(|hit| hit.candidate.title == "src/app/changes.rs:2")
    );
    let cancel = app.spotlight.files.cancel.clone();
    let paths = app.spotlight.files.paths.as_ptr();
    app.search = "SAC:3".into();
    search(&mut app);
    assert!(Arc::ptr_eq(&cancel, &app.spotlight.files.cancel));
    assert_eq!(paths, app.spotlight.files.paths.as_ptr());
    let hit = app
        .spotlight
        .hits
        .iter()
        .find(|hit| hit.candidate.title == "src/app/changes.rs:3")
        .unwrap()
        .clone();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let count = app.state.messages.len();
    app.activate_search_hit(hit);
    assert!(!app.search_open);
    assert!(
        app.files
            .navigation_target()
            .unwrap()
            .ends_with("src/app/changes.rs")
    );
    assert_eq!(app.draft, "Keep draft");
    assert!(app.state.busy);
    assert_eq!(app.state.messages.len(), count);
    assert!(commands.try_recv().is_err());
}
#[test]
fn native_ranked_selection_enter_and_pointer_open_exact_visible_result_without_rescan() {
    for pointer in [false, true] {
        let mut app = fixture();
        let size = Vec2::new(1000.0, 800.0);
        for _ in 0..4 {
            frame(&mut app, size, vec![]);
        }
        let paths = app.spotlight.files.paths.as_ptr();
        let wanted = app
            .spotlight
            .hits
            .iter()
            .position(|hit| hit.candidate.title == "src/app/changes.rs:2")
            .unwrap();
        let expected = app.spotlight.hits[wanted].candidate.path.clone().unwrap();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        if pointer {
            let rect = app
                .ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("spotlight-row", wanted))))
                .unwrap();
            for pressed in [true, false] {
                frame(
                    &mut app,
                    size,
                    vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            pressed,
                            button: egui::PointerButton::Primary,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        } else {
            for _ in 0..wanted {
                frame(
                    &mut app,
                    size,
                    vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
                );
            }
            assert_eq!(app.spotlight.selected, wanted);
            assert_eq!(paths, app.spotlight.files.paths.as_ptr());
            frame(
                &mut app,
                size,
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
            );
        }
        assert!(!app.search_open);
        assert_eq!(app.files.navigation_target().as_deref(), expected.to_str());
        assert_eq!(app.draft, "Keep draft");
        assert!(app.state.busy);
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn fuzzy_results_stale_query_approval_and_root_change_cannot_authorize_hidden_old_result() {
    let mut app = fixture();
    let hit = app
        .spotlight
        .hits
        .iter()
        .find(|hit| hit.candidate.title == "src/app/changes.rs:2")
        .unwrap()
        .clone();
    app.search = "something-else".into();
    app.activate_search_hit(hit.clone());
    assert!(!app.files.is_open());
    app.search = "sac:2".into();
    search(&mut app);
    app.state.dialogs.push_back(questions::demo_dialog());
    app.activate_search_hit(hit.clone());
    assert!(!app.files.is_open());
    assert_eq!(app.state.dialogs.len(), 1);
    app.state.dialogs.clear();
    app.prefs.cwd = app.prefs.cwd.join("src");
    app.activate_search_hit(hit);
    assert!(!app.files.is_open());
}
#[test]
fn ranked_results_narrow_light_dark_zoom_keep_cached_projection_and_no_worker_timer() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let mut app = fixture();
            app.prefs.theme = mode;
            app.ctx.set_zoom_factor(zoom);
            app.state.busy = false;
            app.focus_search = false;
            app.ctx
                .memory_mut(|memory| memory.surrender_focus(egui::Id::new("spotlight-query")));
            let size = Vec2::new(380.0, 620.0);
            let paths = app.spotlight.files.paths.as_ptr();
            let hits = app.spotlight.hits.as_ptr();
            for _ in 0..100 {
                frame(&mut app, size, vec![]);
            }
            let output = frame(&mut app, size, vec![]);
            assert_eq!(paths, app.spotlight.files.paths.as_ptr());
            assert_eq!(hits, app.spotlight.hits.as_ptr());
            assert!(!app.spotlight.files.pending());
            // Query caret and other egui deadlines remain legitimate.
            // The ranking itself retains no receiver or recurring discovery.
            assert!(
                !output.viewport_output[&egui::ViewportId::ROOT]
                    .repaint_delay
                    .is_zero()
            );
            let rect = egui::AreaState::load(&app.ctx, egui::Id::new("spotlight-search"))
                .unwrap()
                .rect();
            assert!(
                rect.left() >= 0.0 && rect.right() <= size.x + 1.0 && rect.bottom() <= size.y + 1.0
            );
        }
    }
}
