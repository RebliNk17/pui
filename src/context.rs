//! Pi's reported total with clearly labeled, adapter-derived category estimates.
use crate::{
    inspector::detail,
    protocol::{PromptUsage, State},
};
use eframe::egui::{self, Align, Layout, RichText, Sense, Vec2};

pub fn number(value: f64) -> String {
    if value >= 1_000_000.0 {
        format!("{:.1}m", value / 1_000_000.0)
    } else if value >= 1000.0 {
        format!("{:.0}k", value / 1000.0)
    } else {
        format!("{value:.0}")
    }
}

pub fn show(ui: &mut egui::Ui, state: &State, demo: bool) {
    let colors = crate::theme::palette(ui.ctx());
    let window = state.context_window;
    let parts = state.context_breakdown.as_ref();
    let estimated_total = state.context_tokens.is_none() && parts.is_some();
    let used = state.context_tokens.or_else(|| parts.map(|p| p.total()));
    let prefix = if estimated_total { "~" } else { "" };
    let scaled = parts.map(|p| p.scaled(used.unwrap_or_default()));
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(
                used.filter(|_| window > 0.0)
                    .map(|t| format!("{prefix}{:.0}% used", t / window * 100.0))
                    .unwrap_or_else(|| "Not reported".into()),
            )
            .size(12.0),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(
                    used.map(|t| format!("{prefix}{} / {}", number(t), number(window)))
                        .unwrap_or_else(|| "—".into()),
                )
                .size(11.0)
                .color(colors.muted),
            );
        });
    });
    let demo_cats = [
        ("Messages", 473000.0, colors.blue),
        ("MCP tools", 2900.0, colors.blue),
        ("Tools", 1600.0, egui::Color32::from_rgb(47, 157, 143)),
        (
            "System prompt",
            1200.0,
            egui::Color32::from_rgb(107, 114, 128),
        ),
        ("Autocompact buffer", 33000.0, colors.dim),
        ("Free space", 88300.0, colors.hover),
    ];
    let live_cats = scaled.as_ref().map(|p| {
        vec![
            (
                "Messages",
                p.user + p.assistant + p.tool_results,
                colors.blue,
            ),
            ("MCP tools", p.mcp_tools, colors.accent),
            ("Tools", p.tools, egui::Color32::from_rgb(47, 157, 143)),
            ("System prompt", p.system, colors.muted),
            ("Other context", p.other, colors.dim),
            (
                "Free space",
                (window - used.unwrap_or_default()).max(0.0),
                colors.hover,
            ),
        ]
    });
    let cats = if demo {
        Some(demo_cats.as_slice())
    } else {
        live_cats.as_deref()
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 5.0), Sense::hover());
    ui.painter().rect_filled(rect, 3, colors.hover);
    if let Some(cats) = cats.filter(|_| window > 0.0) {
        let mut x = rect.left();
        for &(_, tokens, color) in cats {
            let width = ((tokens / window).clamp(0.0, 1.0) * rect.width() as f64) as f32;
            let width = width.min((rect.right() - x).max(0.0));
            ui.painter().rect_filled(
                egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(width, 5.0)),
                0,
                color,
            );
            x += width;
        }
    } else if let Some(tokens) = used.filter(|_| window > 0.0) {
        let mut fill = rect;
        fill.set_width((tokens / window).clamp(0.0, 1.0) as f32 * rect.width());
        ui.painter().rect_filled(fill, 3, colors.blue);
    }
    ui.add_space(8.0);
    if let Some(cats) = cats {
        for &(label, tokens, color) in cats {
            if tokens == 0.0 && label == "Other context" {
                continue;
            }
            if label == "Messages" {
                let id = ui.make_persistent_id("message-context");
                let mut open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
                let response = category(ui, label, tokens, window, color, true, open, !demo);
                if response.clicked() {
                    open = !open;
                    ui.ctx().data_mut(|d| d.insert_temp(id, open));
                }
                if open {
                    ui.indent("parts", |ui| {
                        let values = scaled
                            .as_ref()
                            .filter(|_| !demo)
                            .map(|p| [p.user, p.assistant, p.tool_results])
                            .unwrap_or([14000.0, 92000.0, 367000.0]);
                        for (name, value) in ["You", "Replies & thinking", "Tool results"]
                            .into_iter()
                            .zip(values)
                        {
                            category(ui, name, value, window, color, false, false, !demo);
                        }
                    });
                }
            } else {
                category(
                    ui,
                    label,
                    tokens,
                    window,
                    color,
                    false,
                    false,
                    !demo && (label != "Free space" || estimated_total),
                );
            }
        }
        if !demo {
            ui.add_space(5.0);
            ui.label(RichText::new("~ Estimated split of Pi context").size(10.0).color(colors.dim))
                .on_hover_text("Category proportions use Pi’s character-based token estimator on the active context, including prompt and tool schemas. They are scaled to Pi’s reported total when available; provider tokenization, hidden overhead and later extension changes can differ.");
        }
    } else {
        if let Some(tokens) = used {
            category(
                ui,
                "Context (reported)",
                tokens,
                window,
                colors.blue,
                false,
                false,
                false,
            );
            category(
                ui,
                "Free space",
                (window - tokens).max(0.0),
                window,
                colors.hover,
                false,
                false,
                false,
            );
        }
        ui.add_space(5.0);
        ui.label(
            RichText::new("Category estimates appear when the context adapter connects.")
                .size(10.0)
                .color(colors.dim),
        );
    }
    ui.add_space(7.0);
    crate::section::Section::new("Usage details").summary("Tokens and prompt cache").icon(crate::icons::Icon::Details).id_salt("usage-details").show(ui,|ui|{
        ui.label(RichText::new("Prompt cache").size(12.0));
        let usage = PromptUsage::last_response(&state.messages);
        if let Some(label) = usage.cache_label() {
            ui.scope(|ui| detail(ui, "Last response", &label)).response.on_hover_text(usage.cache_tooltip());
        }
        ui.add_space(7.0);ui.label(RichText::new("This session").size(12.0));
        detail(ui,"In / Out",&format!("{} / {}",number(state.tokens["input"].as_f64().unwrap_or_default()),number(state.tokens["output"].as_f64().unwrap_or_default())));
        detail(ui,"Cache r / w",&format!("{} / {}",number(state.tokens["cacheRead"].as_f64().unwrap_or_default()),number(state.tokens["cacheWrite"].as_f64().unwrap_or_default())));
        ui.add_space(8.0);ui.label(RichText::new(if demo{"Auto-compacts when 33k tokens remain. Example category sizes, not your live session."}else{"Session totals are reported by Pi. Context categories are estimates from the active prompt, tool schemas and compaction-aware messages, not cumulative session usage. Replies include thinking and tool calls; other context includes summaries and extension messages. Free space does not subtract an auto-compaction reserve."}).size(10.0).color(colors.dim));
    });
}
#[allow(clippy::too_many_arguments)]
fn category(
    ui: &mut egui::Ui,
    label: &str,
    tokens: f64,
    window: f64,
    color: egui::Color32,
    clickable: bool,
    open: bool,
    estimated: bool,
) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 20.0),
        if clickable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if clickable && response.hovered() {
        ui.painter().rect_filled(rect, 4, colors.hover);
    }
    let p = ui.painter();
    p.rect_filled(
        egui::Rect::from_center_size(rect.left_center() + egui::vec2(4.0, 0.0), Vec2::splat(7.0)),
        2,
        color,
    );
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.into(),
        egui::FontId::proportional(11.0),
        colors.muted,
    );
    job.wrap.max_width = (rect.width() - 107.0).max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = p.layout_job(job);
    p.galley(
        rect.left_center() + egui::vec2(15.0, -galley.size().y / 2.0),
        galley,
        colors.muted,
    );
    if clickable {
        crate::icons::chevron(
            ui,
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2((rect.width() - 87.0).min(76.0), 0.0),
                Vec2::splat(10.0),
            ),
            open,
            colors.dim,
        );
    }
    p.text(
        rect.right_center() - egui::vec2(46.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        format!("{}{}", if estimated { "~" } else { "" }, number(tokens)),
        egui::FontId::proportional(10.0),
        colors.muted,
    );
    p.text(
        rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        if window > 0.0 {
            format!("{:.1}%", tokens / window * 100.0)
        } else {
            "—".into()
        },
        egui::FontId::proportional(10.0),
        colors.text,
    );
    if clickable {
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(label)
    } else {
        response.on_hover_text(label)
    }
}

#[cfg(test)]
mod tests;
