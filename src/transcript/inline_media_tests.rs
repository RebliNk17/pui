use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};

fn collect_text(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_text(shape, out);
            }
        }
        _ => {}
    }
}

fn collect_text_rects(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
    match shape {
        egui::Shape::Text(t) => out.push((
            t.galley.job.text.clone(),
            Rect::from_min_size(t.pos, t.galley.size()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_text_rects(shape, out);
            }
        }
        _ => {}
    }
}

fn click_at(
    ctx: &egui::Context,
    pos: egui::Pos2,
    width: f32,
    mut draw: impl FnMut(&egui::Context),
) -> egui::FullOutput {
    let mut last = egui::FullOutput::default();
    for events in [
        vec![],
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![],
    ] {
        last = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(width, 700.0),
                )),
                events,
                ..Default::default()
            },
            &mut draw,
        );
    }
    last
}

fn collect_rects(shape: &egui::Shape, out: &mut Vec<Rect>) {
    match shape {
        egui::Shape::Rect(r) => out.push(r.rect),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_rects(shape, out);
            }
        }
        _ => {}
    }
}

#[test]
fn collapsed_activity_row_thumbnails_open_gallery_without_toggling_disclosure() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let icon = STANDARD.encode(include_bytes!("../../assets/icon.png"));
    let messages = crate::protocol::parse_history(&[
        serde_json::json!({"role":"user","content":"Check the screenshot"}),
        serde_json::json!({"role":"assistant","content":[
            {"type":"toolCall","id":"r1","name":"read","arguments":{"path":"shots/a.png"}},
            {"type":"toolCall","id":"r2","name":"read","arguments":{"path":"shots/b.png"}}
        ]}),
        serde_json::json!({"role":"toolResult","toolCallId":"r1","content":[{"type":"image","data":icon,"mimeType":"image/png"}],"isError":false}),
        serde_json::json!({"role":"toolResult","toolCallId":"r2","content":[{"type":"image","data":STANDARD.encode(b"other-bytes"),"mimeType":"image/png"}],"isError":false}),
    ]);
    let mut transcript = Transcript::default();
    let mut header_height = 0.0;
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 700.0))),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                transcript.show(ui, &messages, "claude", "medium", false, "thumb-test");
                header_height = transcript.total;
            });
        },
    );
    let mut rects = Vec::new();
    for clipped in &out.shapes {
        collect_rects(&clipped.shape, &mut rects);
    }
    let thumbs: Vec<Rect> = rects
        .into_iter()
        .filter(|r| (r.width() - 38.0).abs() < 0.5 && (r.height() - 22.0).abs() < 0.5)
        .collect();
    assert!(
        thumbs.len() >= 2,
        "expected two 38x22 compact thumbnails on collapsed activity row, found {thumbs:?}"
    );
    let thumb_pos = thumbs[0].center();
    click_at(&ctx, thumb_pos, 800.0, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            transcript.show(ui, &messages, "claude", "medium", false, "thumb-test");
        });
    });
    assert!(
        transcript.gallery.is_open(),
        "clicking activity thumbnail should open image theater"
    );
    assert!(
        (transcript.total - header_height).abs() < 4.0,
        "clicking activity thumbnail must not expand the collapsed activity row"
    );
}

#[test]
fn edited_html_files_render_compact_card_and_emit_open_action() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let messages = crate::protocol::parse_history(&[
        serde_json::json!({"role":"assistant","content":[
            {"type":"text","text":"Updated the mockup."},
            {"type":"toolCall","id":"w1","name":"write","arguments":{"path":"docs/campaign-map-designs.html","content":"<html></html>"}},
            {"type":"toolCall","id":"e1","name":"edit","arguments":{"path":"docs/campaign-map-designs.html","oldText":"a","newText":"b"}}
        ]}),
        serde_json::json!({"role":"toolResult","toolCallId":"w1","content":[{"type":"text","text":"ok"}],"isError":false}),
        serde_json::json!({"role":"toolResult","toolCallId":"e1","content":[{"type":"text","text":"ok"}],"isError":false}),
    ]);
    let mut transcript = Transcript::default();
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 700.0))),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                transcript.show(ui, &messages, "claude", "medium", false, "html-card");
            });
        },
    );
    let mut texts = Vec::new();
    for clipped in &out.shapes {
        collect_text_rects(&clipped.shape, &mut texts);
    }
    let html_badges: Vec<_> = texts.iter().filter(|(t, _)| t == "HTML").collect();
    assert_eq!(
        html_badges.len(),
        1,
        "multiple edits to the same HTML file should deduplicate to one card"
    );
    let card_pos = html_badges[0].1.center();
    let mut captured_action = None;
    click_at(&ctx, card_pos, 800.0, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            transcript.show(ui, &messages, "claude", "medium", false, "html-card");
            if transcript.action.is_some() {
                captured_action = transcript.action.clone();
            }
        });
    });
    assert_eq!(
        captured_action,
        Some(TranscriptAction::OpenTarget(
            "docs/campaign-map-designs.html".into()
        ))
    );
}

