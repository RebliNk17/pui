use super::*;
use crate::app::files::tests::{app, click, frame};

pub(super) fn fixture() -> (egui::Context, Pui, Vec<String>) {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.prefs.sidebar_view.empty_groups = true;
    app.prefs.sidebar_view.sections = ["Review", "Later", "Ideas"]
        .into_iter()
        .map(|name| {
            let mut section = Section::new(name.into());
            section.collapsed = true;
            section
        })
        .collect();
    let ids = order(&app);
    let path = app.sessions[0].path.clone();
    app.prefs.chats.entry(path).or_default().section = Some(ids[0].clone());
    app.draft = "Unsent draft".into();
    app.state.busy = true;
    let (client, _, _commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app, ids)
}
pub(super) fn order(app: &Pui) -> Vec<String> {
    app.prefs
        .sidebar_view
        .sections
        .iter()
        .map(|section| section.id.clone())
        .collect()
}
fn snapshot(ctx: &egui::Context, app: &Pui, source: &str) -> Drag {
    Drag {
        source: source.into(),
        widget: ctx
            .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("section-header-id", source))))
            .unwrap(),
        owner: app.agents.active.clone(),
        root: app.prefs.cwd.clone(),
        order: order(app),
        active: true,
    }
}
pub(super) fn rect(ctx: &egui::Context, id: &str) -> egui::Rect {
    ctx.data(|data| {
        data.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new(("section-header-rects", id)))
    })
    .unwrap()
    .0
}
pub(super) fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
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
pub(super) fn start(ctx: &egui::Context, app: &mut Pui, id: &str) {
    let from = rect(ctx, id).center();
    frame(
        ctx,
        app,
        vec![egui::Event::PointerMoved(from), pointer(from, true)],
    );
    frame(
        ctx,
        app,
        vec![egui::Event::PointerMoved(from + egui::vec2(14.0, 0.0))],
    );
    assert!(
        app.section_view
            .drag
            .as_ref()
            .is_some_and(|drag| drag.active),
        "real primary header drag starts"
    );
}
pub(super) fn release(ctx: &egui::Context, app: &mut Pui, target: &str, after: bool) {
    let r = rect(ctx, target);
    let pos = egui::pos2(
        r.center().x,
        if after {
            r.bottom() - 5.0
        } else {
            r.top() + 5.0
        },
    );
    frame(ctx, app, vec![egui::Event::PointerMoved(pos)]);
    frame(
        ctx,
        app,
        vec![egui::Event::PointerMoved(pos), pointer(pos, false)],
    );
}
#[test]
fn existing_save_reopen_preserves_only_reordered_sections_and_never_pending_drag() {
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
    let (ctx, mut app, ids) = fixture();
    start(&ctx, &mut app, &ids[0]);
    release(&ctx, &mut app, &ids[2], true);
    let expected = order(&app);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    start(&ctx, &mut app, &ids[1]);
    let cwd = app.prefs.cwd.clone();
    app.demo = false;
    let mut storage = Storage::default();
    eframe::App::save(&mut app, &mut storage);
    app.demo = true;
    let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    cc.storage = Some(&storage);
    let restored = Pui::new(
        &cc,
        Options {
            demo: true,
            cwd: Some(cwd),
            ..Default::default()
        },
    );
    assert_eq!(order(&restored), expected);
    assert!(restored.section_view.drag.is_none());
    assert!(restored.section_view.drag_controls.is_empty());
    assert!(restored.client.is_none());
    assert!(restored.state.dialogs.is_empty());
}

