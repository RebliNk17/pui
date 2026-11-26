//! Bounded, window-local shell tabs. Hiding or selecting another tab never drops a PTY.
use super::*;
pub(super) const MAX_TABS: usize = 8;
pub(super) const MAX_TOTAL_TABS: usize = 32;
pub(super) fn consume_activation(ctx: &egui::Context) {
    ctx.input_mut(|input| {
        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
        let space = input.consume_key(egui::Modifiers::NONE, egui::Key::Space);
        if space {
            input
                .events
                .retain(|event| !matches!(event,egui::Event::Text(text)if text==" "));
        }
    });
}
pub(super) fn poll_tab(tab: &mut Tab) {
    let Some(rx) = &tab.starting else {
        return;
    };
    match rx.try_recv() {
        Ok(result) => {
            match result {
                Ok(session) => {
                    let pending = std::mem::take(&mut tab.pending);
                    if let Err(error) = session.send(pending) {
                        tab.error = Some(error);
                    }
                    tab.session = Some(session);
                }
                Err(error) => tab.error = Some(error),
            }
            tab.starting = None;
        }
        Err(mpsc::TryRecvError::Disconnected) => {
            tab.starting = None;
            tab.error = Some("Could not start terminal.".into());
        }
        Err(mpsc::TryRecvError::Empty) => {}
    }
}

