//! In-chat find bar (`Cmd+F`), match navigation (`Enter`/`Shift+Enter`/`Cmd+G`), and highlight state.
use super::*;
use crate::markdown::find_case_insensitive_ranges;
use crate::protocol::{Block, Message};
use crate::transcript::{FindTarget, View};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Match {
    pub message_index: usize,
    pub occurrence_in_message: usize,
}

#[derive(Default)]
pub(super) struct ChatFind {
    pub open: bool,
    pub query: String,
    pub focus: bool,
    pub select_all: bool,
    pub active: usize,
    pub matches: Vec<Match>,
    pub scroll_to_active: bool,
    pub history: Vec<String>,
    history_index: Option<usize>,
    history_draft: String,
    show_suggestions: bool,
    last_revision: u64,
    last_query: String,
    last_view: View,
}

const MAX_FIND_HISTORY: usize = 20;

impl ChatFind {
    pub fn open(&mut self) {
        self.open = true;
        self.focus = true;
        self.select_all = !self.query.is_empty();
        self.history_index = None;
        self.show_suggestions = true;
        if !self.matches.is_empty() {
            self.scroll_to_active = true;
        }
    }

    pub fn close(&mut self) {
        self.push_history();
        self.open = false;
        self.focus = false;
        self.select_all = false;
        self.scroll_to_active = false;
        self.history_index = None;
        self.show_suggestions = false;
    }

    pub fn push_history(&mut self) {
        if self.query.is_empty() {
            return;
        }
        self.history.retain(|item| item != &self.query);
        self.history.insert(0, self.query.clone());
        self.history.truncate(MAX_FIND_HISTORY);
        self.history_index = None;
    }

    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next_idx = match self.history_index {
            None => {
                self.history_draft = self.query.clone();
                0
            }
            Some(idx) => (idx + 1).min(self.history.len() - 1),
        };
        self.history_index = Some(next_idx);
        self.query = self.history[next_idx].clone();
    }

    pub fn history_next(&mut self) {
        let Some(idx) = self.history_index else {
            return;
        };
        if idx == 0 {
            self.history_index = None;
            self.query = std::mem::take(&mut self.history_draft);
        } else {
            let next_idx = idx - 1;
            self.history_index = Some(next_idx);
            self.query = self.history[next_idx].clone();
        }
    }

    pub fn suggestions(&self) -> Vec<String> {
        if self.history.is_empty() {
            return Vec::new();
        }
        if self.query.is_empty() {
            return self.history.iter().take(6).cloned().collect();
        }
        let needle = self.query.to_lowercase();
        let mut out = Vec::new();
        for item in &self.history {
            if item != &self.query && item.to_lowercase().starts_with(&needle) {
                out.push(item.clone());
                if out.len() == 6 {
                    return out;
                }
            }
        }
        for item in &self.history {
            let lower = item.to_lowercase();
            if item != &self.query && !lower.starts_with(&needle) && lower.contains(&needle) {
                out.push(item.clone());
                if out.len() == 6 {
                    break;
                }
            }
        }
        out
    }

    pub fn clear_results(&mut self) {
        self.matches.clear();
        self.active = 0;
        self.last_revision = 0;
        self.last_query.clear();
        self.scroll_to_active = false;
    }

    pub fn sync(&mut self, messages: &[Message], view: View) -> Option<usize> {
        let rev = messages_revision(messages);
        if self.last_revision == rev && self.last_query == self.query && self.last_view == view {
            return None;
        }
        let query_changed = self.last_query != self.query;
        self.last_revision = rev;
        self.last_query = self.query.clone();
        self.last_view = view;
        self.matches.clear();
        if self.query.is_empty() {
            self.active = 0;
            self.scroll_to_active = false;
            return None;
        }
        for (message_index, msg) in messages.iter().enumerate() {
            let mut count = 0usize;
            if msg.role == "user" || msg.role == "custom" {
                count += find_case_insensitive_ranges(&msg.text, &self.query).len();
            } else if msg.role == "assistant" {
                if msg.blocks.is_empty() {
                    if view == View::Verbose && !msg.thinking.is_empty() {
                        count += find_case_insensitive_ranges(&msg.thinking, &self.query).len();
                    }
                    count += find_case_insensitive_ranges(&msg.text, &self.query).len();
                } else {
                    for block in &msg.blocks {
                        match block {
                            Block::Text(text) if !text.is_empty() => {
                                count += find_case_insensitive_ranges(text, &self.query).len();
                            }
                            Block::Thinking(text) if view == View::Verbose && !text.is_empty() => {
                                count += find_case_insensitive_ranges(text, &self.query).len();
                            }
                            _ => {}
                        }
                    }
                }
            }
            for occurrence_in_message in 0..count {
                self.matches.push(Match {
                    message_index,
                    occurrence_in_message,
                });
            }
        }
        if self.matches.is_empty() {
            self.active = 0;
            self.scroll_to_active = false;
            None
        } else {
            if query_changed || self.active >= self.matches.len() {
                self.active = self.matches.len() - 1;
            }
            if query_changed {
                self.scroll_to_active = true;
                Some(self.matches[self.active].message_index)
            } else {
                None
            }
        }
    }

    pub fn next(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        self.active = (self.active + 1) % self.matches.len();
        self.scroll_to_active = true;
        Some(self.matches[self.active].message_index)
    }

    pub fn prev(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        self.active = if self.active == 0 {
            self.matches.len() - 1
        } else {
            self.active - 1
        };
        self.scroll_to_active = true;
        Some(self.matches[self.active].message_index)
    }

    pub fn target(&self) -> Option<FindTarget> {
        if !self.open || self.query.is_empty() {
            return None;
        }
        let m = self.matches.get(self.active).copied();
        Some(FindTarget {
            query: self.query.clone(),
            message_index: m.map_or(usize::MAX, |m| m.message_index),
            occurrence_in_message: m.map_or(0, |m| m.occurrence_in_message),
            scroll_to_active: self.scroll_to_active && m.is_some(),
        })
    }
}

