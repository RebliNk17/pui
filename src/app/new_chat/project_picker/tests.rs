use super::*;

#[test]
fn worktrees_and_their_subfolders_are_hidden_but_submodules_and_projects_remain() {
    let root = std::env::temp_dir().join(format!("pui-project-picker-{}", uuid::Uuid::new_v4()));
    let main = root.join("main");
    let linked = root.join("linked");
    let submodule = root.join("submodule");
    std::fs::create_dir_all(main.join(".git/worktrees/review")).unwrap();
    std::fs::create_dir_all(linked.join("src/nested")).unwrap();
    std::fs::create_dir_all(&submodule).unwrap();
    std::fs::write(main.join(".git/worktrees/review/commondir"), "../..\n").unwrap();
    std::fs::write(
        linked.join(".git"),
        "gitdir: ../main/.git/worktrees/review\n",
    )
    .unwrap();
    std::fs::write(
        submodule.join(".git"),
        "gitdir: ../main/.git/modules/library\n",
    )
    .unwrap();
    assert!(linked_worktree(&linked));
    assert!(linked_worktree(&linked.join("src/nested")));
    assert!(!linked_worktree(&main));
    assert!(!linked_worktree(&submodule));
    assert!(!linked_worktree(&root.join("non-git")));
    assert!(linked.exists(), "Filtering never deletes a worktree");
}

#[test]
fn search_matches_all_words_in_name_and_path() {
    let path = std::path::Path::new("/Users/SomeOne/personal/Pui");
    assert!(matches(path, ""));
    assert!(matches(path, "personal pui"));
    assert!(matches(path, &"SOMEONE PUI".to_lowercase()));
    assert!(!matches(path, "pui worktree"));
}

fn frame(
    ctx: &egui::Context,
    picker: &mut Picker,
    events: Vec<egui::Event>,
    size: Vec2,
) -> (egui::FullOutput, Option<PathBuf>, bool) {
    let mut selected = None;
    let mut browse = false;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                (selected, browse) = picker.show(
                    ui,
                    "chat-a",
                    std::path::Path::new("/fixture/main"),
                    220.0,
                    vec![],
                    true,
                );
            });
        },
    );
    (output, selected, browse)
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
fn open(ctx: &egui::Context, picker: &mut Picker, size: Vec2) {
    frame(ctx, picker, vec![], size);
    picker.projects = vec![
        "/fixture/main".into(),
        "/fixture/second".into(),
        "/elsewhere/other".into(),
    ];
    picker.focus = true;
    egui::Popup::open_id(ctx, popup_id());
    for _ in 0..3 {
        frame(ctx, picker, vec![], size);
    }
}
#[test]
fn native_search_enter_escape_and_bounds_in_light_dark_narrow_zoom() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut picker = Picker::default();
            let size = Vec2::new(420.0, 520.0);
            open(&ctx, &mut picker, size);
            frame(
                &ctx,
                &mut picker,
                vec![egui::Event::Text("second".into())],
                size,
            );
            assert_eq!(picker.query, "second");
            let rect = ctx.read_response(popup_id()).unwrap().rect;
            assert!(
                ctx.screen_rect().expand(1.0).contains_rect(rect),
                "{rect:?}"
            );
            let (_, selected, browse) = frame(&ctx, &mut picker, vec![key(egui::Key::Enter)], size);
            assert_eq!(selected, Some(PathBuf::from("/fixture/second")));
            assert!(!browse);
            assert!(!egui::Popup::is_id_open(&ctx, popup_id()));
            open(&ctx, &mut picker, size);
            let (_, selected, browse) =
                frame(&ctx, &mut picker, vec![key(egui::Key::Escape)], size);
            assert!(selected.is_none() && !browse);
            assert!(!egui::Popup::is_id_open(&ctx, popup_id()));
        }
    }
}

#[test]
fn tabbed_project_row_enter_selects_focused_row_not_search_highlight() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut picker = Picker::default();
    let size = Vec2::new(600.0, 600.0);
    open(&ctx, &mut picker, size);
    frame(&ctx, &mut picker, vec![key(egui::Key::Tab)], size);
    frame(&ctx, &mut picker, vec![key(egui::Key::Tab)], size);
    let (_, selected, browse) = frame(&ctx, &mut picker, vec![key(egui::Key::Enter)], size);
    assert_eq!(selected, Some(PathBuf::from("/fixture/second")));
    assert!(!browse);
}
