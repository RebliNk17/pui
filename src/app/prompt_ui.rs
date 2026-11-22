//! Theme-aware prompt primitives. Presentation only: callers own consent and state.
use super::*;

#[cfg(test)]
mod tests;

pub(super) fn background(ctx: &egui::Context) -> Color32 {
    theme::palette(ctx).surface
}

pub(super) fn subtle(ctx: &egui::Context) -> Color32 {
    theme::palette(ctx).bg
}

pub(super) fn border(ctx: &egui::Context) -> Color32 {
    theme::palette(ctx).border
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

/// Dense inline prompts keep the same surface and focus treatment as dialogs.
pub(super) fn compact_inline(ctx: &egui::Context) -> egui::Frame {
    inline(ctx).inner_margin(13)
}

pub(super) fn popup(ctx: &egui::Context) -> egui::Frame {
    surface(ctx).inner_margin(10).corner_radius(10)
}

/// One hit target and one geometry for the label and switch, with a shared inset.
pub(super) fn compact_toggle_row(
    ui: &mut egui::Ui,
    value: &mut bool,
    title: &str,
) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let (target, mut response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), Sense::click());
    if response.clicked() && ui.is_enabled() {
        *value = !*value;
        response.mark_changed();
    }
    let content = target.shrink2(egui::vec2(7.0, 0.0));
    ui.painter().text(
        content.left_center(),
        egui::Align2::LEFT_CENTER,
        title,
        FontId::proportional(11.5),
        colors.text,
    );
    let rect = egui::Rect::from_center_size(
        content.right_center() - egui::vec2(15.0, 0.0),
        egui::vec2(30.0, 18.0),
    );
    ui.painter().rect_filled(
        rect,
        9,
        if *value {
            colors.switch_on
        } else {
            colors.switch_off
        },
    );
    let x = if *value {
        rect.right() - 9.0
    } else {
        rect.left() + 9.0
    };
    ui.painter()
        .circle_filled(egui::pos2(x, rect.center().y), 6.5, colors.text);
    if response.has_focus() {
        ui.painter().rect_stroke(
            target,
            5,
            Stroke::new(1.0, colors.accent),
            egui::StrokeKind::Inside,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, title)
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
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
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("prompt-rendered-this-pass"), pass));
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

pub(super) fn modal_owns_current_pass(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(egui::Id::new("prompt-rendered-this-pass")) == Some(pass))
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

/// Small popover title without the visual weight of a dialog heading.
pub(super) fn compact_heading(ui: &mut egui::Ui, title: &str) {
    let colors = theme::palette(ui.ctx());
    ui.add(egui::Label::new(RichText::new(title).size(13.0).strong().color(colors.text)).wrap());
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
    let (fill, text, stroke) = if !enabled {
        (subtle(ui.ctx()), colors.muted, border(ui.ctx()))
    } else {
        match kind {
            Button::Secondary => (background(ui.ctx()), colors.text, border(ui.ctx())),
            Button::Primary => {
                let bg = ui.visuals().selection.bg_fill;
                (bg, Color32::WHITE, bg)
            }
            Button::Danger => (colors.red, colors.bg, colors.red),
        }
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

/// Compact, named icon action for dense popovers and inline footers.
pub(super) fn icon_action(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    enabled: bool,
) -> egui::Response {
    ui.push_id(label, |ui| {
        ui.add_enabled_ui(enabled, |ui| {
            let colors = theme::palette(ui.ctx());
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
            if response.hovered() && ui.is_enabled() {
                ui.painter().rect_filled(rect, 5, colors.hover);
            }
            if response.has_focus() {
                ui.painter().rect_stroke(
                    rect.shrink(0.5),
                    5,
                    Stroke::new(1.0, colors.accent),
                    egui::StrokeKind::Inside,
                );
            }
            icons::draw(
                ui,
                rect.shrink(6.0),
                icon,
                if ui.is_enabled() {
                    colors.muted
                } else {
                    colors.dim
                },
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
            });
            response
                .on_hover_text(label)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
        })
        .inner
    })
    .inner
}

/// A quiet secondary action: one keyboard-accessible target, without button chrome.
pub(super) fn text_link(
    ui: &mut egui::Ui,
    label: &str,
    icon: Icon,
    enabled: bool,
) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| {
        let colors = theme::palette(ui.ctx());
        let color = if ui.is_enabled() {
            colors.accent
        } else {
            colors.dim
        };
        let mut job = egui::text::LayoutJob::simple_singleline(
            label.into(),
            FontId::proportional(11.0),
            color,
        );
        job.wrap.max_width = (ui.available_width() - 18.0).max(0.0);
        job.wrap.max_rows = 1;
        let galley = ui.painter().layout_job(job);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2((galley.size().x + 18.0).min(ui.available_width()), 22.0),
            Sense::click(),
        );
        icons::draw(
            ui,
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(6.0, 0.0),
                Vec2::splat(12.0),
            ),
            icon,
            color,
        );
        let pos = rect.left_center() + egui::vec2(18.0, -galley.size().y / 2.0);
        let text_rect = egui::Rect::from_min_size(pos, galley.size());
        ui.painter().galley(pos, galley, color);
        if response.hovered() || response.has_focus() {
            ui.painter().line_segment(
                [text_rect.left_bottom(), text_rect.right_bottom()],
                Stroke::new(1.0, color),
            );
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Link, ui.is_enabled(), label)
        });
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    })
    .inner
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
