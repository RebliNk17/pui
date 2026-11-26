//! Quiet tool rows: details stay attached to the action, not a generic result inspector.
pub(crate) mod answers;
use crate::{
    icons::{self, Icon},
    markdown,
    protocol::{DenialKind, Tool},
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
    row_with_view(ui, tool, false, None);
}

pub fn row_with_gallery(ui: &mut egui::Ui, tool: &Tool, gallery: &mut crate::media::Gallery) {
    row_with_view(ui, tool, false, Some(gallery));
}

pub fn row_verbose_with_gallery(
    ui: &mut egui::Ui,
    tool: &Tool,
    gallery: &mut crate::media::Gallery,
) {
    row_with_view(ui, tool, true, Some(gallery));
}

fn denial_badge_style(
    kind: DenialKind,
    colors: &crate::theme::Palette,
) -> (egui::Color32, Stroke, egui::Color32) {
    match kind {
        DenialKind::AutoMode | DenialKind::Sandbox | DenialKind::ClassifierUnavailable => (
            colors.amber.gamma_multiply(0.14),
            Stroke::new(1.0_f32, colors.amber.gamma_multiply(0.45)),
            colors.amber,
        ),
        DenialKind::PolicyRule => (
            colors.surface,
            Stroke::new(1.0_f32, colors.border),
            colors.muted,
        ),
    }
}

fn denial_reason_callout(ui: &mut egui::Ui, kind: DenialKind, reason: &str) {
    let colors = crate::theme::palette(ui.ctx());
    let (fill, stroke) = match kind {
        DenialKind::AutoMode | DenialKind::Sandbox | DenialKind::ClassifierUnavailable => (
            colors.amber.gamma_multiply(0.08),
            Stroke::new(1.0_f32, colors.amber.gamma_multiply(0.28)),
        ),
        DenialKind::PolicyRule => (colors.surface, Stroke::new(1.0_f32, colors.border)),
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(7)
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    RichText::new(reason)
                        .size(11.0)
                        .line_height(Some(16.0))
                        .color(colors.text),
                )
                .wrap()
                .selectable(true),
            );
        });
}

