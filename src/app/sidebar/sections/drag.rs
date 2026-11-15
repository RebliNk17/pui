//! Header-only, transactional section ordering. No chat moves or execution state.
use super::*;

pub(super) struct Drag {
    source: String,
    widget: egui::Id,
    owner: String,
    root: PathBuf,
    order: Vec<String>,
    active: bool,
}

fn reorder(sections: &mut Vec<Section>, source: &str, target: &str, after: bool) -> bool {
    let Some(from) = sections.iter().position(|section| section.id == source) else {
        return false;
    };
    let Some(to) = sections.iter().position(|section| section.id == target) else {
        return false;
    };
    if from == to {
        return false;
    }
    let insertion = to + usize::from(after) - usize::from(from < to);
    if from == insertion {
        return false;
    }
    let section = sections.remove(from);
    sections.insert(insertion, section);
    true
}

impl Pui {
    fn section_drag_available(&self) -> bool {
        !self.prefs.hide_sidebar
            && !self.settings_open
            && self.question_designs.is_none()
            && !self.search_open
            && !self.search_blocked()
            && !self.section_view.editing()
    }
    fn section_drag_current(&self, drag: &Drag) -> bool {
        self.section_drag_available()
            && drag.owner == self.agents.active
            && drag.root == self.prefs.cwd
            && drag.order.len() == self.prefs.sidebar_view.sections.len()
            && drag
                .order
                .iter()
                .zip(&self.prefs.sidebar_view.sections)
                .all(|(id, section)| id == &section.id)
    }
    fn cancel_section_drag(&mut self, ctx: &egui::Context) {
        self.section_view.drag_cancel_pass = Some(ctx.cumulative_pass_nr());
        if let Some(drag) = self.section_view.drag.take()
            && ctx.is_being_dragged(drag.widget)
        {
            ctx.stop_dragging();
        }
    }
    /// Exact previous-pass header ownership, before global shortcuts/composer.
    pub(in crate::app) fn section_drag_inputs(&mut self, ctx: &egui::Context) {
        self.section_view.drag_keyboard_pass = None;
        // Foreign surfaces have first refusal. Never clear their drag payload.
        if ctx.memory(|memory| memory.top_modal_layer().is_some())
            || egui::Popup::is_any_open(ctx)
            || egui::DragAndDrop::has_any_payload(ctx)
        {
            self.cancel_section_drag(ctx);
            return;
        }
        let visible = !self.prefs.hide_sidebar
            && !self.settings_open
            && self.question_designs.is_none()
            && self
                .section_view
                .drag_controls_pass
                .is_some_and(|pass| ctx.cumulative_pass_nr().saturating_sub(pass) <= 1);
        let (pointer, focused, escape) = (
            ctx.input(|input| input.pointer.interact_pos()),
            ctx.memory(|memory| memory.focused()),
            ctx.input(|input| input.key_pressed(egui::Key::Escape)),
        );
        let layer = pointer.and_then(|pos| ctx.layer_id_at(pos));
        let pointer_phase =
            ctx.input(|input| input.pointer.primary_pressed() || input.pointer.primary_released());
        // Idle headers do not need response/layer scans. Keep only the existing
        // bounded tuples; active gestures still validate owner/lifecycle every pass.
        if self.section_view.drag.is_none()
            && !pointer_phase
            && !ctx.input(|input| input.events.iter().any(keyboard_event))
        {
            if !visible {
                self.section_view.drag_controls.clear();
            }
            return;
        }
        let header_owns = visible
            && self.section_view.drag_controls.iter().any(|(section, id)| {
                self.prefs
                    .sidebar_view
                    .sections
                    .iter()
                    .any(|item| &item.id == section)
                    && ctx.read_response(*id).is_some_and(|response| {
                        response.enabled()
                            && (focused == Some(*id)
                                || focused.is_none()
                                    && escape
                                    && ctx.memory(|memory| memory.focus_lost_by_escape(*id))
                                || pointer_phase
                                    && pointer.is_some_and(|pos| {
                                        response.interact_rect.contains(pos)
                                            && layer == Some(response.layer_id)
                                    }))
                    })
            });
        let dragging = self.section_view.drag.is_some();
        if dragging || header_owns {
            self.section_view.drag_keyboard_pass = Some(ctx.cumulative_pass_nr());
            ctx.input_mut(|input| {
                input.events.retain(|event| match event {
                    egui::Event::Key {
                        key: egui::Key::Escape,
                        ..
                    } => false,
                    _ if dragging => !keyboard_event(event),
                    egui::Event::Key {
                        key: egui::Key::Enter | egui::Key::Backspace,
                        modifiers,
                        ..
                    } if modifiers.command || modifiers.ctrl => false,
                    egui::Event::Key {
                        key: egui::Key::Enter | egui::Key::Space,
                        ..
                    }
                    | egui::Event::Text(_)
                    | egui::Event::Paste(_)
                        if pointer_phase =>
                    {
                        false
                    }
                    _ => true,
                })
            });
        }
        let pointer_present = ctx.input(|input| {
            input.pointer.has_pointer()
                && input.focused
                && (input.pointer.primary_down() || input.pointer.primary_released())
        });
        if self.section_view.drag.as_ref().is_some_and(|drag| {
            !visible
                || !pointer_present
                || !self.section_drag_current(drag)
                || escape
                || drag.active
                    && !ctx.is_being_dragged(drag.widget)
                    && ctx.drag_stopped_id() != Some(drag.widget)
        }) {
            self.cancel_section_drag(ctx);
        }
        if !visible {
            self.section_view.drag_controls.clear();
        }
    }
    /// Called only for an enabled, visible header; menu/body never acts as a target.
    pub(super) fn section_drag_header(
        &mut self,
        ui: &egui::Ui,
        section: &str,
        heading: &egui::Response,
    ) -> Option<(String, bool)> {
        let rect = heading.interact_rect.intersect(ui.clip_rect());
        if !heading.enabled() || !rect.is_positive() {
            return None;
        }
        if self.section_view.drag_controls.len() < LIMIT * 2 {
            self.section_view
                .drag_controls
                .push((section.into(), heading.id));
        }
        let foreign = egui::Popup::is_any_open(ui.ctx())
            || ui.memory(|memory| memory.top_modal_layer().is_some())
            || egui::DragAndDrop::has_any_payload(ui.ctx());
        if foreign || !self.section_drag_available() {
            return None;
        }
        let pointer = ui.input(|input| input.pointer.interact_pos());
        let layer = pointer.and_then(|pos| ui.ctx().layer_id_at(pos));
        // Bind the press, not a later drag threshold after a session/root change.
        if heading.is_pointer_button_down_on()
            && ui.input(|input| input.pointer.primary_pressed())
            && self.section_view.drag_cancel_pass != Some(ui.ctx().cumulative_pass_nr())
            && self.prefs.sidebar_view.sections.len() <= LIMIT
            && self.prefs.cwd.as_os_str().len() <= 4096
            && self.agents.active.len() <= 4096
            && self
                .prefs
                .sidebar_view
                .sections
                .iter()
                .all(|item| item.id.len() <= 80)
        {
            self.section_view.drag = Some(Drag {
                source: section.into(),
                widget: heading.id,
                owner: self.agents.active.clone(),
                root: self.prefs.cwd.clone(),
                order: self
                    .prefs
                    .sidebar_view
                    .sections
                    .iter()
                    .map(|item| item.id.clone())
                    .collect(),
                active: false,
            });
        }
        if heading.drag_started_by(egui::PointerButton::Primary)
            && let Some(drag) = self
                .section_view
                .drag
                .as_mut()
                .filter(|drag| drag.widget == heading.id)
        {
            drag.active = true;
        }
        let drag = self.section_view.drag.as_ref().filter(|drag| drag.active)?;
        if !self.section_drag_current(drag) {
            return None;
        }
        self.section_view.drag_keyboard_pass = Some(ui.ctx().cumulative_pass_nr());
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let pos = pointer.filter(|pos| rect.contains(*pos) && layer == Some(heading.layer_id))?;
        if section == drag.source {
            return None;
        }
        let after = pos.y >= rect.center().y;
        let y = if after { rect.bottom() } else { rect.top() };
        ui.painter().line_segment(
            [
                egui::pos2(rect.left() + 4.0, y),
                egui::pos2(rect.right() - 4.0, y),
            ],
            Stroke::new(2.0, theme::palette(ui.ctx()).accent),
        );
        ui.input(|input| input.pointer.primary_released())
            .then(|| (section.into(), after))
    }
    pub(super) fn finish_section_drag(
        &mut self,
        ctx: &egui::Context,
        target: Option<(String, bool)>,
    ) {
        let released = ctx.input(|input| input.pointer.primary_released());
        let source_visible = self.section_view.drag.as_ref().is_none_or(|drag| {
            self.section_view
                .drag_controls
                .iter()
                .any(|(id, widget)| id == &drag.source && widget == &drag.widget)
        });
        if !source_visible {
            self.cancel_section_drag(ctx);
        }
        if released
            && let Some(drag) = self.section_view.drag.take()
            && self.section_drag_current(&drag)
            && let Some((target, after)) = target
        {
            reorder(
                &mut self.prefs.sidebar_view.sections,
                &drag.source,
                &target,
                after,
            );
        }
        // The sidebar renders before questions. Quarantine paired Space/Text/Paste
        // and release-frame chords even when a question arrives during this pass.
        if self.section_view.drag_keyboard_pass == Some(ctx.cumulative_pass_nr()) {
            ctx.input_mut(|input| input.events.retain(|event| !keyboard_event(event)));
        }
    }
}
fn keyboard_event(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key { .. }
            | egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Copy
            | egui::Event::Cut
            | egui::Event::Ime(_)
    )
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
