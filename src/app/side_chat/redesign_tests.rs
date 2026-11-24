//! Shared-chat presentation and ownership regressions for the embedded renderer.
use super::*;

pub(super) fn frame(
    ctx: &egui::Context,
    app: &mut Pui,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    sized_frame(ctx, app, egui::vec2(1600.0, 1100.0), events)
}
pub(super) fn sized_frame(
    ctx: &egui::Context,
    app: &mut Pui,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    )
}
pub(super) fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
pub(super) fn ready(
    ctx: &egui::Context,
) -> (Pui, String, String, std::sync::mpsc::Receiver<Value>) {
    let mut app = super::tests::app(ctx);
    app.prefs.hide_sidebar = true;
    let main = app.agents.active.clone();
    assert!(app.start_docked_side_chat("side question"));
    let side = app.docked_side_chat_target().unwrap();
    app.view_agent(&side);
    let (client, _, records) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.loading_chat = false;
    app.connected = true;
    app.ultracode_ready = true;
    app.state.model = "fixture-model".into();
    app.state.thinking = "high".into();
    app.view_agent(&main);
    (app, main, side, records)
}

#[test]
fn shared_transcript_renders_markdown_and_shared_composer_controls_with_distinct_ids() {
    for dark in [false, true] {
        let ctx = egui::Context::default();
        ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        let (mut app, main, side, _) = ready(&ctx);
        app.agents.parked.get_mut(&side).unwrap().state.messages = vec![Message {
            role: "assistant".into(),
            text: "## A proper side conversation\n\nRendered **bold** and `code`.".into(),
            ..Default::default()
        }];
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let out = frame(&ctx, &mut app, vec![]);
        let mut texts = vec![];
        fn collect(shape: &egui::Shape, texts: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => texts.push(text.galley.job.text.clone()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, texts);
                    }
                }
                _ => {}
            }
        }
        for shape in &out.shapes {
            collect(&shape.shape, &mut texts);
        }
        assert!(
            texts
                .iter()
                .any(|text| text.contains("A proper side conversation"))
        );
        assert!(
            !texts
                .iter()
                .any(|text| text.contains("## A proper side conversation"))
        );
        assert!(!texts.iter().any(|text| text.contains("**bold**")));
        assert!(
            ctx.read_response(egui::Id::new((
                "side-chat",
                side.as_str(),
                "composer-model"
            )))
            .is_some()
        );
        assert!(
            ctx.read_response(egui::Id::new((
                "side-chat",
                side.as_str(),
                "composer-effort"
            )))
            .is_some()
        );
        assert!(ctx.read_response(egui::Id::new("composer-model")).is_some());
        assert_eq!(app.agents.active, main);
        assert!(app.agents.parked[&side].state.messages.len() == 1);
    }
}

#[test]
fn side_attachments_send_with_the_side_draft_only_and_restore_main_images() {
    let ctx = egui::Context::default();
    let (mut app, main, side, records) = ready(&ctx);
    let image =
        crate::media::Attachment::from_bytes(include_bytes!("../../../assets/icon.png")).unwrap();
    app.draft_images.set_images(vec![image.clone()]);
    app.draft = "main kept".into();
    app.view_agent(&side);
    app.draft_images.set_images(vec![image]);
    app.view_agent(&main);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("btw-editor", side.as_str()))));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.draft, "main kept");
    assert_eq!(app.draft_images.images().len(), 1);
    assert!(app.agents.parked[&side].draft.is_empty());
    let prompt = records
        .try_iter()
        .find(|record| record["type"] == "prompt")
        .unwrap();
    assert_eq!(prompt["message"], "side question");
    assert_eq!(prompt["images"].as_array().unwrap().len(), 1);
}

#[test]
fn side_formatting_and_pending_approval_never_mutate_or_answer_main() {
    let ctx = egui::Context::default();
    let (mut app, main, side, _) = ready(&ctx);
    let (client, _, main_records) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.dialogs.push_back(questions::demo_dialog());
    app.draft = "main preserved".into();
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let id = egui::Id::new(("btw-editor", side.as_str()));
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::B, egui::Modifiers::COMMAND)],
    );
    assert_eq!(app.agents.active, main);
    assert_eq!(app.draft, "main preserved");
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(
        main_records
            .try_iter()
            .all(|record| record["type"] != "extension_ui_response" && record["type"] != "prompt")
    );
}

#[test]
fn scoped_ids_restore_without_leaking_across_context_or_memory_locks() {
    let ctx = egui::Context::default();
    let original = composer_text::set_owner(Some("side-fixture".into()));
    let id = ctx.memory(|_| composer_text::widget_id(&ctx, "composer"));
    assert_eq!(id, egui::Id::new(("btw-editor", "side-fixture")));
    let id = ctx.data(|_| composer_text::widget_id(&ctx, "composer-model"));
    assert_ne!(id, egui::Id::new("composer-model"));
    composer_text::set_owner(original);
    assert_eq!(
        composer_text::widget_id(&ctx, "composer"),
        egui::Id::new("composer")
    );
}
