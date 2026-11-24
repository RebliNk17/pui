use super::*;
use base64::Engine as _;
fn response() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(80, 60, image::Rgba([30, 80, 120, 255]));
    let mut output = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    serde_json::json!({"data":base64::engine::general_purpose::STANDARD.encode(output.get_ref())})
        .to_string()
        .into_bytes()
}
fn metrics(state: &mut State, id: i32) -> transport::Command {
    match state.receive(
        id,
        true,
        br#"{"cssVisualViewport":{"pageX":0,"pageY":20,"clientWidth":80,"clientHeight":60}}"#,
    ) {
        transport::Reply::Command(command) => command,
        _ => panic!("capture command"),
    }
}
pub(super) fn captured(ctx: &egui::Context) -> (State, transport::Ticket, Frozen) {
    let mut state = State::default();
    let (ticket, command) = state.start(ctx).unwrap();
    let capture = metrics(&mut state, command.id);
    assert_eq!(capture.method, "Page.captureScreenshot");
    assert_eq!(capture.params["captureBeyondViewport"], false);
    assert!(capture.params["clip"]["scale"].as_f64().unwrap() <= 1.0 / 3.0);
    let transport::Reply::Decode(received, bytes) = state.receive(capture.id, true, &response())
    else {
        panic!("decode")
    };
    assert_eq!(received.request, ticket.request);
    let frozen = transport::decode(&ticket, &bytes).unwrap();
    (state, ticket, frozen)
}
#[test]
fn activation_mock_raw_capture_crop_annotations_review_then_explicit_add_only() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut state = State::default();
    let mut editor = Editor::new("page", state.epoch, 42);
    frame(&ctx, &mut editor, true, vec![], 700.0);
    frame(&ctx, &mut editor, true, vec![], 700.0);
    assert!(click(&ctx, &mut editor, "Capture viewport", true).capture);
    assert!(
        editor.ticket.is_none(),
        "disclosure click only requests native capture"
    );
    let (ticket, command) = state.start(&ctx).unwrap();
    editor.epoch = ticket.epoch;
    editor.ticket = Some(ticket.clone());
    editor.phase = Phase::Capturing;
    let capture = metrics(&mut state, command.id);
    let transport::Reply::Decode(ticket, bytes) = state.receive(capture.id, true, &response())
    else {
        panic!("decode")
    };
    ticket.complete(transport::decode(&ticket, &bytes));
    editor.poll(&ctx);
    assert!(matches!(editor.phase, Phase::Editing));
    editor.edit(Tool::Crop, [10, 10], [49, 39]);
    editor.edit(Tool::Rectangle, [15, 15], [40, 30]);
    editor.edit(Tool::Arrow, [12, 12], [45, 35]);
    assert_eq!(editor.edits.as_ref().unwrap().marks.len(), 2);
    let previous = editor.history.pop().unwrap();
    editor.edits = Some(previous);
    assert_eq!(editor.edits.as_ref().unwrap().marks.len(), 1);
    assert!(editor.add(true).is_none());
    frame(&ctx, &mut editor, true, vec![], 700.0);
    frame(&ctx, &mut editor, true, vec![], 700.0);
    assert!(click(&ctx, &mut editor, "Review image", true).review);
    editor.review(&ctx);
    let result = editor
        .render_rx
        .as_ref()
        .unwrap()
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert_eq!(result.preview.as_ref().unwrap().size, [40, 30]);
    editor.render_rx = None;
    editor.review = Some(result);
    editor.phase = Phase::Review;
    assert!(editor.add(false).is_none());
    frame(&ctx, &mut editor, true, vec![], 700.0);
    frame(&ctx, &mut editor, true, vec![], 700.0);
    assert!(click(&ctx, &mut editor, "Add image to draft", true).add);
    let event = editor.add(true).unwrap();
    assert_eq!(
        (
            event.page.as_str(),
            event.draft_token,
            event.epoch,
            event.request
        ),
        ("page", 42, ticket.epoch, ticket.request)
    );
    assert_eq!(event.image.mime, "image/png");
    assert!(editor.add(true).is_none(), "exactly once");
    let raw = base64::engine::general_purpose::STANDARD
        .decode(event.image.data.as_bytes())
        .unwrap();
    let output = image::load_from_memory(&raw).unwrap().into_rgba8();
    assert_eq!(output.dimensions(), (40, 30));
    assert_eq!(output.get_pixel(5, 5).0, [239, 68, 68, 255]);
}
#[test]
fn stale_cancel_duplicate_timeout_and_page_incarnations_refuse_late_results() {
    let ctx = egui::Context::default();
    let mut state = State::default();
    let (old, command) = state.start(&ctx).unwrap();
    state.invalidate();
    assert!(old.cancelled());
    let old_request = old.request;
    drop(old);
    assert!(matches!(
        state.receive(command.id, true, b"{}"),
        transport::Reply::Ignored
    ));
    let (ticket, command) = state.start(&ctx).unwrap();
    let capture = metrics(&mut state, command.id);
    assert!(matches!(
        state.receive(command.id, true, b"{}"),
        transport::Reply::Ignored
    ));
    assert!(matches!(
        state.receive(capture.id, true, &response()),
        transport::Reply::Decode(_, _)
    ));
    assert!(matches!(
        state.receive(capture.id, true, &response()),
        transport::Reply::Ignored
    ));
    state.timeout(old_request);
    assert!(ticket.mailbox.lock().unwrap().is_none());
    state.timeout(ticket.request);
    assert!(ticket.mailbox.lock().unwrap().as_ref().unwrap().is_err());
    ticket.mailbox.lock().unwrap().take();
    ticket.complete(transport::decode(&ticket, &response()));
    assert!(
        ticket.mailbox.lock().unwrap().is_none(),
        "late decode after timeout"
    );
    let other = State::default();
    assert_ne!(state.epoch, other.epoch);
    let mut browser = BrowserView {
        screenshot: Some(Editor::new("page", 123, 1)),
        ..Default::default()
    };
    assert!(browser.screenshot_ui(&ctx, "other", 1, true).is_none());
    assert!(browser.screenshot.is_none());
}
#[test]
fn shared_budget_malformed_response_and_png_header_are_bounded() {
    let ctx = egui::Context::default();
    let lease = Lease::new(&ctx);
    lease.reserve(20 * 1024 * 1024).unwrap();
    assert!(State::default().start(&ctx).is_err());
    drop(lease);
    let (mut state, ticket, _) = captured(&ctx);
    for bytes in [
        b"bad".as_slice(),
        br#"{"data":"???"}"#,
        br#"{"data":"aGVsbG8="}"#,
    ] {
        assert!(transport::decode(&ticket, bytes).is_err());
    }
    assert!(transport::decode(&ticket, &vec![b'x'; transport::MAX_RESPONSE + 1]).is_err());
    let mut large = image::RgbaImage::new(1001, 1000);
    large.put_pixel(0, 0, image::Rgba([255; 4]));
    let mut output = std::io::Cursor::new(Vec::new());
    large
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    let bytes=serde_json::json!({"data":base64::engine::general_purpose::STANDARD.encode(output.get_ref())}).to_string();
    assert!(
        transport::decode(&ticket, bytes.as_bytes())
            .unwrap_err_string()
            .contains("1 megapixel")
    );
    state.invalidate();
    drop(ticket); // retained frozen output from captured() was discarded
    assert!(State::default().start(&ctx).is_ok());
}
trait ResultMessage {
    fn unwrap_err_string(self) -> String;
}
impl<T> ResultMessage for Result<T, String> {
    fn unwrap_err_string(self) -> String {
        match self {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        }
    }
}
pub(super) fn frame(
    ctx: &egui::Context,
    editor: &mut Editor,
    can_add: bool,
    events: Vec<egui::Event>,
    width: f32,
) -> ui::Decision {
    let mut decision = ui::Decision::default();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 760.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            decision = ui::show(ctx, editor, can_add);
        },
    );
    decision
}
fn click(ctx: &egui::Context, editor: &mut Editor, name: &str, can_add: bool) -> ui::Decision {
    let pos = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("screenshot-control", name))))
        .unwrap()
        .center();
    frame(
        ctx,
        editor,
        can_add,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        700.0,
    );
    frame(
        ctx,
        editor,
        can_add,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        700.0,
    )
}
#[test]
fn quiet_prompt_light_dark_narrow_zoom_escape_chords_and_disabled_add() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [260.0, 400.0, 900.0] {
            for zoom in [0.8, 1.0, 1.6] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let (_, ticket, frozen) = captured(&ctx);
                let mut editor = Editor::new("fixture", ticket.epoch, 1);
                editor.ticket = Some(ticket);
                editor.frozen = Some(frozen);
                editor.phase = Phase::Review;
                for _ in 0..2 {
                    frame(&ctx, &mut editor, false, vec![], width);
                }
                for name in ["Cancel", "Add image to draft"] {
                    let rect = ctx
                        .data(|d| {
                            d.get_temp::<egui::Rect>(egui::Id::new(("screenshot-control", name)))
                        })
                        .unwrap();
                    assert!(rect.is_finite());
                    assert!(
                        rect.top() >= 0.0 && rect.bottom() <= ctx.screen_rect().bottom(),
                        "{name} {width} {zoom} {rect:?}"
                    );
                    assert!(
                        rect.left() >= 0.0 && rect.right() <= ctx.screen_rect().right(),
                        "{name} {width} {zoom} {rect:?}"
                    );
                }
                let key = |key, modifiers| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                };
                let decision = frame(
                    &ctx,
                    &mut editor,
                    false,
                    vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
                    width,
                );
                assert!(!decision.add && !decision.capture);
                let decision = frame(
                    &ctx,
                    &mut editor,
                    false,
                    vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
                    width,
                );
                assert!(decision.cancel);
            }
        }
    }
}
