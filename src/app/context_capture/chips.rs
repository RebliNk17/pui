//! Ephemeral associations, not a parser for user-authored [Context] blocks.
//! Bodies exist only in the draft. Any edit invalidates all associations; removal
//! also verifies the complete bounded draft before touching a byte.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    ops::Range,
    sync::atomic::{AtomicU64, Ordering},
};

pub(super) const MAX_CAPTURES: usize = 16;
pub(super) const MAX_DRAFT_BYTES: usize = 256 * 1024;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct Chip {
    id: u64,
    label: String,
    range: Range<usize>,
    bytes: usize,
}
impl Chip {
    fn widget_id(&self) -> egui::Id {
        egui::Id::new("captured-context-chip").with(self.id)
    }
}
#[derive(Default)]
pub(in crate::app) struct Draft {
    chips: Vec<Chip>,
    binding: Option<(usize, [u8; 32])>,
    // Native Shift+Tab resolves its target at the *next* begin_pass, where
    // Escape can clear focus before we render. Remember only that exact move.
    reverse_focus: Option<egui::Id>,
}
impl Draft {
    pub(in crate::app) fn clear(&mut self) {
        self.chips.clear();
        self.binding = None;
        self.reverse_focus = None;
    }
    // O(1) on settled frames. Same-length edits are explicitly invalidated by
    // the composer/lifecycle callers, then rechecked cryptographically on remove.
    fn check_length(&mut self, draft: &str) {
        if self
            .binding
            .as_ref()
            .is_some_and(|(len, _)| *len != draft.len())
        {
            self.clear();
        }
    }
    fn validate(&mut self, draft: &str) {
        if self.binding.as_ref().is_some_and(|(len, hash)| {
            *len != draft.len() || draft.len() > MAX_DRAFT_BYTES || hash != &fingerprint(draft)
        }) {
            self.clear();
        }
    }
    fn bind(&mut self, draft: &str) {
        self.binding = (!self.chips.is_empty()).then(|| (draft.len(), fingerprint(draft)));
    }
    pub(super) fn append(
        &mut self,
        draft: &mut String,
        snippet: Snippet,
    ) -> Result<(), &'static str> {
        self.validate(draft);
        if self.chips.len() >= MAX_CAPTURES {
            return Err(
                "Up to 16 captured context chips fit in one draft. Remove one or send when ready. Existing text was kept.",
            );
        }
        let separator = if draft.is_empty() { "" } else { "\n\n" };
        let addition = format!("[Context: {}]\n{}\n[/Context]", snippet.label, snippet.text);
        if draft
            .len()
            .saturating_add(separator.len())
            .saturating_add(addition.len())
            > MAX_DRAFT_BYTES
        {
            return Err("Draft is too large to add more context. Existing text was kept.");
        }
        let start = draft.len();
        draft.push_str(separator);
        draft.push_str(&addition);
        self.chips.push(Chip {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            label: snippet.label,
            range: start..draft.len(),
            bytes: snippet.text.len(),
        });
        self.bind(draft);
        Ok(())
    }
    fn remove(&mut self, draft: &mut String, id: u64) -> bool {
        self.validate(draft);
        let Some(index) = self.chips.iter().position(|chip| chip.id == id) else {
            return false;
        };
        let mut range = self.chips[index].range.clone();
        // A capture added to an empty draft owns no leading separator. If it
        // goes first, discard only the next capture's known generated separator.
        let trim_next = range.start == 0 && self.chips.get(index + 1).is_some();
        if trim_next {
            range.end += 2;
        }
        draft.replace_range(range.clone(), "");
        self.chips.remove(index);
        for (offset, chip) in self.chips[index..].iter_mut().enumerate() {
            if trim_next && offset == 0 {
                chip.range.start += 2;
            }
            chip.range.start -= range.len();
            chip.range.end -= range.len();
        }
        self.bind(draft);
        true
    }
}
fn fingerprint(draft: &str) -> [u8; 32] {
    Sha256::digest(draft.as_bytes()).into()
}

