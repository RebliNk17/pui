//! Interactive, provider-free comparison. No RPC, saved answers or agent work.
use super::*;

#[derive(Default)]
pub(in crate::app) struct Preview {
    flows: [Flow; 2],
}
#[derive(Default)]
struct Flow {
    page: usize,
    choice: Option<usize>,
    checks: [bool; 2],
    custom: [bool; 2],
    drafts: [String; 3],
    skipped: [bool; 3],
    finished: bool,
    cancelled: bool,
}
const CHOICES: [(&str, &str); 2] = [
    (
        "Explicit New task action",
        "Keep ordinary chat unchanged. Review acceptance checks before implementation.",
    ),
    (
        "A mode toggle within chat",
        "Turn an existing conversation into a verified task, keeping its context.",
    ),
];
const CHECKS: [(&str, &str); 2] = [
    ("Unit tests", "Check behavior without provider calls."),
    ("Visual checks", "Review both themes and narrow windows."),
];
impl Preview {
    pub(in crate::app) fn show(&mut self, ctx: &egui::Context) {
        let colors = theme::palette(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors.bg).inner_margin(28))
            .show(ctx, |ui| {
                ui.heading("Try both question designs");
                ui.label(theme::small(ui, "Preview only · no agent or provider calls · nothing is saved or sent to your chats"));
                ui.add_space(8.0);
                ui.label(theme::small(ui, "Each side is independent: 1. choose one → 2. choose several → 3. type an answer. Use the arrows to revisit a step."));
                ui.add_space(18.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.columns(2, |columns| {
                        for (design, ui) in columns.iter_mut().enumerate() {
                            ui.push_id(design, |ui| {
                                ui.heading(if design == 0 { "A · Clean selection rows" } else { "B · Shortcut cards" });
                                ui.add(egui::Label::new(theme::small(ui, if design == 0 {
                                    "No visible numbers. Radio circles / checkboxes, with the custom answer integrated in one panel."
                                } else {
                                    "Separate shortcut badges. Titles and explanations aligned together, with a dedicated answer box."
                                })).wrap());
                                ui.add_space(14.0);
                                self.flows[design].show(ui, design);
                            });
                        }
                    });
                });
            });
    }
}
impl Flow {
    fn advance(&mut self) {
        self.skipped[self.page] = false;
        if self.page < 2 {
            self.page += 1;
        } else {
            self.finished = true;
        }
    }
    fn show(&mut self, ui: &mut egui::Ui, design: usize) {
        let colors = theme::palette(ui.ctx());
        egui::Frame::new()
            .fill(colors.sidebar)
            .stroke(Stroke::new(1.0, colors.border))
            .corner_radius(16)
            .inner_margin(18)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if self.finished || self.cancelled {
                    ui.label(
                        RichText::new(if self.cancelled {
                            "Preview cancelled"
                        } else {
                            "All done — preview answers"
                        })
                        .size(16.0)
                        .strong(),
                    );
                    ui.add_space(12.0);
                    if !self.cancelled {
                        let choice = if self.skipped[0] {
                            "Skipped".into()
                        } else if self.custom[0] {
                            self.drafts[0].clone()
                        } else {
                            self.choice
                                .map(|i| CHOICES[i].0.to_owned())
                                .unwrap_or_else(|| "Not answered".into())
                        };
                        let checks = CHECKS
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| self.checks[*i])
                            .map(|(_, c)| c.0)
                            .collect::<Vec<_>>()
                            .join(", ");
                        for (label, value) in [
                            ("Start", choice),
                            (
                                "Checks",
                                if self.skipped[1] {
                                    "Skipped".into()
                                } else {
                                    let value = format!("{checks} {}", self.drafts[1]);
                                    if value.trim().is_empty() {
                                        "Not answered".into()
                                    } else {
                                        value
                                    }
                                },
                            ),
                            (
                                "Free text",
                                if self.skipped[2] {
                                    "Skipped".into()
                                } else if self.drafts[2].trim().is_empty() {
                                    "Not answered".into()
                                } else {
                                    self.drafts[2].clone()
                                },
                            ),
                        ] {
                            ui.label(RichText::new(label).size(12.0).color(colors.muted));
                            ui.add(egui::Label::new(value).wrap());
                            ui.add_space(8.0);
                        }
                    }
                    ui.label(theme::small(
                        ui,
                        "Nothing was sent. In a real chat, the agent would now continue.",
                    ));
                    if ui.button("Try again").clicked() {
                        *self = Self::default();
                    }
                    return;
                }
                ui.horizontal(|ui| {
                    icons::inline(ui, Icon::Details, colors.accent);
                    ui.label(theme::small(ui, "A few quick questions"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("Cancel").clicked() {
                            self.cancelled = true;
                        }
                        if navigation_arrow(ui, true, self.page < 2).clicked() {
                            self.page += 1;
                        }
                        ui.label(theme::small(ui, format!("{} of 3", self.page + 1)));
                        if navigation_arrow(ui, false, self.page > 0).clicked() {
                            self.page -= 1;
                        }
                    });
                });
                ui.add_space(16.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(match self.page {
                            0 => "How should users start a task?",
                            1 => "Which checks should run?",
                            _ => "Any other details or constraints?",
                        })
                        .size(15.0)
                        .strong(),
                    )
                    .wrap(),
                );
                ui.label(theme::small(
                    ui,
                    match self.page {
                        0 => "Choose one · selecting an option advances",
                        1 => "Select all that apply · Continue when ready",
                        _ => "Write freely. Your answer stays here if you go back.",
                    },
                ));
                ui.add_space(12.0);
                if self.page < 2 {
                    let page = self.page;
                    let options = if page == 0 { CHOICES } else { CHECKS };
                    for (n, (title, description)) in options.into_iter().enumerate() {
                        let selected = if page == 0 {
                            self.choice == Some(n) && !self.custom[0]
                        } else {
                            self.checks[n]
                        };
                        if row(ui, design, n, title, description, selected, page == 1).clicked() {
                            if page == 0 {
                                self.choice = Some(n);
                                self.custom[0] = false;
                                self.advance();
                                break;
                            } else {
                                self.checks[n] = !self.checks[n];
                            }
                        }
                        ui.add_space(7.0);
                    }
                    // A click may have moved to the next question; render it on
                    // the next frame, never bind that click to its custom input.
                    if page != self.page {
                        return;
                    }
                    if (!self.custom[page] || design == 1)
                        && row(
                            ui,
                            design,
                            2,
                            "Something else",
                            "Write your own answer",
                            self.custom[page],
                            page == 1,
                        )
                        .clicked()
                    {
                        self.custom[page] = !self.custom[page];
                    }
                }
                let editing = self.page == 2 || self.custom[self.page];
                let mut submit = false;
                if editing {
                    ui.add_space(8.0);
                    let integrated = design == 0;
                    let field_id = ui.make_persistent_id(("preview-answer", self.page));
                    egui::Frame::new()
                        .fill(colors.bg)
                        .stroke(Stroke::new(
                            1.0,
                            if integrated {
                                colors.accent
                            } else {
                                colors.border
                            },
                        ))
                        .corner_radius(if integrated { 10 } else { 12 })
                        .inner_margin(if integrated { 12 } else { 16 })
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                icons::inline(ui, Icon::Edit, colors.accent);
                                ui.label(
                                    RichText::new(if self.page == 2 {
                                        "Your answer"
                                    } else {
                                        "Something else"
                                    })
                                    .size(13.0),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if self.page < 2 && ui.small_button("Close").clicked() {
                                        self.custom[self.page] = false;
                                    }
                                });
                            });
                            ui.add_space(10.0);
                            let edit = ui.add(
                                egui::TextEdit::multiline(&mut self.drafts[self.page])
                                    .id(field_id)
                                    .frame(false)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(if integrated { 3 } else { 5 })
                                    .char_limit(4000)
                                    .font(FontId::proportional(13.5))
                                    .hint_text(
                                        RichText::new(if integrated {
                                            "Tell us what you have in mind…"
                                        } else {
                                            "Share your preference, details or constraints…"
                                        })
                                        .color(colors.dim),
                                    )
                                    .return_key(egui::KeyboardShortcut::new(
                                        egui::Modifiers::SHIFT,
                                        egui::Key::Enter,
                                    )),
                            );
                            if edit.has_focus()
                                && ui.input_mut(|i| {
                                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                                })
                            {
                                submit = true;
                            }
                            ui.add_space(10.0);
                            if !integrated {
                                ui.separator();
                            }
                            ui.horizontal(|ui| {
                                ui.label(theme::small(
                                    ui,
                                    "Enter to continue · Shift+Enter for a new line",
                                ));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    submit |= continue_button(
                                        ui,
                                        !self.drafts[self.page].trim().is_empty(),
                                        self.page == 2,
                                    )
                                    .clicked();
                                });
                            });
                        });
                }
                if !editing && self.page == 1 {
                    ui.add_space(12.0);
                    components::right_aligned(ui, |ui| {
                        submit =
                            continue_button(ui, self.checks.iter().any(|v| *v), false).clicked();
                    });
                }
                let can_submit = if editing {
                    !self.drafts[self.page].trim().is_empty()
                } else {
                    self.checks.iter().any(|v| *v)
                };
                if submit && can_submit {
                    self.advance();
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui.small_button("Skip").clicked() {
                        let page = self.page;
                        self.advance();
                        self.skipped[page] = true;
                    }
                    ui.add(
                        egui::Label::new(theme::small(
                            ui,
                            "The agent continues after your last answer.",
                        ))
                        .wrap(),
                    );
                });
            });
    }
}
fn continue_button(ui: &mut egui::Ui, enabled: bool, last: bool) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    ui.add_enabled(
        enabled,
        egui::Button::new(
            RichText::new(if last {
                "Submit & continue"
            } else {
                "Continue"
            })
            .color(if enabled { colors.bg } else { colors.dim }),
        )
        .fill(if enabled {
            colors.accent
        } else {
            colors.surface
        })
        .corner_radius(7),
    )
}
fn row(
    ui: &mut egui::Ui,
    design: usize,
    n: usize,
    title: &str,
    description: &str,
    selected: bool,
    multi: bool,
) -> egui::Response {
    if design == 1 {
        return components::choice(ui, n + 1, title, description, selected, false);
    }
    let colors = theme::palette(ui.ctx());
    let frame = egui::Frame::new()
        .fill(if selected {
            colors.hover
        } else {
            colors.sidebar
        })
        .stroke(Stroke::new(
            1.0,
            if selected {
                colors.accent
            } else if design == 0 {
                colors.sidebar
            } else {
                colors.border
            },
        ))
        .corner_radius(10)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(24.0, 20.0), Sense::hover());
                let c = r.center();
                let color = if selected { colors.accent } else { colors.dim };
                if design == 0 {
                    if multi {
                        ui.painter().rect_stroke(
                            egui::Rect::from_center_size(c, Vec2::splat(14.0)),
                            3,
                            Stroke::new(1.2, color),
                            egui::StrokeKind::Inside,
                        );
                        if selected {
                            check(ui, c, color);
                        }
                    } else {
                        ui.painter().circle_stroke(c, 6.0, Stroke::new(1.2, color));
                        if selected {
                            ui.painter().circle_filled(c, 3.0, color);
                        }
                    }
                } else {
                    ui.painter().rect_filled(
                        egui::Rect::from_center_size(c, Vec2::new(24.0, 22.0)),
                        5,
                        colors.bg,
                    );
                    if selected {
                        check(ui, c, color);
                    } else {
                        ui.painter().text(
                            c,
                            egui::Align2::CENTER_CENTER,
                            (n + 1).to_string(),
                            FontId::proportional(11.0),
                            color,
                        );
                    }
                }
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.add(egui::Label::new(RichText::new(title).size(13.5)).wrap());
                    ui.add(
                        egui::Label::new(RichText::new(description).size(12.0).color(colors.muted))
                            .wrap(),
                    );
                });
            });
        });
    ui.interact(
        frame.response.rect,
        frame.response.id.with("choice"),
        Sense::click(),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}
