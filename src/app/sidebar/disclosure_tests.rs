use super::*;
use crate::app::files::tests as file_tests;

fn fixture() -> Pui {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    app.sessions.clear();
    app.state = State {
        session_file: Some("/fixture/disclosure.jsonl".into()),
        ..Default::default()
    };
    app.demo_agent_tree();
    let root = app.agents.active.clone();
    for node in app.agents.nodes.values_mut() {
        node.status = "Idle".into();
    }
    app.agents.nodes.get_mut(&root).unwrap().status = "Working…".into();
    app.projects = false;
    app.prefs.sidebar_view.group = sidebar_view::Group::None;
    app
}
fn render(app: &mut Pui, events: Vec<egui::Event>) -> egui::FullOutput {
    app.ctx.clone().run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(900.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| app.sidebar(ctx),
    )
}
fn rects(app: &Pui) -> (String, egui::Rect, egui::Rect) {
    let row = sidebar_tree::forest(app).remove(0);
    let rect = |name| {
        app.ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new((name, &row.key))))
            .unwrap()
    };
    (
        row.key.clone(),
        rect("sidebar-row-rect"),
        rect("sidebar-disclosure-rect"),
    )
}
fn chevron(out: &egui::FullOutput, rect: egui::Rect) -> bool {
    out.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Path(p)if p.points.len()==3&&p.points.iter().all(|point|rect.contains(*point))))
}
fn ring(out: &egui::FullOutput, rect: egui::Rect) -> bool {
    out.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Path(p)if p.points.len()==17&&p.points.iter().all(|point|rect.contains(*point))))
}
fn click(app: &mut Pui, pos: egui::Pos2) {
    for pressed in [true, false] {
        render(
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn hovering_whole_parent_swaps_ring_for_left_chevron_without_layout_shift_and_restores_on_exit() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            let mut app = fixture();
            app.prefs.theme = mode;
            app.ctx.set_zoom_factor(zoom);
            for _ in 0..4 {
                render(&mut app, vec![]);
            }
            let (_, row, toggle) = rects(&app);
            assert!((toggle.center().x - row.left() - 8.0).abs() < 0.1);
            let out = render(&mut app, vec![]);
            assert!(ring(&out, toggle));
            assert!(!chevron(&out, toggle));
            let title = row.left_center() + egui::vec2(65.0, 0.0);
            let out = render(&mut app, vec![egui::Event::PointerMoved(title)]);
            assert!(chevron(&out, toggle));
            assert!(!ring(&out, toggle));
            assert_eq!(rects(&app).1, row);
            render(&mut app, vec![egui::Event::PointerGone]);
            let out = render(&mut app, vec![]);
            assert!(ring(&out, toggle));
            assert!(!chevron(&out, toggle));
        }
    }
}

#[test]
fn left_toggle_keyboard_and_right_overflow_keep_ownership_without_approving_sending_or_stopping() {
    let mut app = fixture();
    app.demo = false;
    app.state.busy = true;
    app.draft = "Keep draft".into();
    app.state.dialogs.push_back(json!({"id":"disclosure-confirm","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."}));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    for _ in 0..4 {
        file_tests::frame(&app.ctx.clone(), &mut app, vec![]);
    }
    let root = app.agents.active.clone();
    let (key, row, toggle) = rects(&app);
    let ctx = app.ctx.clone();
    for pressed in [true, false] {
        file_tests::frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(toggle.center()),
                egui::Event::PointerButton {
                    pos: toggle.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(app.prefs.closed_threads.contains(&key));
    assert_eq!(app.agents.active, root);
    let out = file_tests::frame(&ctx, &mut app, vec![egui::Event::PointerGone]);
    assert!(
        chevron(&out, toggle),
        "Keyboard-focused disclosure remains discoverable"
    );
    file_tests::frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(!app.prefs.closed_threads.contains(&key));
    let before = app.prefs.closed_threads.clone();
    click(&mut app, row.right_center() - egui::vec2(12.0, 0.0));
    assert!(egui::Popup::is_any_open(&ctx));
    assert_eq!(app.prefs.closed_threads, before);
    assert_eq!(app.draft, "Keep draft");
    assert_eq!(app.agents.active, root);
    assert!(app.state.busy && app.state.dialogs.len() == 1 && app.pending_dialog.is_none());
    assert!(commands.try_recv().is_err());
}

#[test]
fn saved_session_parent_uses_same_left_swap_and_title_click_still_opens_chat() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_agents: true,
            demo_sidebar_density: true,
            ..Default::default()
        },
    );
    for _ in 0..4 {
        render(&mut app, vec![]);
    }
    let branch = sidebar_tree::forest(&app)
        .into_iter()
        .find(|row| !row.children.is_empty())
        .unwrap();
    let toggle = ctx
        .data(|data| {
            data.get_temp::<egui::Rect>(egui::Id::new(("sidebar-disclosure-rect", &branch.key)))
        })
        .unwrap();
    let row = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("sidebar-row-rect", &branch.key))))
        .unwrap();
    let title = row.left_center() + egui::vec2(65.0, 0.0);
    let out = render(&mut app, vec![egui::Event::PointerMoved(title)]);
    assert!(chevron(&out, toggle));
    assert!(
        !out.shapes.iter().any(
            |s| matches!(&s.shape,egui::Shape::Circle(c)if toggle.contains(c.center)&&c.radius<=3.0)
        ),
        "Status dot must be replaced, not overdrawn"
    );
    click(&mut app, toggle.center());
    assert!(app.prefs.closed_threads.contains(&branch.key));
    click(&mut app, title);
    assert!(
        app.prefs.closed_threads.contains(&branch.key),
        "Title does not toggle the group"
    );
    assert_eq!(
        app.state.session_file.as_deref(),
        branch.path.as_ref().and_then(|path| path.to_str())
    );
}

#[test]
fn leaf_marker_is_still_a_chat_link_and_hidden_side_chats_have_no_phantom_toggle() {
    let mut app = fixture();
    for _ in 0..4 {
        render(&mut app, vec![]);
    }
    let row = sidebar_tree::forest(&app).remove(0);
    let child = row.children[1].clone();
    let child_rect = app
        .ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("sidebar-row-rect", &child.key))))
        .unwrap();
    click(&mut app, child_rect.left_center() + egui::vec2(8.0, 0.0));
    assert!(matches!(&child.target,sidebar_tree::Target::Agent(id) if id==&app.agents.active));
    let mut app = fixture();
    app.prefs.sidebar_view.side_chats = false;
    for _ in 0..4 {
        render(&mut app, vec![]);
    }
    let root = sidebar_tree::forest(&app).remove(0);
    let row = app
        .ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("sidebar-row-rect", &root.key))))
        .unwrap();
    let out = render(&mut app, vec![egui::Event::PointerMoved(row.center())]);
    assert!(!chevron(
        &out,
        egui::Rect::from_center_size(row.left_center() + egui::vec2(8.0, 0.0), Vec2::splat(20.0))
    ));
    assert!(app.prefs.closed_threads.is_empty());
}
