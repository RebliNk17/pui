//! Native, multi-step question cards carried over the supported Pi input RPC.
use super::*;

const TITLE: &str = "Pui questions";

mod components;
pub(super) mod designs;

#[derive(Clone, Deserialize)]
pub(super) struct Choice {
    label: String,
    #[serde(default)]
    description: String,
}
#[derive(Clone, Deserialize)]
pub(super) struct Question {
    id: String,
    question: String,
    options: Vec<Choice>,
    #[serde(default, rename = "multiSelect")]
    multi_select: bool,
}
#[derive(Deserialize)]
pub(super) struct Request {
    version: u32,
    request_id: String,
    questions: Vec<Question>,
}
impl Request {
    pub(super) fn summary(&self) -> &str {
        self.questions
            .first()
            .map(|q| q.question.as_str())
            .unwrap_or(TITLE)
    }
    pub(super) fn parse(dialog: &Value) -> Option<Self> {
        if dialog["method"] != "input" || dialog["title"] != TITLE {
            return None;
        }
        let payload = protocol::string(dialog, "placeholder");
        if payload.len() > 400_000 {
            return None;
        }
        let request: Self = serde_json::from_str(payload).ok()?;
        let mut ids = std::collections::HashSet::new();
        (request.version == 1
            && !request.request_id.is_empty()
            && (1..=8).contains(&request.questions.len())
            && request.questions.iter().all(|q| {
                !q.id.trim().is_empty()
                    && ids.insert(&q.id)
                    && !q.question.trim().is_empty()
                    && q.options.len() <= 8
                    && q.options.iter().all(|o| !o.label.trim().is_empty())
            }))
        .then_some(request)
    }
}

