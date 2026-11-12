//! Session-local workspace panes. The sessions sidebar's width stays window-global.
mod persistence;
use super::*;
use egui::containers::panel::PanelState;
pub(super) use persistence::Saved;
use std::collections::BTreeMap;

const PANELS: [&str; 7] = [
    "workspace-files-dock",
    "workspace-files-stack",
    "files-tree",
    "workspace-changes-dock",
    "workspace-changes-stack",
    "project-terminal",
    "floating-inspector-v2",
];

#[derive(Default)]
struct Layout {
    files: files::Browser,
    changes: changes::Changes,
    inspector: bool,
    hide_inspector: bool,
    hide_sidebar: bool,
    terminal: (bool, bool),
    panels: Vec<Option<PanelState>>,
}

#[derive(Default)]
pub(super) struct SessionLayouts {
    saved: BTreeMap<String, Layout>,
}

impl SessionLayouts {
    pub(super) fn forget_path(&mut self, path: &std::path::Path) {
        let canonical = sync::identity(path);
        self.saved.retain(|key, _| {
            key.starts_with("agent:") || sync::identity(std::path::Path::new(key)) != canonical
        });
    }
    pub(super) fn forget_agent(&mut self, id: &str) {
        self.saved.remove(&format!("agent:{id}"));
    }
}

impl Pui {
    fn layout_key(&self) -> String {
        self.pending_session
            .as_ref()
            .map(|path| path.display().to_string())
            .or_else(|| self.state.session_file.clone())
            .unwrap_or_else(|| format!("agent:{}", self.agents.active))
    }

    pub(super) fn leave_session_layout(&mut self) {
        self.close_model_popovers();
        context_capture::clear_source(&self.ctx);
        let mut selection = egui::text_selection::LabelSelectionState::load(&self.ctx);
        selection.clear_selection();
        selection.store(&self.ctx);
        self.transcript.gallery.release_textures();
        self.draft_images.gallery.release_textures();
        self.files.hide_web();
        self.files.begin_frame();
        self.workspace_rx = None;
        self.diff_rx = None; // abandoned Git worker cannot select a file in another chat
        self.changes.cancel_git_read();
        self.changes.cancel_inline_reads();
        self.changes.close_branches(&self.ctx);
        let panels = PANELS
            .iter()
            .map(|name| PanelState::load(&self.ctx, egui::Id::new(name)))
            .collect();
        // Review notes are bounded per window, not per parked session layout.
        let review_comments = std::mem::take(&mut self.changes.review_comments);
        let layout = Layout {
            files: std::mem::take(&mut self.files),
            changes: std::mem::take(&mut self.changes),
            inspector: std::mem::take(&mut self.inspector_panel),
            hide_inspector: std::mem::take(&mut self.prefs.hide_inspector),
            hide_sidebar: std::mem::take(&mut self.prefs.hide_sidebar),
            terminal: self.terminals.layout(),
            panels,
        };
        self.changes.review_comments = review_comments;
        self.session_layouts.saved.insert(self.layout_key(), layout);
        self.terminals.set_layout(&self.ctx, (false, false));
        self.set_layout_panels(&[]);
    }

    pub(super) fn enter_session_layout(&mut self) {
        let key = self.layout_key();
        let had_memory_layout = self.session_layouts.saved.contains_key(&key)
            || self
                .session_layouts
                .saved
                .contains_key(&format!("agent:{}", self.agents.active));
        let layout = self
            .session_layouts
            .saved
            .remove(&self.layout_key())
            // An unsaved parked chat can acquire its session path in the background.
            .or_else(|| {
                self.session_layouts
                    .saved
                    .remove(&format!("agent:{}", self.agents.active))
            })
            .unwrap_or_default();
        self.files.hide_web();
        self.files = layout.files;
        let review_comments = std::mem::take(&mut self.changes.review_comments);
        self.changes = layout.changes;
        self.changes.review_comments = review_comments;
        self.inspector_panel = layout.inspector;
        self.prefs.hide_inspector = layout.hide_inspector;
        self.prefs.hide_sidebar = layout.hide_sidebar;
        self.terminals.set_layout(&self.ctx, layout.terminal);
        self.set_layout_panels(&layout.panels);
        if !had_memory_layout {
            self.restore_workspace_layout();
        }
    }

    fn set_layout_panels(&self, panels: &[Option<PanelState>]) {
        self.ctx.data_mut(|data| {
            for (index, name) in PANELS.iter().enumerate() {
                let id = egui::Id::new(name);
                if let Some(Some(state)) = panels.get(index).filter(|state| {
                    state.as_ref().is_some_and(|state| {
                        let size = state.rect.size();
                        state.rect.is_finite()
                            && size.x > 0.0
                            && size.y > 0.0
                            && size.x <= 16_384.0
                            && size.y <= 16_384.0
                    })
                }) {
                    data.insert_persisted(id, *state);
                } else {
                    data.remove::<PanelState>(id);
                }
            }
        });
    }
}

#[cfg(test)]
mod persistence_tests;
#[cfg(test)]
mod terminal_tests;
#[cfg(test)]
mod tests;
