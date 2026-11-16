//! Saved root chat -> custom section. Presentation metadata only; no session navigation.
use super::*;
use std::path::Path;
fn bounded_path(path: &Path) -> PathBuf {
    let mut copy = path.to_path_buf();
    copy.shrink_to_fit();
    copy
}
fn bounded_text(text: &str) -> String {
    text.to_owned().into_boxed_str().into_string()
}
pub(super) struct Hit {
    widget: egui::Id,
    layer: egui::LayerId,
    rect: egui::Rect,
    pass: u64,
    root: PathBuf,
    owner: String,
}
enum Source {
    Saved(PathBuf),
    Runtime(String),
}
impl Source {
    fn membership(&self, app: &Pui) -> Option<String> {
        match self {
            Self::Saved(path) => app
                .prefs
                .chats
                .get(path)
                .and_then(|meta| meta.section.clone()),
            Self::Runtime(id) => app.section_view.unsaved.get(id).cloned(),
        }
    }
    fn eligible(&self, app: &Pui) -> bool {
        match self {
            Self::Saved(path) => {
                app.sessions.iter().any(|session| &session.path == path)
                    && !app
                        .agents
                        .nodes
                        .values()
                        .any(|node| node.path.as_ref() == Some(path) && node.parent.is_some())
                    && !app.deletion.deleting_chat(path)
                    && app
                        .prefs
                        .chats
                        .get(path)
                        .is_none_or(|meta| !meta.pinned && !meta.hidden && !meta.archived)
            }
            Self::Runtime(id) => {
                app.agents
                    .nodes
                    .get(id)
                    .is_some_and(|node| node.path.is_none() && node.parent.is_none())
                    && !app.deletion.deleting_agent(id)
                    && (app.section_view.unsaved.contains_key(id)
                        || app.section_view.unsaved.len() < 256)
            }
        }
    }
}
pub(super) struct Drag {
    source: Source,
    root: PathBuf,
    owner: String,
    widget: egui::Id,
    sections: Vec<String>,
    membership: Option<String>,
    active: bool,
}
impl Pui {
    fn chat_section_source_hit(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        menu: egui::Rect,
        disclosure: egui::Rect,
    ) {
        let rect = response.interact_rect.intersect(ui.clip_rect());
        if response.enabled()
            && self.prefs.cwd.as_os_str().len() <= 4096
            && self.agents.active.len() <= 4096
            && ui
                .input(|input| input.pointer.hover_pos())
                .is_some_and(|pos| {
                    rect.contains(pos) && !menu.contains(pos) && !disclosure.contains(pos)
                })
        {
            self.section_view.chat_drag_hit = Some(Hit {
                widget: response.id,
                layer: response.layer_id,
                rect,
                pass: ui.ctx().cumulative_pass_nr(),
                root: bounded_path(&self.prefs.cwd),
                owner: bounded_text(&self.agents.active),
            });
        }
    }
    fn chat_section_needs_source(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        menu: egui::Rect,
        disclosure: egui::Rect,
    ) -> bool {
        self.chat_section_source_hit(ui, response, menu, disclosure);
        if let Some(drag) = self.section_view.chat_drag.as_ref() {
            if drag.widget == response.id && ui.is_rect_visible(response.interact_rect) {
                self.section_view.chat_drag_seen =
                    Some((response.id, ui.ctx().cumulative_pass_nr()));
            }
            return drag.widget == response.id;
        }
        ui.input(|input| input.pointer.primary_pressed()) && response.is_pointer_button_down_on()
    }
    fn chat_move_available(&self) -> bool {
        !self.prefs.hide_sidebar
            && !self.settings_open
            && !self.search_open
            && !self.search_blocked()
            && !self.section_view.editing()
            && self.section_view.drag.is_none()
    }
    fn chat_move_current(&self, drag: &Drag) -> bool {
        self.chat_move_available()
            && drag.root == self.prefs.cwd
            && drag.owner == self.agents.active
            && drag.sections.iter().map(String::as_str).eq(self
                .prefs
                .sidebar_view
                .sections
                .iter()
                .map(|section| section.id.as_str()))
            && drag.membership == drag.source.membership(self)
            && drag.source.eligible(self)
    }
    pub(in crate::app) fn chat_section_drag_inputs(&mut self, ctx: &egui::Context) {
        self.section_view.chat_drop = None;
        self.section_view.chat_drag_press = None;
        if !self.chat_move_available() {
            self.section_view.chat_drag_hit = None;
        }
        let press = ctx.input(|input| {
            input
                .pointer
                .primary_pressed()
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        if self.chat_move_available()
            && !egui::Popup::is_any_open(ctx)
            && ctx.memory(|memory| memory.top_modal_layer().is_none())
            && let (Some(pos), Some(hit)) = (press, self.section_view.chat_drag_hit.as_ref())
            && hit.root == self.prefs.cwd
            && hit.owner == self.agents.active
            && hit.pass.checked_add(1) == Some(ctx.cumulative_pass_nr())
            && hit.rect.contains(pos)
            && ctx.layer_id_at(pos) == Some(hit.layer)
            && ctx
                .read_response(hit.widget)
                .is_some_and(|response| response.enabled())
        {
            self.section_view.chat_drag_press = Some((hit.widget, ctx.cumulative_pass_nr()));
            ctx.input_mut(|input|input.events.retain(|event|!matches!(event,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)&&!matches!(event,egui::Event::Text(_)|egui::Event::Paste(_))));
        }
        let Some(drag) = self.section_view.chat_drag.as_ref() else {
            return;
        };
        let foreign = egui::Popup::is_any_open(ctx)
            || ctx.memory(|memory| memory.top_modal_layer().is_some())
            || egui::DragAndDrop::has_any_payload(ctx);
        let source_seen = self
            .section_view
            .chat_drag_seen
            .is_some_and(|(widget, pass)| {
                widget == drag.widget && pass.checked_add(1) == Some(ctx.cumulative_pass_nr())
            });
        let source_visible = source_seen
            && ctx.read_response(drag.widget).is_some_and(|response| {
                response.enabled() && response.interact_rect.intersects(ctx.screen_rect())
            });
        let lost_drag = drag.active
            && !ctx.is_being_dragged(drag.widget)
            && ctx.drag_stopped_id() != Some(drag.widget);
        let cancel = foreign
            || !source_visible
            || lost_drag
            || !self.chat_move_current(drag)
            || ctx.input(|input| {
                !input.focused
                    || input.key_pressed(egui::Key::Escape)
                    || (!input.pointer.primary_down() && !input.pointer.primary_released())
            });
        let widget = drag.widget;
        if !foreign {
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Key { .. }
                            | egui::Event::Text(_)
                            | egui::Event::Paste(_)
                            | egui::Event::Copy
                            | egui::Event::Cut
                            | egui::Event::Ime(_)
                    )
                })
            });
        }
        if cancel {
            self.section_view.chat_drag_seen = None;
            self.section_view.chat_drag = None;
            if ctx.is_being_dragged(widget) {
                ctx.stop_dragging();
            }
        }
    }
    pub(in crate::app) fn chat_section_source(
        &mut self,
        ui: &egui::Ui,
        path: &Path,
        response: &egui::Response,
        menu: egui::Rect,
        disclosure: egui::Rect,
    ) {
        if !self.chat_section_needs_source(ui, response, menu, disclosure) {
            return;
        }
        if path.as_os_str().len() > 4096 {
            return;
        }
        self.chat_section_source_inner(
            ui,
            Source::Saved(bounded_path(path)),
            response,
            menu,
            disclosure,
        );
    }
    pub(in crate::app) fn runtime_chat_section_source(
        &mut self,
        ui: &egui::Ui,
        id: &str,
        response: &egui::Response,
        menu: egui::Rect,
        disclosure: egui::Rect,
    ) {
        if !self.chat_section_needs_source(ui, response, menu, disclosure) {
            return;
        }
        if id.len() > 4096 {
            return;
        }
        self.chat_section_source_inner(
            ui,
            Source::Runtime(bounded_text(id)),
            response,
            menu,
            disclosure,
        );
    }
    fn chat_section_source_inner(
        &mut self,
        ui: &egui::Ui,
        source: Source,
        response: &egui::Response,
        menu: egui::Rect,
        disclosure: egui::Rect,
    ) {
        if !self.chat_move_available()
            || !response.enabled()
            || !source.eligible(self)
            || self.prefs.cwd.as_os_str().len() > 4096
            || self.agents.active.len() > 4096
            || source
                .membership(self)
                .is_some_and(|section| section.len() > 80)
            || self.prefs.sidebar_view.sections.len() > LIMIT
            || self
                .prefs
                .sidebar_view
                .sections
                .iter()
                .any(|section| section.id.len() > 80)
        {
            return;
        }
        if ui.is_rect_visible(response.interact_rect)
            && self
                .section_view
                .chat_drag
                .as_ref()
                .is_some_and(|drag| drag.widget == response.id)
        {
            self.section_view.chat_drag_seen = Some((response.id, ui.ctx().cumulative_pass_nr()));
        }
        let pos = ui.input(|input| input.pointer.interact_pos());
        if self.section_view.chat_drag_press == Some((response.id, ui.ctx().cumulative_pass_nr()))
            && response.is_pointer_button_down_on()
            && ui.input(|input| input.pointer.primary_pressed())
            && pos.is_some_and(|pos| {
                response
                    .interact_rect
                    .intersect(ui.clip_rect())
                    .contains(pos)
                    && ui.ctx().layer_id_at(pos) == Some(response.layer_id)
                    && !menu.contains(pos)
                    && !disclosure.contains(pos)
            })
            && !egui::Popup::is_any_open(ui.ctx())
            && ui.memory(|memory| memory.top_modal_layer().is_none())
        {
            self.section_view.chat_drag_seen = Some((response.id, ui.ctx().cumulative_pass_nr()));
            self.section_view.chat_drag = Some(Drag {
                membership: source.membership(self).as_deref().map(bounded_text),
                source,
                root: bounded_path(&self.prefs.cwd),
                owner: bounded_text(&self.agents.active),
                widget: response.id,
                sections: self
                    .prefs
                    .sidebar_view
                    .sections
                    .iter()
                    .map(|section| bounded_text(&section.id))
                    .collect(),
                active: false,
            });
        }
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(drag) = self
                .section_view
                .chat_drag
                .as_mut()
                .filter(|drag| drag.widget == response.id)
        {
            drag.active = true;
        }
    }
    pub(super) fn chat_section_target(
        &mut self,
        ui: &egui::Ui,
        section: &str,
        response: &egui::Response,
    ) {
        let Some(drag) = self
            .section_view
            .chat_drag
            .as_ref()
            .filter(|drag| drag.active)
        else {
            return;
        };
        if !self.chat_move_current(drag)
            || !response.enabled()
            || egui::Popup::is_any_open(ui.ctx())
            || ui.memory(|memory| memory.top_modal_layer().is_some())
        {
            return;
        }
        let rect = response.interact_rect.intersect(ui.clip_rect());
        let pos = ui.input(|input| input.pointer.interact_pos());
        if pos.is_some_and(|pos| {
            rect.contains(pos) && ui.ctx().layer_id_at(pos) == Some(response.layer_id)
        }) {
            ui.painter().rect_stroke(
                rect,
                5,
                Stroke::new(2.0, theme::palette(ui.ctx()).accent),
                egui::StrokeKind::Inside,
            );
            if ui.input(|input| input.pointer.primary_released()) {
                self.section_view.chat_drop = Some(section.into());
            }
        }
    }
    pub(in crate::app) fn finish_chat_section_drag(&mut self, ctx: &egui::Context) {
        let owns = self.section_view.chat_drag.is_some();
        if ctx.input(|input| input.pointer.primary_released())
            && let Some(drag) = self.section_view.chat_drag.take()
            && drag.active
            && self.section_view.chat_drag_seen == Some((drag.widget, ctx.cumulative_pass_nr()))
            && !egui::Popup::is_any_open(ctx)
            && ctx.memory(|memory| memory.top_modal_layer().is_none())
            && self.chat_move_current(&drag)
            && let Some(section) = self.section_view.chat_drop.take()
            && self
                .prefs
                .sidebar_view
                .sections
                .iter()
                .any(|item| item.id == section)
        {
            match drag.source {
                Source::Saved(path) => {
                    self.prefs.chats.entry(path).or_default().section = Some(section)
                }
                Source::Runtime(id) => self.set_unsaved_section(&id, Some(section)),
            }
        }
        if ctx.input(|input| input.pointer.primary_released()) {
            self.section_view.chat_drag_seen = None;
            self.section_view.chat_drag_press = None;
            self.section_view.chat_drop = None;
        }
        if owns
            && !egui::Popup::is_any_open(ctx)
            && ctx.memory(|memory| memory.top_modal_layer().is_none())
        {
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Key { .. }
                            | egui::Event::Text(_)
                            | egui::Event::Paste(_)
                            | egui::Event::Copy
                            | egui::Event::Cut
                            | egui::Event::Ime(_)
                    )
                })
            });
        }
    }
}

#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod tests;