fn row_with_view(
    ui: &mut egui::Ui,
    tool: &Tool,
    verbose: bool,
    mut gallery: Option<&mut crate::media::Gallery>,
) {
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
    let denial_reason = tool
        .denial_kind
        .and(tool.denial_reason.as_deref())
        .filter(|s| !s.is_empty());
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 22.0),
        if expandable && !verbose {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let response = if expandable && !verbose {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    let response = if let Some(reason) = denial_reason {
        response.on_hover_text(reason)
    } else {
        response
    };
    let icon_color = match tool.denial_kind {
        Some(DenialKind::AutoMode | DenialKind::Sandbox | DenialKind::ClassifierUnavailable) => {
            colors.amber
        }
        Some(DenialKind::PolicyRule) => colors.muted,
        None if tool.failed => colors.red,
        None if command && tool.finished => colors.green,
        None => colors.dim,
    };
    icons::draw(
        ui,
        Rect::from_center_size(rect.left_center() + egui::vec2(7.0, 0.0), Vec2::splat(14.0)),
        icon,
        icon_color,
    );
    let p = ui.painter();
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
    let chevron_width = if expandable && !verbose { 16.0 } else { 0.0 };
    let denial_badge = tool.denial_kind.map(|denial_kind| {
        let (pill_fill, pill_stroke, pill_text) = denial_badge_style(denial_kind, colors);
        let max_pill_w = (rect.width() - 25.0 - time_width - chevron_width - 44.0).max(48.0);
        let mut badge_job = egui::text::LayoutJob::simple_singleline(
            denial_kind.badge().to_owned(),
            FontId::proportional(10.0),
            pill_text,
        );
        badge_job.wrap.max_width = (max_pill_w - 12.0).max(20.0);
        badge_job.wrap.max_rows = 1;
        badge_job.wrap.break_anywhere = true;
        badge_job.wrap.overflow_character = Some('…');
        let badge_galley = p.layout_job(badge_job);
        let pill_w = (badge_galley.size().x + 12.0).min(max_pill_w);
        let pill_right = rect.right() - time_width;
        let pill_rect = Rect::from_min_max(
            egui::pos2(pill_right - pill_w, rect.center().y - 8.5),
            egui::pos2(pill_right, rect.center().y + 8.5),
        );
        (pill_rect, pill_fill, pill_stroke, pill_text, badge_galley)
    });
    let status_width = if let Some((pill_rect, ..)) = &denial_badge {
        pill_rect.width() + 6.0
    } else if tool.failed || !tool.finished || command {
        20.0
    } else {
        0.0
    };
    let text_right =
        (rect.max.x - time_width - status_width - chevron_width - 4.0).max(rect.min.x + 25.0);
    let text_rect = Rect::from_min_max(
        rect.min + egui::vec2(25.0, 0.0),
        egui::pos2(text_right, rect.max.y),
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
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = p.layout_job(job);
    p.with_clip_rect(text_rect.intersect(ui.clip_rect()))
        .galley(
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
    if let Some((pill_rect, pill_fill, pill_stroke, pill_text, badge_galley)) = denial_badge {
        p.rect(
            pill_rect,
            8.5,
            pill_fill,
            pill_stroke,
            egui::StrokeKind::Inside,
        );
        let badge_clip = pill_rect
            .shrink2(egui::vec2(5.0, 0.0))
            .intersect(ui.clip_rect());
        p.with_clip_rect(badge_clip).galley(
            egui::pos2(
                pill_rect.left() + 6.0,
                pill_rect.center().y - badge_galley.size().y / 2.0,
            ),
            badge_galley,
            pill_text,
        );
    } else {
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
    if let Some(reason) = denial_reason
        && !(open && expandable)
    {
        let (reason_rect, reason_resp) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
        let text_area = Rect::from_min_max(
            reason_rect.min + egui::vec2(25.0, 0.0),
            reason_rect.max - egui::vec2(4.0, 0.0),
        );
        if text_area.width() > 8.0 {
            let mut reason_job = egui::text::LayoutJob::simple_singleline(
                reason.to_owned(),
                FontId::proportional(11.0),
                colors.muted,
            );
            reason_job.wrap.max_width = text_area.width();
            reason_job.wrap.max_rows = 1;
            reason_job.wrap.break_anywhere = true;
            reason_job.wrap.overflow_character = Some('…');
            let reason_galley = ui.painter().layout_job(reason_job);
            ui.painter()
                .with_clip_rect(text_area.intersect(ui.clip_rect()))
                .galley(
                    text_area.left_center() - egui::vec2(0.0, reason_galley.size().y / 2.0),
                    reason_galley,
                    colors.muted,
                );
        }
        reason_resp.on_hover_text(reason);
    }
    if open && expandable {
        ui.add_space(4.0);
        ui.indent(id.with("inset"), |ui| {
            let shown_reason =
                if let (Some(denial_kind), Some(reason)) = (tool.denial_kind, denial_reason) {
                    denial_reason_callout(ui, denial_kind, reason);
                    Some(reason)
                } else {
                    None
                };
            match kind {
                Detail::Command => {
                    if shown_reason.is_some() {
                        ui.add_space(4.0);
                    }
                    code_block(ui, id, &tool.command);
                }
                Detail::Diff => diff_block(ui, id, tool),
                Detail::Error => {
                    if shown_reason.map(str::trim) != Some(tool.output.trim()) {
                        if shown_reason.is_some() {
                            ui.add_space(4.0);
                        }
                        code_block(ui, id, &tool.output);
                    }
                }
                Detail::Script => {
                    if shown_reason.is_some() {
                        ui.add_space(4.0);
                    }
                    for child in &tool.children {
                        row_with_view(ui, child, verbose, gallery.as_deref_mut());
                    }
                    if !tool.code.is_empty() {
                        code_block(ui, id.with("script"), &tool.code);
                    }
                    if !tool.output.is_empty() {
                        code_block(ui, id.with("script-result"), &tool.output);
                    }
                }
                Detail::None => {}
            }
        });
        ui.add_space(5.0);
    }
    if !tool.images.is_empty()
        && let Some(gallery) = gallery
    {
        ui.add_space(4.0);
        ui.indent(id.with("tool-images"), |ui| {
            gallery.thumbnails(ui, &tool.images);
        });
        ui.add_space(4.0);
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

    #[test]
    fn denied_tool_rows_render_distinct_badges_and_reasons_across_themes_and_widths() {
        fn collect_text_boxes(
            shape: &egui::Shape,
            clip: Rect,
            out: &mut Vec<(String, Rect, Rect)>,
        ) {
            match shape {
                egui::Shape::Text(text) => {
                    let bounds = text.galley.rect.translate(text.pos.to_vec2());
                    out.push((text.galley.job.text.clone(), bounds, clip));
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect_text_boxes(shape, clip, out);
                    }
                }
                _ => {}
            }
        }

        let cases = [
            (
                "auto-1",
                "bash",
                "rm -rf /Users/dev/very/long/protected/path/outside/workspace",
                DenialKind::AutoMode,
                "Built-in safety rule: destructive action or protected/external resource.",
                "Auto blocked bash: Built-in safety rule: destructive action or protected/external resource.\nPui Auto mode: ...",
            ),
            (
                "sandbox-1",
                "bash",
                "cat /Users/dev/.ssh/id_rsa",
                DenialKind::Sandbox,
                "sandbox-exec: /bin/bash: /Users/dev/.ssh/id_rsa: Operation not permitted",
                "sandbox-exec: /bin/bash: /Users/dev/.ssh/id_rsa: Operation not permitted",
            ),
            (
                "unavail-1",
                "edit",
                "src/very/long/nested/module/file_name_for_narrow_check.rs",
                DenialKind::ClassifierUnavailable,
                "Classifier check unavailable — google/flash: authentication failed.",
                "Auto blocked edit: Classifier check unavailable — google/flash: authentication failed.\nPui Auto mode: ...",
            ),
            (
                "policy-1",
                "write",
                "src/plan_blocked_output.rs",
                DenialKind::PolicyRule,
                "Plan mode is read-only. Approve the written plan and change mode in Pui.",
                "Plan mode is read-only. Approve the written plan and change mode in Pui.",
            ),
        ];

        for theme in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
            for width in [280.0_f32, 320.0, 640.0] {
                for expanded in [false, true] {
                    let ctx = egui::Context::default();
                    crate::theme::install(&ctx);
                    theme.apply(&ctx);
                    for (id_str, name, target, kind, reason, raw_output) in cases {
                        let tool = Tool {
                            id: id_str.into(),
                            name: name.into(),
                            command: if name == "bash" {
                                target.into()
                            } else {
                                String::new()
                            },
                            path: if name != "bash" {
                                target.into()
                            } else {
                                String::new()
                            },
                            output: raw_output.into(),
                            finished: true,
                            failed: true,
                            duration_ms: Some(18),
                            denial_kind: Some(kind),
                            denial_reason: Some(reason.into()),
                            ..Default::default()
                        };
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(width, 400.0),
                                )),
                                ..Default::default()
                            },
                            |ctx| {
                                egui::CentralPanel::default().show(ctx, |ui| {
                                    let id = ui.make_persistent_id((&tool.id, "detail"));
                                    ui.ctx().data_mut(|d| d.insert_temp(id, expanded));
                                    row(ui, &tool);
                                });
                            },
                        );
                        let mut text_boxes = Vec::new();
                        for clipped in output.shapes {
                            collect_text_boxes(&clipped.shape, clipped.clip_rect, &mut text_boxes);
                        }
                        let badge_entry = text_boxes
                            .iter()
                            .find(|(text, _, _)| text == kind.badge())
                            .unwrap_or_else(|| {
                                panic!(
                                    "missing badge {:?} (theme={theme:?}, width={width}, expanded={expanded}): {:?}",
                                    kind.badge(),
                                    text_boxes.iter().map(|(t, _, _)| t).collect::<Vec<_>>()
                                )
                            });
                        let reason_entry = text_boxes
                            .iter()
                            .find(|(text, _, _)| text == reason)
                            .unwrap_or_else(|| {
                                panic!(
                                    "missing denial reason {:?} (theme={theme:?}, width={width}, expanded={expanded}): {:?}",
                                    reason,
                                    text_boxes.iter().map(|(t, _, _)| t).collect::<Vec<_>>()
                                )
                            });
                        let label_text = tool.label();
                        let label_entry = text_boxes
                            .iter()
                            .find(|(text, _, _)| text == &label_text)
                            .unwrap_or_else(|| {
                                panic!(
                                    "missing tool label {:?} (theme={theme:?}, width={width})",
                                    label_text
                                )
                            });

                        // Clipped bounds of label, badge, and reason must stay inside viewport width
                        // and label clip rect must never overlap the badge bounds.
                        let label_visible = label_entry.1.intersect(label_entry.2);
                        let badge_visible = badge_entry.1.intersect(badge_entry.2);
                        let reason_visible = reason_entry.1.intersect(reason_entry.2);
                        assert!(
                            badge_visible.right() <= width + 0.5,
                            "badge overflowed width={width}: {badge_visible:?}"
                        );
                        assert!(
                            reason_visible.right() <= width + 0.5,
                            "reason overflowed width={width}: {reason_visible:?}"
                        );
                        assert!(
                            label_visible.right() <= badge_visible.left() + 0.5,
                            "label overlapped badge at width={width}: label={label_visible:?} badge={badge_visible:?}"
                        );
                    }
                }
            }
        }
    }
}
