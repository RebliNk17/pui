//! Parent-integration regression: real explicit UI decisions and real draft ingress.
//! The only mocked boundary is the native CEF transport response.
use super::*;
use crate::app::context_capture::browser::{self as handoff, Handoff};
use base64::Engine as _;

const PAGE: &str = "http://localhost:3000/screenshot-handoff";
fn input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 900.0),
        )),
        events,
        ..Default::default()
    }
}
fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
fn launch_frame(ctx: &egui::Context, app: &mut Pui, events: Vec<egui::Event>) -> Option<u64> {
    let mut token = None;
    let _ = ctx.run(input(events), |ctx| {
        // Same ordering as the application: prepare from the previous live
        // registered control before a later file/browser UI can mutate a draft.
        app.prepare_file_block_capture(ctx);
        let owner = app.review_comment_owner();
        context_capture::bind_file_owner(ctx, &app.prefs.cwd, &owner);
        egui::CentralPanel::default().show(ctx, |ui| {
            let response = prompt_ui::button(ui, "Screenshot", prompt_ui::Button::Secondary, true);
            handoff::register(&response);
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("handoff-launch"), response.rect));
            if response.clicked() {
                token = handoff::freeze(ctx);
            }
        });
    });
    token
}
fn activate(ctx: &egui::Context, app: &mut Pui) -> u64 {
    launch_frame(ctx, app, vec![]);
    launch_frame(ctx, app, vec![]);
    let pos = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("handoff-launch")))
        .unwrap()
        .center();
    assert!(launch_frame(ctx, app, pointer(pos, true)).is_none());
    let token = launch_frame(ctx, app, pointer(pos, false))
        .expect("explicit registered activation freezes owner");
    assert_eq!(handoff::token(ctx), Some(token));
    token
}
fn control(ctx: &egui::Context, label: &str) -> egui::Pos2 {
    ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("screenshot-control", label))))
        .unwrap()
        .center()
}
fn disclosure(ctx: &egui::Context, editor: &mut Editor) {
    for _ in 0..2 {
        tests::frame(ctx, editor, true, vec![], 900.0);
    }
    let pos = control(ctx, "Capture viewport");
    assert!(!tests::frame(ctx, editor, true, pointer(pos, true), 900.0).capture);
    assert!(tests::frame(ctx, editor, true, pointer(pos, false), 900.0).capture);
    assert!(
        editor.ticket.is_none(),
        "only explicit disclosure asks native transport to start"
    );
}
fn capture_mock(ctx: &egui::Context, editor: &mut Editor) -> State {
    let mut state = State::default();
    let (ticket, command) = state.start(ctx).unwrap();
    assert_eq!(command.method, "Page.getLayoutMetrics");
    let transport::Reply::Command(capture) = state.receive(
        command.id,
        true,
        br#"{"cssVisualViewport":{"pageX":0,"pageY":40,"clientWidth":80,"clientHeight":60}}"#,
    ) else {
        panic!("visible viewport capture command")
    };
    assert_eq!(capture.method, "Page.captureScreenshot");
    assert_eq!(capture.params["captureBeyondViewport"], false);
    let source =
        image::RgbaImage::from_fn(80, 60, |x, y| image::Rgba([x as u8, y as u8, 120, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    source
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let response = serde_json::json!({"data":base64::engine::general_purpose::STANDARD.encode(bytes.get_ref())}).to_string();
    let transport::Reply::Decode(decoded_ticket, response) =
        state.receive(capture.id, true, response.as_bytes())
    else {
        panic!("bounded native response")
    };
    // Exercise actual worker decode, never decode page pixels on the UI thread.
    let decoded = std::thread::spawn(move || transport::decode(&decoded_ticket, &response))
        .join()
        .unwrap();
    ticket.complete(decoded);
    editor.epoch = ticket.epoch;
    editor.ticket = Some(ticket);
    editor.phase = Phase::Capturing;
    editor.poll(ctx);
    assert!(matches!(editor.phase, Phase::Editing));
    editor.edit(Tool::Crop, [10, 10], [49, 39]);
    editor.edit(Tool::Rectangle, [15, 15], [40, 30]);
    editor.edit(Tool::Arrow, [12, 12], [45, 35]);
    assert_eq!(editor.edits.as_ref().unwrap().marks.len(), 2);
    state
}
fn browser_frame(
    ctx: &egui::Context,
    browser: &mut BrowserView,
    token: u64,
    events: Vec<egui::Event>,
) -> Option<DraftImage> {
    let mut event = None;
    let _ = ctx.run(input(events), |ctx| {
        event = browser.screenshot_ui(ctx, PAGE, token, true);
    });
    event
}
fn click_browser(
    ctx: &egui::Context,
    browser: &mut BrowserView,
    token: u64,
    label: &str,
) -> Option<DraftImage> {
    browser_frame(ctx, browser, token, vec![]);
    browser_frame(ctx, browser, token, vec![]);
    let pos = control(ctx, label);
    assert!(browser_frame(ctx, browser, token, pointer(pos, true)).is_none());
    browser_frame(ctx, browser, token, pointer(pos, false))
}
fn reviewed_event(ctx: &egui::Context, token: u64) -> DraftImage {
    let mut browser = BrowserView::default();
    browser.seed_capture_page(PAGE);
    browser.begin_screenshot(ctx, PAGE, token).unwrap();
    let editor = browser.screenshot.as_mut().unwrap();
    disclosure(ctx, editor);
    let state = capture_mock(ctx, editor);
    browser.test_pages.get_mut(PAGE).unwrap().screenshot = state;
    assert!(click_browser(ctx, &mut browser, token, "Review image").is_none());
    let editor = browser.screenshot.as_mut().unwrap();
    assert!(matches!(editor.phase, Phase::Rendering));
    let review = editor
        .render_rx
        .take()
        .unwrap()
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let (tx, rx) = mpsc::sync_channel(1);
    tx.send(review)
        .unwrap_or_else(|_| panic!("review result receiver"));
    editor.render_rx = Some(rx);
    browser_frame(ctx, &mut browser, token, vec![]);
    assert!(matches!(
        browser.screenshot.as_ref().unwrap().phase,
        Phase::Review
    ));
    let event = click_browser(ctx, &mut browser, token, "Add image to draft")
        .expect("explicit reviewed Add returns one event");
    assert!(browser.screenshot.is_none());
    assert_eq!(
        handoff::token(ctx),
        Some(token),
        "successful screenshot consumption must not cancel its handoff token"
    );
    assert!(browser_frame(ctx, &mut browser, token, vec![]).is_none());
    event
}
fn queue(event: &DraftImage, ctx: &egui::Context, document: u64) {
    assert_eq!(
        event.page, PAGE,
        "screenshot event key is resolved to current page metadata by parent"
    );
    handoff::queue(
        ctx,
        Handoff::Image {
            token: event.draft_token,
            page: PAGE.into(),
            epoch: document,
            image: event.image.clone(),
        },
    );
}

#[test]
fn explicit_screenshot_review_appends_exact_cropped_annotated_pixels_to_original_draft_once_without_rpc()
 {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.draft = "Keep this draft 世界".into();
    app.state.busy = true;
    let document = app.files.seed_browser_capture(PAGE);
    let messages = app.state.messages.len();
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let token = activate(&ctx, &mut app);
    let before = app.draft.clone();
    let revision = app.draft_images.revision();
    let event = reviewed_event(&ctx, token);
    assert!(
        app.files.browser_capture_matches(PAGE, document),
        "visible unchanged page admits handoff"
    );
    assert!(
        app.draft_images.is_empty(),
        "review is not an implicit append"
    );
    assert_eq!(app.draft, before);
    assert!(commands.try_recv().is_err());
    queue(&event, &ctx, document);
    app.collect_context_capture();
    assert_eq!(app.draft, before);
    assert_ne!(app.draft_images.revision(), revision);
    let images = app.draft_images.images();
    assert_eq!(images.len(), 1);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(images[0].data.as_bytes())
        .unwrap();
    let result = image::load_from_memory(&bytes).unwrap().into_rgba8();
    assert_eq!(result.dimensions(), (40, 30));
    assert_eq!(
        result.get_pixel(0, 29).0,
        [10, 39, 120, 255],
        "crop retains exact frozen source pixels"
    );
    assert_eq!(
        result.get_pixel(5, 5).0,
        [239, 68, 68, 255],
        "rectangle is burned into reviewed attachment"
    );
    assert_eq!(
        result.get_pixel(35, 25).0,
        [239, 68, 68, 255],
        "arrow is burned into reviewed attachment"
    );
    assert!(handoff::token(&ctx).is_none());
    queue(&event, &ctx, document);
    app.collect_context_capture();
    assert_eq!(
        app.draft_images.images().len(),
        1,
        "replayed result cannot append twice"
    );
    assert_eq!(app.draft, before);
    assert_eq!(app.state.messages.len(), messages);
    assert!(app.state.busy);
    assert!(
        commands.try_recv().is_err(),
        "capture/review/Add never sends, stops, or approves RPC"
    );
}

#[test]
fn reviewed_screenshot_rejects_changed_draft_images_root_and_session_with_page_still_visible() {
    for mutation in ["draft", "images", "root", "session"] {
        let ctx = egui::Context::default();
        let mut app = crate::app::files::tests::app(&ctx);
        app.draft = "Original draft".into();
        let document = app.files.seed_browser_capture(PAGE);
        let (client, _events, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let token = activate(&ctx, &mut app);
        let event = reviewed_event(&ctx, token);
        match mutation {
            "draft" => app.draft.push_str(" changed"),
            "images" => {
                app.draft_images
                    .append_reviewed("Concurrent image".into(), event.image.clone())
                    .unwrap();
            }
            "root" => app.prefs.cwd = app.prefs.cwd.join("other-root"),
            "session" => app.state.session_file = Some("/changed-screenshot-session".into()),
            _ => unreachable!(),
        }
        let before = app.draft.clone();
        let images = app.draft_images.images();
        let revision = app.draft_images.revision();
        let messages = app.state.messages.len();
        assert!(
            app.files.browser_capture_matches(PAGE, document),
            "{mutation}: not rejected by hidden/stale-page short-circuit"
        );
        assert_eq!(handoff::token(&ctx), Some(token));
        queue(&event, &ctx, document);
        app.collect_context_capture();
        assert_eq!(app.draft, before, "{mutation}");
        assert_eq!(app.draft_images.revision(), revision, "{mutation}");
        assert_eq!(
            app.draft_images
                .images()
                .iter()
                .map(|i| i.key)
                .collect::<Vec<_>>(),
            images.iter().map(|i| i.key).collect::<Vec<_>>(),
            "{mutation}"
        );
        assert_eq!(app.state.messages.len(), messages, "{mutation}");
        assert!(handoff::token(&ctx).is_none(), "{mutation}");
        assert!(
            handoff::notice(&ctx).unwrap().contains("not added"),
            "{mutation}"
        );
        assert!(commands.try_recv().is_err(), "{mutation}");
    }
}
