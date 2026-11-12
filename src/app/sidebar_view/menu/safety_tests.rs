use super::tests::{app, click, frame, key, label, open};
use super::*;

#[test]
fn zoom_short_window_and_back_button_fit_without_covering_trigger() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4, 2.0] {
            for size in [
                egui::vec2(220.0, 300.0),
                egui::vec2(390.0, 600.0),
                egui::vec2(900.0, 400.0),
            ] {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                ctx.set_zoom_factor(zoom);
                app.prefs.color_styles.apply(&ctx, mode);
                let trigger = open(&mut app, size);
                let group = label(&frame(&mut app, size, vec![]).0, "Group byFolder");
                click(&mut app, size, group);
                for _ in 0..4 {
                    frame(&mut app, size, vec![]);
                }
                let rect = ctx.memory(|memory| memory.area_rect(id())).unwrap();
                assert!(
                    rect.left() >= 0.0 && rect.right() <= size.x && rect.bottom() <= size.y,
                    "{rect:?} at {zoom}/{size:?}"
                );
                assert!(!rect.intersects(trigger));
                if placement(ctx.screen_rect(), trigger).compact {
                    let back = label(&frame(&mut app, size, vec![]).0, "Back to view options");
                    click(&mut app, size, back);
                    assert!(load(&ctx).page.is_none());
                }
                click(&mut app, size, egui::pos2(size.x - 3.0, size.y - 3.0));
                assert!(
                    !egui::Popup::is_id_open(&ctx, id()),
                    "Outside click dismisses"
                );
            }
        }
    }
}

#[test]
fn focused_trigger_keyboard_activation_and_tab_trap_return_focus() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let size = egui::vec2(900.0, 600.0);
    open(&mut app, size);
    let trigger = load(&ctx).trigger.unwrap();
    frame(&mut app, size, vec![key(egui::Key::Escape)]);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(trigger));
    frame(&mut app, size, vec![key(egui::Key::Enter)]);
    assert!(egui::Popup::is_id_open(&ctx, id()));
    frame(&mut app, size, vec![key(egui::Key::Tab)]);
    assert_eq!(load(&ctx).root_index, 1);
    let mut event = key(egui::Key::Tab);
    if let egui::Event::Key { modifiers, .. } = &mut event {
        *modifiers = egui::Modifiers::SHIFT;
    }
    frame(&mut app, size, vec![event]);
    assert_eq!(load(&ctx).root_index, 0);
    frame(&mut app, size, vec![key(egui::Key::Escape)]);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(trigger));
}

#[test]
fn full_update_menu_escape_and_shortcuts_never_stop_send_or_approve() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "Keep this unsent".into();
    app.state.busy = true;
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let dialog = json!({"id":"view-menu-permission", "method":"confirm", "title":"Allow bash?", "message":"Run command\n{\"command\":\"echo example\"}\n\nApprove this call only."});
    assert!(prompts::approval(&dialog).is_some());
    app.state.dialogs.push_back(dialog);
    let size = egui::vec2(1500.0, 950.0);
    let mut native = eframe::Frame::_new_kittest();
    let mut draw = |app: &mut Pui, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ctx| eframe::App::update(app, ctx, &mut native),
        )
    };
    for _ in 0..3 {
        draw(&mut app, vec![]);
    }
    demo(&ctx);
    draw(&mut app, vec![]);
    let mut approve = key(egui::Key::Enter);
    if let egui::Event::Key { modifiers, .. } = &mut approve {
        *modifiers = egui::Modifiers::COMMAND;
    }
    draw(&mut app, vec![approve]);
    draw(&mut app, vec![key(egui::Key::Escape)]);
    assert!(egui::Popup::is_id_open(&ctx, id()));
    draw(&mut app, vec![key(egui::Key::Escape)]);
    assert!(!egui::Popup::is_any_open(&ctx));
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy);
    assert_eq!(app.draft, "Keep this unsent");
    assert!(commands.try_recv().is_err());
    assert!(app.pending_dialog.is_none());
}

#[test]
fn resize_open_hover_menu_to_compact_keeps_keyboard_on_the_visible_page() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let wide = egui::vec2(900.0, 600.0);
    let narrow = egui::vec2(390.0, 600.0);
    open(&mut app, wide);
    let group = label(&frame(&mut app, wide, vec![]).0, "Group byFolder");
    frame(&mut app, wide, vec![egui::Event::PointerMoved(group)]);
    assert_eq!(load(&ctx).page, Some(Page::Group));
    assert!(!load(&ctx).child);
    for _ in 0..3 {
        frame(&mut app, narrow, vec![]);
    }
    assert!(load(&ctx).child);
    frame(&mut app, narrow, vec![key(egui::Key::Home)]);
    frame(&mut app, narrow, vec![key(egui::Key::Enter)]);
    assert_eq!(app.prefs.sidebar_view.group, Group::Date);
    for _ in 0..3 {
        frame(&mut app, wide, vec![]);
    }
    let output = frame(&mut app, wide, vec![]).0;
    label(&output, "Group byDate");
    label(&output, "Date");
    assert_eq!(load(&ctx).page, Some(Page::Group));
}

#[test]
fn short_menu_scrolls_keyboard_target_into_view_and_closes_when_owner_hides() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let size = egui::vec2(220.0, 250.0);
    open(&mut app, size);
    for value in [
        egui::Key::Home,
        egui::Key::ArrowDown,
        egui::Key::ArrowDown,
        egui::Key::Enter,
        egui::Key::End,
    ] {
        frame(&mut app, size, vec![key(value)]);
    }
    for _ in 0..4 {
        frame(&mut app, size, vec![]);
    }
    let target = label(&frame(&mut app, size, vec![]).0, "Hidden chats");
    let popup = ctx.memory(|memory| memory.area_rect(id())).unwrap();
    assert!(
        popup.contains(target) && ctx.screen_rect().contains(target),
        "Scrolled target {target:?} in {popup:?}"
    );
    click(&mut app, size, target);
    assert!(app.show_hidden && !app.show_archived);
    app.prefs.hide_sidebar = true;
    frame(&mut app, size, vec![]);
    assert!(!egui::Popup::is_id_open(&ctx, id()));
}

#[test]
fn settled_open_and_closed_menu_do_not_schedule_animation() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let size = egui::vec2(900.0, 600.0);
    open(&mut app, size);
    frame(&mut app, size, vec![key(egui::Key::ArrowRight)]);
    for _ in 0..12 {
        frame(&mut app, size, vec![]);
    }
    let output = frame(&mut app, size, vec![]).0;
    assert!(output.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1));
    frame(&mut app, size, vec![key(egui::Key::Escape)]);
    frame(&mut app, size, vec![key(egui::Key::Escape)]);
    for _ in 0..12 {
        frame(&mut app, size, vec![]);
    }
    let output = frame(&mut app, size, vec![]).0;
    assert!(output.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1));
}