#[test]
fn pui_open_url_renders_browser_preview_card_with_title_and_open_button() {
    for mode in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        mode.apply(&ctx);
        let messages = crate::protocol::parse_history(&[
            serde_json::json!({"role":"assistant","content":[
                {"type":"toolCall","id":"o1","name":"pui_open","arguments":{"target":"http://127.0.0.1:8765/"}}
            ]}),
            serde_json::json!({"role":"toolResult","toolCallId":"o1","content":[{"type":"text","text":"Opened"}],"details":{"target":"http://127.0.0.1:8765/","opening":true},"isError":false}),
        ]);
        let mut transcript = Transcript::default();
        transcript
            .visuals
            .page_snapshots
            .record_title("http://127.0.0.1:8765/", "Error response".into());
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(620.0, 700.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    transcript.show(ui, &messages, "claude", "medium", false, "open-card");
                });
            },
        );
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            collect_text_rects(&clipped.shape, &mut texts);
        }
        assert!(texts.iter().any(|(t, _)| t == "Error response"));
        assert!(texts
            .iter()
            .any(|(t, _)| t == "127.0.0.1:8765, opened in Browser"));
        let open_pos = texts
            .iter()
            .find(|(t, _)| t == "Open")
            .expect("Open button visible")
            .1
            .center();
        let mut captured = None;
        click_at(&ctx, open_pos, 620.0, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                transcript.show(ui, &messages, "claude", "medium", false, "open-card");
                if transcript.action.is_some() {
                    captured = transcript.action.clone();
                }
            });
        });
        assert_eq!(
            captured,
            Some(TranscriptAction::OpenTarget(
                "http://127.0.0.1:8765/".into()
            ))
        );
    }
}

#[test]
fn pui_visual_renders_header_and_body_in_light_dark_and_narrow_widths() {
    for (mode, width) in [
        (crate::theme::Theme::Dark, 800.0),
        (crate::theme::Theme::Light, 360.0),
    ] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        mode.apply(&ctx);
        let messages = crate::protocol::parse_history(&[
            serde_json::json!({"role":"assistant","content":[
                {"type":"toolCall","id":"v1","name":"pui_visual","arguments":{"title":"Map picker","html":"<svg width=\"120\" height=\"80\" xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"120\" height=\"80\" fill=\"#223344\"/></svg>","height":160}}
            ]}),
            serde_json::json!({"role":"toolResult","toolCallId":"v1","content":[{"type":"text","text":"Displayed"}],"isError":false}),
        ]);
        let mut transcript = Transcript::default();
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, 700.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    transcript.show(ui, &messages, "claude", "medium", false, "visual-card");
                });
            },
        );
        let mut labels = Vec::new();
        for clipped in &out.shapes {
            collect_text(&clipped.shape, &mut labels);
        }
        assert!(
            labels.iter().any(|t| t.contains("Made a visual")),
            "expected 'Made a visual' header in {labels:?}"
        );
    }
}

#[test]
fn markdown_inline_local_photo_resolves_safely_and_remote_images_never_fetch() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let messages = vec![Message {
        role: "assistant".into(),
        text: "In the game:\n\n![App icon](assets/icon.png)\n\n![Remote](https://example.com/pic.png)".into(),
        blocks: vec![Block::Text(
            "In the game:\n\n![App icon](assets/icon.png)\n\n![Remote](https://example.com/pic.png)".into(),
        )],
        ..Default::default()
    }];
    let mut transcript = Transcript::default();
    transcript.cwd = Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 700.0))),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                transcript.show(ui, &messages, "claude", "medium", false, "md-photo");
            });
        },
    );
    let mut labels = Vec::new();
    for clipped in &out.shapes {
        collect_text(&clipped.shape, &mut labels);
    }
    // Remote image falls back to link text "Remote" without fetching, while local image renders its inline photo box.
    assert!(labels.iter().any(|t| t.contains("Remote")));
    assert!(
        crate::markdown::resolve_local_image("https://example.com/pic.png", transcript.cwd.as_deref()).is_none()
    );
    assert!(
        crate::markdown::resolve_local_image("assets/icon.png", transcript.cwd.as_deref()).is_some()
    );
}
