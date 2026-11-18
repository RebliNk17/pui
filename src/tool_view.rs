//! Quiet tool rows: details stay attached to the action, not a generic result inspector.
pub(crate) mod answers;
use crate::{
    icons::{self, Icon},
    markdown,
    protocol::Tool,
};
use eframe::egui::{self, Align, FontId, Layout, Rect, RichText, Sense, Stroke, Vec2};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Detail {
    None,
    Command,
    Diff,
    Error,
    Script,
}

fn detail(tool: &Tool) -> Detail {
    if tool.name == "codemode" {
        Detail::Script
    } else if matches!(tool.name.as_str(), "bash" | "powershell") && !tool.command.is_empty() {
        Detail::Command
    } else if matches!(tool.name.as_str(), "edit" | "write")
        && !tool.diff.is_empty()
        && !tool.failed
    {
        Detail::Diff
    } else if tool.failed && !tool.output.is_empty() {
        Detail::Error
    } else {
        Detail::None
    }
}

fn duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}m {}s", ms / 60_000, ms % 60_000 / 1000)
    }
}

pub fn row(ui: &mut egui::Ui, tool: &Tool) {
    row_with_view(ui, tool, false);
}

pub fn row_verbose(ui: &mut egui::Ui, tool: &Tool) {
    row_with_view(ui, tool, true);
}

fn row_with_view(ui: &mut egui::Ui, tool: &Tool, verbose: bool) {
    let colors = crate::theme::palette(ui.ctx());
    let id = ui.make_persistent_id((&tool.id, "detail"));
    let kind = detail(tool);
    let expandable = kind != Detail::None;
    let small_diff = kind == Detail::Diff && tool.added + tool.removed <= 12;
    let mut open = verbose
        || ui
            .ctx()
            .data(|d| d.get_temp::<bool>(id))
            .unwrap_or(small_diff);
    let command = matches!(tool.name.as_str(), "bash" | "powershell");
    let icon = match tool.name.as_str() {
        "bash" | "powershell" => Icon::Command,
        "read" | "write" => Icon::File,
        "edit" => Icon::Edit,
        _ => Icon::Code,
    };
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 22.0),
        if expandable && !verbose {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let response = if expandable {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    let p = ui.painter();
    icons::draw(
        ui,
        Rect::from_center_size(rect.left_center() + egui::vec2(7.0, 0.0), Vec2::splat(14.0)),
        icon,
        if tool.failed {
            colors.red
        } else if command && tool.finished {
            colors.green
        } else {
            colors.dim
        },
    );
    let time = tool.duration_ms.map(duration);
    let time_width = time
        .as_ref()
        .map(|s| {
            p.layout_no_wrap(s.clone(), FontId::proportional(10.0), colors.dim)
                .size()
                .x
                + 10.0
        })
        .unwrap_or_default();
    let status_width = if tool.failed || !tool.finished || command {
        20.0
    } else {
        0.0
    };
    let chevron_width = if expandable { 16.0 } else { 0.0 };
    let text_rect = Rect::from_min_max(
        rect.min + egui::vec2(25.0, 0.0),
        rect.max - egui::vec2(time_width + status_width + chevron_width, 0.0),
    );
    let mut job = egui::text::LayoutJob::simple_singleline(
        tool.label(),
        if command {
            FontId::monospace(11.0)
        } else {
            FontId::proportional(12.0)
        },
        colors.muted,
    );
    job.wrap.max_width = text_rect.width().max(20.0);
    job.wrap.max_rows = 1;
    let galley = p.layout_job(job);
    p.galley(
        text_rect.left_center() - egui::vec2(0.0, galley.size().y / 2.0),
        galley,
        colors.muted,
    );
    if let Some(time) = time {
        p.text(
            rect.right_center(),
            egui::Align2::RIGHT_CENTER,
            time,
            FontId::proportional(10.0),
            colors.dim,
        );
    }
    let status = rect.right_center() - egui::vec2(time_width + status_width / 2.0, 0.0);
    if tool.failed {
        p.line_segment(
            [
                status + egui::vec2(-3.0, -3.0),
                status + egui::vec2(3.0, 3.0),
            ],
            Stroke::new(1.0, colors.red),
        );
        p.line_segment(
            [
                status + egui::vec2(-3.0, 3.0),
                status + egui::vec2(3.0, -3.0),
            ],
            Stroke::new(1.0, colors.red),
        );
    } else if command && tool.finished {
        p.add(egui::Shape::line(
            vec![
                status + egui::vec2(-4.0, 0.0),
                status + egui::vec2(-1.0, 3.0),
                status + egui::vec2(5.0, -4.0),
            ],
            Stroke::new(1.0, colors.green),
        ));
    } else if !tool.finished {
        p.circle_filled(status, 2.0, colors.accent);
    }
    if expandable && !verbose {
        icons::chevron(
            ui,
            Rect::from_center_size(
                rect.right_center() - egui::vec2(time_width + status_width + 8.0, 0.0),
                Vec2::splat(12.0),
            ),
            open,
            colors.dim,
        );
        if response.clicked() {
            open = !open;
            ui.ctx().data_mut(|d| d.insert_temp(id, open));
        }
    }
    if open && expandable {
        ui.add_space(4.0);
        ui.indent(id.with("inset"), |ui| match kind {
            Detail::Command => code_block(ui, id, &tool.command),
            Detail::Diff => diff_block(ui, id, tool),
            Detail::Error => code_block(ui, id, &tool.output),
            Detail::Script => {
                for child in &tool.children {
                    row_with_view(ui, child, verbose);
                }
                if !tool.code.is_empty() {
                    code_block(ui, id.with("script"), &tool.code);
                }
                if !tool.output.is_empty() {
                    code_block(ui, id.with("script-result"), &tool.output);
                }
            }
            Detail::None => {}
        });
        ui.add_space(5.0);
    }
    if verbose {
        if kind == Detail::None && !tool.args.is_empty() {
            code_block(ui, id.with("arguments"), &tool.args);
        }
        if !tool.output.is_empty() && !matches!(kind, Detail::Error | Detail::Script) {
            code_block(ui, id.with("output"), &tool.output);
        }
    }
}

fn code_block(ui: &mut egui::Ui, id: egui::Id, source: &str) {
    let colors = crate::theme::palette(ui.ctx());
    egui::Frame::new()
        .fill(colors.code_bg)
        .corner_radius(7)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::both()
                .id_salt(id.with("code"))
                .max_height(260.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(source)
                                .monospace()
                                .size(11.0)
                                .line_height(Some(18.0)),
                        )
                        .extend()
                        .selectable(true),
                    );
                });
        });
}