impl Terminals {
    pub(in crate::app) fn total_tabs(&self) -> usize {
        self.projects
            .values()
            .map(|project| project.tabs.len())
            .sum::<usize>()
            + self.restored_roots.len()
    }
    pub(super) fn owns_focus(&self, ctx: &egui::Context) -> bool {
        let input = self
            .projects
            .get(&self.root)
            .and_then(Project::active)
            .map(Tab::input_id);
        ctx.memory(|memory| {
            memory.focused().map_or_else(
                || {
                    input.is_some_and(|id| memory.had_focus_last_frame(id))
                        || self
                            .controls
                            .iter()
                            .any(|id| memory.had_focus_last_frame(*id))
                },
                |id| Some(id) == input || self.controls.contains(&id),
            )
        })
    }
    pub(super) fn surrender_focus(&self, ctx: &egui::Context) {
        if self.owns_focus(ctx) {
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
        }
    }
    fn select_tab(&mut self, ctx: &egui::Context, index: usize) {
        let Some(project) = self.projects.get_mut(&self.root) else {
            return;
        };
        if index >= project.tabs.len() {
            return;
        }
        capture::clear(ctx);
        project.selected = index;
        self.focus = true;
        project.tabs[index].focus(ctx);
    }
    pub(in crate::app) fn focus_tab_by_id(&mut self, ctx: &egui::Context, tab_id: &str) -> bool {
        if tab_id.is_empty() {
            return false;
        }
        if let Some(index) = self
            .projects
            .get(&self.root)
            .and_then(|project| project.tabs.iter().position(|tab| tab.tab_id == tab_id))
        {
            self.open = true;
            self.select_tab(ctx, index);
            ctx.request_repaint();
            return true;
        }
        let found = self.projects.iter().find_map(|(root, project)| {
            project
                .tabs
                .iter()
                .position(|tab| tab.tab_id == tab_id)
                .map(|index| (root.clone(), index))
        });
        let Some((root, index)) = found else {
            return false;
        };
        if self.root != root {
            capture::clear(ctx);
            self.root = root;
            self.confirm_stop = None;
        }
        self.open = true;
        self.select_tab(ctx, index);
        ctx.request_repaint();
        true
    }
    pub(super) fn close_tab(&mut self, ctx: &egui::Context, root: &Path, id: u64) {
        let Some(project) = self.projects.get_mut(root) else {
            return;
        };
        let Some(index) = project.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        project.tabs.remove(index);
        project.selected = if index < project.selected {
            project.selected - 1
        } else {
            project.selected.min(project.tabs.len().saturating_sub(1))
        };
        capture::clear(ctx);
        // Keep an empty group so a deliberately closed final tab never
        // implicitly spawns on show(), unlike a never-opened project.
        if self.root == root {
            if let Some(tab) = project.active() {
                tab.focus(ctx);
            }
            self.focus = true;
        }
    }
    pub(super) fn tab_inputs(&mut self, ctx: &egui::Context) {
        if !self.owns_focus(ctx)
            || self.confirm_stop.is_some()
            || egui::Popup::is_any_open(ctx)
            || ctx.memory(|memory| memory.top_modal_layer().is_some())
        {
            return;
        }
        let project = self.projects.get(&self.root);
        let count = project.map_or(0, |project| project.tabs.len());
        let mut selected = project.map_or(0, |project| project.selected);
        let mut changed = false;
        let toolbar = ctx.memory(|memory| {
            memory.focused().map_or_else(
                || {
                    self.controls
                        .iter()
                        .any(|id| memory.had_focus_last_frame(*id))
                },
                |id| self.controls.contains(&id),
            )
        });
        let mut escaped = false;
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                let egui::Event::Key {
                    key: egui::Key::Tab,
                    modifiers,
                    pressed: true,
                    ..
                } = event
                else {
                    return true;
                };
                if !modifiers.ctrl || modifiers.alt || modifiers.mac_cmd {
                    return true;
                }
                if count > 0 {
                    selected = if modifiers.shift {
                        (selected + count - 1) % count
                    } else {
                        (selected + 1) % count
                    };
                    changed = true;
                }
                false
            });
            // Focused terminal surfaces own these chords before inline approvals.
            input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
            input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
            escaped = toolbar && input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
        });
        if escaped && let Some(tab) = self.projects.get(&self.root).and_then(Project::active) {
            tab.focus(ctx);
            self.focus = true;
        }
        if changed {
            self.select_tab(ctx, selected);
        }
    }
    pub(super) fn tab_rail(&mut self, ui: &mut egui::Ui, demo: bool) {
        let Some(project) = self.projects.get(&self.root) else {
            ui.label(theme::small(
                ui,
                "Terminal limit reached in this window. Close another shell tab to continue.",
            ));
            return;
        };
        let can_add = project.tabs.len() < MAX_TABS && self.total_tabs() < MAX_TOTAL_TABS;
        let enabled = prompt_ui::keyboard_available(ui) && !egui::Popup::is_any_open(ui.ctx());
        let mut select = None;
        let mut close = None;
        let mut add = false;
        ui.spacing_mut().interact_size.y = 26.0;
        ui.spacing_mut().button_padding = egui::vec2(6.0, 4.0);
        egui::TopBottomPanel::bottom("terminal-shell-actions")
            .exact_height(30.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(2, 2)))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().interact_size.y = 24.0;
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let plus = chrome::new_shell(ui, enabled && can_add);
                    self.controls.push(plus.id);
                    add = plus.clicked();
                    let remove = ui
                        .add_enabled_ui(enabled && project.active().is_some(), |ui| {
                            icons::button(
                                ui,
                                Icon::Close,
                                "Close selected shell tab · running shells require confirmation",
                            )
                        })
                        .inner;
                    self.controls.push(remove.id);
                    for response in [&plus, &remove] {
                        if response.has_focus() {
                            ui.memory_mut(|memory| {
                                memory.set_focus_lock_filter(
                                    response.id,
                                    egui::EventFilter {
                                        escape: true,
                                        ..Default::default()
                                    },
                                )
                            });
                        }
                    }
                    if remove.clicked() {
                        close = project.active().map(|tab| (tab.id, tab.running()));
                    }
                    #[cfg(test)]
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(
                            egui::Id::new("terminal-tab-add"),
                            (plus.id, plus.rect, plus.enabled()),
                        );
                        data.insert_temp(
                            egui::Id::new("terminal-tab-close"),
                            (remove.id, remove.rect),
                        );
                    });
                });
            });
        egui::ScrollArea::vertical()
            .id_salt(("terminal-tabs", &self.root))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (index, tab) in project.tabs.iter().enumerate() {
                    let selected = project.selected == index;
                    let response = chrome::shell_row(ui, tab, selected, enabled);
                    self.controls.push(response.id);
                    if selected && self.focus {
                        response.scroll_to_me(Some(Align::Center));
                    }
                    if response.clicked() {
                        select = Some(index);
                    }
                    if response.has_focus() {
                        ui.memory_mut(|memory| {
                            memory.set_focus_lock_filter(
                                response.id,
                                egui::EventFilter {
                                    escape: true,
                                    ..Default::default()
                                },
                            )
                        });
                    }
                    #[cfg(test)]
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(
                            egui::Id::new(("terminal-tab", index)),
                            (response.id, response.rect, tab.id),
                        )
                    });
                }
            });
        if select.is_some() || add || close.is_some() {
            // Enter/Space activating a toolbar button is UI input, never a
            // carriage return/literal space for the newly selected PTY.
            consume_activation(ui.ctx());
        }
        if let Some(index) = select {
            self.select_tab(ui.ctx(), index);
        }
        if add {
            self.start(ui.ctx(), demo);
            self.focus = true;
        }
        if let Some((id, running)) = close {
            if running {
                self.confirm_stop = Some((self.root.clone(), id));
            } else {
                let root = self.root.clone();
                self.close_tab(ui.ctx(), &root, id);
            }
        }
    }
}

#[cfg(test)]
mod input_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod tests;