#[derive(Clone, Serialize)]
struct Answer {
    id: String,
    answer: String,
    selections: Vec<String>,
    custom: bool,
    skipped: bool,
}
#[derive(Default)]
pub(super) struct View {
    rpc_id: String,
    page: usize,
    selected: usize,
    editing: bool,
    focus: bool,
    drafts: Vec<String>,
    checked: Vec<Vec<usize>>,
    custom_enabled: Vec<bool>,
    answers: Vec<Option<Answer>>,
}
#[derive(Clone, Copy)]
enum Action {
    Choose(usize),
    Custom,
    SubmitCustom,
    Skip,
    Back,
    Next,
    Cancel,
}
impl View {
    fn go(&mut self, page: usize, request: &Request) {
        self.page = page;
        self.selected = self.answers[page]
            .as_ref()
            .and_then(|a| {
                request.questions[page]
                    .options
                    .iter()
                    .position(|o| o.label == a.answer)
            })
            .unwrap_or(0);
        self.editing = request.questions[page].options.is_empty() || self.custom_enabled[page];
        self.focus = true;
    }
    fn act(&mut self, action: Action, request: &Request) -> Option<Value> {
        let q = &request.questions[self.page];
        let answer = match action {
            Action::Cancel => return Some(json!({"cancelled":true})),
            Action::Back => {
                self.go(self.page.saturating_sub(1), request);
                return None;
            }
            Action::Next => {
                self.go((self.page + 1).min(request.questions.len() - 1), request);
                return None;
            }
            Action::Custom => {
                self.answers[self.page] = None;
                self.custom_enabled[self.page] = !q.multi_select || !self.custom_enabled[self.page];
                self.editing = self.custom_enabled[self.page];
                self.focus = true;
                return None;
            }
            Action::SubmitCustom => {
                let custom = self.custom_enabled[self.page] || q.options.is_empty();
                let text = if custom {
                    self.drafts[self.page].trim()
                } else {
                    ""
                };
                if (custom && text.is_empty()) || text.chars().count() > 4000 {
                    return None;
                }
                let selections: Vec<_> = q
                    .options
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| self.checked[self.page].contains(i))
                    .map(|(_, o)| o.label.clone())
                    .collect();
                if !custom && selections.is_empty() {
                    return None;
                }
                Answer {
                    id: q.id.clone(),
                    answer: text.into(),
                    selections,
                    custom,
                    skipped: false,
                }
            }
            Action::Choose(index) => {
                let option = q.options.get(index)?;
                if q.multi_select {
                    self.answers[self.page] = None;
                    let checked = &mut self.checked[self.page];
                    if checked.contains(&index) {
                        checked.retain(|i| *i != index);
                    } else {
                        checked.push(index);
                    }
                    self.selected = index;
                    self.focus = true;
                    return None;
                }
                self.custom_enabled[self.page] = false;
                Answer {
                    id: q.id.clone(),
                    answer: option.label.clone(),
                    selections: vec![],
                    custom: false,
                    skipped: false,
                }
            }
            Action::Skip => {
                self.custom_enabled[self.page] = false;
                self.checked[self.page].clear();
                Answer {
                    id: q.id.clone(),
                    answer: String::new(),
                    selections: vec![],
                    custom: false,
                    skipped: true,
                }
            }
        };
        self.answers[self.page] = Some(answer);
        if self.page + 1 < request.questions.len() {
            self.go(self.page + 1, request);
            None
        } else if let Some(missing) = self.answers.iter().position(Option::is_none) {
            self.go(missing, request);
            None
        } else {
            Some(json!({"value":json!({"answers":self.answers}).to_string()}))
        }
    }

    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        dialog: &Value,
        request: &Request,
        waiting: bool,
        keyboard: bool,
    ) -> Option<Value> {
        let id = protocol::string(dialog, "id");
        if self.rpc_id != id {
            *self = Self {
                rpc_id: id.into(),
                drafts: vec![String::new(); request.questions.len()],
                checked: vec![vec![]; request.questions.len()],
                custom_enabled: vec![false; request.questions.len()],
                answers: vec![None; request.questions.len()],
                ..Default::default()
            };
            self.go(0, request);
        }
        let q = &request.questions[self.page];
        let colors = theme::palette(ui.ctx());
        let card_id = egui::Id::new(("question-card", id));
        let edit_id = egui::Id::new(("question-custom", id, self.page));
        if self.focus && !self.editing {
            ui.memory_mut(|m| m.request_focus(card_id));
            self.focus = false;
        }
        let body_height = content_height(ui, q, self.editing)
            .min((ui.ctx().screen_rect().height() - 300.0).clamp(180.0, 600.0));
        let mut action = None;
        let owns_keyboard = keyboard
            && ui.memory(|m| {
                m.focused() == Some(if self.editing { edit_id } else { card_id })
                    || (!self.editing && m.focused().is_none())
            });
        let mut moved = false;
        if owns_keyboard {
            // Consume before the composer sees these keys, including while a
            // shared answer is awaiting acknowledgment from its owning window.
            ui.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                    action = Some(Action::Cancel);
                }
                if self.editing {
                    if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter) {
                        action = Some(Action::SubmitCustom);
                    }
                } else {
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                        self.selected = self.selected.saturating_sub(1);
                        moved = true;
                    }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                        self.selected = (self.selected + 1).min(q.options.len());
                        moved = true;
                    }
                    for (n, key) in [
                        egui::Key::Num1,
                        egui::Key::Num2,
                        egui::Key::Num3,
                        egui::Key::Num4,
                        egui::Key::Num5,
                        egui::Key::Num6,
                        egui::Key::Num7,
                        egui::Key::Num8,
                        egui::Key::Num9,
                    ]
                    .iter()
                    .enumerate()
                    {
                        if n <= q.options.len() && i.consume_key(egui::Modifiers::NONE, *key) {
                            action = Some(if n == q.options.len() {
                                Action::Custom
                            } else {
                                Action::Choose(n)
                            });
                        }
                    }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) && self.page > 0 {
                        action = Some(Action::Back);
                    }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)
                        && self.page + 1 < request.questions.len()
                    {
                        action = Some(Action::Next);
                    }
                    if q.multi_select && i.consume_key(egui::Modifiers::NONE, egui::Key::Space) {
                        action = Some(if self.selected == q.options.len() {
                            Action::Custom
                        } else {
                            Action::Choose(self.selected)
                        });
                    }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                        action = Some(if q.multi_select {
                            Action::SubmitCustom
                        } else if self.selected == q.options.len() {
                            Action::Custom
                        } else {
                            Action::Choose(self.selected)
                        });
                    }
                }
            });
        }
        egui::Frame::new()
            .fill(colors.sidebar)
            .stroke(Stroke::new(1.0, colors.border))
            .corner_radius(16)
            .inner_margin(14)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add_enabled_ui(!waiting, |ui| {
                    ui.horizontal(|ui| {
                        icons::inline(ui, Icon::Details, colors.accent);
                        ui.label(theme::small(ui, "A few quick questions"));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("Cancel").clicked() {
                                action = Some(Action::Cancel);
                            }
                            if navigation_arrow(ui, true, self.page + 1 < request.questions.len())
                                .clicked()
                            {
                                action = Some(Action::Next);
                            }
                            ui.label(theme::small(
                                ui,
                                format!("{} of {}", self.page + 1, request.questions.len()),
                            ));
                            if navigation_arrow(ui, false, self.page > 0).clicked() {
                                action = Some(Action::Back);
                            }
                        });
                    });
                    ui.add_space(7.0);
                    // Everything that can grow scrolls; navigation and submission
                    // stay visible even in the minimum supported window size.
                    egui::ScrollArea::vertical()
                        .id_salt(("question-options", id, self.page))
                        .min_scrolled_height(body_height)
                        .max_height(body_height)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(
                                egui::Label::new(RichText::new(&q.question).size(15.0).strong())
                                    .wrap(),
                            );
                            if q.multi_select {
                                ui.label(theme::small(ui, "Select all that apply"));
                            }
                            ui.add_space(9.0);
                            for (index, option) in q.options.iter().enumerate() {
                                let selected = !self.editing && self.selected == index;
                                let answer = self.answers[self.page].as_ref().is_some_and(|a| {
                                    !a.custom && !a.skipped && a.answer == option.label
                                });
                                let checked = self.checked[self.page].contains(&index);
                                let row = components::choice(
                                    ui,
                                    index + 1,
                                    &option.label,
                                    &option.description,
                                    if q.multi_select { checked } else { answer },
                                    selected,
                                );
                                if selected && moved {
                                    row.scroll_to_me(Some(Align::Center));
                                }
                                if row.clicked() {
                                    action = Some(Action::Choose(index));
                                }
                                ui.add_space(3.0);
                            }
                            if !q.options.is_empty() {
                                let row = components::choice(
                                    ui,
                                    q.options.len() + 1,
                                    "Something else",
                                    "Write your own answer",
                                    self.custom_enabled[self.page],
                                    !self.editing && self.selected == q.options.len(),
                                );
                                if self.selected == q.options.len() && moved {
                                    row.scroll_to_me(Some(Align::Center));
                                }
                                if row.clicked() {
                                    action = Some(Action::Custom);
                                }
                            }
                            if self.editing {
                                ui.add_space(8.0);
                                let edit = components::editor(
                                    ui,
                                    &mut self.drafts[self.page],
                                    edit_id,
                                    self.focus,
                                );
                                if edit.changed() {
                                    self.answers[self.page] = None;
                                }
                                if self.focus {
                                    edit.request_focus();
                                    self.focus = false;
                                }
                                if keyboard
                                    && edit.has_focus()
                                    && ui.input_mut(|i| {
                                        i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                                    })
                                {
                                    action = Some(Action::SubmitCustom);
                                }
                            }
                        });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button("Skip")
                            .on_hover_text("Leave this question unanswered")
                            .clicked()
                        {
                            action = Some(Action::Skip);
                        }
                        components::right_aligned(ui, |ui| {
                            let can_submit = if self.editing {
                                !self.drafts[self.page].trim().is_empty()
                            } else {
                                !self.checked[self.page].is_empty()
                            };
                            if (self.editing || q.multi_select)
                                && components::submit(
                                    ui,
                                    can_submit,
                                    self.page + 1 == request.questions.len(),
                                )
                                .clicked()
                            {
                                action = Some(Action::SubmitCustom);
                            }
                            ui.add(
                                egui::Label::new(theme::small(
                                    ui,
                                    if self.editing {
                                        "Enter: answer · Shift+Enter: new line"
                                    } else if q.multi_select {
                                        "1–9 / Space: toggle · Enter: confirm"
                                    } else {
                                        "1–9: choose · ↑↓ navigate · Enter: select"
                                    },
                                ))
                                .wrap(),
                            );
                        });
                    });
                });
                ui.add(
                    egui::Label::new(theme::small(
                        ui,
                        if waiting {
                            "Waiting for the owning window…"
                        } else {
                            "The agent continues automatically after your last answer."
                        },
                    ))
                    .wrap(),
                );
            });
        ui.add_space(6.0);
        if waiting {
            None
        } else {
            action.and_then(|a| self.act(a, request))
        }
    }
}

