use super::*;

pub(super) fn app(ctx: &egui::Context) -> Pui {
    crate::app::files::tests::app(ctx)
}
pub(super) fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
pub(super) fn frame(
    app: &mut Pui,
    size: Vec2,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, egui::Rect) {
    let ctx = app.ctx.clone();
    let mut trigger = egui::Rect::NOTHING;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            app.sidebar_view_inputs(ctx);
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(230.0_f32.min(ui.available_width()), 32.0),
                    Layout::right_to_left(Align::Center),
                    |ui| trigger = app.sidebar_view_options(ui).rect,
                );
            });
        },
    );
    (output, trigger)
}
pub(super) fn label(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    fn find(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() / 2.0)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, label))
        .unwrap_or_else(|| panic!("Missing {label}"))
}
pub(super) fn click(app: &mut Pui, size: Vec2, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            app,
            size,
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
pub(super) fn open(app: &mut Pui, size: Vec2) -> egui::Rect {
    for _ in 0..3 {
        frame(app, size, vec![]);
    }
    let trigger = frame(app, size, vec![]).1;
    click(app, size, trigger.center());
    for _ in 0..2 {
        frame(app, size, vec![]);
    }
    trigger
}

#[test]
fn placement_preserves_trigger_and_compacts_before_submenus_overlap() {
    for width in [180.0, 220.0, 390.0, 620.0, 900.0, 1500.0] {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 600.0));
        let trigger = egui::Rect::from_min_size(
            egui::pos2(150.0_f32.min(width - 36.0), 20.0),
            Vec2::splat(24.0),
        );
        let placement = placement(screen, trigger);
        let rect = egui::Rect::from_min_size(
            placement.pos,
            egui::vec2(placement.width, placement.max_height),
        );
        assert!(!rect.intersects(trigger), "{width} {rect:?}");
        assert!(rect.left() >= EDGE && rect.right() <= width - EDGE);
        if !placement.compact {
            assert!(rect.right() + WIDTH + GAP <= width - EDGE);
        }
    }
}

#[test]
fn hover_child_and_narrow_drill_in_change_only_presentation() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [220.0, 390.0, 900.0] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.prefs.color_styles.apply(&ctx, theme);
            app.draft = "Keep the draft".into();
            app.state.busy = true;
            let size = egui::vec2(width, 600.0);
            let trigger = open(&mut app, size);
            let group = label(&frame(&mut app, size, vec![]).0, "Group byFolder");
            let compact = placement(ctx.screen_rect(), trigger).compact;
            if compact {
                click(&mut app, size, group);
            } else {
                frame(&mut app, size, vec![egui::Event::PointerMoved(group)]);
            }
            for _ in 0..2 {
                frame(&mut app, size, vec![]);
            }
            let date = label(&frame(&mut app, size, vec![]).0, "Date");
            click(&mut app, size, date);
            assert_eq!(app.prefs.sidebar_view.group, Group::Date);
            assert!(!app.projects);
            assert_eq!(app.draft, "Keep the draft");
            assert!(app.state.busy);
            assert!(egui::Popup::is_id_open(&ctx, id()));
            let root = ctx.memory(|memory| memory.area_rect(id())).unwrap();
            assert!(!root.intersects(trigger));
            assert!(root.left() >= 0.0 && root.right() <= width);
            frame(&mut app, size, vec![key(egui::Key::Escape)]);
            assert!(egui::Popup::is_id_open(&ctx, id()));
            frame(&mut app, size, vec![key(egui::Key::Escape)]);
            assert!(!egui::Popup::is_any_open(&ctx));
        }
    }
}

