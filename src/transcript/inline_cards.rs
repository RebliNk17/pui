//! Inline chat cards for edited HTML files, Browser previews (`pui_open`), and local file links.
use crate::{
    icons::{self, Icon},
    inline_visual::InlineVisualHost,
    media::Gallery,
    protocol::Tool,
    transcript::TranscriptAction,
};
use eframe::egui::{self, Align, FontId, Layout, Rect, RichText, Sense, Stroke, Vec2};
use std::path::Path;

pub(super) fn middle_truncate(name: &str, max_chars: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= max_chars || max_chars < 8 {
        return name.to_owned();
    }
    let tail_len = (max_chars / 2).min(10);
    let head_len = max_chars.saturating_sub(tail_len + 1);
    let head: String = chars[..head_len].iter().collect();
    let tail: String = chars[chars.len() - tail_len..].iter().collect();
    format!("{head}…{tail}")
}

fn display_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or(path)
        .to_owned()
}

fn host_or_name(target: &str) -> String {
    if let Ok(url) = url::Url::parse(target)
        && matches!(url.scheme(), "http" | "https")
        && let Some(host) = url.host_str()
    {
        return match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
    }
    display_name(target)
}

fn extension_badge(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty() && e.len() <= 5)
        .map(|e| e.to_ascii_uppercase())
        .unwrap_or_else(|| "FILE".into())
}

/// Compact pill card matching `[ [HTML] campaign…igns.html [↗] ]`.
pub(super) fn compact_file_card(
    ui: &mut egui::Ui,
    path: &str,
    badge: &str,
    action: &mut Option<TranscriptAction>,
) {
    let colors = crate::theme::palette(ui.ctx());
    let filename = display_name(path);
    let truncated = middle_truncate(&filename, 22);
    let badge_galley = ui.painter().layout_no_wrap(
        badge.to_owned(),
        FontId::new(9.5, egui::FontFamily::Name("body-bold".into())),
        colors.muted,
    );
    let name_galley =
        ui.painter()
            .layout_no_wrap(truncated, FontId::proportional(12.5), colors.text);
    let badge_w = (badge_galley.size().x + 14.0).max(36.0);
    let card_w = (12.0 + badge_w + 10.0 + name_galley.size().x + 14.0 + 28.0 + 8.0)
        .min(ui.available_width().max(160.0))
        .max(160.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(card_w, 40.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if response.hovered() {
            colors.hover
        } else {
            colors.surface
        };
        let stroke_color = if response.hovered() {
            colors.muted
        } else {
            colors.border
        };
        ui.painter().rect_filled(rect, 10, fill);
        ui.painter().rect_stroke(
            rect,
            10,
            Stroke::new(1.0_f32, stroke_color),
            egui::StrokeKind::Inside,
        );
        // Pill badge on left
        let badge_rect = Rect::from_center_size(
            egui::pos2(rect.left() + 10.0 + badge_w * 0.5, rect.center().y),
            Vec2::new(badge_w, 22.0),
        );
        ui.painter().rect_filled(badge_rect, 6, colors.bg);
        ui.painter().rect_stroke(
            badge_rect,
            6,
            Stroke::new(1.0_f32, colors.border),
            egui::StrokeKind::Inside,
        );
        ui.painter().galley(
            badge_rect.center() - badge_galley.size() * 0.5,
            badge_galley,
            colors.muted,
        );
        // Action icon tile on right
        let btn_rect = Rect::from_center_size(
            egui::pos2(rect.right() - 20.0, rect.center().y),
            Vec2::splat(26.0),
        );
        ui.painter().rect_filled(btn_rect, 6, colors.bg);
        icons::draw(
            ui,
            Rect::from_center_size(btn_rect.center(), Vec2::splat(13.0)),
            if badge == "HTML" {
                Icon::Expand
            } else {
                Icon::File
            },
            if response.hovered() {
                colors.text
            } else {
                colors.muted
            },
        );
        // Filename in center, clipped cleanly before action button
        let text_left = badge_rect.right() + 10.0;
        let text_right = btn_rect.left() - 8.0;
        if text_right > text_left + 12.0 {
            let clip = Rect::from_min_max(
                egui::pos2(text_left, rect.top()),
                egui::pos2(text_right, rect.bottom()),
            );
            ui.painter().with_clip_rect(clip).galley(
                egui::pos2(text_left, rect.center().y - name_galley.size().y * 0.5),
                name_galley,
                colors.text,
            );
        }
    }
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{path}\nClick to open in Pui"));
    if response.clicked() {
        *action = Some(TranscriptAction::Open(path.to_owned()));
    }
    response.context_menu(|ui| {
        crate::theme::menu_controls(ui);
        if ui
            .button(if badge == "HTML" {
                "Open in Browser"
            } else {
                "Open in Files"
            })
            .clicked()
        {
            *action = Some(TranscriptAction::Open(path.to_owned()));
            ui.close();
        }
        if badge == "HTML" && ui.button("View Source in Files").clicked() {
            *action = Some(TranscriptAction::ViewSource(path.to_owned()));
            ui.close();
        }
        if ui.button("Copy Path").clicked() {
            ui.ctx().copy_text(path.to_owned());
            ui.close();
        }
    });
}

/// Render compact HTML file boxes for any `.html`/`.htm` files edited/written in an activity run.
pub(super) fn html_file_cards(
    ui: &mut egui::Ui,
    tools: &[&Tool],
    action: &mut Option<TranscriptAction>,
) {
    let mut paths = Vec::new();
    for tool in tools {
        for path in tool.edited_html_files() {
            if !paths.iter().any(|existing| existing == &path) {
                paths.push(path);
            }
        }
    }
    if paths.is_empty() {
        return;
    }
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(8.0);
        for path in &paths {
            compact_file_card(ui, path, "HTML", action);
        }
    });
    ui.add_space(6.0);
}

