//! Presentation-only sections. Root chats may be saved or pathless; trees/pins keep precedence.
use super::*;
use std::collections::BTreeSet;
mod confirmation;

const LIMIT: usize = 64;
const NAME_LIMIT: usize = 80;

#[derive(Clone, Serialize)]
pub(in crate::app) struct Section {
    pub id: String,
    pub name: String,
    pub collapsed: bool,
    pub expanded: bool,
}
impl<'de> Deserialize<'de> for Section {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Saved {
            Name(String),
            Record {
                id: String,
                name: String,
                #[serde(default)]
                collapsed: bool,
                #[serde(default)]
                expanded: bool,
            },
        }
        Ok(match Saved::deserialize(deserializer)? {
            Saved::Name(name) => Self::new(name),
            Saved::Record {
                id,
                name,
                collapsed,
                expanded,
            } => Self {
                id,
                name,
                collapsed,
                expanded,
            },
        })
    }
}
impl Section {
    pub fn new(name: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            collapsed: false,
            expanded: false,
        }
    }
}
#[derive(Default)]
pub(in crate::app) struct View {
    editing: Option<(String, String)>,
    remove: Option<String>,
    creating: Option<String>,
    focus: bool,
    // Runtime agent identities only. Never serialize arbitrary pathless drafts.
    unsaved: BTreeMap<String, String>,
    pub(super) unsaved_popup: Option<egui::Id>,
}
impl View {
    pub(in crate::app) fn confirming(&self) -> bool {
        self.remove.is_some()
    }
    pub(in crate::app) fn editing(&self) -> bool {
        self.editing.is_some()
    }
    pub(in crate::app) fn creating(&self) -> Option<&str> {
        self.creating.as_deref()
    }
}

pub(in crate::app) fn normalize(options: &mut sidebar_view::Options) {
    options.sections.truncate(LIMIT);
    let mut ids = BTreeSet::new();
    for section in &mut options.sections {
        if section.id.is_empty()
            || section.id.len() > 80
            || section.id.chars().any(char::is_control)
            || !ids.insert(section.id.clone())
        {
            section.id = uuid::Uuid::new_v4().to_string();
            ids.insert(section.id.clone());
        }
        section.name = section
            .name
            .chars()
            .filter(|ch| !ch.is_control())
            .take(NAME_LIMIT)
            .collect::<String>()
            .trim()
            .into();
        if section.name.is_empty() {
            section.name = "Untitled section".into();
        }
    }
}

/// Partition the already-filtered, sorted root indices. Never flatten or clone trees.
pub(in crate::app) fn partition(
    app: &Pui,
    branches: &[sidebar_tree::Branch],
    visible: &mut Vec<usize>,
) -> BTreeMap<String, Vec<usize>> {
    let ids: BTreeSet<_> = app
        .prefs
        .sidebar_view
        .sections
        .iter()
        .map(|section| section.id.as_str())
        .collect();
    let mut grouped = BTreeMap::<String, Vec<usize>>::new();
    visible.retain(|index| {
        let branch = &branches[*index];
        let pathless_root = if let sidebar_tree::Target::Agent(id) = &branch.target {
            app.agents
                .nodes
                .get(id)
                .filter(|node| node.parent.is_none() && node.path.is_none())
                .and_then(|_| app.section_view.unsaved.get(id))
        } else {
            None
        };
        let section = branch
            .path
            .as_ref()
            .and_then(|path| app.prefs.chats.get(path))
            .and_then(|prefs| prefs.section.as_ref())
            .or(pathless_root)
            .filter(|section| ids.contains(section.as_str()));
        if let Some(section) = section {
            grouped.entry(section.clone()).or_default().push(*index);
            false
        } else {
            true
        }
    });
    grouped
}

pub(in crate::app) fn assignment_menu(
    ui: &mut egui::Ui,
    sections: &[Section],
    meta: &mut ChatPrefs,
) {
    assignment(ui, sections, &mut meta.section);
}
fn assignment(ui: &mut egui::Ui, sections: &[Section], member: &mut Option<String>) {
    ui.menu_button("Move to section", |ui| {
        theme::menu_controls(ui);
        ui.set_width(240.0_f32.min((ui.ctx().screen_rect().width() - 40.0).max(120.0)));
        if ui.add(egui::Button::selectable(member.is_none(), "No section · use grouping").truncate()).clicked() {
            *member = None;
            ui.close();
        }
        egui::ScrollArea::vertical().id_salt("section-choices").max_height(240.0).show(ui, |ui| {
            for section in sections {
                ui.push_id(&section.id, |ui| {
                    if ui.add_sized([ui.available_width(), 24.0], egui::Button::selectable(member.as_deref() == Some(&section.id), &section.name).truncate()).on_hover_text(&section.name).clicked() {
                        *member = Some(section.id.clone());
                        ui.close();
                    }
                });
            }
        });
        ui.separator();
        ui.add(egui::Label::new(theme::small(ui, "Presentation only. Pinned chats stay in Pinned; child chats stay with their parent.")).wrap());
    });
}

