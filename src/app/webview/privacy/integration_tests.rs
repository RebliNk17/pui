//! Exercise the real More popup and viewport lifecycle, not standalone menu bodies.
use super::*;

const KEY: &str = "https://example.test/start";
fn frame(
    ctx: &egui::Context,
    browser: &mut BrowserView,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(760.0, 860.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    browser.navigation(ui, KEY);
                    browser.tools(ui, KEY);
                });
                browser.viewport_with_state(ui, KEY, PageState::default());
            });
        },
    )
}
fn click(ctx: &egui::Context, browser: &mut BrowserView, position: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            ctx,
            browser,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}
fn labels(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(text) => out.push((
            text.galley.job.text.clone(),
            egui::Rect::from_min_size(text.pos, text.galley.size()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                labels(shape, out);
            }
        }
        _ => {}
    }
}
fn position(output: egui::FullOutput, value: &str) -> egui::Pos2 {
    let mut text = Vec::new();
    for shape in output.shapes {
        labels(&shape.shape, &mut text);
    }
    text.iter()
        .find(|(label, _)| label == value)
        .unwrap_or_else(|| {
            panic!(
                "Missing {value:?}; found {:?}",
                text.iter().map(|(label, _)| label).collect::<Vec<_>>()
            )
        })
        .1
        .center()
}
fn fixture(mode: theme::Theme) -> (egui::Context, BrowserView) {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    mode.apply(&ctx);
    let mut browser = BrowserView::default();
    browser.bind_owner(std::path::Path::new("/workspace"), "session-a");
    browser.privacy.ensure(KEY);
    for _ in 0..3 {
        frame(&ctx, &mut browser, vec![]);
    }
    (ctx, browser)
}
fn more(ctx: &egui::Context, browser: &mut BrowserView) {
    let rect = ctx
        .data(|data| {
            data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", "More browser tools")))
        })
        .unwrap();
    click(ctx, browser, rect.center());
    for _ in 0..3 {
        frame(ctx, browser, vec![]);
    }
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

#[test]
fn real_more_popup_name_field_retains_focus_and_typing_until_explicit_create_confirmation() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let (ctx, mut browser) = fixture(mode);
        let original = browser.privacy.context(KEY);
        more(&ctx, &mut browser);
        let field_id = egui::Id::new(("browser-group-name", KEY));
        let field = ctx
            .read_response(field_id)
            .expect("name field is in actual More popup")
            .rect;
        assert!(ctx.screen_rect().contains_rect(field));
        click(&ctx, &mut browser, field.center());
        frame(&ctx, &mut browser, vec![egui::Event::Text("Work".into())]);
        assert_eq!(
            browser.privacy.draft, "Work",
            "clicking an editable popup field must not close More"
        );
        assert!(browser.privacy.groups.is_empty());
        assert_eq!(browser.privacy.context(KEY), original);
        assert!(!browser.privacy_confirming());
        let create = position(frame(&ctx, &mut browser, vec![]), "Create and switch…");
        click(&ctx, &mut browser, create);
        assert!(
            browser.privacy_confirming(),
            "Create stages, never immediately switches"
        );
        assert!(browser.privacy.groups.is_empty());
        for _ in 0..3 {
            frame(&ctx, &mut browser, vec![]);
        }
        let confirm = position(frame(&ctx, &mut browser, vec![]), "Switch group");
        click(&ctx, &mut browser, confirm);
        assert!(!browser.privacy_confirming());
        assert_eq!(browser.privacy.groups.len(), 1);
        assert!(browser.session_suspended(KEY));
        assert_ne!(browser.privacy.context(KEY), original);
        assert!(
            browser.state(KEY).location.is_empty(),
            "confirmation must not create or reopen a native page"
        );
    }
}

#[test]
fn suspended_viewport_still_renders_restart_confirmation_and_escape_leaves_resume_available() {
    let (ctx, mut browser) = fixture(theme::Theme::Dark);
    browser.stage_privacy(KEY, Change::Switch(Target::Named("Work".into())));
    let effect = browser.privacy.decide(KEY, true).unwrap();
    browser.apply_privacy_effect(effect);
    assert!(browser.session_suspended(KEY));
    browser.stage_privacy(KEY, Change::Clear);
    for _ in 0..3 {
        frame(&ctx, &mut browser, vec![]);
    }
    assert!(browser.privacy_confirming());
    position(frame(&ctx, &mut browser, vec![]), "Restart group");
    frame(&ctx, &mut browser, vec![key(egui::Key::Escape)]);
    assert!(
        !browser.privacy_confirming(),
        "a suspended tab must not trap its pending dialog behind Resume"
    );
    assert!(browser.session_suspended(KEY));
    // egui retains the dismissed modal for one pass; that pass registers
    // Resume disabled, so let the next live enabled response settle before press.
    frame(&ctx, &mut browser, vec![]);
    let evidence = || {
        ctx.data(|d| {
            d.get_temp::<(egui::Id, bool, Option<egui::LayerId>)>(egui::Id::new((
                "browser-resume-response",
                KEY,
            )))
            .unwrap()
        })
    };
    assert!(
        !evidence().1 && evidence().2.is_some(),
        "first nonmodal pass sees prior modal and registers disabled Resume"
    );
    assert!(ctx.memory(|m| m.top_modal_layer().is_none()));
    let resume = position(frame(&ctx, &mut browser, vec![]), "Resume");
    assert!(
        evidence().1 && evidence().2.is_none(),
        "settled Resume must be enabled without a modal"
    );
    click(&ctx, &mut browser, resume);
    assert!(!browser.session_suspended(KEY));
}

#[test]
fn parking_cancels_executable_privacy_intent_without_changing_group_or_resuming() {
    let (ctx, mut browser) = fixture(theme::Theme::Light);
    browser.stage_privacy(KEY, Change::Clear);
    let context = browser.privacy.context(KEY);
    browser.cancel_intents();
    assert!(!browser.privacy_confirming());
    assert_eq!(browser.privacy.context(KEY), context);
    browser.bind_owner(std::path::Path::new("/workspace"), "session-a");
    frame(&ctx, &mut browser, vec![]);
    assert!(
        !browser.privacy_confirming(),
        "same-owner reentry must not restore a parked approval"
    );
    assert!(browser.privacy.groups.is_empty());
}

#[test]
fn restored_http_tabs_are_suspended_without_recreating_groups_or_browser_state() {
    let mut browser = BrowserView::default();
    let other = "https://other.test".to_owned();
    browser.restore_suspended(&[KEY.to_owned(), other.clone()]);
    assert!(browser.session_suspended(KEY));
    assert!(browser.session_suspended(&other));
    assert_ne!(
        browser.privacy.context(KEY),
        browser.privacy.context(&other)
    );
    assert!(browser.privacy.groups.is_empty());
    assert!(!browser.privacy_confirming());
    assert!(browser.state(KEY).location.is_empty());
    assert!(browser.state(&other).location.is_empty());
    browser.active = KEY.into();
    browser.resume_session(KEY);
    assert!(!browser.session_suspended(KEY));
    assert!(browser.session_suspended(&other));
    assert!(
        browser.state(KEY).location.is_empty(),
        "Resume is intent; native creation remains in explicit UI show"
    );
}
