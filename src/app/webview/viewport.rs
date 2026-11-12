//! Transactional, bounded CSS viewport settings. Never navigates or calls CEF.
use super::*;

pub(super) const MIN_SIDE: u32 = 240;
pub(super) const MAX_SIDE: u32 = 3840;
pub(super) const MAX_AREA: u32 = 3840 * 2160;
const FIELD_LIMIT: usize = 10;

/// Defensive bounds for internal options as well as the explicitly validated form.
pub(super) fn bounded_dimensions(width: u32, height: u32) -> (u32, u32) {
    let width = width.clamp(MIN_SIDE, MAX_SIDE);
    let height = height.clamp(MIN_SIDE, MAX_SIDE).min(MAX_AREA / width);
    (width, height)
}

fn parse_side(value: &str, name: &str) -> Result<u32, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > FIELD_LIMIT || !value.bytes().all(|c| c.is_ascii_digit()) {
        return Err(format!("{name}: enter a whole number in CSS pixels."));
    }
    let value = value
        .parse::<u32>()
        .map_err(|_| format!("{name}: enter {MIN_SIDE}–{MAX_SIDE} CSS pixels."))?;
    if !(MIN_SIDE..=MAX_SIDE).contains(&value) {
        return Err(format!("{name}: enter {MIN_SIDE}–{MAX_SIDE} CSS pixels."));
    }
    Ok(value)
}

pub(super) fn dimensions(width: &str, height: &str) -> Result<(u32, u32), String> {
    let width = parse_side(width, "Width")?;
    let height = parse_side(height, "Height")?;
    if width.checked_mul(height).is_none_or(|area| area > MAX_AREA) {
        return Err("Viewport must be at most 8,294,400 CSS pixels (3840 × 2160).".into());
    }
    Ok((width, height))
}

pub(super) struct Editor {
    pub(super) key: String,
    width: String,
    height: String,
    focus: bool,
    attempted: bool,
}
impl Editor {
    fn new(key: &str, options: &Options) -> Self {
        // Displayed/oriented dimensions become the explicit custom dimensions.
        let (width, height) = options
            .dimensions()
            .unwrap_or_else(|| bounded_dimensions(options.width, options.height));
        Self {
            key: key.into(),
            width: width.to_string(),
            height: height.to_string(),
            focus: true,
            attempted: false,
        }
    }

    fn apply(&mut self, options: &mut Options, available: bool) -> bool {
        self.attempted = true;
        if !available {
            return false;
        }
        let Ok((width, height)) = dimensions(&self.width, &self.height) else {
            return false;
        };
        // One atomic presentation change, only after explicit Apply/plain Enter.
        options.width = width;
        options.height = height;
        options.device = Device::Custom;
        options.rotate = false;
        true
    }
}

#[derive(Default)]
struct Keys {
    apply: bool,
    cancel: bool,
}

fn keys(ui: &mut egui::Ui) -> Keys {
    let mut keys = Keys::default();
    if !prompt_ui::keyboard_available(ui) {
        return keys;
    }
    ui.input_mut(|input| {
        input.events.retain(|event| {
            let egui::Event::Key {
                key,
                pressed,
                repeat,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            match key {
                egui::Key::Enter => {
                    keys.apply |= *pressed && !repeat && *modifiers == egui::Modifiers::NONE;
                    false // Modified Enter must not approve/send anything beneath this popup.
                }
                egui::Key::Escape => {
                    keys.cancel |= *pressed;
                    false
                }
                egui::Key::Backspace if modifiers.command => false, // Not an agent denial.
                _ => true,
            }
        });
    });
    keys
}

fn field(ui: &mut egui::Ui, key: &str, label: &str, value: &mut String) -> egui::Response {
    ui.vertical(|ui| {
        ui.label(theme::small(ui, format!("{label} · CSS px")));
        let response = ui.add(
            egui::TextEdit::singleline(value)
                .id(egui::Id::new(("custom-viewport-field", key, label)))
                .font(FontId::proportional(12.0))
                .char_limit(FIELD_LIMIT)
                .desired_width(ui.available_width()),
        );
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new(("custom-viewport", label)),
                (response.id, response.rect),
            );
        });
        response
    })
    .inner
}

fn consume_action_space(ui: &mut egui::Ui, owned_focus: bool) {
    if owned_focus && ui.input(|input| input.key_pressed(egui::Key::Space)) {
        // egui synthesizes button clicks without consuming their keyboard
        // events. Do not deliver the same activation text to the composer.
        ui.input_mut(|input| {
            input.events.retain(|event| {
                !matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::Space,
                        ..
                    }
                ) && !matches!(event, egui::Event::Text(text) if text == " ")
            });
        });
    }
}

