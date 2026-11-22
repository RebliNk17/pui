//! Unread activity while the reader is away from the live transcript tail.
use super::*;

#[derive(Default)]
pub(super) struct NewMessages {
    session: String,
    observed: Option<(usize, u64)>,
    pub unread: bool,
    pub jump_to_latest: bool,
}

impl NewMessages {
    pub fn rebase(&mut self) {
        self.observed = None;
    }

    pub fn observe(&mut self, messages: &[Message], session: &str) {
        if self.session != session {
            *self = Self {
                session: session.into(),
                ..Default::default()
            };
        }
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        // Only inspect the live tail, not every historical message. Ignore UI
        // disclosures, progress animations and hidden thinking-only deltas.
        if let Some(message) = messages.last() {
            message.role.hash(&mut hash);
            message.timestamp.hash(&mut hash);
            message.text.hash(&mut hash);
            message.error.hash(&mut hash);
            for block in &message.blocks {
                match block {
                    Block::Text(text) => text.hash(&mut hash),
                    Block::Tool(id) => id.hash(&mut hash),
                    Block::Thinking(_) => {}
                }
            }
            for image in &message.images {
                image.key.hash(&mut hash);
            }
            hash_tools(&message.tools, &mut hash);
        }
        let current = (messages.len(), hash.finish());
        if let Some(previous) = self.observed {
            if current.0 < previous.0 {
                self.unread = false;
            } else if current != previous {
                self.unread = true;
            }
        }
        self.observed = Some(current);
    }

    pub fn show(&self, ui: &mut egui::Ui, viewport: Rect) -> Option<egui::Response> {
        if !self.unread || viewport.width() < 150.0 || viewport.height() < 52.0 {
            return None;
        }
        let colors = crate::theme::palette(ui.ctx());
        let label = ui.painter().layout_no_wrap(
            "New messages".into(),
            FontId::proportional(12.0),
            colors.text,
        );
        let size = egui::vec2(label.size().x + 60.0, 34.0);
        let rect = Rect::from_center_size(
            egui::pos2(viewport.center().x, viewport.bottom() - 12.0 - size.y / 2.0),
            size,
        );
        // Registered after the transcript so the pill owns its hit target, but
        // kept on the panel's layer so dialogs and popovers remain above it.
        let response = ui.interact(
            rect,
            ui.make_persistent_id(("new-messages", &self.session)),
            Sense::click(),
        );
        let highlighted = ui.is_enabled() && (response.hovered() || response.has_focus());
        let painter = ui.painter().with_clip_rect(viewport);
        painter.add(
            egui::epaint::Shadow {
                offset: [0, 3],
                blur: 12,
                spread: 0,
                color: egui::Color32::from_black_alpha(35),
            }
            .as_shape(rect, 17),
        );
        painter.rect(
            rect,
            17,
            if highlighted {
                colors.hover
            } else {
                colors.surface
            },
            Stroke::new(
                1.0,
                if highlighted {
                    colors.accent
                } else {
                    colors.border
                },
            ),
            egui::StrokeKind::Inside,
        );
        painter.circle_filled(
            rect.left_center() + egui::vec2(15.0, 0.0),
            2.5,
            colors.accent,
        );
        painter.galley(
            rect.left_center() + egui::vec2(25.0, -label.size().y / 2.0),
            label,
            colors.text,
        );
        icons::draw(
            ui,
            Rect::from_center_size(
                rect.right_center() - egui::vec2(18.0, 0.0),
                Vec2::splat(14.0),
            ),
            Icon::ArrowDown,
            colors.accent,
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "New messages")
        });
        Some(
            response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Jump to the newest message and follow new replies"),
        )
    }
}

fn hash_tools(tools: &[crate::protocol::Tool], hash: &mut impl Hasher) {
    tools.len().hash(hash);
    for tool in tools {
        tool.id.hash(hash);
        tool.args.hash(hash);
        tool.output.hash(hash);
        tool.finished.hash(hash);
        tool.failed.hash(hash);
        hash_tools(&tool.children, hash);
    }
}

#[cfg(test)]
#[path = "new_messages_tests.rs"]
mod tests;
