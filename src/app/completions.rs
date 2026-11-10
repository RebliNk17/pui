//! Composer completion is cursor-aware, keyboard-first and filename-only.
use super::*;
use egui::{
    text::{CCursor, CCursorRange},
    widgets::text_edit::TextEditState,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const LOCAL: &[(&str, &str)] = &[
    ("new", "Start a new chat"),
    ("model", "Choose model and thinking effort"),
    ("settings", "Open Pui settings"),
    ("compact", "Compact context; optional instructions"),
    ("stop", "Stop the agent and clear waiting messages"),
    ("files", "Toggle workspace files"),
    ("terminal", "Toggle workspace terminal"),
    ("help", "Show composer help"),
];

#[derive(Clone, Debug, PartialEq)]
struct Token {
    start: usize,
    end: usize,
    query: String,
    file: bool,
}
fn token(text: &str, cursor: usize) -> Option<Token> {
    let cursor = text
        .char_indices()
        .nth(cursor)
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let before = &text[..cursor];
    let start = before
        .rfind(char::is_whitespace)
        .map(|i| i + before[i..].chars().next().unwrap().len_utf8())
        .unwrap_or(0);
    let word = &before[start..];
    let file = word.starts_with('@');
    if !(file || start == 0 && word.starts_with('/')) {
        return None;
    }
    let end = text[cursor..]
        .find(char::is_whitespace)
        .map(|n| cursor + n)
        .unwrap_or(text.len());
    Some(Token {
        start,
        end,
        query: word[1..].to_lowercase(),
        file,
    })
}
fn cursor(ctx: &egui::Context, text: &str) -> Option<usize> {
    let state = TextEditState::load(ctx, egui::Id::new("composer"))?;
    let range = state.cursor.char_range()?;
    range.single().map(|c| c.index.min(text.chars().count()))
}
#[derive(Clone)]
struct Choice {
    label: String,
    description: String,
    insert: String,
}
type SearchResult = Result<(Vec<PathBuf>, bool), String>;
#[derive(Default)]
pub(super) struct View {
    token: Option<Token>,
    root: PathBuf,
    choices: Vec<Choice>,
    selected: usize,
    scroll_selection: bool,
    dismissed: bool,
    rx: Option<Receiver<SearchResult>>,
    cancel: Arc<AtomicBool>,
    limited: bool,
    error: Option<String>,
    popup_rect: Option<egui::Rect>,
}
impl Drop for View {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl View {
    fn reset_search(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.rx = None;
        self.choices.clear();
        self.selected = 0;
        self.scroll_selection = true;
        self.limited = false;
        self.error = None;
        self.popup_rect = None;
        self.dismissed = false;
    }
}
impl Pui {
    pub(super) fn completion_active(&self, ctx: &egui::Context) -> bool {
        !self.completions.dismissed
            && ctx.memory(|m| m.has_focus(egui::Id::new("composer")))
            && cursor(ctx, &self.draft)
                .and_then(|c| token(&self.draft, c))
                .is_some()
    }
    /// Called before TextEdit and history recall consume Up/Down/Enter/Escape.
    pub(super) fn completion_keys(&mut self, ctx: &egui::Context) {
        self.refresh_completions(ctx);
        if !self.completion_active(ctx) {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.completions.dismissed = true;
            return;
        }
        let count = self.completions.choices.len();
        for (key, down) in [(egui::Key::ArrowDown, true), (egui::Key::ArrowUp, false)] {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) && count > 0 {
                self.completions.scroll_selection = true;
                self.completions.selected = if down {
                    (self.completions.selected + 1) % count
                } else {
                    (self.completions.selected + count - 1) % count
                };
            }
        }
        if count > 0
            && ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
            })
        {
            self.accept_completion(ctx, self.completions.selected);
        }
    }
    fn refresh_completions(&mut self, ctx: &egui::Context) {
        // Keep candidates alive between pointer-down and pointer-up: clicking a
        // popup row temporarily moves focus away from the TextEdit.
        let popup_pointer = self.completions.popup_rect.is_some_and(|rect| {
            ctx.input(|i| i.pointer.interact_pos().is_some_and(|p| rect.contains(p)))
        });
        if !ctx.memory(|m| m.has_focus(egui::Id::new("composer"))) && popup_pointer {
            return;
        }
        let current = if ctx.memory(|m| m.has_focus(egui::Id::new("composer"))) {
            cursor(ctx, &self.draft).and_then(|c| token(&self.draft, c))
        } else {
            None
        };
        if self.completions.token != current || self.completions.root != self.prefs.cwd {
            self.completions.reset_search();
            self.completions.token = current;
            self.completions.root = self.prefs.cwd.clone();
            if let Some(t) = &self.completions.token {
                if t.file {
                    let root = self.prefs.cwd.clone();
                    let query = t.query.clone();
                    let cancel = self.completions.cancel.clone();
                    let ctx = ctx.clone();
                    let (tx, rx) = mpsc::channel();
                    self.completions.rx = Some(rx);
                    std::thread::spawn(move || {
                        let result = files::mention_paths(&root, &query, &cancel);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    });
                }
            }
        }
        let Some(t) = self.completions.token.clone() else {
            return;
        };
        if !t.file {
            self.completions.choices = LOCAL
                .iter()
                .map(|(name, description)| Choice {
                    label: format!("/{name}"),
                    description: (*description).into(),
                    insert: format!("/{name} "),
                })
                .chain(
                    self.state
                        .commands
                        .iter()
                        .filter(|c| {
                            !c.name.starts_with("pui-")
                                && !LOCAL.iter().any(|(name, _)| *name == c.name)
                        })
                        .map(|c| Choice {
                            label: format!("/{}", c.name),
                            description: c.description.clone().unwrap_or_default(),
                            insert: format!("/{} ", c.name),
                        }),
                )
                .filter(|c| c.label[1..].to_lowercase().contains(&t.query))
                .collect();
        } else if let Some(result) = self.completions.rx.as_ref().and_then(|r| r.try_recv().ok()) {
            self.completions.rx = None;
            match result {
                Ok((paths, limited)) => {
                    self.completions.limited = limited;
                    self.completions.choices = paths
                        .into_iter()
                        .map(|path| {
                            let label = path
                                .strip_prefix(&self.prefs.cwd)
                                .unwrap_or(&path)
                                .display()
                                .to_string();
                            let absolute = path.to_string_lossy();
                            let reference = if absolute.chars().any(char::is_whitespace) {
                                serde_json::to_string(absolute.as_ref()).unwrap()
                            } else {
                                absolute.into_owned()
                            };
                            Choice {
                                label,
                                description: "Workspace file reference".into(),
                                insert: format!("@{reference} "),
                            }
                        })
                        .collect();
                }
                Err(error) => self.completions.error = Some(error),
            }
        }
        self.completions.selected = self
            .completions
            .selected
            .min(self.completions.choices.len().saturating_sub(1));
    }
    fn accept_completion(&mut self, ctx: &egui::Context, index: usize) {
        let Some(t) = self.completions.token.clone() else {
            return;
        };
        let Some(choice) = self.completions.choices.get(index) else {
            return;
        };
        let insert = choice.insert.clone();
        let end = self.draft[..t.start].chars().count() + insert.chars().count();
        self.draft.replace_range(t.start..t.end, &insert);
        let id = egui::Id::new("composer");
        let mut state = TextEditState::load(ctx, id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(CCursorRange::one(CCursor::new(end))));
        state.store(ctx, id);
        self.completions.dismissed = true;
        self.focus_composer = true;
    }
    pub(super) fn completion_popup(
        &mut self,
        ctx: &egui::Context,
        anchor: egui::Rect,
        layer: egui::LayerId,
    ) {
        self.refresh_completions(ctx);
        let pointer_in_popup = self.completions.popup_rect.is_some_and(|rect| {
            ctx.input(|i| i.pointer.interact_pos().is_some_and(|p| rect.contains(p)))
        });
        if self.completions.dismissed || (!self.completion_active(ctx) && !pointer_in_popup) {
            return;
        }
        ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                egui::Id::new("composer"),
                egui::EventFilter {
                    tab: true,
                    escape: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                },
            )
        });
        let width = anchor.width().clamp(180.0, 560.0);
        let mut chosen = None;
        let mut open = true;
        let popup = egui::Popup::new(
            egui::Id::new("composer-completions"),
            ctx.clone(),
            anchor,
            layer,
        )
        .open_bool(&mut open)
        .align(egui::emath::RectAlign::TOP_START)
        .align_alternatives(&[])
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(width)
        .gap(8.0)
        .show(|ui| {
            ui.set_width(width - 20.0);
            ui.label(theme::small(
                ui,
                "↑↓ select · Tab / Enter insert · Esc dismiss",
            ));
            if self.completions.rx.is_some() {
                ui.label(theme::small(ui, "Finding workspace files…"));
            }
            if let Some(error) = &self.completions.error {
                ui.label(theme::small(ui, error));
            }
            if self.completions.choices.is_empty() && self.completions.rx.is_none() {
                ui.label(theme::small(ui, "No matches"));
            }
            egui::ScrollArea::vertical()
                .id_salt("composer-completion-results")
                .max_height((anchor.top() - 70.0).clamp(50.0, 260.0))
                .show(ui, |ui| {
                    for (index, choice) in self.completions.choices.iter().enumerate() {
                        let selected = index == self.completions.selected;
                        let response = ui
                            .selectable_label(selected, RichText::new(&choice.label).size(12.0))
                            .on_hover_text(&choice.description);
                        if selected && self.completions.scroll_selection {
                            response.scroll_to_me(Some(Align::Center));
                        }
                        if response.clicked() {
                            chosen = Some(index);
                        }
                    }
                });
            if self.completions.limited {
                ui.label(theme::small(
                    ui,
                    "Search bounded; type a more specific path to narrow results.",
                ));
            }
        });
        self.completions.popup_rect = popup.map(|p| p.response.rect);
        self.completions.scroll_selection = false;
        if let Some(index) = chosen {
            self.accept_completion(ctx, index);
        }
        if !open {
            self.completions.dismissed = true;
        }
    }
    /// TUI built-ins do not execute over RPC. Handle supported equivalents here;
    /// leave extension commands, prompt templates and skills to Pi.
    pub(super) fn submit_command(&mut self) -> bool {
        let text = self.draft.trim();
        let Some(command) = text.strip_prefix('/') else {
            return false;
        };
        let (name, args) = command
            .split_once(char::is_whitespace)
            .unwrap_or((command, ""));
        if name.is_empty() || name.contains('/') {
            return false;
        }
        let args = args.trim().to_owned();
        if !LOCAL.iter().any(|(n, _)| *n == name) {
            if self.state.commands.iter().any(|c| c.name == name) || name.starts_with("pui-") {
                return false;
            }
            self.state.error = Some(format!(
                "Unknown or unavailable command /{name}. Type / to see supported commands; terminal-only Pi commands require the Pi CLI."
            ));
            return true;
        }
        if !self.draft_images.is_empty() {
            self.state.error = Some(
                "Remove attachments before running a Pui command. Your draft was kept.".into(),
            );
            return true;
        }
        if matches!(name, "new" | "compact")
            && (!self.state.ready()
                || self.loading_chat
                || self.history_pending.is_some()
                || self.restoring_code())
        {
            self.state.error = Some("Wait for the current operation to finish before running this command. Your draft was kept.".into());
            return true;
        }
        if name != "compact" && !args.is_empty() {
            self.state.error = Some(format!(
                "/{name} does not take arguments. Your draft was kept."
            ));
            return true;
        }
        let name = name.to_owned();
        if name == "compact" && (!self.connected || self.demo || !self.ultracode_ready) {
            self.state.error = Some("Connect to Pi before compacting. Your draft was kept.".into());
            return true;
        }
        self.draft.clear();
        self.recall = Default::default();
        self.completions = Default::default();
        match name.as_str() {
            "new" => self.new_session(),
            "model" => {
                self.model_picker_open = true;
                self.model_picker_focus = true;
                self.model_query.clear();
            }
            "settings" => self.settings_open = true,
            "compact" => self.send(json!({"type":"compact","customInstructions":args})),
            "stop" => self.stop(),
            "files" => self.files.toggle(&self.ctx, &self.prefs.cwd),
            "terminal" => self.terminals.toggle(&self.ctx, &self.prefs.cwd, self.demo),
            "help" => {
                self.diagnostics_open = true;
                self.state.notice = Some("Use / for Pui actions, Pi extension commands, prompts and skills. Use @ to reference workspace files. Attach any file with + or drag/drop; images can also be pasted. Enter/Tab inserts a completion; Enter again sends. Terminal-only commands such as /login require the Pi CLI.".into());
            }
            _ => unreachable!(),
        }
        true
    }
}

#[cfg(test)]
mod tests;
