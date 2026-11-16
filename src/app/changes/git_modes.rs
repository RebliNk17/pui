//! Explicit staged/unstaged inspection; no index mutation or persisted action intent.
use super::*;
impl Changes {
    pub(super) fn git_mode_inputs(&mut self, ctx: &egui::Context, visible: bool) {
        if !visible
            || self
                .review
                .as_ref()
                .map_or(self.source != Source::Git, |doc| doc.3)
        {
            self.mode_controls.clear();
            return;
        }
        if ctx.memory(|m| m.top_modal_layer().is_some()) || egui::Popup::is_any_open(ctx) {
            return;
        }
        let focused = ctx.memory(|m| m.focused());
        let pointer = ctx.input(|i| {
            (i.pointer.primary_pressed() || i.pointer.primary_released())
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        let pointer_owns = self.mode_controls.iter().any(|id| {
            ctx.read_response(*id).is_some_and(|r| {
                r.enabled()
                    && pointer.is_some_and(|p| {
                        r.interact_rect.contains(p) && ctx.layer_id_at(p) == Some(r.layer_id)
                    })
            })
        });
        let owns = self.mode_controls.iter().any(|id| {
            focused == Some(*id) || focused.is_none() && ctx.memory(|m| m.focus_lost_by_escape(*id))
        });
        if owns || pointer_owns {
            let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if escape && let Some(id) = focused {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
            ctx.input_mut(|i|i.events.retain(|e|!(matches!(e,egui::Event::Text(_)|egui::Event::Paste(_))||matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)||pointer_owns&&matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Space,..}))));
        }
    }
    pub(super) fn git_mode_controls(&mut self, ui: &mut egui::Ui) {
        self.mode_controls.clear();
        egui::TopBottomPanel::top("git-diff-modes")
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 4)))
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (mode, label) in [
                        (workspace::DiffMode::Combined, "Combined"),
                        (workspace::DiffMode::Staged, "Staged"),
                        (workspace::DiffMode::Unstaged, "Unstaged"),
                    ] {
                        let response = ui
                            .push_id(("git-diff-mode", label), |ui| {
                                ui.add_enabled(
                                    !egui::Popup::is_any_open(ui.ctx())
                                        && ui.memory(|m| m.top_modal_layer().is_none()),
                                    egui::Button::selectable(
                                        self.git_mode == mode,
                                        theme::small(ui, label),
                                    ),
                                )
                            })
                            .inner;
                        self.mode_controls.push(response.id);
                        #[cfg(test)]
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(
                                egui::Id::new(("git-diff-mode", label)),
                                (response.id, response.rect),
                            )
                        });
                        if response.clicked() && self.git_mode != mode {
                            self.git_mode = mode;
                            self.diffs.clear();
                            self.inline.clear();
                            self.cancel_index_intent();
                            context_capture::clear_source(ui.ctx());
                            inline_review::Inline::clear_selection(ui.ctx());
                        }
                    }
                });
            });
    }
    pub(super) fn git_mode_includes(&self, file: &workspace::FileChange) -> bool {
        if self.git_mode == workspace::DiffMode::Combined {
            return true;
        }
        let bytes = file.status.as_bytes();
        if bytes.len() != 2 {
            return false;
        }
        match self.git_mode {
            workspace::DiffMode::Combined => true,
            workspace::DiffMode::Staged => !matches!(bytes[0], b' ' | b'?' | b'!'),
            workspace::DiffMode::Unstaged => !matches!(bytes[1], b' ' | b'!'),
        }
    }
}
#[cfg(test)]
mod tests;
