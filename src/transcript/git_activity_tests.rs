use super::*;
use crate::protocol::Tool;

fn texts(shape: &egui::Shape, out: &mut Vec<egui::epaint::TextShape>) {
    match shape {
        egui::Shape::Text(text) => out.push(text.clone()),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| texts(s, out)),
        _ => {}
    }
}

#[test]
fn compact_git_line_stays_single_row_and_expands_to_raw_commands() {
    for mode in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
        for width in [240.0, 900.0] {
            for zoom in [1.0, 1.5] {
                let ctx = egui::Context::default();
                crate::theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let command = "git commit -m 'Add Continue button' && git push origin master";
                let tool = Tool {
                    id: "git-summary-fixture".into(), name: "bash".into(), command: command.into(), finished: true,
                    output: "[master 11057c3d] Add Continue button\nTo gitlab.com:example/repo.git\n 1234567..11057c3d master -> master\n".into(),
                    ..Default::default()
                };
                let mut gallery = crate::media::Gallery::default();
                let mut frame = |events| {
                    let mut changed = false;
                    let mut height = 0.0;
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(width, 800.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                let top = ui.cursor().top();
                                changed = activity(ui, &[&tool], 0, false, false, &mut gallery);
                                height = ui.cursor().top() - top;
                            });
                        },
                    );
                    let mut labels = Vec::new();
                    for shape in output.shapes {
                        texts(&shape.shape, &mut labels);
                    }
                    (labels, height, changed)
                };
                for _ in 0..3 {
                    frame(vec![]);
                }
                let (labels, collapsed, _) = frame(vec![]);
                let header = labels
                    .iter()
                    .find(|t| t.galley.job.text == "Committed and pushed to master")
                    .expect("same-line Git summary");
                assert_eq!(header.galley.rows.len(), 1);
                let header_rect = Rect::from_min_size(header.pos, header.galley.size());
                assert!(
                    header_rect.right() <= width + 1.0,
                    "{mode:?}/{width}/{zoom}: {header_rect:?}"
                );
                assert!(!labels.iter().any(|t| t.galley.job.text == command));
                let pos = header.pos + egui::vec2(8.0, 5.0);
                let click = |pressed| {
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                };
                frame(click(true));
                let (_, expanded, changed) = frame(click(false));
                assert!(
                    changed && expanded > collapsed,
                    "existing disclosure still works"
                );
                let (labels, _, _) = frame(vec![]);
                assert!(
                    labels.iter().any(|t| t.galley.job.text == command),
                    "raw command remains available"
                );
            }
        }
    }
}
