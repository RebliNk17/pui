//! Quiet, fixed-height browser controls shared by the file/page dock.
use super::*;

pub(in crate::app) fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    tip: &str,
    selected: bool,
) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    let reload = matches!(icon, Icon::Refresh | Icon::Stop);
    if selected || reload || (response.hovered() && ui.is_enabled()) {
        ui.painter().rect_filled(
            rect.shrink(1.0),
            6,
            if selected {
                colors.bg
            } else if response.hovered() && ui.is_enabled() {
                colors.hover
            } else {
                colors.surface
            },
        );
        if selected || reload {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                6,
                Stroke::new(1.0, colors.border),
                egui::StrokeKind::Inside,
            );
        }
    }
    icons::draw(
        ui,
        rect,
        icon,
        if !ui.is_enabled() {
            colors.dim.gamma_multiply(0.5)
        } else if selected || response.hovered() {
            colors.text
        } else {
            colors.muted
        },
    );
    #[cfg(test)]
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new(("browser-control", tip)), rect));
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tip));
    response
        .on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Reload and Stop share a visible button surface, including at rest.
pub(super) fn reload_button(ui: &mut egui::Ui, loading: bool) -> egui::Response {
    let (icon, tip) = if loading {
        (Icon::Stop, "Stop loading")
    } else {
        (Icon::Refresh, "Reload page")
    };
    icon_button(ui, icon, tip, false)
}

/// A single, accessible eye toggle: pressed means rendered preview.
pub(super) fn preview_toggle(ui: &mut egui::Ui, source: &mut bool) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let selected = !*source;
    let mut response = ui.add_sized(
        Vec2::splat(28.0),
        egui::Button::new("")
            .fill(if selected {
                colors.accent_bg
            } else {
                Color32::TRANSPARENT
            })
            .stroke(Stroke::new(
                1.0,
                if selected {
                    colors.accent
                } else {
                    colors.border
                },
            ))
            .corner_radius(6),
    );
    if response.clicked() {
        *source = !*source;
        response.mark_changed();
    }
    let center = response.rect.center();
    let stroke = Stroke::new(
        1.3,
        if !*source {
            colors.accent
        } else {
            colors.muted
        },
    );
    let points = (0..=32)
        .map(|step| {
            let angle = step as f32 * std::f32::consts::TAU / 32.0;
            center + egui::vec2(7.0 * angle.cos(), 4.5 * angle.sin())
        })
        .collect();
    ui.painter().add(egui::Shape::closed_line(points, stroke));
    ui.painter().circle_stroke(center, 2.0, stroke);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            ui.is_enabled(),
            !*source,
            "Preview",
        )
    });
    #[cfg(test)]
    ui.ctx().data_mut(|data| {
        data.insert_temp(egui::Id::new(("browser-control", "Preview")), response.rect);
        data.insert_temp(
            egui::Id::new("file-preview-toggle"),
            (response.id, response.rect),
        );
    });
    response.on_hover_text(if *source {
        "Show preview"
    } else {
        "Show source"
    })
}

pub(in crate::app) fn label_button(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let response = ui.add(
        egui::Button::new(RichText::new(label).size(11.0).color(if selected {
            colors.text
        } else {
            colors.muted
        }))
        .fill(if selected {
            colors.bg
        } else {
            Color32::TRANSPARENT
        })
        .stroke(if selected {
            Stroke::new(1.0, colors.border)
        } else {
            Stroke::NONE
        })
        .corner_radius(6)
        .min_size(Vec2::new(32.0, 26.0)),
    );
    #[cfg(test)]
    ui.ctx().data_mut(|data| {
        data.insert_temp(egui::Id::new(("browser-control", label)), response.rect);
    });
    response
}

pub(in crate::app) fn divider(ui: &mut egui::Ui) {
    let colors = theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(9.0, 24.0), Sense::hover());
    ui.painter().line_segment(
        [
            rect.center_top() + egui::vec2(0.0, 5.0),
            rect.center_bottom() - egui::vec2(0.0, 5.0),
        ],
        Stroke::new(1.0, colors.border),
    );
}

