//! Bounded presentation-only filtering of the already pinned history list.
use super::*;
pub(super) fn id() -> egui::Id {
    popup_id().with("history-filter")
}
#[derive(Default)]
pub(super) struct Filter {
    query: String,
    hits: Option<Vec<usize>>,
    clear_control: Option<egui::Id>,
    clear_key: Option<egui::Id>,
}
impl Filter {
    pub(super) fn demo_query(&mut self, snapshot: &workspace::history::Snapshot) {
        self.query = "approvals".into();
        self.project(snapshot);
    }
    pub(super) fn hidden(&mut self) {
        self.clear_control = None;
        self.clear_key = None;
    }
    pub(super) fn key(&mut self, focus: Option<egui::Id>) {
        self.clear_key = focus.filter(|id| Some(*id) == self.clear_control);
    }
    pub(super) fn includes(&self, index: usize) -> bool {
        self.hits
            .as_ref()
            .is_none_or(|hits| hits.binary_search(&index).is_ok())
    }
    pub(super) fn project(&mut self, snapshot: &workspace::history::Snapshot) {
        // UI char limit plus capacity reconciliation bounds retained query storage.
        self.query = self.query.chars().take(128).collect();
        self.query.shrink_to_fit();
        let query = self.query.trim().to_lowercase();
        self.hits = if query.is_empty() {
            None
        } else {
            Some(
                snapshot
                    .commits
                    .iter()
                    .take(workspace::history::MAX_COMMITS)
                    .enumerate()
                    .filter_map(|(index, commit)| {
                        (commit.oid.contains(&query)
                            || commit.subject.to_lowercase().contains(&query))
                        .then_some(index)
                    })
                    .collect(),
            )
        };
    }
    pub(super) fn show(&mut self, ui: &mut egui::Ui, snapshot: &workspace::history::Snapshot) {
        self.clear_control = None;
        let clear_key = self.clear_key.take();
        let response=ui.add(egui::TextEdit::singleline(&mut self.query).id(id()).char_limit(128).desired_width(ui.available_width()).hint_text("Filter loaded subjects or IDs…")).on_hover_text("Literal Unicode-lowercase substring in this pinned list only. No older history, commit bodies or repository search.");
        if response.changed() {
            self.project(snapshot);
        }
        if let Some(hits) = &self.hits {
            let count = hits.len();
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::small(
                    ui,
                    format!("{} of {} loaded commits", count, snapshot.commits.len()),
                ));
                let clear = ui
                    .push_id((id(), "clear", &snapshot.head, &self.query), |ui| {
                        ui.button("Clear filter")
                    })
                    .inner;
                #[cfg(test)]
                ui.ctx()
                    .data_mut(|data| data.insert_temp(id().with("clear-control"), clear.id));
                self.clear_control = Some(clear.id);
                if clear.clicked() || clear_key == Some(clear.id) {
                    self.query.clear();
                    self.query.shrink_to_fit();
                    self.hits = None;
                    ui.ctx().memory_mut(|memory| memory.request_focus(id()));
                }
            });
        }
    }
}
#[cfg(test)]
mod tests;
