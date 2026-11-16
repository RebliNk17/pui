//! Resize edges render after global shortcuts; reserve their input before globals.
#[cfg(test)]
mod tests;
#[cfg(test)]
mod vertical_tests;
use super::*;
const PANELS: &[&str] = &[
    "sessions-v4",
    "workspace-files-dock",
    "workspace-changes-dock",
    "files-tree",
    "project-terminal",
    "workspace-files-stack",
    "workspace-changes-stack",
];
impl Pui {
    pub(super) fn panel_resize_inputs(&mut self, ctx: &egui::Context) {
        if egui::Popup::is_any_open(ctx) || ctx.memory(|memory| memory.top_modal_layer().is_some())
        {
            return;
        }
        let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let focus = ctx.memory(|memory| memory.focused());
        let owner = PANELS
            .iter()
            .map(|name| egui::Id::new(name).with("__resize"))
            .find(|id| {
                (focus == Some(*id)
                    || focus.is_none()
                        && escape
                        && ctx.memory(|memory| memory.focus_lost_by_escape(*id)))
                    && ctx
                        .read_response(*id)
                        .is_some_and(|response| response.enabled())
            });
        if let Some(owner) = owner {
            let escape=ctx.input_mut(|input|{
    input.events.retain(|event|!matches!(event,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)&&!matches!(event,egui::Event::Text(_)|egui::Event::Paste(_)));
    input.consume_key(egui::Modifiers::NONE,egui::Key::Escape)
   });
            if escape {
                ctx.memory_mut(|memory| memory.surrender_focus(owner));
            }
        }
    }
}
