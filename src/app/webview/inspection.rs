//! Explicit, ephemeral top-document inspection. Page text is data, never actions.
use super::*;
use serde::Deserialize;

mod transport;
pub(super) use transport::State;
#[cfg(target_os = "macos")]
pub(super) use transport::{Command, command, message_id, receive};

const SELECTOR_LIMIT: usize = 256;
const TEXT_LIMIT: usize = 512;
const TRANSPORT_LIMIT: usize = 16 * 1024;

/// Deliberately excludes attributes, pseudos, escapes, lists and universal queries.
/// This is a selector supplied by the user, not a promise of stable identity.
fn selector(value: &str) -> Result<&str, &'static str> {
    let value = value.trim();
    if value.is_empty() || value.len() > SELECTOR_LIMIT {
        return Err("Enter a selector of 1–256 ASCII characters.");
    }
    let mut count = 0;
    let mut need_part = true;
    for part in value.split_ascii_whitespace() {
        if part == ">" {
            if need_part {
                return Err("Put spaces around a child combinator ( > ).");
            }
            need_part = true;
            continue;
        }
        count += 1;
        if count > 6 {
            return Err("Use at most six selector components.");
        }
        let mut start = true;
        for (index, c) in part.bytes().enumerate() {
            if c == b'.' || c == b'#' {
                if start && index > 0 {
                    return Err("An ID or class needs a name.");
                }
                start = true;
            } else if (start && (c.is_ascii_alphabetic() || c == b'_'))
                || (!start && (c.is_ascii_alphanumeric() || c == b'_' || c == b'-'))
            {
                start = false;
            } else {
                return Err(
                    "Use tags, #ids, .classes, spaces or spaced > only; no attributes, pseudos or scripts.",
                );
            }
        }
        if start {
            return Err("An ID or class needs a name.");
        }
        need_part = false;
    }
    if need_part {
        return Err("The selector must end with a tag, ID or class.");
    }
    Ok(value)
}

/// Keep directional/control characters inspectable instead of allowing page
/// text to visually spoof native field labels. Ordinary RTL text stays literal.
fn visible_data(value: &str) -> String {
    let mut result = String::new();
    for ch in value.chars() {
        if ch.is_control()
            || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            use std::fmt::Write;
            let _ = write!(result, "\\u{{{:04X}}}", ch as u32);
        } else {
            result.push(ch);
        }
    }
    result
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Facts {
    tag: String,
    text: String,
    text_note: String,
    rect: [f64; 4],
    display: String,
    visibility: String,
    position: String,
    color: String,
    font_size: String,
}
impl Facts {
    fn valid(&self) -> bool {
        !self.tag.is_empty()
            && self.tag.len() <= 64
            && self
                .tag
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            && self.text.chars().count() <= TEXT_LIMIT
            && [
                &self.text_note,
                &self.display,
                &self.visibility,
                &self.position,
                &self.color,
                &self.font_size,
            ]
            .iter()
            .all(|v| v.len() <= 160)
            && self.rect.iter().all(|n| n.is_finite() && n.abs() <= 1.0e9)
            && self.rect[2] >= 0.0
            && self.rect[3] >= 0.0
    }
}

pub(super) struct Editor {
    pub(super) key: String,
    epoch: u64,
    query: String,
    focus: bool,
}
impl Editor {
    pub(super) fn new(key: &str, epoch: u64) -> Self {
        Self {
            key: key.into(),
            epoch,
            query: String::new(),
            focus: true,
        }
    }
}

#[derive(Default)]
pub(super) struct Decision {
    pub(super) inspect: Option<String>,
    pub(super) close: bool,
}

