//! Conversation-scoped notifications, kept out of the transcript and composer.
use super::*;

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;

impl Pui {
    pub(super) fn notifications_button(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let colors = theme::palette(ui.ctx());
        let count = self.state.diagnostics.len() + usize::from(self.state.notice.is_some());
        let (rect, response) = ui.allocate_exact_size(Vec2::new(52.0, 28.0), Sense::click());
        if response.hovered() || self.diagnostics_open {
            ui.painter().rect_filled(rect, 7, colors.hover);
        }
        icons::draw(
            ui,
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(14.0, 0.0),
                Vec2::splat(20.0),
            ),
            Icon::Bell,
            if self.diagnostics_open {
                colors.text
            } else {
                colors.muted
            },
        );
        if count > 0 {
            let color = if self.state.diagnostics.is_empty() {
                colors.blue
            } else {
                colors.amber
            };
            let badge = egui::Rect::from_center_size(
                rect.right_center() - egui::vec2(12.0, 0.0),
                Vec2::new(20.0, 17.0),
            );
            ui.painter()
                .rect_filled(badge, 5, color.gamma_multiply(0.14));
            ui.painter().text(
                badge.center(),
                egui::Align2::CENTER_CENTER,
                count.to_string(),
                FontId::proportional(10.0),
                color,
            );
        }
        if response.clicked() {
            self.diagnostics_open = !self.diagnostics_open;
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Notifications")
        });
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(if count == 0 {
                "Notifications".into()
            } else {
                format!("Notifications · {count}")
            })
    }

    pub(super) fn notifications_popup(&mut self, button: &egui::Response) {
        if !self.diagnostics_open {
            return;
        }
        let ctx = &button.ctx;
        let colors = theme::palette(ctx);
        let width = 400.0_f32.min(ctx.screen_rect().width() - 24.0);
        let mut open = true;
        egui::Popup::from_response(button)
            .id(egui::Id::new("conversation-notifications"))
            .open_bool(&mut open)
            .align(egui::RectAlign::BOTTOM_END)
            .align_alternatives(&[])
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .gap(10.0)
            .width(width)
            .frame(
                egui::Frame::popup(&ctx.style())
                    .fill(colors.surface)
                    .stroke(Stroke::new(1.0, colors.border))
                    .corner_radius(12)
                    .inner_margin(16),
            )
            .show(|ui| {
                ui.set_width(width - 32.0);
                ui.spacing_mut().item_spacing.y = 12.0;
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.label(RichText::new("Notifications").size(15.0).strong());
                        ui.label(theme::small(ui, "Updates from this conversation"));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icons::button(ui, Icon::Close, "Close notifications").clicked() {
                            ui.close();
                        }
                    });
                });
                ui.separator();
                let has_notifications = self.state.notice.is_some() || !self.state.diagnostics.is_empty();
                if has_notifications {
                    // Keep actions visible even when a server sends a very long diagnostic.
                    let max_height = (ctx.screen_rect().bottom() - button.rect.bottom() - 180.0)
                        .clamp(80.0, 360.0);
                    let mut dismiss_notice = false;
                    let mut dismiss_diagnostic = None;
                    egui::ScrollArea::vertical()
                        .id_salt("notification-list")
                        .max_height(max_height)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.set_width((ui.available_width() - 6.0).max(0.0));
                            if let Some(notice) = &self.state.notice {
                                ui.push_id("session-notice", |ui| {
                                    dismiss_notice = notification_card(ui, "Session update", notice, Icon::Details, colors.blue);
                                });
                            }
                            for (index, message) in self.state.diagnostics.iter().enumerate().rev() {
                                ui.push_id(("diagnostic", message), |ui| {
                                    let title = if is_mcp_diagnostic(message) { "Connection warning" } else { "Pi / extensions" };
                                    if notification_card(ui, title, message, Icon::Bell, colors.amber) {
                                        dismiss_diagnostic = Some(index);
                                    }
                                });
                            }
                            if self.state.diagnostics.iter().any(|message| is_mcp_diagnostic(message)) {
                                ui.add_space(2.0);
                                ui.label(RichText::new("MCP connection help").size(11.0).strong().color(colors.muted));
                                ui.add(egui::Label::new(theme::small(ui, "Local chat is still available. Check the remote service, or diagnose and reauthenticate in the Pi CLI:")).wrap());
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("/mcp").monospace().size(12.0).color(colors.text));
                                    if icons::button(ui, Icon::Copy, "Copy Pi CLI command").clicked() {
                                        ctx.copy_text("/mcp".into());
                                    }
                                });
                            }
                        });
                    if dismiss_notice {
                        self.state.notice = None;
                    }
                    if let Some(index) = dismiss_diagnostic {
                        self.state.diagnostics.remove(index);
                    }
                } else {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        icons::inline(ui, Icon::Bell, colors.dim);
                        ui.add_space(4.0);
                        ui.label(RichText::new("You’re all caught up").size(13.0));
                        ui.label(theme::small(ui, "Session updates and connection notices appear here."));
                    });
                    ui.add_space(12.0);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Esc to close").size(10.0).color(colors.dim));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(has_notifications, egui::Button::new(theme::small(ui, "Clear all")).frame(false)).clicked() {
                            self.state.diagnostics.clear();
                            self.state.notice = None;
                        }
                    });
                });
            });
        self.diagnostics_open = open;
    }
}

fn is_mcp_diagnostic(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("mcp") || message.contains("503")
}

fn notification_card(
    ui: &mut egui::Ui,
    title: &str,
    message: &str,
    icon: Icon,
    color: Color32,
) -> bool {
    let colors = theme::palette(ui.ctx());
    let mut dismissed = false;
    egui::Frame::new()
        .fill(colors.bg)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(9)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal(|ui| {
                icons::inline(ui, icon, color);
                ui.label(RichText::new(title).size(12.0).strong().color(color));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    dismissed = icons::button(ui, Icon::Close, "Dismiss notification").clicked();
                });
            });
            ui.add(
                egui::Label::new(RichText::new(message).size(12.0).color(colors.text))
                    .wrap()
                    .selectable(true),
            );
        });
    dismissed
}