impl Pui {
    fn context_chips_available(&self, ctx: &egui::Context) -> bool {
        !self.settings_open
            && !self.search_open
            && !self.model_picker_open
            && !self.effort_picker_open
            && !self.kanban.open
            && !self.automation.open
            && (!self.tasks.open
                || self
                    .tasks
                    .current()
                    .is_some_and(|task| self.task_in_chat(task)))
            && ctx.read_response(egui::Id::new("composer")).is_some()
            && self.terminals.layout() != (true, true)
            && self.question_designs.is_none()
            && !self.transcript.gallery.is_open()
            && !self.draft_images.gallery.is_open()
            && !egui::Popup::is_any_open(ctx)
            && !self.completion_active(ctx)
            && ctx.memory(|m| m.top_modal_layer().is_none())
    }
    pub(in crate::app) fn context_chip_inputs(&mut self, ctx: &egui::Context) {
        self.context_chips.check_length(&self.draft);
        let reverse_focus = self.context_chips.reverse_focus.take();
        if self.context_chips.chips.is_empty() || !self.context_chips_available(ctx) {
            return;
        }
        let focused = ctx.memory(|m| m.focused());
        if focused == Some(egui::Id::new("composer"))
            && ctx.input(|i| {
                i.events.iter().any(|event| matches!(event,
                egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers, .. }
                    if modifiers.shift && !modifiers.ctrl && !modifiers.alt && !modifiers.command
            ))
            })
        {
            self.context_chips.reverse_focus = self.context_chips.chips.last().map(Chip::widget_id);
        }
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        // Approvals render before composer pills. A real pointer activation on
        // the previous live pill must own its chords before approval sees them.
        let pointer = ctx.input(|i| {
            (i.pointer.primary_pressed() || i.pointer.primary_released())
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        let pointer_chip = pointer.and_then(|pos| {
            self.context_chips
                .chips
                .iter()
                .find(|chip| {
                    ctx.read_response(chip.widget_id()).is_some_and(|response| {
                        response.enabled()
                            && response.sense.senses_click()
                            && response.interact_rect.contains(pos)
                    })
                })
                .map(Chip::widget_id)
        });
        let Some(chip) = self.context_chips.chips.iter().find(|c| {
            let id = c.widget_id();
            Some(id) == focused
                || Some(id) == pointer_chip
                || focused.is_none()
                    && escape
                    && (reverse_focus == Some(id) || ctx.memory(|m| m.had_focus_last_frame(id)))
        }) else {
            return;
        };
        let id = chip.id;
        let (remove, cancel) = ctx.input_mut(|i| {
            // A chip owns these keys, never a permission below it or Send/Stop.
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
            let space = i.key_pressed(egui::Key::Space);
            let remove = [
                egui::Key::Enter,
                egui::Key::Space,
                egui::Key::Delete,
                egui::Key::Backspace,
            ]
            .into_iter()
            .fold(false, |any, key| {
                i.consume_key(egui::Modifiers::NONE, key) || any
            });
            if remove && space {
                i.events
                    .retain(|event| !matches!(event, egui::Event::Text(text) if text == " "));
            }
            (
                remove,
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if remove {
            self.remove_context_chip(id);
        }
        if remove || cancel {
            self.focus_composer = true;
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("composer")));
        }
    }
    fn remove_context_chip(&mut self, id: u64) {
        if self.context_chips.remove(&mut self.draft, id) {
            self.recall = Default::default();
            self.focus_composer = true;
        }
    }
    pub(in crate::app) fn context_chips(&mut self, ui: &mut egui::Ui) {
        self.context_chips.check_length(&self.draft);
        if self.context_chips.chips.is_empty() {
            return;
        }
        let enabled = self.context_chips_available(ui.ctx());
        let colors = theme::palette(ui.ctx());
        ui.label(theme::small(
            ui,
            "Captured context · untrusted · draft only",
        ));
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(5.0, 4.0);
            let max_width = ui.available_width().clamp(48.0, 220.0);
            for chip in &self.context_chips.chips {
                let mut job = egui::text::LayoutJob::simple(
                    chip.label.clone(), FontId::proportional(11.0), colors.muted, max_width - 30.0,
                );
                job.wrap.max_rows = 1;
                job.wrap.break_anywhere = true;
                let galley = ui.fonts(|fonts| fonts.layout_job(job));
                let (rect, _) = ui.allocate_exact_size(egui::vec2((galley.size().x + 30.0).min(max_width), 24.0), Sense::hover());
                let response = ui.interact(rect, chip.widget_id(), if enabled { Sense::click() } else { Sense::hover() });
                if response.has_focus() {
                    ui.memory_mut(|m| m.request_focus_with_filter(response.id, egui::EventFilter { escape: true, ..Default::default() }));
                }
                ui.painter().rect_filled(rect, 7, if response.hovered() { colors.surface } else { colors.bg });
                ui.painter().rect_stroke(rect, 7, Stroke::new(1.0, if response.has_focus() { colors.accent } else { colors.border }), egui::StrokeKind::Inside);
                ui.painter().galley(rect.left_center() + egui::vec2(7.0, -galley.size().y / 2.0), galley, colors.muted);
                icons::draw(ui, egui::Rect::from_center_size(rect.right_center() - egui::vec2(11.0, 0.0), egui::vec2(12.0, 12.0)), Icon::Close, colors.muted);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, format!("Remove captured context: {}", chip.label)));
                if response.on_hover_text(format!("{}\nFrozen untrusted excerpt · {} bytes · never auto-sends\nRemove this capture: click, Enter or Delete. Editing the draft detaches all chips without deleting text.", chip.label, chip.bytes)).clicked() && enabled {
                    remove = Some(chip.id);
                }
                #[cfg(test)]
                ui.ctx().data_mut(|d| d.insert_temp(chip.widget_id().with("rect"), rect));
            }
        });
        if let Some(id) = remove {
            self.remove_context_chip(id);
        }
        ui.add_space(3.0);
    }
}

#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
