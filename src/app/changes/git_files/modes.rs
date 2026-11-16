//! Mode-scoped full-review projection and explicit replacement request.
use super::*;
pub(super) fn includes(mode: workspace::DiffMode, file: &workspace::FileChange) -> bool {
    let s = file.status.as_bytes();
    if mode == workspace::DiffMode::Combined {
        return true;
    }
    s.len() == 2
        && match mode {
            workspace::DiffMode::Combined => true,
            workspace::DiffMode::Staged => !matches!(s[0], b' ' | b'?' | b'!'),
            workspace::DiffMode::Unstaged => !matches!(s[1], b' ' | b'!'),
        }
}
impl Changes {
    pub(in crate::app::changes) fn review_mode_controls(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
    ) {
        let mut chosen = None;
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in [
                (workspace::DiffMode::Combined, "Combined"),
                (workspace::DiffMode::Staged, "Staged"),
                (workspace::DiffMode::Unstaged, "Unstaged"),
            ] {
                let enabled = self.git_files.pending.is_none()
                    && !self.git_files.stale
                    && !egui::Popup::is_any_open(ui.ctx())
                    && ui.memory(|m| m.top_modal_layer().is_none());
                let response = ui
                    .push_id(("review-diff-mode", label), |ui| {
                        ui.add_enabled(
                            enabled,
                            egui::Button::selectable(
                                self.git_files.mode == mode,
                                theme::small(ui, label),
                            ),
                        )
                    })
                    .inner;
                self.mode_controls.push(response.id);
                #[cfg(test)]
                ui.ctx().data_mut(|d| {
                    d.insert_temp(
                        egui::Id::new(("review-diff-mode", label)),
                        (response.id, response.rect),
                    )
                });
                if response.clicked() && self.git_files.mode != mode {
                    chosen = Some(mode);
                }
            }
        });
        if let Some(mode) = chosen {
            self.choose_review_mode(ui.ctx(), workspace, mode);
        }
    }
    fn choose_review_mode(
        &mut self,
        ctx: &egui::Context,
        workspace: &Workspace,
        mode: workspace::DiffMode,
    ) {
        let Some(doc) = &self.review else {
            return;
        };
        if doc.3 || self.git_files.pending.is_some() || self.git_files.stale {
            return;
        }
        let path = doc.0.clone();
        let owner = doc.2.focus_id();
        let Some(list) = List::new_mode(workspace, mode) else {
            return;
        };
        let target = list.files.iter().position(|f| f.path == path).unwrap_or(0);
        let Some(file) = list.files.get(target).cloned() else {
            self.git_files.error =
                Some("No files in requested mode; existing review retained.".into());
            return;
        };
        clear_selection(ctx);
        self.review_comments.stale(Some(false));
        self.git_files.error = None;
        // Keep the current mode/list/body until the replacement succeeds.
        self.git_files.pending = Some(Pending {
            rx: start_mode(ctx, workspace, file, mode),
            target,
            owner,
        });
        self.git_files.requested_mode = Some(mode);
    }
}
#[cfg(test)]
mod tests;