pub(in crate::app) fn tab(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    title: &str,
    tip: &str,
    icon: Icon,
    active: bool,
    width: f32,
) -> (bool, bool) {
    let colors = theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::hover());
    let close_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 17.0, rect.center().y),
        Vec2::splat(22.0),
    );
    let response = ui.interact(
        egui::Rect::from_min_max(rect.min, egui::pos2(close_rect.left() - 2.0, rect.bottom())),
        ui.make_persistent_id((&id, "tab")),
        Sense::click(),
    );
    let close = ui.interact(
        close_rect,
        ui.make_persistent_id((&id, "close")),
        Sense::click(),
    );
    if active || response.hovered() || close.hovered() {
        ui.painter()
            .rect_filled(rect, 7, if active { colors.bg } else { colors.hover });
    }
    if active {
        ui.painter().rect_stroke(
            rect,
            7,
            Stroke::new(1.0, colors.border),
            egui::StrokeKind::Inside,
        );
    }
    icons::draw(
        ui,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 17.0, rect.center().y),
            Vec2::splat(16.0),
        ),
        icon,
        colors.muted,
    );
    let mut job = egui::text::LayoutJob::simple_singleline(
        title.into(),
        FontId::proportional(12.0),
        if active { colors.text } else { colors.muted },
    );
    job.wrap.max_width = (width - 65.0).max(20.0);
    job.wrap.max_rows = 1;
    let galley = ui.painter().layout_job(job);
    let elided = galley.elided;
    ui.painter().galley(
        egui::pos2(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0),
        galley,
        colors.text,
    );
    if close.hovered() {
        ui.painter().rect_filled(close_rect, 4, colors.hover);
    }
    icons::draw(
        ui,
        close_rect.shrink(4.0),
        Icon::Close,
        if close.hovered() {
            colors.text
        } else {
            colors.dim
        },
    );
    response
        .clone()
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if elided || tip != title {
        response.clone().on_hover_text(if elided && tip != title {
            format!("{title}\n{tip}")
        } else {
            tip.to_owned()
        });
    }
    close
        .clone()
        .on_hover_text("Close tab")
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    (response.clicked(), close.clicked())
}

pub(in crate::app) fn address(
    ui: &mut egui::Ui,
    address: &mut String,
    page: bool,
    focus: bool,
) -> bool {
    let colors = theme::palette(ui.ctx());
    let mut submitted = false;
    let (field, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
    ui.painter().rect_filled(field, 6, colors.surface);
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(field.left() + 15.0, field.center().y),
        Vec2::splat(14.0),
    );
    icons::draw(
        ui,
        icon_rect,
        if page { Icon::Globe } else { Icon::File },
        colors.dim,
    );
    let id = ui.make_persistent_id("viewer-address-input");
    // Consume before the composer can interpret Enter as a send.
    submitted |= ui.memory(|memory| memory.has_focus(id))
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let editor = egui::Rect::from_min_max(
        egui::pos2(field.left() + 28.0, field.top() + 4.0),
        egui::pos2(
            (field.right() - 32.0).max(field.left() + 28.0),
            field.bottom() - 4.0,
        ),
    );
    let response = ui.put(
        editor,
        egui::TextEdit::singleline(address)
            .frame(false)
            .font(FontId::proportional(12.0))
            .hint_text("Open a URL or file path…")
            .id(id),
    );
    let go = egui::Rect::from_min_max(egui::pos2(field.right() - 28.0, field.top()), field.max);
    submitted |= ui
        .scope_builder(egui::UiBuilder::new().max_rect(go), |ui| {
            icon_button(ui, Icon::ArrowRight, "Go to address", false).clicked()
        })
        .inner;
    ui.painter().rect_stroke(
        field,
        6,
        Stroke::new(
            1.0,
            if response.has_focus() {
                colors.accent
            } else {
                colors.border
            },
        ),
        egui::StrokeKind::Inside,
    );
    #[cfg(test)]
    ui.ctx().data_mut(|data| {
        data.insert_temp(egui::Id::new("browser-address-field"), field);
        data.insert_temp(
            egui::Id::new("browser-address-editor"),
            (response.id, response.rect),
        )
    });
    if focus {
        response.request_focus();
    }
    submitted
}
