//! Bounded hunk offsets for a <=32KiB/1,024-line immutable literal patch.
use super::*;
#[derive(Default)]
pub(in crate::app::changes::branches::history::details) struct Viewer {
    identity: u64,
    hunks: Vec<u16>,
    selected: Option<usize>,
    folded: Vec<bool>,
    pub(in crate::app::changes::branches::history::details) fold_controls: Vec<egui::Id>,
    pub(in crate::app::changes::branches::history::details) controls: [Option<egui::Id>; 2],
    pub(in crate::app::changes::branches::history::details) key: Option<egui::Id>,
}
impl Viewer {
    pub(super) fn new(text: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let identity = NEXT.fetch_add(1, Ordering::Relaxed);
        let hunks: Vec<u16> = text
            .lines()
            .enumerate()
            .filter(|(_, line)| line.starts_with("@@ -") && line.contains(" @@"))
            .filter_map(|(index, _)| u16::try_from(index).ok())
            .collect();
        Self {
            identity,
            folded: vec![false; hunks.len()],
            hunks,
            ..Default::default()
        }
    }
    pub(super) fn show(&mut self, ui: &mut egui::Ui, text: &str) {
        let key = self.key.take();
        self.controls = [None; 2];
        self.fold_controls.clear();
        let mut target = None;
        let previous = self.selected.and_then(|n| n.checked_sub(1));
        let next = self
            .selected
            .map_or(Some(0), |n| n.checked_add(1))
            .filter(|n| *n < self.hunks.len());
        ui.horizontal_wrapped(|ui| {
            for (index, (label, destination)) in [("Previous hunk", previous), ("Next hunk", next)]
                .into_iter()
                .enumerate()
            {
                let response = ui
                    .push_id(("commit-patch-hunk", self.identity, index), |ui| {
                        ui.add_enabled(
                            destination.is_some(),
                            egui::Button::new(theme::small(ui, label)).frame(false),
                        )
                    })
                    .inner;
                self.controls[index] = Some(response.id);
                #[cfg(test)]
                ui.ctx().data_mut(|d| {
                    d.insert_temp(
                        popup_id().with(("commit-patch-hunk", index)),
                        (response.id, response.rect, response.enabled()),
                    )
                });
                if (response.clicked() || key == Some(response.id))
                    && let Some(n) = destination
                {
                    self.selected = Some(n);
                    self.folded[n] = false;
                    target = Some(self.hunks[n] as usize);
                }
            }
            ui.label(theme::small(
                ui,
                self.selected.map_or_else(
                    || format!("{} hunks", self.hunks.len()),
                    |n| format!("Hunk {} / {}", n + 1, self.hunks.len()),
                ),
            ));
        });
        if text.is_empty() {
            ui.label(theme::small(ui, "(empty patch)"));
            return;
        }
        let colors = theme::palette(ui.ctx());
        let mut hunk = false;
        let mut hidden = false;
        let mut ordinal = 0;
        for (index, line) in text.lines().enumerate() {
            if line.starts_with("diff --git ") {
                hunk = false;
                hidden = false;
            }
            if self.hunks.get(ordinal).copied().map(usize::from) == Some(index) {
                let n = ordinal;
                ordinal += 1;
                let response=ui.push_id(("commit-patch-fold",self.identity,n),|ui|ui.add(egui::Button::new(RichText::new(format!("{} {}",if self.folded[n]{"▸"}else{"▾"},line)).monospace().size(10.5)).frame(false).wrap())).inner.on_hover_text("Fold or unfold this hunk. Presentation only; original patch text is retained.");
                self.fold_controls.push(response.id);
                if response.enabled() && (response.clicked() || key == Some(response.id)) {
                    self.folded[n] = !self.folded[n];
                }
                hidden = self.folded[n];
                hunk = true;
                if target == Some(index) {
                    response.scroll_to_me(Some(egui::Align::Center));
                    #[cfg(test)]
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(popup_id().with("commit-patch-hunk-target"), index)
                    });
                }
                #[cfg(test)]
                ui.ctx().data_mut(|d| {
                    d.insert_temp(
                        popup_id().with(("commit-patch-fold", n)),
                        (response.id, response.rect, hidden),
                    )
                });
                continue;
            }
            if hidden {
                continue;
            }
            if line.starts_with("@@") {
                hunk = true;
            }
            let bg = if hunk && line.starts_with('+') {
                colors.diff_add_bg
            } else if hunk && line.starts_with('-') {
                colors.diff_remove_bg
            } else if line.starts_with("@@") {
                colors.accent_bg
            } else {
                egui::Color32::TRANSPARENT
            };
            let response = egui::Frame::new()
                .fill(bg)
                .inner_margin(egui::Margin::symmetric(4, 1))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.add(
                        egui::Label::new(
                            RichText::new(if line.is_empty() { " " } else { line })
                                .monospace()
                                .size(10.5)
                                .color(colors.diff_text),
                        )
                        .wrap()
                        .selectable(false),
                    );
                })
                .response;
            if target == Some(index) {
                response.scroll_to_me(Some(egui::Align::Center));
                #[cfg(test)]
                ui.ctx().data_mut(|d| {
                    d.insert_temp(popup_id().with("commit-patch-hunk-target"), index)
                });
            }
        }
    }
}
#[cfg(test)]
mod tests;
