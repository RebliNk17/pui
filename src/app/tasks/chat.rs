//! Read-only workflow progress lives in the conversation, not a second workspace.
use super::*;
use std::{cell::RefCell, path::Path, time::Instant};

/// Display-only identities. Authority checks still resolve paths afresh in sync.
/// Bound both retention and staleness without adding an idle repaint timer.
#[derive(Default)]
pub(super) struct DisplayPaths(RefCell<BTreeMap<PathBuf, (PathBuf, Instant)>>);
impl DisplayPaths {
    fn resolve(&self, path: &Path) -> PathBuf {
        let mut cache = self.0.borrow_mut();
        if let Some((resolved, at)) = cache.get(path) {
            if at.elapsed() < Duration::from_secs(2) {
                return resolved.clone();
            }
        }
        if cache.len() >= 512 && !cache.contains_key(path) {
            if let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, (_, at))| *at)
                .map(|(path, _)| path.clone())
            {
                cache.remove(&oldest);
            }
        }
        let resolved = sync::identity(path);
        cache.insert(path.into(), (resolved.clone(), Instant::now()));
        resolved
    }
}

pub(in crate::app) struct ChatCard {
    id: String,
    objective: String,
    phase: Phase,
}

#[cfg(test)]
#[path = "chat_tests.rs"]
mod tests;

impl Pui {
    pub(in crate::app) fn task_in_chat(&self, task: &Task) -> bool {
        if let Some(parent) = &task.parent_session {
            return self.state.session_file.as_deref().is_some_and(|path| {
                Path::new(path) == parent
                    || self.tasks.display_paths.resolve(Path::new(path))
                        == self.tasks.display_paths.resolve(parent)
            });
        }
        self.tasks.parents.get(&task.id) == Some(&self.agents.active)
            && !self.agents.active.is_empty()
    }

    /// Viewing a task explicitly keeps its context; ordinary chats only see
    /// tasks owned by that exact session, never the last globally selected task.
    pub(in crate::app) fn task_for_chat(&self) -> Option<&Task> {
        self.tasks
            .current()
            .filter(|t| self.task_in_chat(t))
            .or_else(|| {
                self.tasks
                    .records
                    .iter()
                    .filter(|t| self.task_in_chat(t))
                    .max_by_key(|t| t.updated)
            })
    }

    pub(in crate::app) fn workflow_task(&self) -> Option<&Task> {
        if self.tasks.open {
            self.tasks.current()
        } else {
            self.task_for_chat()
        }
    }

    pub(in crate::app) fn inspector_workflow(&self) -> Option<&Task> {
        if self.tasks.open {
            return self.tasks.current();
        }
        self.tasks
            .records
            .iter()
            .filter(|t| t.phase.active() && self.task_in_chat(t))
            .max_by_key(|t| t.updated)
            .or_else(|| self.task_for_chat())
    }

    fn task_same_session(&self, left: &Task, right: &Task) -> bool {
        if left.id == right.id {
            return true;
        }
        match (&left.parent_session, &right.parent_session) {
            (Some(left), Some(right)) => sync::identity(left) == sync::identity(right),
            (None, None) => self
                .tasks
                .parents
                .get(&left.id)
                .is_some_and(|parent| self.tasks.parents.get(&right.id) == Some(parent)),
            _ => false,
        }
    }

    pub(in crate::app) fn toggle_chat_tasks(&mut self) {
        if !self.tasks.open && self.task_for_chat().is_none() {
            return;
        }
        self.kanban.open = false;
        self.automation.open = false;
        if self.tasks.open {
            self.task_parent_chat();
        } else if let Some(id) = self.task_for_chat().map(|t| t.id.clone()) {
            self.open_task_from_board(id, None);
        }
    }

    pub(in crate::app) fn chat_task_cards(&self) -> Vec<ChatCard> {
        self.tasks
            .records
            .iter()
            .filter(|t| self.task_in_chat(t))
            .map(|t| ChatCard {
                id: t.id.clone(),
                objective: t.objective.clone(),
                phase: t.phase,
            })
            .collect()
    }
}

/// Rendering footer summaries must not borrow or clone the conversation history.
pub(in crate::app) fn task_chat_cards(ui: &mut egui::Ui, tasks: &[ChatCard]) -> Option<String> {
    let colors = theme::palette(ui.ctx());
    let mut selected = None;
    for task in tasks {
        egui::Frame::new()
            .fill(colors.surface)
            .corner_radius(12)
            .inner_margin(14)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(theme::caption(ui, "WORKFLOW"));
                ui.add(
                    egui::Label::new(
                        RichText::new(if task.objective.is_empty() {
                            "Untitled workflow"
                        } else {
                            &task.objective
                        })
                        .strong(),
                    )
                    .wrap(),
                );
                ui.horizontal_wrapped(|ui| {
                    ui.label(theme::small(ui, task.phase.label()));
                    if ui.button("Open workflow").clicked() {
                        selected = Some(task.id.clone());
                    }
                });
            });
        ui.add_space(12.0);
    }
    selected
}