/// Inline presentation card for a `pui_open` tool call:
/// - Local image -> full-width inline photo view
/// - HTTP/HTTPS URL or local HTML -> Browser preview card with thumbnail, title, Open button & menu
/// - Other local file -> compact file card
pub(super) fn open_card(
    ui: &mut egui::Ui,
    tool: &Tool,
    gallery: &mut Gallery,
    visuals: &mut InlineVisualHost,
    cwd: Option<&Path>,
    action: &mut Option<TranscriptAction>,
) {
    let Some(target) = tool.open_target.as_deref() else {
        return;
    };
    let is_url = target.starts_with("http://") || target.starts_with("https://");
    let is_html = !is_url && crate::activity::is_html_path(target);
    if !is_url && crate::activity::is_image_path(target) {
        if let Some(attachment) = crate::markdown::resolve_local_image(target, cwd) {
            let name = display_name(target);
            gallery.inline_photo(
                ui,
                &attachment,
                std::slice::from_ref(&attachment),
                0,
                Some(&name),
            );
            return;
        }
    }
    if !is_url && !is_html {
        ui.add_space(4.0);
        compact_file_card(ui, target, &extension_badge(target), action);
        ui.add_space(6.0);
        return;
    }

    // Browser preview card (Screenshot 3 style)
    let colors = crate::theme::palette(ui.ctx());
    let card_width = ui.available_width().clamp(220.0, 500.0);
    let preview_height = (card_width * 0.42).clamp(140.0, 220.0);
    let host_label = host_or_name(target);
    let title = visuals
        .page_snapshots
        .title(target)
        .unwrap_or_else(|| host_label.clone());
    let subtitle = if tool.open_queued {
        format!("{host_label}, queued for Browser")
    } else {
        format!("{host_label}, opened in Browser")
    };

    ui.add_space(6.0);
    egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0_f32, colors.border))
        .corner_radius(10)
        .inner_margin(0)
        .show(ui, |ui| {
            ui.set_width((card_width - 2.0).max(200.0));
            ui.spacing_mut().item_spacing.y = 0.0;

            // Top preview image / placeholder area
            let (preview_rect, preview_resp) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), preview_height),
                Sense::click(),
            );
            if ui.is_rect_visible(preview_rect) {
                let top_radius = egui::CornerRadius {
                    nw: 9,
                    ne: 9,
                    sw: 0,
                    se: 0,
                };
                ui.painter()
                    .rect_filled(preview_rect, top_radius, colors.bg);
                if let Some(tex) = visuals.page_texture(ui.ctx(), target) {
                    let uv = top_cover_uv(tex.size_vec2(), preview_rect.size());
                    egui::Image::new((tex.id(), preview_rect.size()))
                        .uv(uv)
                        .corner_radius(top_radius)
                        .paint_at(ui, preview_rect);
                } else {
                    let icon_rect = Rect::from_center_size(
                        preview_rect.center() - egui::vec2(0.0, 12.0),
                        Vec2::splat(26.0),
                    );
                    icons::draw(
                        ui,
                        icon_rect,
                        if is_url { Icon::Globe } else { Icon::Code },
                        colors.dim,
                    );
                    ui.painter().text(
                        preview_rect.center() + egui::vec2(0.0, 16.0),
                        egui::Align2::CENTER_CENTER,
                        &host_label,
                        FontId::proportional(11.5),
                        colors.muted,
                    );
                }
            }
            if preview_resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(format!("Open {target} in Browser"))
                .clicked()
            {
                *action = Some(TranscriptAction::Open(target.to_owned()));
            }

            // Divider line
            let (div_rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
            ui.painter().line_segment(
                [div_rect.left_center(), div_rect.right_center()],
                Stroke::new(1.0_f32, colors.border),
            );

            // Footer bar with title, subtitle, Open button, and "..." menu
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            let more = ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("•••").size(11.0).color(colors.muted),
                                    )
                                    .frame(false)
                                    .min_size(Vec2::new(26.0, 28.0)),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text("More preview actions");
                            egui::Popup::menu(&more).show(|ui| {
                                crate::theme::menu_controls(ui);
                                if ui.button("Open in Browser").clicked() {
                                    *action = Some(TranscriptAction::Open(target.to_owned()));
                                    ui.close();
                                }
                                if is_html && ui.button("View Source in Files").clicked() {
                                    *action = Some(TranscriptAction::ViewSource(target.to_owned()));
                                    ui.close();
                                }
                                if ui
                                    .button(if is_url { "Copy URL" } else { "Copy Path" })
                                    .clicked()
                                {
                                    ui.ctx().copy_text(target.to_owned());
                                    ui.close();
                                }
                                if is_url && ui.button("Open in Default Browser").clicked() {
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(target));
                                    ui.close();
                                }
                            });

                            let open_btn = ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Open").size(12.0).color(colors.text),
                                    )
                                    .fill(colors.hover)
                                    .stroke(Stroke::new(1.0_f32, colors.border))
                                    .corner_radius(7)
                                    .min_size(Vec2::new(56.0, 28.0)),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            if open_btn.clicked() {
                                *action = Some(TranscriptAction::Open(target.to_owned()));
                            }

                            ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&title)
                                            .font(FontId::new(
                                                13.0,
                                                egui::FontFamily::Name("body-bold".into()),
                                            ))
                                            .color(colors.text),
                                    )
                                    .truncate(),
                                );
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&subtitle).size(11.0).color(colors.muted),
                                    )
                                    .truncate(),
                                );
                            });
                        });
                    });
                });
        });
    ui.add_space(6.0);
}