impl Pui {
    pub(in crate::app) fn unsaved_section_inputs(&mut self, ctx: &egui::Context) {
        let Some(owner) = self.section_view.unsaved_popup else {
            return;
        };
        if !egui::Popup::is_id_open(ctx, owner)
            || ctx.memory(|memory| memory.top_modal_layer().is_some())
            || self.prefs.hide_sidebar
            || self.settings_open
            || self.question_designs.is_some()
        {
            egui::Popup::close_id(ctx, owner);
            self.section_view.unsaved_popup = None;
            return;
        }
        let escape = ctx.input_mut(|input| {
            input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
            input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
        });
        if escape {
            egui::Popup::close_id(ctx, owner);
            self.section_view.unsaved_popup = None;
        }
    }
    pub(in crate::app) fn forget_unsaved_sections(&mut self, ids: &BTreeSet<String>) {
        self.section_view.unsaved.retain(|id, _| !ids.contains(id));
    }
    pub(in crate::app) fn promote_unsaved_section(&mut self, id: &str) {
        let Some(node) = self.agents.nodes.get(id) else {
            return;
        };
        if node.parent.is_some() {
            self.section_view.unsaved.remove(id);
            return;
        }
        let Some(path) = node.path.clone() else {
            return;
        };
        if let Some(section) = self.section_view.unsaved.remove(id)
            && self
                .prefs
                .sidebar_view
                .sections
                .iter()
                .any(|candidate| candidate.id == section)
        {
            self.prefs
                .chats
                .entry(path)
                .or_default()
                .section
                .get_or_insert(section);
        }
    }
    fn set_unsaved_section(&mut self, id: &str, section: Option<String>) {
        if let Some(section) = section {
            if self.section_view.unsaved.contains_key(id) || self.section_view.unsaved.len() < 256 {
                self.section_view.unsaved.insert(id.into(), section);
            }
        } else {
            self.section_view.unsaved.remove(id);
        }
    }
    pub(super) fn unsaved_section_menu(&mut self, ui: &mut egui::Ui, id: &str) {
        self.section_view
            .unsaved
            .retain(|id, _| self.agents.nodes.contains_key(id));
        let Some(node) = self.agents.nodes.get(id) else {
            return;
        };
        if node.path.is_some() || node.parent.is_some() {
            return;
        }
        let mut section = self.section_view.unsaved.get(id).cloned();
        let available =
            self.section_view.unsaved.contains_key(id) || self.section_view.unsaved.len() < 256;
        ui.add_enabled_ui(available, |ui| {
            assignment(ui, &self.prefs.sidebar_view.sections, &mut section)
        });
        if !available {
            ui.label(theme::small(ui,"Pathless section limit reached (256 chats). Save a chat or remove an assignment first."));
        }
        self.set_unsaved_section(id, section);
    }
    pub(in crate::app) fn demo_sidebar_sections(&mut self) {
        if !self.demo {
            return;
        }
        let review = Section::new("Review next".into());
        let later = Section::new("Later".into());
        for (index, session) in self.sessions.iter().take(4).enumerate() {
            self.prefs
                .chats
                .entry(session.path.clone())
                .or_default()
                .section = Some(if index < 2 {
                review.id.clone()
            } else {
                later.id.clone()
            });
        }
        // Include one pathless root to exercise runtime-only membership without a
        // synthetic session path or process/model activity.
        let unsent = self.ensure_agent();
        self.agents.nodes.get_mut(&unsent).unwrap().name = "Unsent plan".into();
        self.set_unsaved_section(&unsent, Some(review.id.clone()));
        self.prefs.sidebar_view.sections = vec![review, later];
        self.prefs.sidebar_view.empty_groups = true;
        self.prefs.hide_inspector = true;
    }
    pub(in crate::app) fn new_sidebar_section(&mut self) {
        if self.section_view.confirming() {
            return;
        }
        if let Some((id, _)) = self.section_view.editing.clone() {
            self.cancel_section_rename(&id);
        }
        if self.prefs.sidebar_view.sections.len() >= LIMIT {
            return;
        }
        let name = (1..)
            .map(|n| format!("Section {n}"))
            .find(|name| {
                !self
                    .prefs
                    .sidebar_view
                    .sections
                    .iter()
                    .any(|section| &section.name == name)
            })
            .unwrap();
        let section = Section::new(name);
        let id = section.id.clone();
        self.prefs.sidebar_view.sections.push(section);
        self.begin_section_rename(&id);
        self.section_view.creating = Some(id);
    }
    fn begin_section_rename(&mut self, id: &str) {
        let Some(section) = self
            .prefs
            .sidebar_view
            .sections
            .iter()
            .find(|section| section.id == id)
        else {
            return;
        };
        let editor = egui::Id::new(("section-rename", id));
        let mut state = egui::text_edit::TextEditState::default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(section.name.chars().count()),
            )));
        state.store(&self.ctx, editor);
        self.section_view.creating = None;
        self.section_view.editing = Some((id.into(), section.name.clone()));
        self.section_view.focus = true;
        self.focus_composer = false;
        self.ctx.request_repaint();
    }
    fn section_name_valid(&self, id: &str, name: &str) -> bool {
        let name = name.trim();
        !name.is_empty()
            && name.chars().count() <= NAME_LIMIT
            && !name.chars().any(char::is_control)
            && !self
                .prefs
                .sidebar_view
                .sections
                .iter()
                .any(|section| section.id != id && section.name.eq_ignore_ascii_case(name))
    }
    fn remove_sidebar_section(&mut self, id: &str) {
        self.prefs
            .sidebar_view
            .sections
            .retain(|section| section.id != id);
        self.section_view.unsaved.retain(|_, section| section != id);
        for meta in self.prefs.chats.values_mut() {
            if meta.section.as_deref() == Some(id) {
                meta.section = None;
            }
        }
        self.section_view.editing = None;
        self.section_view.creating = None;
    }
    fn cancel_section_rename(&mut self, id: &str) {
        if self.section_view.creating.as_deref() == Some(id) {
            self.prefs
                .sidebar_view
                .sections
                .retain(|section| section.id != id);
        }
        self.section_view.creating = None;
        self.section_view.editing = None;
    }

    pub(super) fn section_rows(
        &mut self,
        ui: &mut egui::Ui,
        branches: &[sidebar_tree::Branch],
        groups: &BTreeMap<String, Vec<usize>>,
        query: &str,
    ) -> Option<Action> {
        let mut action = None;
        // Actions are deferred until the header menu closes; index is never a persistent identity.
        let ids: Vec<_> = self
            .prefs
            .sidebar_view
            .sections
            .iter()
            .map(|section| section.id.clone())
            .collect();
        for id in ids {
            let Some(index) = self
                .prefs
                .sidebar_view
                .sections
                .iter()
                .position(|section| section.id == id)
            else {
                continue;
            };
            let section = self.prefs.sidebar_view.sections[index].clone();
            let members = groups.get(&id).map(Vec::as_slice).unwrap_or_default();
            let editing = self
                .section_view
                .editing
                .as_ref()
                .is_some_and(|(key, _)| key == &id);
            if members.is_empty() && !self.prefs.sidebar_view.empty_groups && !editing {
                continue;
            }
            ui.add_space(5.0);
            let colors = theme::palette(ui.ctx());
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::hover());
            let menu_rect = egui::Rect::from_center_size(
                rect.right_center() - egui::vec2(12.0, 0.0),
                Vec2::splat(24.0),
            );
            let heading_rect = egui::Rect::from_min_max(
                rect.min,
                egui::pos2(menu_rect.left() - 4.0, rect.bottom()),
            );
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new(("section-header-rects", &id)),
                    (heading_rect, menu_rect),
                )
            });
            let heading = ui.interact(
                heading_rect,
                ui.make_persistent_id(("section-heading", &id)),
                if editing {
                    Sense::hover()
                } else {
                    Sense::click()
                },
            );
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new(("section-header-id", &id)), heading.id)
            });
            if heading.hovered() || heading.has_focus() {
                ui.painter().rect_filled(heading_rect, 5, colors.hover);
            }
            icons::chevron(
                ui,
                egui::Rect::from_center_size(
                    rect.left_center() + egui::vec2(6.0, 0.0),
                    Vec2::splat(12.0),
                ),
                !section.collapsed,
                colors.dim,
            );
            if editing {
                let editor_id = egui::Id::new(("section-rename", &id));
                let focused = ui.memory(|memory| memory.has_focus(editor_id));
                let cancel = focused
                    && ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                    });
                let save = focused
                    && ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    });
                let (_, value) = self.section_view.editing.as_mut().unwrap();
                let editor = ui.put(
                    egui::Rect::from_min_max(
                        rect.min + egui::vec2(18.0, 3.0),
                        egui::pos2(menu_rect.left() - 5.0, rect.bottom() - 3.0),
                    ),
                    egui::TextEdit::singleline(value)
                        .id(editor_id)
                        .char_limit(NAME_LIMIT)
                        .font(FontId::proportional(12.0)),
                );
                let value = value.clone();
                if std::mem::take(&mut self.section_view.focus) {
                    editor.request_focus();
                    editor.scroll_to_me(Some(Align::Center));
                } else if cancel {
                    self.cancel_section_rename(&id);
                    editor.surrender_focus();
                } else if save {
                    if self.section_name_valid(&id, &value) {
                        self.prefs.sidebar_view.sections[index].name = value.trim().into();
                        self.section_view.editing = None;
                        self.section_view.creating = None;
                        editor.surrender_focus();
                    } else {
                        editor.request_focus();
                        editor.on_hover_text(
                            "Use a unique, nonblank section name (up to 80 characters).",
                        );
                    }
                } else if editor.lost_focus() {
                    self.cancel_section_rename(&id);
                }
                if !self
                    .prefs
                    .sidebar_view
                    .sections
                    .iter()
                    .any(|section| section.id == id)
                {
                    continue;
                }
            } else {
                paint_text(
                    ui,
                    &section.name,
                    rect.left_center() + egui::vec2(18.0, 0.0),
                    rect.width() - 78.0,
                    FontId::proportional(12.0),
                    colors.text,
                );
                ui.painter().text(
                    menu_rect.left_center() - egui::vec2(6.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    members.len().to_string(),
                    FontId::proportional(10.0),
                    colors.dim,
                );
                let keyboard = heading.has_focus()
                    && ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Space)
                            || input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                            || input.consume_key(
                                egui::Modifiers::NONE,
                                if section.collapsed {
                                    egui::Key::ArrowRight
                                } else {
                                    egui::Key::ArrowLeft
                                },
                            )
                    });
                if heading.clicked() || keyboard {
                    self.prefs.sidebar_view.sections[index].collapsed = !section.collapsed;
                }
                heading.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::CollapsingHeader,
                        true,
                        !section.collapsed,
                        format!("{} · {} chats", section.name, members.len()),
                    )
                });
            }
            let menu = ui
                .interact(
                    menu_rect,
                    ui.make_persistent_id(("section-menu", &id)),
                    Sense::click(),
                )
                .on_hover_text("Section options")
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if menu.hovered() || menu.has_focus() {
                ui.painter().rect_filled(menu_rect, 5, colors.hover);
            }
            menu.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("{} section options", section.name),
                )
            });
            for dx in [-4.0, 0.0, 4.0] {
                ui.painter().circle_filled(
                    menu_rect.center() + egui::vec2(dx, 0.0),
                    1.0,
                    colors.muted,
                );
            }
            let mut header_menu = |ui: &mut egui::Ui| {
                theme::menu_controls(ui);
                if ui.button("Rename section").clicked() {
                    self.begin_section_rename(&id);
                    ui.close();
                }
                if ui
                    .add_enabled(index > 0, egui::Button::new("Move section up"))
                    .clicked()
                {
                    self.prefs.sidebar_view.sections.swap(index, index - 1);
                    ui.close();
                }
                if ui
                    .add_enabled(
                        index + 1 < self.prefs.sidebar_view.sections.len(),
                        egui::Button::new("Move section down"),
                    )
                    .clicked()
                {
                    self.prefs.sidebar_view.sections.swap(index, index + 1);
                    ui.close();
                }
                ui.separator();
                if ui.button("Remove section…").clicked() {
                    self.section_view.remove = Some(id.clone());
                    ui.close();
                }
            };
            egui::Popup::menu(&menu).show(&mut header_menu);
            if !self
                .prefs
                .sidebar_view
                .sections
                .iter()
                .find(|section| section.id == id)
                .is_some_and(|section| section.collapsed)
                || editing
            {
                if members.is_empty() {
                    ui.label(theme::small(
                        ui,
                        "No chats · use a chat’s Move to section menu",
                    ));
                }
                let expanded = self.prefs.sidebar_view.view_all || section.expanded;
                for member in members
                    .iter()
                    .take(if expanded { members.len() } else { 5 })
                {
                    if let Some(next) = self.branch_row(ui, &branches[*member], 0, query) {
                        action = Some(next);
                    }
                }
                if members.len() > 5
                    && ui
                        .add(
                            egui::Button::new(theme::small(
                                ui,
                                if expanded {
                                    "Show less".into()
                                } else {
                                    format!("Show {} more", members.len() - 5)
                                },
                            ))
                            .small()
                            .frame(false),
                        )
                        .clicked()
                {
                    self.prefs.sidebar_view.view_all = false;
                    if let Some(section) = self
                        .prefs
                        .sidebar_view
                        .sections
                        .iter_mut()
                        .find(|section| section.id == id)
                    {
                        section.expanded = !expanded;
                    }
                }
            }
        }
        action
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod unsaved_input_tests;
#[cfg(test)]
mod unsaved_layout_tests;
#[cfg(test)]
mod unsaved_lifecycle_tests;
#[cfg(test)]
mod unsaved_tests;