fn check(ui: &egui::Ui, c: egui::Pos2, color: Color32) {
    ui.painter().add(egui::Shape::line(
        vec![
            c + egui::vec2(-4.0, 0.0),
            c + egui::vec2(-1.0, 3.0),
            c + egui::vec2(4.0, -3.0),
        ],
        Stroke::new(1.5, color),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_select_playground_does_not_fill_unbounded_scroll_height() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut preview = Preview::default();
        preview.flows[0].page = 1;
        preview.flows[1].page = 1;
        for width in [1480.0, 1000.0, 1480.0] {
            let mut heights = Vec::new();
            for _ in 0..40 {
                let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, 960.0))), ..Default::default() }, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.columns(2, |columns| {
                                for (design, ui) in columns.iter_mut().enumerate() {
                                    ui.push_id(design, |ui| {
                                        let top = ui.cursor().top();
                                        preview.flows[design].show(ui, design);
                                        let height = ui.cursor().top() - top;
                                        assert!(height < 650.0, "multi-select stretched into remaining height: {height}");
                                        heights.push(height);
                                    });
                                }
                            });
                        });
                    });
                });
            }
            for values in heights[8..].chunks_exact(2) {
                assert!((values[0] - heights[8]).abs() < 0.5);
                assert!((values[1] - heights[9]).abs() < 0.5);
            }
        }
    }

    #[test]
    fn playground_clicks_type_submit_and_reset_without_crossing_design_state() {
        fn labels(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(t) => out.push((
                    t.galley.job.text.clone(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                )),
                egui::Shape::Vec(v) => {
                    for s in v {
                        labels(s, out);
                    }
                }
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut preview = Preview::default();
        let frame = |preview: &mut Preview, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1480.0, 960.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| preview.show(ctx),
            )
        };
        let click = |preview: &mut Preview, label: &str, design: usize| {
            let mut text = vec![];
            for s in frame(preview, vec![]).shapes {
                labels(&s.shape, &mut text);
            }
            let pos = text
                .iter()
                .find(|(t, r)| t == label && (r.center().x > 740.0) == (design == 1))
                .unwrap_or_else(|| panic!("Missing {label}: {text:?}"))
                .1
                .center();
            for pressed in [true, false] {
                frame(
                    preview,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        };
        for _ in 0..3 {
            frame(&mut preview, vec![]);
        }
        for design in 0..2 {
            click(&mut preview, "Explicit New task action", design);
            assert_eq!(preview.flows[design].page, 1);
            click(&mut preview, "Unit tests", design);
            click(&mut preview, "Visual checks", design);
            assert_eq!(preview.flows[design].checks, [true, true]);
            click(&mut preview, "Continue", design);
            assert_eq!(preview.flows[design].page, 2);
            let hint = if design == 0 {
                "Tell us what you have in mind…"
            } else {
                "Share your preference, details or constraints…"
            };
            click(&mut preview, hint, design);
            frame(
                &mut preview,
                vec![egui::Event::Text("Keep it local".into())],
            );
            assert_eq!(preview.flows[design].drafts[2], "Keep it local");
            click(&mut preview, "Submit & continue", design);
            assert!(preview.flows[design].finished);
            click(&mut preview, "Try again", design);
            assert_eq!(preview.flows[design].page, 0);
            assert!(!preview.flows[design].finished);
            assert_eq!(
                preview.flows[1 - design].page,
                0,
                "each preview is independent"
            );
        }
    }
}