impl Pui {
    pub(in crate::app) fn task_header(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            if ui.button("← Conversation").clicked() {
                self.task_parent_chat();
            }
            self.task_picker(ui);
            if self.tasks.current().is_some_and(|task| task.phase.active())
                && ui
                    .add_enabled(
                        !self.demo && self.tasks.live == self.tasks.selected,
                        egui::Button::new("Stop workflow"),
                    )
                    .clicked()
            {
                self.tasks.stop();
            }
            ui.menu_button("More", |ui| {
                theme::menu_controls(ui);
                if let Some(id) = self.tasks.selected.clone() {
                    if ui
                        .button(RichText::new("Delete workflow history…").color(colors.red))
                        .clicked()
                    {
                        self.request_delete(deletion::Target::Task(id));
                        ui.close();
                    }
                }
            });
        });
    }

    fn task_picker(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width().clamp(80.0, 320.0);
        let context = self.workflow_task();
        let mut selected = context.map(|t| t.id.clone());
        let title = context
            .map(|t| {
                if t.objective.is_empty() {
                    "Untitled workflow"
                } else {
                    &t.objective
                }
            })
            .unwrap_or("Choose workflow");
        let response = ui
            .allocate_ui_with_layout(
                Vec2::new(width, ui.spacing().interact_size.y),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.set_width(width);
                    egui::ComboBox::from_id_salt("task-history")
                        .selected_text(title)
                        .width(width)
                        .truncate()
                        .height(300.0)
                        .show_ui(ui, |ui| {
                            // ComboBox defaults to Extend inside its popup. Bound BOTH the
                            // menu UI and each selectable label, including unbroken titles.
                            ui.set_width((width - 16.0).max(48.0));
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                            for task in self.tasks.records.iter().filter(|t| {
                                context.is_some_and(|context| self.task_same_session(t, context))
                            }) {
                                let label = format!(
                                    "{} · {}",
                                    task.phase.label(),
                                    if task.objective.is_empty() {
                                        "Untitled"
                                    } else {
                                        &task.objective
                                    }
                                );
                                let row = ui.add_sized(
                                    [ui.available_width(), 28.0],
                                    egui::Button::selectable(
                                        selected.as_ref() == Some(&task.id),
                                        &label,
                                    )
                                    .truncate(),
                                );
                                let galley = egui::WidgetText::from(label.as_str()).into_galley(
                                    ui,
                                    Some(egui::TextWrapMode::Truncate),
                                    (row.rect.width() - 2.0 * ui.spacing().button_padding.x)
                                        .max(0.0),
                                    egui::TextStyle::Button,
                                );
                                let row = if galley.elided {
                                    row.on_hover_text(&label)
                                } else {
                                    row
                                };
                                if row.clicked() {
                                    selected = Some(task.id.clone());
                                }
                            }
                            #[cfg(test)]
                            ui.ctx().data_mut(|d| {
                                d.insert_temp(
                                    egui::Id::new("task-picker-popup-rect"),
                                    ui.min_rect(),
                                )
                            });
                        })
                        .response
                },
            )
            .inner;
        let text_width = response.rect.width()
            - 2.0 * ui.spacing().button_padding.x
            - ui.spacing().icon_spacing
            - ui.spacing().icon_width;
        let galley = egui::WidgetText::from(title).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            text_width.max(0.0),
            egui::TextStyle::Button,
        );
        let response = if galley.elided {
            response.on_hover_text(title)
        } else {
            response
        };
        #[cfg(test)]
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("task-picker-rect"), response.rect));
        let _ = response;
        if selected != context.map(|t| t.id.clone()) {
            if let Some(id) = selected {
                self.open_task_from_board(id, None);
            }
        }
    }

    pub(in crate::app) fn task_parent_chat(&mut self) {
        let Some(task) = self.tasks.current().cloned() else {
            self.tasks.open = false;
            return;
        };
        self.tasks.selected_run = None;
        self.tasks.open = false;
        if self.task_in_chat(&task) {
            return;
        }
        if let Some(parent) = self.tasks.parents.get(&task.id).cloned() {
            if self.agents.active == parent || self.agents.parked.contains_key(&parent) {
                self.view_agent(&parent);
                return;
            }
        }
        if let Some(path) = task.parent_session {
            self.open_chat(Session {
                path,
                cwd: task.source.clone(),
                project: task.source,
                title: "Workflow conversation".into(),
                created: None,
                modified: std::time::SystemTime::UNIX_EPOCH,
            });
        }
    }
}
