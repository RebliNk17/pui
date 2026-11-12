//! Shared expandable surfaces: useful summaries, no debug-tree triangles.
use crate::{
    icons::{self, Icon},
    theme,
};
use eframe::egui::{self, Color32, FontId, Rect, Response, Sense, Stroke, Vec2};

pub struct Section {
    title: String,
    id: egui::Id,
    summary: String,
    icon: Icon,
    status: Option<(String, Color32)>,
    default_open: bool,
}
impl Section {
    pub fn new(title: impl Into<String>) -> Self {
        let title = title.into();
        Self {
            id: egui::Id::new(&title),
            title,
            summary: String::new(),
            icon: Icon::Details,
            status: None,
            default_open: false,
        }
    }
    pub fn id_salt(mut self, salt: impl std::hash::Hash) -> Self {
        self.id = egui::Id::new(salt);
        self
    }
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = icon;
        self
    }
    pub fn status(mut self, text: impl Into<String>, color: Color32) -> Self {
        self.status = Some((text.into(), color));
        self
    }
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn show(self, ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) -> Response {
        let colors = theme::palette(ui.ctx());
        let id = ui.make_persistent_id(self.id);
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            id,
            self.default_open,
        );
        let width = ui.available_width().max(1.0);
        let response = egui::Frame::new()
            .fill(colors.surface)
            .stroke(Stroke::new(1.0, colors.border))
            .corner_radius(10)
            .show(ui, |ui| {
                ui.set_width((width - 2.0).max(1.0));
                let (rect, _) = ui.allocate_exact_size(
                    Vec2::new(
                        ui.available_width(),
                        if self.summary.is_empty() { 44.0 } else { 58.0 },
                    ),
                    Sense::hover(),
                );
                let mut response = ui.interact(rect, id.with("header"), Sense::click());
                if response.clicked() {
                    response.request_focus();
                }
                let mut open = state.is_open();
                if response.has_focus() {
                    ui.memory_mut(|memory| {
                        memory.set_focus_lock_filter(
                            response.id,
                            egui::EventFilter {
                                horizontal_arrows: true,
                                vertical_arrows: true,
                                ..Default::default()
                            },
                        )
                    });
                    ui.input_mut(|i| {
                        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                            open = true;
                        }
                        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                            open = false;
                        }
                    });
                }
                if response.clicked() {
                    open = !open;
                }
                if state.is_open() != open {
                    response.mark_changed();
                }
                state.set_open(open);
                state.store(ui.ctx());
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::CollapsingHeader,
                        ui.is_enabled(),
                        open,
                        &self.title,
                    )
                });
                if response.hovered() {
                    ui.painter().rect_filled(rect.shrink(1.0), 9, colors.hover);
                }
                if response.has_focus() {
                    ui.painter().rect_stroke(
                        rect.shrink(2.0),
                        8,
                        Stroke::new(1.5, colors.accent),
                        egui::StrokeKind::Inside,
                    );
                }
                let tile = Rect::from_center_size(
                    rect.left_center() + egui::vec2(26.0, 0.0),
                    Vec2::splat(28.0),
                );
                ui.painter().rect_filled(tile, 7, colors.bg);
                icons::draw(ui, tile, self.icon, colors.muted);
                let toggle = Rect::from_center_size(
                    rect.right_center() - egui::vec2(23.0, 0.0),
                    Vec2::splat(24.0),
                );
                ui.painter().rect_filled(
                    toggle,
                    7,
                    if open { colors.accent_bg } else { colors.bg },
                );
                let tint = if open { colors.accent } else { colors.muted };
                let c = toggle.center();
                ui.painter().line_segment(
                    [c - egui::vec2(4.0, 0.0), c + egui::vec2(4.0, 0.0)],
                    Stroke::new(1.4, tint),
                );
                if !open {
                    ui.painter().line_segment(
                        [c - egui::vec2(0.0, 4.0), c + egui::vec2(0.0, 4.0)],
                        Stroke::new(1.4, tint),
                    );
                }
                ui.interact(toggle, id.with("toggle-hint"), Sense::hover())
                    .on_hover_text(if open { "Hide details" } else { "Show details" });
                let mut elided = false;
                let left = rect.left() + 50.0;
                let mut right = toggle.left() - 10.0;
                if let Some((status, color)) = &self.status {
                    let badge_width = (status.chars().count() as f32 * 6.0 + 18.0).min(110.0);
                    if rect.width() > 280.0 {
                        let badge = Rect::from_center_size(
                            egui::pos2(right - badge_width / 2.0, rect.center().y),
                            Vec2::new(badge_width, 24.0),
                        );
                        ui.painter()
                            .rect_filled(badge, 6, color.gamma_multiply(0.12));
                        elided |= paint(
                            ui,
                            status,
                            badge.left_center() + egui::vec2(9.0, 0.0),
                            badge.width() - 18.0,
                            11.0,
                            *color,
                        );
                        right = badge.left() - 12.0;
                    }
                }
                let text_width = (right - left).max(1.0);
                elided |= paint(
                    ui,
                    &self.title,
                    egui::pos2(
                        left,
                        rect.center().y - if self.summary.is_empty() { 0.0 } else { 9.0 },
                    ),
                    text_width,
                    12.0,
                    colors.text,
                );
                if !self.summary.is_empty() {
                    let summary = if rect.width() <= 280.0 {
                        self.status
                            .as_ref()
                            .map(|(status, _)| format!("{status} · {}", self.summary))
                            .unwrap_or_else(|| self.summary.clone())
                    } else {
                        self.summary.clone()
                    };
                    elided |= paint(
                        ui,
                        &summary,
                        egui::pos2(left, rect.center().y + 11.0),
                        text_width,
                        10.5,
                        colors.muted,
                    );
                }
                if open {
                    egui::Frame::new().inner_margin(12).show(ui, |ui| {
                        ui.set_width((rect.width() - 24.0).max(1.0));
                        ui.push_id(id, contents);
                    });
                }
                let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                if elided {
                    response.on_hover_text(
                        [
                            self.title.as_str(),
                            self.summary.as_str(),
                            self.status.as_ref().map(|s| s.0.as_str()).unwrap_or(""),
                        ]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    )
                } else {
                    response
                }
            })
            .inner;
        ui.add_space(4.0);
        response
    }
}
fn paint(
    ui: &egui::Ui,
    text: &str,
    position: egui::Pos2,
    width: f32,
    size: f32,
    color: Color32,
) -> bool {
    let mut job =
        egui::text::LayoutJob::simple_singleline(text.into(), FontId::proportional(size), color);
    job.wrap.max_width = width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    let elided = galley.elided;
    ui.painter().galley(
        position - egui::vec2(0.0, galley.size().y / 2.0),
        galley,
        color,
    );
    elided
}

#[cfg(test)]
mod tests;