fn form(ui: &mut egui::Ui, editor: &mut Editor, options: &mut Options, available: bool) -> bool {
    let available = available && ui.is_enabled() && prompt_ui::keyboard_available(ui);
    let keys = keys(ui);
    let focused = ui.memory(|memory| memory.focused());
    ui.label(RichText::new("Custom viewport").size(12.0).strong());
    ui.add(egui::Label::new(theme::small(ui, "240–3840 per side · max 8.3M CSS pixels")).wrap());
    let mut width_response = None;
    if ui.available_width() >= 230.0 {
        ui.columns(2, |columns| {
            width_response = Some(field(
                &mut columns[0],
                &editor.key,
                "Width",
                &mut editor.width,
            ));
            field(&mut columns[1], &editor.key, "Height", &mut editor.height);
        });
    } else {
        width_response = Some(field(ui, &editor.key, "Width", &mut editor.width));
        field(ui, &editor.key, "Height", &mut editor.height);
    }
    if std::mem::take(&mut editor.focus) && available {
        if let Some(response) = width_response {
            response.request_focus();
        }
    }
    let validation = dimensions(&editor.width, &editor.height);
    if !available {
        ui.add(
            egui::Label::new(theme::small(
                ui,
                "Browser unavailable or controls disabled; cannot apply.",
            ))
            .wrap(),
        );
    } else if editor.attempted || validation.is_err() {
        if let Err(error) = &validation {
            ui.add(
                egui::Label::new(
                    RichText::new(error)
                        .size(11.0)
                        .color(theme::palette(ui.ctx()).red),
                )
                .wrap(),
            );
        }
    }
    let mut apply = keys.apply;
    let mut cancel = keys.cancel;
    prompt_ui::footer(ui, |ui| {
        let response = prompt_ui::button(
            ui,
            "Apply",
            prompt_ui::Button::Primary,
            available && validation.is_ok(),
        );
        apply |= response.clicked();
        consume_action_space(ui, focused == Some(response.id));
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new(("custom-viewport", "Apply")),
                (response.id, response.rect),
            );
        });
        let response = prompt_ui::button(ui, "Cancel", prompt_ui::Button::Secondary, true);
        cancel |= response.clicked() || (keys.apply && response.has_focus());
        consume_action_space(ui, focused == Some(response.id));
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new(("custom-viewport", "Cancel")),
                (response.id, response.rect),
            );
        });
    });
    ui.add(
        egui::Label::new(theme::small(
            ui,
            "Enter: apply · Esc / outside click: discard. No reload.",
        ))
        .wrap(),
    );
    // Cancellation wins over a simultaneous Enter or click.
    cancel || (apply && editor.apply(options, available))
}

fn popup_cancel(ui: &mut egui::Ui) -> bool {
    let mut cancel = false;
    if prompt_ui::keyboard_available(ui) {
        ui.input_mut(|input| {
            input.events.retain(|event| {
                let egui::Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } = event
                else {
                    return true;
                };
                match key {
                    egui::Key::Escape => {
                        cancel |= *pressed;
                        false
                    }
                    egui::Key::Enter if *modifiers != egui::Modifiers::NONE => false,
                    egui::Key::Backspace if modifiers.command => false,
                    _ => true,
                }
            });
        });
    }
    cancel
}

pub(super) fn settings(
    ui: &mut egui::Ui,
    key: &str,
    options: &mut Options,
    editor: &mut Option<Editor>,
    available: bool,
) {
    // Guard the entire popover before any preset/form widget can synthesize a
    // click from an approval shortcut. Plain Enter still activates presets.
    if popup_cancel(ui) {
        *editor = None;
        ui.close();
        return;
    }
    ui.set_width(ui.available_width());
    // A popup remembers its previous content height. Reset the available height
    // when swapping presets/form so reopened controls cannot be clipped away.
    let max_height = (ui.ctx().screen_rect().height() - 48.0).max(60.0);
    ui.set_max_height(max_height);
    egui::ScrollArea::vertical()
        .id_salt(("viewport-settings", key))
        .max_height(max_height)
        .show(ui, |ui| {
            if let Some(draft) = editor.as_mut() {
                if form(ui, draft, options, available) {
                    *editor = None;
                    ui.close();
                }
                return;
            }
            ui.label(RichText::new("Viewport").size(12.0).strong());
            ui.add_space(4.0);
            for (device, label) in [
                (Device::Responsive, "Responsive · fill pane"),
                (Device::Desktop, "Desktop · 1440 × 900"),
                (Device::Tablet, "Tablet · 768 × 1024"),
                (Device::Mobile, "Mobile · 390 × 844"),
            ] {
                let response = ui.selectable_value(&mut options.device, device, label);
                #[cfg(test)]
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new(("custom-viewport", label)), (response.id, response.rect));
                });
                consume_action_space(ui, response.has_focus());
                if response.clicked() {
                    ui.close();
                }
            }
            let custom = chrome::label_button(ui, "Custom dimensions…", options.device == Device::Custom);
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new(("custom-viewport", "Open")), (custom.id, custom.rect));
            });
            consume_action_space(ui, custom.has_focus());
            if custom.clicked() {
                *editor = Some(Editor::new(key, options));
                // Navigation renders before this toolbar next frame. Transfer
                // focus now so a fast Enter can never submit the address field.
                ui.memory_mut(|memory| {
                    memory.request_focus(egui::Id::new(("custom-viewport-field", key, "Width")));
                });
            }
            ui.separator();
            ui.add_enabled_ui(options.dimensions().is_some(), |ui| {
                prompt_ui::compact_toggle_row(ui, &mut options.fit, "Fit viewport to panel");
                prompt_ui::compact_toggle_row(ui, &mut options.rotate, "Landscape orientation");
            });
            if options.mobile() {
                ui.add(egui::Label::new(theme::small(ui, "Touch + Android user agent. Not a real device. Reload for server-side detection.")).wrap());
            }
        });
}

#[cfg(test)]
mod tests;
