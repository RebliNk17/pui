//! Bounded, presentation-only layout checkpoints. Restore never starts a shell or reads a tab.
use super::*;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::app) struct Saved {
    pub key: String,
    pub browser: files::SavedLayout,
    pub changes_open: bool,
    pub changes_expanded: bool,
    pub hide_inspector: bool,
    pub hide_sidebar: bool,
    pub terminal: (bool, bool),
    // Window-local tab presentation. Missing means a legacy layout-only checkpoint.
    pub terminal_tabs: Option<terminal::SavedTabs>,
    pub panels: Vec<Option<PanelState>>,
}
impl Pui {
    pub(in crate::app) fn save_workspace_layouts(&mut self) {
        let retained = std::mem::take(&mut self.prefs.workspace_layouts);
        let mut layouts: Vec<_> = self
            .session_layouts
            .saved
            .iter()
            .rev()
            .take(63)
            .map(|(key, layout)| Saved {
                key: key.clone(),
                browser: layout.files.saved_layout(),
                changes_open: layout.inspector,
                changes_expanded: layout.changes.expanded(),
                hide_inspector: layout.hide_inspector,
                hide_sidebar: layout.hide_sidebar,
                terminal: layout.terminal,
                terminal_tabs: None,
                panels: layout.panels.clone(),
            })
            .collect();
        let mut terminal_tabs = self.terminals.saved_tabs();
        for layout in retained.iter().take(64) {
            if let Some(saved) = &layout.terminal_tabs {
                terminal_tabs.retain_unopened(saved);
            }
        }
        let key = self.layout_key();
        layouts.retain(|layout| layout.key != key);
        layouts.push(Saved {
            key,
            browser: self.files.saved_layout(),
            changes_open: self.inspector_panel,
            changes_expanded: self.changes.expanded(),
            hide_inspector: self.prefs.hide_inspector,
            hide_sidebar: self.prefs.hide_sidebar,
            terminal: self.terminals.layout(),
            terminal_tabs: Some(terminal_tabs),
            panels: PANELS
                .iter()
                .map(|name| PanelState::load(&self.ctx, egui::Id::new(name)))
                .collect(),
        });
        for mut layout in retained {
            if layouts.len() >= 64 {
                break;
            }
            if !self.session_layouts.saved.contains_key(&layout.key)
                && !layouts.iter().any(|saved| saved.key == layout.key)
            {
                // One bounded window snapshot, not 64 competing copies of the same tabs.
                layout.terminal_tabs = None;
                layouts.push(layout);
            }
        }
        self.prefs.workspace_layouts = layouts;
        self.prefs.closed_projects = self.closed_projects.clone();
        self.prefs.expanded_projects = self.expanded_projects.clone();
    }

    pub(in crate::app) fn restore_workspace_layout(&mut self) {
        // Tabs belong to the window/project, not a chat. Consume all presentation
        // metadata once, including hidden roots, without activating any process.
        for layout in self.prefs.workspace_layouts.iter_mut().take(64) {
            if let Some(saved) = layout.terminal_tabs.take() {
                self.terminals.restore_tabs(&saved);
            }
        }
        let key = self.layout_key();
        let Some(index) = self
            .prefs
            .workspace_layouts
            .iter()
            .position(|layout| layout.key == key)
        else {
            return;
        };
        // Consume once; an intentionally closed in-memory view must not rehydrate again.
        let layout = self.prefs.workspace_layouts.remove(index);
        self.files.restore_layout(&self.prefs.cwd, &layout.browser);
        self.inspector_panel = layout.changes_open;
        self.changes.set_expanded(layout.changes_expanded);
        self.prefs.hide_inspector = layout.hide_inspector;
        self.prefs.hide_sidebar = layout.hide_sidebar;
        // Presentation only: no PTY is created. Explicitly opening Terminal starts
        // its selected shell; checkpoint hydration alone never launches a process.
        self.terminals
            .restore_saved_layout(&self.ctx, &self.prefs.cwd, layout.terminal);
        self.set_layout_panels(&layout.panels);
    }
}