fn diff_block(ui: &mut egui::Ui, id: egui::Id, tool: &Tool) {
    let colors = crate::theme::palette(ui.ctx());
    egui::Frame::new()
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(8)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::Frame::new()
                .fill(colors.surface)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 90.0).max(30.0);
                        ui.add_sized(
                            [width, 16.0],
                            egui::Label::new(RichText::new(&tool.path).monospace().size(11.0))
                                .truncate(),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("−{}", tool.removed))
                                    .size(10.0)
                                    .color(colors.red),
                            );
                            ui.label(
                                RichText::new(format!("+{}", tool.added))
                                    .size(10.0)
                                    .color(colors.green),
                            );
                        });
                    });
                });
            egui::ScrollArea::both()
                .id_salt(id.with("diff"))
                .max_height(300.0)
                .auto_shrink([false, true])
                .show(ui, |ui| markdown::diff(ui, &tool.diff));
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_and_generic_successes_are_plain_rows() {
        for name in ["read", "enter_worktree"] {
            assert_eq!(
                detail(&Tool {
                    name: name.into(),
                    output: "file contents".into(),
                    finished: true,
                    ..Default::default()
                }),
                Detail::None
            );
        }
    }
    #[test]
    fn commands_edits_and_failures_have_specific_details() {
        assert_eq!(
            detail(&Tool {
                name: "bash".into(),
                command: "echo hello".into(),
                ..Default::default()
            }),
            Detail::Command
        );
        assert_eq!(
            detail(&Tool {
                name: "edit".into(),
                diff: "+new\n-old".into(),
                ..Default::default()
            }),
            Detail::Diff
        );
        assert_eq!(
            detail(&Tool {
                name: "read".into(),
                failed: true,
                output: "permission denied".into(),
                ..Default::default()
            }),
            Detail::Error
        );
    }
    #[test]
    fn codemode_success_exposes_nested_actions_script_and_result() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tool = Tool {
            id: "script".into(),
            name: "codemode".into(),
            finished: true,
            output: "read completed".into(),
            ..Default::default()
        };
        tool.set_args(&serde_json::json!({"code":"text(await tools.read({path: \"main.rs\"}));"}));
        tool.set_nested(&serde_json::json!({"details":{"calls":[{"id":"script/1","name":"read","args":"{\"path\":\"main.rs\"}","status":"ok"}]}}));
        assert_eq!(detail(&tool), Detail::Script);
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let id = ui.make_persistent_id((&tool.id, "detail"));
                ui.ctx().data_mut(|d| d.insert_temp(id, true));
                row(ui, &tool);
            });
        });
        fn collect(shape: &egui::Shape, text: &mut String) {
            match shape {
                egui::Shape::Text(t) => text.push_str(&t.galley.job.text),
                egui::Shape::Vec(v) => {
                    for s in v {
                        collect(s, text)
                    }
                }
                _ => {}
            }
        }
        let mut text = String::new();
        for shape in output.shapes {
            collect(&shape.shape, &mut text);
        }
        assert!(text.contains("Read main.rs"));
        assert!(text.contains("text(await tools.read"));
        assert!(text.contains("read completed"));
    }
    #[test]
    fn timings_are_quiet_and_compact() {
        assert_eq!(duration(13), "13ms");
        assert_eq!(duration(1250), "1.2s");
        assert_eq!(duration(61_000), "1m 1s");
    }
    #[test]
    fn expanded_command_has_no_result_inspector_labels() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let tool = Tool {
            id: "cmd".into(),
            name: "bash".into(),
            command: "echo hello".into(),
            output: "hello".into(),
            args: "{}".into(),
            finished: true,
            duration_ms: Some(44),
            ..Default::default()
        };
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let id = ui.make_persistent_id((&tool.id, "detail"));
                ui.ctx().data_mut(|d| d.insert_temp(id, true));
                row(ui, &tool);
            });
        });
        fn texts(shape: &egui::Shape, found: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => found.push(text.galley.job.text.clone()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        texts(shape, found);
                    }
                }
                _ => {}
            }
        }
        let mut labels = Vec::new();
        for shape in output.shapes {
            texts(&shape.shape, &mut labels);
        }
        assert!(labels.iter().any(|s| s == "echo hello"));
        assert!(labels.iter().any(|s| s == "44ms"));
        for unwanted in ["Output", "Copy", "Arguments", "hello"] {
            assert!(
                !labels.iter().any(|s| s == unwanted),
                "Unexpected label: {unwanted}"
            );
        }
    }
}