/// Compact inline transcript card for a `pui_workflow` call (Claude Desktop `VIe` parity):
/// title + chevron, agent count + elapsed time + tokens, live 6×6px agent status micro-grid.
pub(super) fn workflow_card(
    ui: &mut egui::Ui,
    tool: &Tool,
    visuals: Option<&InlineVisualHost>,
    action: &mut Option<TranscriptAction>,
) {
    let Some(workflow_id) = tool.workflow_id.as_deref() else {
        return;
    };
    let colors = crate::theme::palette(ui.ctx());
    let snap = visuals.and_then(|v| v.workflow_cards.get(workflow_id));
    let title = snap
        .map(|s| s.title.as_str())
        .filter(|s| !s.is_empty())
        .or(tool.workflow_title.as_deref())
        .unwrap_or("Workflow");

    let card_width = ui.available_width().clamp(220.0, 340.0);
    ui.add_space(6.0);
    let resp = egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0_f32, colors.border))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width((card_width - 26.0).max(180.0));
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                icons::inline(ui, Icon::Agents, colors.accent);
                ui.add(
                    egui::Label::new(RichText::new(title).size(12.5).color(colors.text)).truncate(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("›").size(14.0).color(colors.dim));
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(s) = snap {
                    let status_label = if s.running {
                        "Workflow"
                    } else if s.failed {
                        "Failed"
                    } else if s.stopped {
                        "Stopped"
                    } else {
                        "Done"
                    };
                    let status_color = if s.failed {
                        colors.red
                    } else if s.running {
                        colors.accent
                    } else {
                        colors.muted
                    };
                    ui.label(RichText::new(status_label).size(11.0).color(status_color));
                    if s.total_agents > 0 {
                        let summary = if s.running && s.done_agents > 0 {
                            format!("· {}/{} agents", s.done_agents, s.total_agents)
                        } else if s.total_agents == 1 {
                            "· 1 agent".to_owned()
                        } else {
                            format!("· {} agents", s.total_agents)
                        };
                        ui.label(RichText::new(summary).size(11.0).color(colors.muted));
                    }
                    if s.failed_agents > 0 {
                        ui.label(
                            RichText::new(format!("· {} failed", s.failed_agents))
                                .size(11.0)
                                .color(colors.red),
                        );
                    }
                    if s.total_tokens > 0 {
                        ui.label(
                            RichText::new(format!(
                                "· {} tok",
                                crate::inline_visual::format_token_count(s.total_tokens)
                            ))
                            .size(11.0)
                            .color(colors.dim),
                        );
                    }
                    if s.elapsed_ms > 0 || s.running {
                        let secs = s.elapsed_ms / 1000;
                        let dur = if secs >= 3600 {
                            format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
                        } else if secs >= 60 {
                            format!("{}m {:02}s", secs / 60, secs % 60)
                        } else {
                            format!("{secs}s")
                        };
                        ui.label(
                            RichText::new(format!("· {dur}"))
                                .size(11.0)
                                .color(colors.dim),
                        );
                    }
                } else {
                    ui.label(
                        RichText::new("Workflow · open to inspect")
                            .size(11.0)
                            .color(colors.muted),
                    );
                }
            });
            if let Some(s) = snap
                && !s.cells.is_empty()
            {
                ui.add_space(2.0);
                crate::inline_visual::paint_workflow_cell_grid(ui, &s.cells, s.running, 6);
            }
        })
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Open workflow in Background tasks");
    if resp.clicked() {
        *action = Some(TranscriptAction::OpenWorkflow(workflow_id.to_owned()));
    }
    ui.add_space(6.0);
}

