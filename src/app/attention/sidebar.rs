//! A row's own unread observation, never a descendant aggregate or a live status.
use super::*;

impl Pui {
    pub(in crate::app) fn sidebar_unread(&self, branch: &sidebar_tree::Branch) -> bool {
        if let Some(path) = &branch.path {
            return self.saved_unread(path);
        }
        let time = now();
        match &branch.target {
            sidebar_tree::Target::Agent(id) => self.attention.entries.iter().any(|entry| {
                matches!(&entry.owner, Owner::Runtime(owner) if owner == id)
                    && entry.unread_at(time)
            }),
            _ => false,
        }
    }

    pub(in crate::app) fn saved_unread(&self, path: &Path) -> bool {
        let owner = Owner::Saved(self.sync.attention_identity(path));
        let time = now();
        self.attention
            .entries
            .iter()
            .any(|entry| entry.owner == owner && entry.unread_at(time))
    }
}

/// A quiet, static lozenge in its own lane after the status/disclosure. The row
/// remains the only click/keyboard target; hovering or painting never marks read.
pub(in crate::app) fn paint_unread(ui: &egui::Ui, row: egui::Rect, y: f32, unread: bool) {
    if !unread {
        return;
    }
    let marker = egui::Rect::from_center_size(
        egui::pos2(row.left() + 23.0, row.top() + y),
        egui::vec2(3.0, 7.0),
    );
    if ui.is_rect_visible(marker) {
        ui.painter()
            .rect_filled(marker, 1.5, theme::palette(ui.ctx()).accent);
    }
}

#[cfg(test)]
mod tests;
