//! Single first-parent lane only; no invented connections across missing/filtered rows.
use super::*;
pub(super) fn connected(
    snapshot: &workspace::history::Snapshot,
    filter: &filter::Filter,
    index: usize,
) -> bool {
    snapshot
        .commits
        .get(index)
        .zip(snapshot.commits.get(index + 1))
        .is_some_and(|(commit, next)| {
            filter.includes(index)
                && filter.includes(index + 1)
                && commit.first_parent.as_deref() == Some(next.oid.as_str())
        })
}
pub(super) fn paint(
    ui: &egui::Ui,
    rect: egui::Rect,
    commit: &workspace::history::Commit,
    index: usize,
    snapshot: &workspace::history::Snapshot,
    filter: &filter::Filter,
) {
    let colors = theme::palette(ui.ctx());
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let center = egui::pos2(rect.left() + 10.0, rect.center().y);
    let stroke = Stroke::new(1.2, colors.dim);
    if index > 0 && connected(snapshot, filter, index - 1) {
        painter.line_segment([egui::pos2(center.x, rect.top()), center], stroke);
    }
    if connected(snapshot, filter, index) {
        painter.line_segment([center, egui::pos2(center.x, rect.bottom())], stroke);
    }
    painter.circle_filled(center, 3.0, colors.bg);
    painter.circle_stroke(center, 3.0, Stroke::new(1.2, colors.accent));
    if commit.parent_count > 1 {
        painter.circle_stroke(center, 5.5, Stroke::new(1.0, colors.accent));
    }
    if commit.parent_count == 0 {
        painter.line_segment(
            [
                center + egui::vec2(-4.0, 6.0),
                center + egui::vec2(4.0, 6.0),
            ],
            stroke,
        );
    }
}
#[cfg(test)]
mod tests;