fn format_card_duration(ms: u64) -> String {
    let secs = ms / 1000;
    if secs >= 3600 {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

/// Claude-style inline transcript card for a `pui_background_agent` or `pui_spawn_agent` call:
/// icon, title + chevron, status + stats (`Done · 12 tool uses · 14.2k tok · 34s`, `Running…`,
/// or red `Failed`), compact muted result preview when finished, and click-to-open.
pub(super) fn agent_card(
    ui: &mut egui::Ui,
    tool: &Tool,
    visuals: Option<&InlineVisualHost>,
    action: &mut Option<TranscriptAction>,
) {
    let Some(agent_id) = tool.agent_id.as_deref() else {
        return;
    };
    let colors = crate::theme::palette(ui.ctx());
    let snap = visuals.and_then(|v| v.agent_cards.get(agent_id));
    let is_background = snap
        .map(|s| s.is_background)
        .unwrap_or_else(|| tool.name == "pui_background_agent");
    let title = snap
        .map(|s| s.title.as_str())
        .filter(|s| !s.is_empty())
        .or(tool.agent_title.as_deref())
        .unwrap_or(if is_background {
            "Background subagent"
        } else {
            "Child session"
        });

    let icon_color = match snap {
        Some(s) if s.failed => colors.red,
        Some(s) if s.running => colors.accent,
        _ => colors.muted,
    };

    let card_width = ui.available_width().clamp(240.0, 520.0);
    ui.add_space(6.0);
    let resp = egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0_f32, colors.border))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width((card_width - 26.0).max(200.0));
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                icons::inline(ui, Icon::Agents, icon_color);
                ui.add(
                    egui::Label::new(RichText::new(title).size(12.5).color(colors.text)).truncate(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("›").size(14.0).color(colors.dim));
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(s) = snap {
                    let (status_label, status_color) = if s.running {
                        ("Running…", colors.accent)
                    } else if s.failed {
                        ("Failed", colors.red)
                    } else if s.stopped {
                        ("Stopped", colors.dim)
                    } else {
                        ("Done", colors.muted)
                    };
                    ui.label(RichText::new(status_label).size(11.0).color(status_color));
                    if s.tool_uses > 0 {
                        let tools_label = if s.tool_uses == 1 {
                            "· 1 tool use".to_owned()
                        } else {
                            format!("· {} tool uses", s.tool_uses)
                        };
                        ui.label(RichText::new(tools_label).size(11.0).color(colors.muted));
                    }
                    if s.tokens > 0 {
                        ui.label(
                            RichText::new(format!(
                                "· {} tok",
                                crate::inline_visual::format_token_count(s.tokens)
                            ))
                            .size(11.0)
                            .color(colors.dim),
                        );
                    }
                    if s.elapsed_ms > 0 || s.running {
                        ui.label(
                            RichText::new(format!("· {}", format_card_duration(s.elapsed_ms)))
                                .size(11.0)
                                .color(colors.dim),
                        );
                    }
                    if s.running {
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_secs(1));
                    }
                } else {
                    let fallback = if is_background {
                        "Background subagent · open to inspect"
                    } else {
                        "Child session · open to inspect"
                    };
                    ui.label(RichText::new(fallback).size(11.0).color(colors.muted));
                }
            });
            if let Some(s) = snap {
                if s.failed
                    && let Some(err) = s.error.as_deref().filter(|e| !e.trim().is_empty())
                {
                    ui.add_space(2.0);
                    let mut job = egui::text::LayoutJob::simple(
                        err.to_owned(),
                        FontId::proportional(11.5),
                        colors.red,
                        ui.available_width(),
                    );
                    job.wrap.max_rows = 2;
                    ui.add(egui::Label::new(job));
                } else if !s.running
                    && let Some(preview) = s.preview.as_deref().filter(|p| !p.trim().is_empty())
                {
                    ui.add_space(2.0);
                    let mut job = egui::text::LayoutJob::simple(
                        preview.to_owned(),
                        FontId::proportional(11.5),
                        colors.muted,
                        ui.available_width(),
                    );
                    job.wrap.max_rows = 3;
                    ui.add(egui::Label::new(job));
                }
            }
        })
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(if is_background {
            "Open subagent in Background tasks"
        } else {
            "Open child session"
        });
    if resp.clicked() {
        *action = Some(TranscriptAction::OpenAgent(agent_id.to_owned()));
    }
    ui.add_space(6.0);
}

fn top_cover_uv(tex: Vec2, slot: Vec2) -> Rect {
    if tex.x <= 0.0 || tex.y <= 0.0 || slot.x <= 0.0 || slot.y <= 0.0 {
        return Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    }
    let tex_aspect = tex.x / tex.y;
    let slot_aspect = slot.x / slot.y;
    if tex_aspect > slot_aspect {
        let keep = (slot_aspect / tex_aspect).clamp(0.01, 1.0);
        let pad = (1.0 - keep) * 0.5;
        Rect::from_min_max(egui::pos2(pad, 0.0), egui::pos2(1.0 - pad, 1.0))
    } else {
        // Anchor at top of page (y=0) so website headers stay visible instead of cropping off the top!
        let keep = (tex_aspect / slot_aspect).clamp(0.01, 1.0);
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, keep))
    }
}
