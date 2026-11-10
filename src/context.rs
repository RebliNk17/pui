//! Context metadata only. Unsupported Pi fields are never presented as real measurements.
use crate::{inspector::detail, protocol::State};
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
    let used = state.context_tokens;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(
                used.filter(|_| window > 0.0)
                    .map(|t| format!("{:.0}% used", t / window * 100.0))
                    .unwrap_or_else(|| "Not reported".into()),
            )
            .size(12.0),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(
                    used.map(|t| format!("{} / {}", number(t), number(window)))
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
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 5.0), Sense::hover());
    ui.painter().rect_filled(rect, 3, colors.hover);
    if demo {
        let mut x = rect.left();
        for (_, tokens, color) in demo_cats {
            let width = (tokens / window * rect.width() as f64) as f32;
            ui.painter().rect_filled(
                egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(width, 5.0)),
                0,
                color,
            );
            x += width;
        }
    } else if let Some(tokens) = used.filter(|_| window > 0.0) {
        let mut fill = rect;
        fill.set_width((tokens / window) as f32 * rect.width());
        ui.painter().rect_filled(fill, 3, colors.blue);
    }
    ui.add_space(8.0);
    if demo {
        for (label, tokens, color) in demo_cats {
            if label == "Messages" {
                let id = ui.make_persistent_id("message-context");
                let mut open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
                let response = category(ui, label, tokens, window, color, true, open);
                if response.clicked() {
                    open = !open;
                    ui.ctx().data_mut(|d| d.insert_temp(id, open));
                }
                if open {
                    ui.indent("parts", |ui| {
                        for (name, value) in [
                            ("You", 14000.0),
                            ("Replies & thinking", 92000.0),
                            ("Tool results", 367000.0),
                        ] {
                            category(ui, name, value, window, color, false, false);
                        }
                    });
                }
            } else {
                category(ui, label, tokens, window, color, false, false);
            }
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
            );
            category(
                ui,
                "Free space",
                (window - tokens).max(0.0),
                window,
                colors.hover,
                false,
                false,
            );
        }
        ui.add_space(5.0);
        ui.label(
            RichText::new("Category breakdown not supplied by Pi RPC.")
                .size(10.0)
                .color(colors.dim),
        );
    }
    ui.add_space(7.0);
    crate::section::Section::new("Usage details").summary("Tokens and prompt cache").icon(crate::icons::Icon::Details).id_salt("usage-details").show(ui,|ui|{
        ui.label(RichText::new("Prompt cache").size(12.0));
        if let Some(pct)=state.messages.iter().rev().find_map(|m|m.cached_pct){detail(ui,"Last turn",&format!("{pct:.0}% cached"));}
        ui.add_space(7.0);ui.label(RichText::new("This session").size(12.0));
        detail(ui,"In / Out",&format!("{} / {}",number(state.tokens["input"].as_f64().unwrap_or_default()),number(state.tokens["output"].as_f64().unwrap_or_default())));
        detail(ui,"Cache r / w",&format!("{} / {}",number(state.tokens["cacheRead"].as_f64().unwrap_or_default()),number(state.tokens["cacheWrite"].as_f64().unwrap_or_default())));
        ui.add_space(8.0);ui.label(RichText::new(if demo{"Auto-compacts when 33k tokens remain. Example category sizes, not your live session."}else{"Usage is reported by Pi. Per-category context sizes and compaction reserve are not exposed by this RPC interface."}).size(10.0).color(colors.dim));
    });
}
fn category(
    ui: &mut egui::Ui,
    label: &str,
    tokens: f64,
    window: f64,
    color: egui::Color32,
    clickable: bool,
    open: bool,
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
    p.text(
        rect.left_center() + egui::vec2(15.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(11.0),
        colors.muted,
    );
    if clickable {
        crate::icons::chevron(
            ui,
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(76.0, 0.0),
                Vec2::splat(10.0),
            ),
            open,
            colors.dim,
        );
    }
    p.text(
        rect.right_center() - egui::vec2(46.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        number(tokens),
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
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}