#[test]
fn pointer_choices_toggle_independently_and_keep_exclusive_visibility() {
    for width in [390.0, 900.0] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let size = egui::vec2(width, 600.0);
        open(&mut app, size);
        let show = label(&frame(&mut app, size, vec![]).0, "Show");
        click(&mut app, size, show);
        for _ in 0..2 {
            frame(&mut app, size, vec![]);
        }
        for caption in ["Metadata", "Scheduled", "Side chats", "empty groups"] {
            let target = label(&frame(&mut app, size, vec![]).0, caption);
            click(&mut app, size, target);
        }
        assert!(app.prefs.sidebar_view.metadata && app.prefs.sidebar_view.empty_groups);
        assert!(!app.prefs.sidebar_view.scheduled && !app.prefs.sidebar_view.side_chats);
        for (caption, expected) in [
            ("Archived chats", (true, false)),
            ("Hidden chats", (false, true)),
            ("Active chats", (false, false)),
        ] {
            let target = label(&frame(&mut app, size, vec![]).0, caption);
            click(&mut app, size, target);
            assert_eq!(
                (app.show_archived, app.show_hidden),
                expected,
                "{width} {caption} target={target:?} state={:?}/{:?}",
                load(&ctx).page,
                load(&ctx).child_index
            );
        }
        frame(&mut app, size, vec![key(egui::Key::Escape)]);
        let group = label(&frame(&mut app, size, vec![]).0, "Group byFolder");
        click(&mut app, size, group);
        for _ in 0..2 {
            frame(&mut app, size, vec![]);
        }
        for value in [
            Group::Date,
            Group::Status,
            Group::Folder,
            Group::Environment,
            Group::None,
        ] {
            let target = label(&frame(&mut app, size, vec![]).0, value.label());
            click(&mut app, size, target);
            assert_eq!(app.prefs.sidebar_view.group, value);
            assert_eq!(app.projects, value == Group::Folder);
        }
        assert!(app.prefs.sidebar_view.metadata && app.prefs.sidebar_view.empty_groups);
    }
}

#[test]
fn hover_crosses_gap_without_losing_child_and_can_switch_to_another_page() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let size = egui::vec2(900.0, 600.0);
    open(&mut app, size);
    let group = label(&frame(&mut app, size, vec![]).0, "Group byFolder");
    frame(&mut app, size, vec![egui::Event::PointerMoved(group)]);
    for _ in 0..2 {
        frame(&mut app, size, vec![]);
    }
    let root = ctx.memory(|memory| memory.area_rect(id())).unwrap();
    let child = ctx
        .memory(|memory| memory.area_rect(id().with("child")))
        .unwrap();
    frame(
        &mut app,
        size,
        vec![egui::Event::PointerMoved(egui::pos2(
            (root.right() + child.left()) / 2.0,
            child.top() + 24.0,
        ))],
    );
    assert_eq!(load(&ctx).page, Some(Page::Group));
    let target = label(&frame(&mut app, size, vec![]).0, "Status");
    click(&mut app, size, target);
    assert_eq!(app.prefs.sidebar_view.group, Group::Status);
    let show = label(&frame(&mut app, size, vec![]).0, "Show");
    frame(&mut app, size, vec![egui::Event::PointerMoved(show)]);
    for _ in 0..2 {
        frame(&mut app, size, vec![]);
    }
    assert_eq!(load(&ctx).page, Some(Page::Show));
    label(&frame(&mut app, size, vec![]).0, "Scheduled");
}

#[test]
fn keyboard_pages_direction_show_and_disabled_actions() {
    for width in [390.0, 900.0] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let size = egui::vec2(width, 600.0);
        open(&mut app, size);
        // Down -> Sort; Right -> first child; Down twice -> Name; Space selects.
        for value in [
            egui::Key::ArrowDown,
            egui::Key::ArrowRight,
            egui::Key::ArrowDown,
            egui::Key::ArrowDown,
            egui::Key::Space,
        ] {
            frame(&mut app, size, vec![key(value)]);
        }
        assert!(app.prefs.session_order.sort == sidebar_order::Sort::Name);
        assert!(
            app.prefs.session_order.reverse,
            "Descending preserved across Name encoding"
        );
        for value in [egui::Key::End, egui::Key::Enter] {
            frame(&mut app, size, vec![key(value)]);
        }
        assert!(!app.prefs.session_order.reverse);
        for value in [
            egui::Key::ArrowLeft,
            egui::Key::ArrowDown,
            egui::Key::Enter,
            egui::Key::Space,
        ] {
            frame(&mut app, size, vec![key(value)]);
        }
        assert!(app.prefs.sidebar_view.metadata);
        for value in [egui::Key::End, egui::Key::Enter] {
            frame(&mut app, size, vec![key(value)]);
        }
        assert!(app.show_hidden && !app.show_archived);
        frame(&mut app, size, vec![key(egui::Key::ArrowLeft)]);
        app.prefs.sidebar_view.sections = (0..64)
            .map(|index| sidebar::sections::Section::new(format!("Section {index}")))
            .collect();
        frame(&mut app, size, vec![key(egui::Key::Home)]);
        for _ in 0..3 {
            frame(&mut app, size, vec![key(egui::Key::ArrowDown)]);
        }
        assert_eq!(load(&ctx).root_index, 4, "Disabled New section skipped");
        let output = frame(&mut app, size, vec![]).0;
        click(&mut app, size, label(&output, "New section"));
        assert_eq!(app.prefs.sidebar_view.sections.len(), 64);
    }
}
