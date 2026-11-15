use super::tests::*;
use super::*;
use crate::app::files::tests::frame;

fn draw(
    ctx: &egui::Context,
    app: &mut Pui,
    events: Vec<egui::Event>,
    time: f64,
) -> egui::FullOutput {
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(620.0, 700.0),
            )),
            events,
            time: Some(time),
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    )
}
#[test]
fn native_first_tab_activation_reverse_escape_and_disabled_question_do_not_leak() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app, ids) = fixture();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let header = ctx
            .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("section-header-id", &ids[0]))))
            .unwrap();
        for _ in 0..80 {
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
            );
            if ctx.memory(|memory| memory.has_focus(header)) {
                break;
            }
        }
        assert!(
            ctx.memory(|memory| memory.has_focus(header)),
            "native traversal reaches actual header"
        );
        app.state.dialogs.push_back(questions::demo_dialog());
        let mut events = vec![key(activation, egui::Modifiers::NONE)];
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        frame(&ctx, &mut app, events);
        assert!(!app.prefs.sidebar_view.sections[0].collapsed);
        assert_eq!(app.state.dialogs.len(), 1);
        assert_eq!(app.draft, "Unsent draft");
        // Reverse from a later empty header to the preceding section's options,
        // not from the first header into the unrelated global sidebar controls.
        let next = ctx
            .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("section-header-id", &ids[2]))))
            .unwrap();
        for _ in 0..60 {
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
            );
            if ctx.memory(|memory| memory.has_focus(next)) {
                break;
            }
        }
        assert!(ctx.memory(|memory| memory.has_focus(next)));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
        );
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(app.state.busy);
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn narrow_light_dark_zoom_drag_indicator_stays_in_heading_and_menu_body_are_not_targets() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app, ids) = fixture();
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            for _ in 0..4 {
                draw(&ctx, &mut app, vec![], 0.5);
            }
            let from = rect(&ctx, &ids[0]).center();
            draw(
                &ctx,
                &mut app,
                vec![egui::Event::PointerMoved(from), pointer(from, true)],
                0.6,
            );
            draw(
                &ctx,
                &mut app,
                vec![egui::Event::PointerMoved(from + egui::vec2(14.0, 0.0))],
                0.7,
            );
            assert!(app.section_view.drag.is_some());
            let target = rect(&ctx, &ids[1]);
            let pos = target.center() + egui::vec2(0.0, 5.0);
            let output = draw(&ctx, &mut app, vec![egui::Event::PointerMoved(pos)], 0.8);
            let accent = theme::palette(&ctx).accent;
            assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::LineSegment{points,stroke} if stroke.width==2.0 && stroke.color==accent && points.iter().all(|point|point.x>=target.left() && point.x<=target.right() && point.y>=target.top() && point.y<=target.bottom()))));
            let menu = ctx
                .data(|data| {
                    data.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new((
                        "section-header-rects",
                        &ids[2],
                    )))
                })
                .unwrap()
                .1
                .center();
            draw(
                &ctx,
                &mut app,
                vec![egui::Event::PointerMoved(menu), pointer(menu, false)],
                0.9,
            );
            assert_eq!(order(&app), ids);
            assert!(app.section_view.drag.is_none());
            assert!(!egui::Popup::is_any_open(&ctx));
            assert_eq!(app.draft, "Unsent draft");
        }
    }
}
#[test]
fn section_order_roundtrip_has_presentation_only_and_retained_metadata_is_bounded_settled() {
    let (ctx, mut app, ids) = fixture();
    start(&ctx, &mut app, &ids[0]);
    let drag = app.section_view.drag.as_ref().unwrap();
    let bytes = drag.source.capacity()
        + drag.owner.capacity()
        + drag.root.capacity()
        + drag.order.iter().map(String::capacity).sum::<usize>();
    assert!(bytes <= 4096 * 2 + 80 * (LIMIT + 1));
    release(&ctx, &mut app, &ids[2], true);
    let value = serde_json::to_value(&app.prefs.sidebar_view).unwrap();
    let restored: sidebar_view::Options = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        restored
            .sections
            .iter()
            .map(|item| &item.id)
            .collect::<Vec<_>>(),
        app.prefs
            .sidebar_view
            .sections
            .iter()
            .map(|item| &item.id)
            .collect::<Vec<_>>()
    );
    for forbidden in ["command", "approval", "draft", "payload", "drag"] {
        assert!(!value.as_object().unwrap().contains_key(forbidden));
    }
    app.state.busy = false;
    for _ in 0..5 {
        draw(&ctx, &mut app, vec![], 1.0);
    }
    draw(&ctx, &mut app, vec![egui::Event::PointerGone], 2.0);
    let mut output = None;
    for time in 3..30 {
        output = Some(draw(&ctx, &mut app, vec![], time as f64));
    }
    assert!(app.section_view.drag.is_none());
    assert!(app.section_view.drag_controls.capacity() <= LIMIT * 2);
    assert!(app.workspace_rx.is_none() && app.diff_rx.is_none());
    // Existing demo content/caret deadlines are not a zero-idle comparator.
    assert!(
        !output.unwrap().viewport_output[&egui::ViewportId::ROOT]
            .repaint_delay
            .is_zero()
    );
    let mut items = (0..LIMIT)
        .map(|index| Section::new(format!("Section {index}")))
        .collect::<Vec<_>>();
    let first = items[0].id.clone();
    let second = items[1].id.clone();
    let last = items[LIMIT - 1].id.clone();
    let start = std::time::Instant::now();
    for _ in 0..10000 {
        assert!(reorder(&mut items, &first, &last, true));
        assert!(reorder(&mut items, &first, &second, false));
    }
    eprintln!(
        "20000bounded64sectionmoves {:?}; three-section owned drag keys{bytes}B + vec/map/UI overhead",
        start.elapsed()
    );
}
