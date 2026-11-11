//! Session-local workspace panes. The sessions sidebar's width stays window-global.
use super::*;
use egui::containers::panel::PanelState;
use std::collections::BTreeMap;

const PANELS: [&str; 4] = [
    "workspace-files-dock",
    "files-tree",
    "workspace-changes-dock",
    "project-terminal",
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

impl Pui {
    fn layout_key(&self) -> String {
        self.pending_session
            .as_ref()
            .map(|path| path.display().to_string())
            .or_else(|| self.state.session_file.clone())
            .unwrap_or_else(|| format!("agent:{}", self.agents.active))
    }

    pub(super) fn leave_session_layout(&mut self) {
        self.files.hide_web();
        self.files.begin_frame();
        let panels = PANELS
            .iter()
            .map(|name| PanelState::load(&self.ctx, egui::Id::new(name)))
            .collect();
        let layout = Layout {
            files: std::mem::take(&mut self.files),
            changes: std::mem::take(&mut self.changes),
            inspector: std::mem::take(&mut self.inspector_panel),
            hide_inspector: std::mem::take(&mut self.prefs.hide_inspector),
            hide_sidebar: std::mem::take(&mut self.prefs.hide_sidebar),
            terminal: self.terminals.layout(),
            panels,
        };
        self.session_layouts.saved.insert(self.layout_key(), layout);
        self.terminals.set_layout(&self.ctx, (false, false));
        self.set_layout_panels(&[]);
    }

    pub(super) fn enter_session_layout(&mut self) {
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
        self.changes = layout.changes;
        self.inspector_panel = layout.inspector;
        self.prefs.hide_inspector = layout.hide_inspector;
        self.prefs.hide_sidebar = layout.hide_sidebar;
        self.terminals.set_layout(&self.ctx, layout.terminal);
        self.set_layout_panels(&layout.panels);
    }

    fn set_layout_panels(&self, panels: &[Option<PanelState>]) {
        self.ctx.data_mut(|data| {
            for (index, name) in PANELS.iter().enumerate() {
                let id = egui::Id::new(name);
                if let Some(Some(state)) = panels.get(index) {
                    data.insert_persisted(id, *state);
                } else {
                    data.remove::<PanelState>(id);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests;