fn messages_revision(messages: &[Message]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    messages.len().hash(&mut h);
    if let Some(last) = messages.last() {
        last.revision.hash(&mut h);
        last.text.len().hash(&mut h);
    }
    h.finish()
}

impl Pui {
    pub(super) fn open_chat_find(&mut self) {
        self.chat_find.open();
        self.close_model_popovers();
    }

    pub(super) fn show_chat_find_bar(&mut self, ctx: &egui::Context, pane_rect: egui::Rect) {
        if !self.chat_find.open {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.chat_find.close();
            self.focus_composer = true;
            return;
        }
        let cmd_prev = ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::G,
            )
        });
        let cmd_next = if !cmd_prev {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::G))
        } else {
            false
        };
        if cmd_prev && let Some(idx) = self.chat_find.prev() {
            self.transcript.jump_to_message(idx);
        } else if cmd_next && let Some(idx) = self.chat_find.next() {
            self.transcript.jump_to_message(idx);
        }

        let colors = theme::palette(ctx);
        let width = 320.0_f32.min((pane_rect.width() - 24.0).max(200.0));
        let pos = egui::pos2(pane_rect.right() - width - 16.0, pane_rect.top() + 10.0);
        let input_id = egui::Id::new("chat-find-input");
        let input_focused = ctx.memory(|m| m.has_focus(input_id));
        let mut jump_to = None;
        let mut close = false;
        let suggestions = self.chat_find.suggestions();
        if input_focused {
            self.chat_find.show_suggestions = true;
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter)) {
                jump_to = self.chat_find.prev();
                self.chat_find.push_history();
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                jump_to = self.chat_find.next();
                self.chat_find.push_history();
            } else if !self.chat_find.query.is_empty()
                && !suggestions.is_empty()
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab))
            {
                self.chat_find.query = suggestions[0].clone();
                self.chat_find.history_index = None;
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
                self.chat_find.history_prev();
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown))
            {
                self.chat_find.history_next();
            }
        }
        let bar_resp = egui::Area::new(egui::Id::new("chat-find-bar"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(colors.surface)
                    .stroke(egui::Stroke::new(1.0_f32, colors.border))
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .shadow(egui::Shadow {
                        offset: [0, 4],
                        blur: 12,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(60),
                    })
                    .show(ui, |ui| {
                        ui.set_width(width - 20.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if ui
                                .add(
                                    egui::Button::new(egui::RichText::new("×").size(13.0))
                                        .frame(false),
                                )
                                .on_hover_text("Close (Esc)")
                                .clicked()
                            {
                                close = true;
                            }
                            let has = !self.chat_find.matches.is_empty();
                            if ui
                                .add_enabled(
                                    has,
                                    egui::Button::new(egui::RichText::new("↓").size(12.0))
                                        .frame(false),
                                )
                                .on_hover_text("Next match (Enter)")
                                .clicked()
                            {
                                jump_to = self.chat_find.next();
                                self.chat_find.push_history();
                            }
                            if ui
                                .add_enabled(
                                    has,
                                    egui::Button::new(egui::RichText::new("↑").size(12.0))
                                        .frame(false),
                                )
                                .on_hover_text("Previous match (Shift+Enter)")
                                .clicked()
                            {
                                jump_to = self.chat_find.prev();
                                self.chat_find.push_history();
                            }
                            let count_text = if self.chat_find.query.is_empty() {
                                String::new()
                            } else if self.chat_find.matches.is_empty() {
                                "0/0".to_owned()
                            } else {
                                format!(
                                    "{}/{}",
                                    self.chat_find.active + 1,
                                    self.chat_find.matches.len()
                                )
                            };
                            if !count_text.is_empty() {
                                ui.label(
                                    egui::RichText::new(count_text)
                                        .size(11.0)
                                        .color(colors.muted),
                                );
                            }
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width().max(100.0), 20.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    crate::icons::inline(
                                        ui,
                                        crate::icons::Icon::Search,
                                        colors.muted,
                                    );
                                    let edit_width = ui.available_width().max(80.0);
                                    let edit =
                                        egui::TextEdit::singleline(&mut self.chat_find.query)
                                            .id(input_id)
                                            .hint_text("Find in chat…")
                                            .desired_width(edit_width)
                                            .frame(false);
                                    let response = ui.add(edit);
                                    if self.chat_find.focus {
                                        response.request_focus();
                                        self.chat_find.focus = false;
                                    }
                                    if self.chat_find.select_all && response.has_focus() {
                                        if let Some(mut state) =
                                            egui::TextEdit::load_state(ctx, input_id)
                                        {
                                            state.cursor.set_char_range(Some(
                                                egui::text::CCursorRange::two(
                                                    egui::text::CCursor::new(0),
                                                    egui::text::CCursor::new(
                                                        self.chat_find.query.chars().count(),
                                                    ),
                                                ),
                                            ));
                                            state.store(ctx, input_id);
                                        }
                                        self.chat_find.select_all = false;
                                    }
                                },
                            );
                        });
                    });
            });
        if !close && self.chat_find.show_suggestions && !suggestions.is_empty() {
            let drop_pos = egui::pos2(pos.x, bar_resp.response.rect.bottom() + 4.0);
            let mut picked = None;
            let header = if self.chat_find.query.is_empty() {
                "Recent searches"
            } else {
                "Suggestions (Tab to complete)"
            };
            let drop_resp = egui::Area::new(egui::Id::new("chat-find-history"))
                .order(egui::Order::Foreground)
                .fixed_pos(drop_pos)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(colors.surface)
                        .stroke(egui::Stroke::new(1.0_f32, colors.border))
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .shadow(egui::Shadow {
                            offset: [0, 4],
                            blur: 12,
                            spread: 0,
                            color: egui::Color32::from_black_alpha(60),
                        })
                        .show(ui, |ui| {
                            ui.set_width(width - 16.0);
                            ui.label(egui::RichText::new(header).size(10.5).color(colors.muted));
                            ui.add_space(2.0);
                            for item in &suggestions {
                                let btn = egui::Button::new(
                                    egui::RichText::new(item).size(12.0).color(colors.text),
                                )
                                .frame(false)
                                .min_size(egui::vec2(ui.available_width(), 20.0));
                                if ui.add(btn).clicked() {
                                    picked = Some(item.clone());
                                }
                            }
                        });
                });
            if let Some(item) = picked {
                self.chat_find.query = item;
                self.chat_find.focus = true;
                self.chat_find.select_all = false;
            } else if !input_focused
                && ctx.input(|i| i.pointer.any_pressed())
                && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| {
                    !bar_resp.response.rect.contains(p) && !drop_resp.response.rect.contains(p)
                })
            {
                self.chat_find.show_suggestions = false;
            }
        }
        if close {
            self.chat_find.close();
            self.focus_composer = true;
        } else if let Some(idx) = jump_to {
            self.transcript.jump_to_message(idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_find_indexes_and_navigates_matches_across_messages() {
        let messages = vec![
            Message::from_json(&serde_json::json!({
                "role": "user",
                "content": [{"type": "text", "text": "Hello world, hello again"}]
            })),
            Message::from_json(&serde_json::json!({
                "role": "assistant",
                "content": [{"type": "text", "text": "HELLO back!"}]
            })),
        ];
        let mut find = ChatFind::default();
        find.open();
        find.query = "hello".into();
        assert_eq!(find.sync(&messages, View::Normal), Some(1));
        assert_eq!(
            find.matches,
            vec![
                Match {
                    message_index: 0,
                    occurrence_in_message: 0
                },
                Match {
                    message_index: 0,
                    occurrence_in_message: 1
                },
                Match {
                    message_index: 1,
                    occurrence_in_message: 0
                },
            ]
        );
        assert_eq!(find.active, 2);
        assert_eq!(find.next(), Some(0));
        assert_eq!(find.active, 0);
        assert_eq!(find.next(), Some(0));
        assert_eq!(find.active, 1);
        assert_eq!(find.next(), Some(1));
        assert_eq!(find.active, 2);
        assert_eq!(find.prev(), Some(0));
        assert_eq!(find.active, 1);
    }

    #[test]
    fn chat_find_records_dedups_and_walks_search_history() {
        let mut find = ChatFind::default();
        find.query = "alpha".into();
        find.push_history();
        find.query = "beta".into();
        find.push_history();
        find.query = "alpha".into();
        find.push_history();
        assert_eq!(find.history, vec!["alpha", "beta"]);

        find.query = "draft".into();
        find.history_prev();
        assert_eq!(find.query, "alpha");
        find.history_prev();
        assert_eq!(find.query, "beta");
        find.history_next();
        assert_eq!(find.query, "alpha");
        find.history_next();
        assert_eq!(find.query, "draft");

        find.query = "p".into();
        assert_eq!(find.suggestions(), vec!["alpha"]);
        find.query = "be".into();
        assert_eq!(find.suggestions(), vec!["beta"]);
        find.query = "beta".into();
        assert!(find.suggestions().is_empty());
    }
}