fn navigation_arrow(ui: &mut egui::Ui, next: bool, enabled: bool) -> egui::Response {
    let label = if next {
        "Next question"
    } else {
        "Previous question"
    };
    ui.add_enabled(enabled, |ui: &mut egui::Ui| {
        let colors = theme::palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
        let color = if !ui.is_enabled() {
            colors.dim
        } else if response.hovered() {
            colors.text
        } else {
            colors.muted
        };
        if response.hovered() && ui.is_enabled() {
            ui.painter().rect_filled(rect, 6, colors.hover);
        }
        let d = if next { 1.0 } else { -1.0 };
        let c = rect.center();
        ui.painter().add(egui::Shape::line(
            vec![
                c + egui::vec2(-2.0 * d, -4.0),
                c + egui::vec2(2.0 * d, 0.0),
                c + egui::vec2(-2.0 * d, 4.0),
            ],
            Stroke::new(1.4, color),
        ));
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
        });
        response
            .on_hover_text(label)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    })
}

fn content_height(ui: &egui::Ui, question: &Question, editing: bool) -> f32 {
    // The bottom composer panel starts with its previous height. A ScrollArea
    // sized from that remaining space can never grow beyond its first 64 px.
    // Measure the wrapped rows instead, so ordinary sets show every choice.
    let width = (ui.available_width() - 28.0).max(100.0);
    let heading = ui
        .painter()
        .layout(
            question.question.clone(),
            FontId::proportional(15.0),
            theme::palette(ui.ctx()).text,
            width,
        )
        .size()
        .y;
    let options = question
        .options
        .iter()
        .map(|option| {
            components::row_height(ui, &option.label, &option.description, width)
                + ui.spacing().item_spacing.y
                + 3.0
        })
        .sum::<f32>();
    heading
        + 20.0
        + if question.multi_select { 22.0 } else { 0.0 }
        + options
        + if question.options.is_empty() {
            0.0
        } else {
            components::row_height(ui, "Something else", "Write your own answer", width) + 8.0
        }
        + if editing { 115.0 } else { 0.0 }
}

#[cfg(test)]
mod tests;

pub(super) fn demo_dialog() -> Value {
    json!({"type":"extension_ui_request", "method":"input", "title":TITLE, "id":"demo-questions", "placeholder":json!({"version":1,"request_id":"demo-request","questions":[
        {"id":"start","question":"How should users start a task?","options":[
            {"label":"New task action (Recommended)","description":"A dedicated entry point. Review the proposed acceptance checks before implementation starts."},
            {"label":"From ordinary chat","description":"Keep everything in the conversation, with no separate task action."},
            {"label":"Support both","description":"Offer a shortcut for new tasks while keeping the same flow available in chat."}]},
        {"id":"workspace","question":"Where should the agent change code?","options":[{"label":"Isolated Git worktree","description":"Keep changes separate. No automatic commits or merges."},{"label":"Current workspace","description":"Work in place with your existing files."}]},
        {"id":"checks","question":"Which checks should run before finishing?","multiSelect":true,"options":[{"label":"Unit tests","description":"Exercise the question flow without provider calls."},{"label":"UI interaction tests","description":"Verify clicks, keyboard selection and automatic continuation."},{"label":"Visual checks","description":"Check that every normal option is visible in both themes."}]}
    ]}).to_string()})
}