#[test]
fn pure_reorder_inserts_before_after_without_swapping_metadata_or_wrong_identity() {
    let sections = || {
        ["a", "b", "c", "d"]
            .into_iter()
            .map(|id| Section {
                id: id.into(),
                name: id.into(),
                collapsed: true,
                expanded: false,
            })
            .collect::<Vec<_>>()
    };
    for (from, to, after, want) in [
        ("a", "c", true, vec!["b", "c", "a", "d"]),
        ("d", "b", false, vec!["a", "d", "b", "c"]),
        ("a", "b", false, vec!["a", "b", "c", "d"]),
        ("b", "a", true, vec!["a", "b", "c", "d"]),
    ] {
        let mut value = sections();
        reorder(&mut value, from, to, after);
        assert_eq!(
            value
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            want
        );
        assert!(
            value
                .iter()
                .all(|item| item.id == item.name && item.collapsed && !item.expanded)
        );
    }
    let mut value = sections();
    assert!(!reorder(&mut value, "missing", "b", false));
    assert!(!reorder(&mut value, "a", "missing", true));
    assert!(!reorder(&mut value, "a", "a", true));
    assert_eq!(value[0].id, "a");
}
#[test]
fn real_header_drag_commits_once_on_release_both_directions_preserving_chats_and_runtime() {
    let (ctx, mut app, ids) = fixture();
    let owner = app.agents.active.clone();
    let prefs = serde_json::to_value(&app.prefs.chats).unwrap();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    start(&ctx, &mut app, &ids[0]);
    assert_eq!(order(&app), ids);
    release(&ctx, &mut app, &ids[2], true);
    assert_eq!(
        order(&app),
        [ids[1].clone(), ids[2].clone(), ids[0].clone()]
    );
    assert!(app.section_view.drag.is_none());
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    start(&ctx, &mut app, &ids[0]);
    release(&ctx, &mut app, &ids[1], false);
    assert_eq!(order(&app), ids);
    assert_eq!(app.agents.active, owner);
    assert_eq!(serde_json::to_value(&app.prefs.chats).unwrap(), prefs);
    assert_eq!(app.draft, "Unsent draft");
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
    assert!(
        app.prefs
            .sidebar_view
            .sections
            .iter()
            .all(|section| section.collapsed)
    );
    click(&ctx, &mut app, rect(&ctx, &ids[0]).center());
    assert!(
        !app.prefs.sidebar_view.sections[0].collapsed,
        "ordinary click remains disclosure"
    );
}
#[test]
fn outside_release_escape_and_pointer_loss_discard_instead_of_stopping_or_replaying() {
    for cancel in 0..3 {
        let (ctx, mut app, ids) = fixture();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        start(&ctx, &mut app, &ids[0]);
        match cancel {
            0 => {
                let outside = egui::pos2(900.0, 700.0);
                frame(
                    &ctx,
                    &mut app,
                    vec![egui::Event::PointerMoved(outside), pointer(outside, false)],
                );
            }
            1 => {
                frame(
                    &ctx,
                    &mut app,
                    vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
                );
            }
            _ => {
                frame(&ctx, &mut app, vec![egui::Event::PointerGone]);
            }
        }
        assert!(app.section_view.drag.is_none());
        release(&ctx, &mut app, &ids[2], true);
        assert_eq!(order(&app), ids);
        assert!(app.state.busy);
        assert_eq!(app.draft, "Unsent draft");
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn invisible_scrolled_source_cancels_and_exact_snapshot_rejects_removed_replacement() {
    let (ctx, mut app, ids) = fixture();
    start(&ctx, &mut app, &ids[0]);
    let drag = snapshot(&ctx, &app, &ids[0]);
    app.prefs.sidebar_view.sections[0].id = uuid::Uuid::new_v4().to_string();
    assert!(!app.section_drag_current(&drag));
    frame(&ctx, &mut app, vec![]);
    assert!(app.section_view.drag.is_none());
    app.prefs.sidebar_view.sections[0].id = ids[0].clone();
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    start(&ctx, &mut app, &ids[0]);
    app.section_view.drag_controls.clear();
    app.finish_section_drag(&ctx, None);
    assert!(app.section_view.drag.is_none());
}

#[test]
fn session_root_hidden_editing_and_concurrent_order_changes_cancel_exact_transaction() {
    for changed in 0..6 {
        let (ctx, mut app, ids) = fixture();
        start(&ctx, &mut app, &ids[0]);
        match changed {
            0 => app.agents.active = "different-runtime-owner".into(),
            1 => app.prefs.cwd = app.prefs.cwd.join("other-root"),
            2 => app.prefs.hide_sidebar = true,
            3 => app.begin_section_rename(&ids[1]),
            4 => app.prefs.sidebar_view.sections.swap(1, 2),
            _ => {
                app.prefs.sidebar_view.sections.remove(0);
            }
        }
        let expected = order(&app);
        frame(&ctx, &mut app, vec![]);
        assert!(app.section_view.drag.is_none(), "change{changed}");
        assert_eq!(order(&app), expected);
    }
}
#[test]
fn pointer_opening_active_release_and_new_question_keyboard_never_send_stop_or_answer() {
    let (ctx, mut app, ids) = fixture();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let chords = || {
        vec![
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            egui::Event::Text(" ".into()),
            egui::Event::Paste("leak".into()),
        ]
    };
    let from = rect(&ctx, &ids[0]).center();
    let mut events = vec![egui::Event::PointerMoved(from), pointer(from, true)];
    events.extend(chords());
    frame(&ctx, &mut app, events);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(from + egui::vec2(14.0, 0.0))],
    );
    assert!(app.section_view.drag.is_some());
    frame(&ctx, &mut app, chords());
    assert_eq!(app.draft, "Unsent draft");
    app.state.dialogs.push_back(questions::demo_dialog());
    let pos = rect(&ctx, &ids[2]).center();
    let mut events = vec![
        egui::Event::PointerMoved(pos),
        pointer(pos, false),
        key(egui::Key::Space, egui::Modifiers::NONE),
    ];
    events.extend(chords());
    frame(&ctx, &mut app, events);
    assert_eq!(order(&app), ids);
    assert!(app.section_view.drag.is_none());
    assert_eq!(app.state.dialogs.len(), 1);
    assert_eq!(app.draft, "Unsent draft");
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
}
#[test]
fn press_is_bound_before_drag_threshold_and_foreign_drag_payload_is_not_taken() {
    let (ctx, mut app, ids) = fixture();
    let from = rect(&ctx, &ids[0]).center();
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(from), pointer(from, true)],
    );
    assert!(
        app.section_view
            .drag
            .as_ref()
            .is_some_and(|drag| !drag.active)
    );
    app.prefs.cwd = app.prefs.cwd.join("changed-before-threshold");
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(from + egui::vec2(14.0, 0.0))],
    );
    assert!(app.section_view.drag.is_none());
    release(&ctx, &mut app, &ids[2], true);
    assert_eq!(order(&app), ids);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    start(&ctx, &mut app, &ids[0]);
    egui::DragAndDrop::set_payload(&ctx, 1234u32);
    app.section_drag_inputs(&ctx);
    assert!(app.section_view.drag.is_none());
    assert_eq!(*egui::DragAndDrop::payload::<u32>(&ctx).unwrap(), 1234);
}

#[test]
fn foreign_popup_or_first_modal_has_first_refusal_without_section_reorder() {
    for modal in [false, true] {
        let (ctx, mut app, ids) = fixture();
        start(&ctx, &mut app, &ids[0]);
        if modal {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                prompt_ui::modal(ctx, egui::Id::new("foreign-section-modal"), 240.0, |ui| {
                    prompt_ui::heading(ui, "Foreign modal", "");
                });
            });
        } else {
            egui::Popup::open_id(&ctx, egui::Id::new("foreign-section-popup"));
        }
        app.section_drag_inputs(&ctx);
        assert!(app.section_view.drag.is_none());
        assert_eq!(order(&app), ids);
        if !modal {
            assert!(egui::Popup::is_id_open(
                &ctx,
                egui::Id::new("foreign-section-popup")
            ));
        }
    }
}
