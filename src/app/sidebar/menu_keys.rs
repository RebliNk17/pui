//! Single-letter accelerators belong to the open chat menu, never the composer.
use super::*;

pub(super) fn read(ui: &egui::Ui) -> Option<egui::Key> {
    if !ui.is_enabled() || !prompt_ui::keyboard_available(ui) {
        return None;
    }
    let deepest =
        egui::containers::menu::MenuState::from_ui(ui, |state, _| state.open_item.is_none());
    let mut shortcut = None;
    ui.input_mut(|input| {
        let cancelling = input.key_pressed(egui::Key::Escape);
        input.events.retain(|event| match event {
            // A key press also produces Text. Consume that even when closing the
            // menu so the accelerator cannot replace a focused draft or title.
            egui::Event::Text(_) | egui::Event::Paste(_) => false,
            egui::Event::Key {
                key,
                pressed,
                repeat,
                modifiers,
                ..
            } if matches!(
                key,
                egui::Key::R
                    | egui::Key::P
                    | egui::Key::M
                    | egui::Key::C
                    | egui::Key::E
                    | egui::Key::A
                    | egui::Key::H
                    | egui::Key::D
            ) && (*modifiers == egui::Modifiers::NONE
                || *modifiers == egui::Modifiers::SHIFT) =>
            {
                if deepest && !cancelling && *pressed && !repeat && shortcut.is_none() {
                    shortcut = Some(*key);
                }
                false
            }
            _ => true,
        });
    });
    shortcut
}

pub(super) fn button(
    ui: &mut egui::Ui,
    label: impl Into<egui::WidgetText>,
    key: egui::Key,
    shortcut: Option<egui::Key>,
) -> bool {
    ui.add(egui::Button::new(label.into()).shortcut_text(key.name()))
        .clicked()
        || shortcut == Some(key)
}

#[cfg(test)]
mod tests;
