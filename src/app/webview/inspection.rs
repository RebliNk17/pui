//! Explicit, ephemeral top-document inspection. Page text is data, never actions.
use super::*;
use serde::Deserialize;

mod capture;
mod scope;
mod selectors;
mod transport;
pub(super) use scope::{FrameChoice, Scope};
#[cfg(target_os = "macos")]
pub(super) use transport::{Command, command_scoped, event, inventory, pick_scoped, receive};
pub(super) use transport::{State, message_id};

const SELECTOR_LIMIT: usize = 1024;
const TEXT_LIMIT: usize = 512;
const TRANSPORT_LIMIT: usize = 16 * 1024;

/// Deliberately excludes attributes, pseudos, escapes, lists and universal queries.
/// This is a selector supplied by the user, not a promise of stable identity.
fn selector(value: &str) -> Result<&str, &'static str> {
    selectors::selector(value)
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
    pub(in crate::app::webview) text: String,
    text_note: String,
    rect: [f64; 4],
    display: String,
    visibility: String,
    position: String,
    color: String,
    font_size: String,
    generated_selector: Option<String>,
    selector_note: String,
}
impl Facts {
    #[cfg(debug_assertions)]
    pub(in crate::app::webview) fn smoke_selector(&self) -> Option<&str> {
        self.generated_selector.as_deref()
    }
    fn valid(&self) -> bool {
        !self.tag.is_empty()
            && self.tag.len() <= 64
            && self
                .tag
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            && self.text.chars().count() <= TEXT_LIMIT
            && self
                .generated_selector
                .as_deref()
                .is_none_or(selectors::generated_selector_valid)
            && self.selector_note.len() <= 160
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
    scope: Scope,
    capture: Option<capture::Review>,
    notice: Option<String>,
    pub(super) page: String,
    pub(super) page_title: String,
    pub(super) document: u64,
}
impl Editor {
    pub(super) fn new(key: &str, epoch: u64) -> Self {
        Self {
            key: key.into(),
            epoch,
            query: String::new(),
            focus: true,
            scope: Scope::default(),
            capture: None,
            notice: None,
            page: key.into(),
            page_title: String::new(),
            document: 0,
        }
    }
}

#[derive(Default)]
pub(super) struct Decision {
    pub(super) inspect: Option<String>,
    pub(super) close: bool,
    pub(super) pick: bool,
    pub(super) cancel_pick: bool,
    pub(super) inventory: bool,
    pub(super) scope: Scope,
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
            || ["Inspect", "Close", "Picker", "Review"].iter().any(|name| {
                ui.ctx()
                    .data(|d| d.get_temp::<egui::Id>(egui::Id::new(("inspection-action", *name))))
                    == Some(id)
            })
    });
    owns_keys |= editor.focus;
    let keyboard = prompt_ui::keyboard_available(ui);
    if keyboard {
        super::privacy::guard_keys(ui.ctx());
    }
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
        editor.capture = None;
        editor.scope = Scope::default();
        // Navigation never executes a new query. The next query needs a fresh click.
        enter = false;
    }
    prompt_ui::heading(
        ui,
        "Element inspection",
        "Read-only facts · explicit scope · no form values",
    );
    ui.add(egui::Label::new(theme::small(ui, "Choose an element by selector or pick on page. Frame inventory is read only on explicit refresh; unavailable/cross-origin scopes refuse.")).wrap());
    let old_scope = editor.scope.clone();
    egui::CollapsingHeader::new("Frame / shadow scope")
        .id_salt(("browser-scope", &editor.key))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                decision.inventory = prompt_ui::button(
                    ui,
                    "Refresh frames",
                    prompt_ui::Button::Secondary,
                    enabled && !state.pending(),
                )
                .clicked();
                egui::ComboBox::from_id_salt(("inspection-frame", &editor.key))
                    .selected_text(if editor.scope.frame_id.is_some() {
                        "Selected frame"
                    } else {
                        "Top document"
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut editor.scope.frame_id, None, "Top document");
                        for frame in &state.frames {
                            ui.selectable_value(
                                &mut editor.scope.frame_id,
                                Some(frame.id.clone()),
                                &frame.label,
                            );
                        }
                    });
            });
            ui.label(theme::small(
                ui,
                "Optional open-shadow host selector (in selected frame)",
            ));
            let mut host = editor.scope.shadow_host.clone().unwrap_or_default();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut host)
                        .id_salt(("shadow-host", &editor.key))
                        .char_limit(1024)
                        .desired_width(ui.available_width())
                        .hint_text("Leave empty for document root"),
                )
                .changed()
            {
                editor.scope.shadow_host =
                    (!host.trim().is_empty()).then(|| host.trim().to_owned());
                editor.capture = None;
            }
        });
    if editor.scope != old_scope {
        editor.capture = None;
    }
    decision.scope = editor.scope.clone();
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
    ui.horizontal_wrapped(|ui| {
        let label = if state.picking() {
            "Cancel picker"
        } else {
            "Pick on page…"
        };
        let r = prompt_ui::button(
            ui,
            label,
            prompt_ui::Button::Secondary,
            keyboard && ui.is_enabled() && (state.picking() || enabled && !state.pending()),
        );
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new(("inspection-action", "Picker")), r.id));
        if r.enabled() && (r.clicked() || (enter && r.has_focus())) {
            if state.picking() {
                decision.cancel_pick = true;
            } else if !epoch_changed {
                decision.pick = true;
            }
        }
        #[cfg(test)]
        ui.ctx().data_mut(|d| {
            d.insert_temp(egui::Id::new(("inspection-control", label)), (r.id, r.rect))
        });
    });
    if state.picking() {
        ui.add(egui::Label::new(theme::small(ui,"Chromium highlights the hovered element. Pick is one-shot; Cancel picker or Escape exits. The selection click is reserved by Chromium, not sent as a normal page action. Frames/shadow roots may refuse; nothing is captured to chat.")).wrap());
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
    ui.add(egui::Label::new(theme::small(ui, "Local, untrusted page data. Only an explicit reviewed Add copies it to the draft; nothing is sent. Selectors may stop matching after page changes.")).wrap());
    if let Some((query, facts)) = &state.facts
        && editor.scope == state.scope
    {
        let review = prompt_ui::button(
            ui,
            "Review for chat…",
            prompt_ui::Button::Secondary,
            enabled && !state.pending(),
        );
        crate::app::context_capture::browser::register(&review);
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new(("inspection-action", "Review")), review.id));
        if review.clicked() || (enter && review.has_focus() && review.enabled()) {
            match capture::Review::new(ui.ctx(), state.epoch, editor, query, facts) {
                Ok(value) => {
                    editor.capture = Some(value);
                    editor.notice = None;
                }
                Err(e) => editor.notice = Some(e.into()),
            }
        }
        if let Some(selector) = &facts.generated_selector {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    format!("Selector: {}", visible_data(selector)),
                ))
                .wrap(),
            );
        }
        ui.add(egui::Label::new(theme::small(ui, &facts.selector_note)).wrap());
        for (name, value) in [
            ("Target", query.clone()),
            ("Tag", facts.tag.clone()),
            ("Descendant text", facts.text.clone()),
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
    if editor.scope != state.scope
        || decision.close
        || decision.inspect.is_some()
        || decision.pick
        || decision.inventory
        || decision.cancel_pick
    {
        editor.capture = None;
    }
    capture::show(
        ui,
        &mut editor.capture,
        state,
        editor.document,
        enabled && editor.scope == state.scope && !state.pending(),
    );
    if let Some(message) = &editor.notice {
        ui.add(egui::Label::new(theme::small(ui, message)).wrap());
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
mod capture_tests;
#[cfg(test)]
mod tests;
