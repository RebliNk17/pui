//! Quiet Mac prompt primitives. Presentation only: callers own consent and state.
use super::*;

pub(super) fn background(ctx: &egui::Context) -> Color32 {
    match ctx.theme() {
        egui::Theme::Light => Color32::from_rgb(253, 252, 254),
        egui::Theme::Dark => Color32::from_rgb(36, 35, 43),
    }
}

pub(super) fn subtle(ctx: &egui::Context) -> Color32 {
    match ctx.theme() {
        egui::Theme::Light => Color32::from_rgb(246, 245, 249),
        egui::Theme::Dark => Color32::from_rgb(29, 28, 35),
    }
}

pub(super) fn border(ctx: &egui::Context) -> Color32 {
    match ctx.theme() {
        egui::Theme::Light => Color32::from_rgb(225, 223, 232),
        egui::Theme::Dark => Color32::from_rgb(62, 59, 73),
    }
}

pub(super) fn surface(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::new()
        .fill(background(ctx))
        .stroke(Stroke::new(1.0, border(ctx)))
        .corner_radius(14)
        .inner_margin(24)
        .shadow(egui::epaint::Shadow {
            offset: [0, 10],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(44),
        })
}

pub(super) fn inline(ctx: &egui::Context) -> egui::Frame {
    surface(ctx)
        .inner_margin(18)
        .shadow(egui::epaint::Shadow::NONE)
}

pub(super) fn popup(ctx: &egui::Context) -> egui::Frame {
    surface(ctx).inner_margin(16)
}

pub(super) fn keyboard_available(ui: &egui::Ui) -> bool {
    ui.memory(|memory| memory.is_above_modal_layer(ui.layer_id()))
}

pub(super) fn style(ui: &mut egui::Ui) {
    let edge = border(ui.ctx());
    let fill = subtle(ui.ctx());
    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
    ui.spacing_mut().button_padding = egui::vec2(12.0, 7.0);
    ui.spacing_mut().interact_size.y = 32.0;
    ui.visuals_mut().extreme_bg_color = fill;
    ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(1.0, edge);
    ui.style_mut()
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(13.0));
}

pub(super) fn modal<R>(
    ctx: &egui::Context,
    id: egui::Id,
    width: f32,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::ModalResponse<R> {
    egui::Modal::new(id)
        .frame(surface(ctx))
        .backdrop_color(Color32::from_black_alpha(
            if ctx.theme() == egui::Theme::Light {
                28
            } else {
                100
            },
        ))
        .show(ctx, |ui| {
            // Width is the content width; account for both padding and screen margins.
            ui.set_width(width.min((ctx.screen_rect().width() - 80.0).max(120.0)));
            style(ui);
            content(ui)
        })
}

pub(super) fn badge(ui: &mut egui::Ui, icon: Icon, danger: bool) {
    let colors = theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
    ui.painter().rect_filled(
        rect,
        10,
        if danger {
            colors.error_bg
        } else {
            colors.accent_bg
        },
    );
    icons::draw(
        ui,
        rect.shrink(8.0),
        icon,
        if danger { colors.red } else { colors.accent },
    );
    ui.add_space(14.0);
}

pub(super) fn heading(ui: &mut egui::Ui, title: &str, description: &str) {
    let colors = theme::palette(ui.ctx());
    ui.add(egui::Label::new(RichText::new(title).size(18.0).strong().color(colors.text)).wrap());
    if !description.is_empty() {
        ui.add_space(3.0);
        ui.add(egui::Label::new(RichText::new(description).size(12.5).color(colors.muted)).wrap());
    }
}

pub(super) fn close(ui: &mut egui::Ui) -> bool {
    let rect = egui::Rect::from_min_size(
        ui.max_rect().right_top() + egui::vec2(-12.0, -12.0),
        Vec2::splat(22.0),
    );
    let response = ui.interact(rect, ui.id().with("prompt-close"), Sense::click());
    let colors = theme::palette(ui.ctx());
    if response.hovered() {
        ui.painter().rect_filled(rect, 5, subtle(ui.ctx()));
    }
    icons::draw(ui, rect.shrink(4.0), Icon::Close, colors.muted);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close"));
    response.on_hover_text("Close · Esc").clicked()
}

#[derive(Clone, Copy)]
pub(super) enum Button {
    Secondary,
    Primary,
    Danger,
}

pub(super) fn button(
    ui: &mut egui::Ui,
    label: &str,
    kind: Button,
    enabled: bool,
) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let (fill, text) = match kind {
        Button::Secondary => (background(ui.ctx()), colors.text),
        // White text keeps contrast in both themes; do not use the page background.
        Button::Primary => (Color32::from_rgb(112, 86, 185), Color32::WHITE),
        Button::Danger => (Color32::from_rgb(177, 57, 49), Color32::WHITE),
    };
    let stroke = match kind {
        Button::Secondary => border(ui.ctx()),
        _ => fill,
    };
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).size(12.0).color(text))
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke))
            .corner_radius(7)
            .min_size(egui::vec2(0.0, 32.0)),
    )
}

/// Bounded height, even inside a scroll area. Call in visual right-to-left order.
pub(super) fn footer<R>(
    ui: &mut egui::Ui,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), 32.0),
        Layout::right_to_left(Align::Center),
        content,
    )
}

pub(super) fn note(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new()
        .fill(subtle(ui.ctx()))
        .corner_radius(7)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(theme::small(ui, text)).wrap());
        });
}

pub(super) fn option(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    selected: bool,
) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let response = egui::Frame::new()
        .fill(if selected {
            colors.accent_bg
        } else {
            background(ui.ctx())
        })
        .stroke(Stroke::new(
            1.0,
            if selected {
                colors.accent
            } else {
                border(ui.ctx())
            },
        ))
        .corner_radius(7)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(title).size(12.5).color(colors.text)).wrap());
            if !description.is_empty() {
                ui.add(egui::Label::new(theme::small(ui, description)).wrap());
            }
        })
        .response;
    let response = ui.interact(response.rect, response.id.with("option"), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, title)
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