pub(super) fn show(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    state: &State,
    available: bool,
) -> Decision {
    let mut decision = Decision::default();
    let field_id = egui::Id::new(("inspection-selector", &editor.key));
    let mut owns_keys = ui.memory(|m| m.focused()).is_none_or(|id| {
        id == field_id
            || ["Inspect", "Close"].iter().any(|name| {
                ui.ctx()
                    .data(|d| d.get_temp::<egui::Id>(egui::Id::new(("inspection-action", *name))))
                    == Some(id)
            })
    });
    owns_keys |= editor.focus;
    let keyboard = prompt_ui::keyboard_available(ui);
    let mut enter = false;
    let focused = ui.memory(|memory| memory.focused());
    if owns_keys && keyboard {
        ui.input_mut(|i| {
            i.events.retain(|event| {
                if let egui::Event::Key {
                    key,
                    pressed,
                    repeat,
                    modifiers,
                    ..
                } = event
                {
                    match key {
                        egui::Key::Escape => {
                            decision.close |= *pressed;
                            return false;
                        }
                        egui::Key::Enter => {
                            enter |= *pressed && !repeat && *modifiers == egui::Modifiers::NONE;
                            return false;
                        }
                        egui::Key::Backspace if modifiers.command => return false,
                        _ => {}
                    }
                }
                true
            })
        });
    }
    let enabled = available && keyboard && ui.is_enabled();
    let epoch_changed = editor.epoch != state.epoch;
    if epoch_changed {
        editor.epoch = state.epoch;
        // Navigation never executes a new query. The next query needs a fresh click.
        enter = false;
    }
    prompt_ui::heading(
        ui,
        "Element inspection",
        "Read-only · current top document · no page clicks or form values",
    );
    ui.add(egui::Label::new(theme::small(ui, "Choose one element by tag, #id, .class, descendant or spaced > selector. Frames, shadow roots, attributes and pseudos are unsupported.")).wrap());
    let field = ui.add(
        egui::TextEdit::singleline(&mut editor.query)
            .id(field_id)
            .char_limit(SELECTOR_LIMIT)
            .desired_width(ui.available_width())
            .hint_text("main > h1"),
    );
    if std::mem::take(&mut editor.focus) && keyboard && ui.is_enabled() {
        // Even an unavailable browser panel owns its selector, never the chat
        // composer that had focus before the inspector was explicitly opened.
        field.request_focus();
    }
    let validation = selector(&editor.query);
    if !enabled {
        ui.add(egui::Label::new(theme::small(ui, "Inspection unavailable while the page is loading, absent, or controls are disabled.")).wrap());
    } else if let Err(error) = validation {
        ui.add(egui::Label::new(theme::small(ui, error)).wrap());
    }
    // Inspect requires its explicit button. Enter in the selector never reads
    // the DOM or leaks through to a composer beneath this native panel.
    let mut inspect = false;
    prompt_ui::footer(ui, |ui| {
        let button = prompt_ui::button(
            ui,
            "Inspect",
            prompt_ui::Button::Primary,
            enabled && validation.is_ok() && !state.pending(),
        );
        inspect |= button.clicked() || (enter && button.has_focus());
        ui.ctx().data_mut(|d| {
            d.insert_temp(egui::Id::new(("inspection-action", "Inspect")), button.id)
        });
        #[cfg(test)]
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new(("inspection-control", "Inspect")),
                (button.id, button.rect),
            )
        });
        let close = prompt_ui::button(ui, "Close", prompt_ui::Button::Secondary, true);
        decision.close |= close.clicked() || (enter && close.has_focus());
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new(("inspection-action", "Close")), close.id));
        if (focused == Some(button.id) || focused == Some(close.id)) && keyboard {
            ui.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::Space,
                            ..
                        }
                    ) && !matches!(event, egui::Event::Text(value) if value == " ")
                })
            });
        }
        #[cfg(test)]
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new(("inspection-control", "Close")),
                (close.id, close.rect),
            )
        });
    });
    if !decision.close && inspect && enabled && !state.pending() && !epoch_changed {
        if let Ok(query) = validation {
            decision.inspect = Some(query.to_owned());
        }
    }
    ui.add(egui::Label::new(theme::small(ui, "Local, untrusted page data · never sent to a chat. Results are a snapshot; selectors may stop matching after page changes.")).wrap());
    if let Some((query, facts)) = &state.facts {
        for (name, value) in [
            ("Selector", query.clone()),
            ("Tag", facts.tag.clone()),
            ("Direct text", facts.text.clone()),
            ("Text limits", facts.text_note.clone()),
            (
                "Viewport CSS px",
                format!(
                    "x {:.1} · y {:.1} · w {:.1} · h {:.1}",
                    facts.rect[0], facts.rect[1], facts.rect[2], facts.rect[3]
                ),
            ),
            (
                "Style",
                format!(
                    "{} · {} · {} · {} · {}",
                    facts.display, facts.visibility, facts.position, facts.color, facts.font_size
                ),
            ),
        ] {
            ui.add(
                egui::Label::new(
                    RichText::new(format!("{name}: {}", visible_data(&value))).size(12.0),
                )
                .wrap()
                .selectable(true),
            );
        }
    }
    if let Some(message) = &state.message {
        ui.add(egui::Label::new(theme::small(ui, message)).wrap());
    }
    if state.pending() {
        ui.label(theme::small(
            ui,
            "Reading a bounded snapshot… Close cancels.",
        ));
    }
    decision
}

#[cfg(test)]
mod tests;
