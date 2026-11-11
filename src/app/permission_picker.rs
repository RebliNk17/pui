//! One permission selector shared by the composer and Settings.
use super::*;
use settings::{PermissionMode, Settings};

impl Pui {
    pub(super) fn permission_scope(&self) -> String {
        format!(
            "{}|{}|{}",
            self.agents.active,
            self.state.session_file.as_deref().unwrap_or(""),
            self.prefs.cwd.display()
        )
    }
    pub(super) fn choose_permission(&mut self, mode: PermissionMode) {
        if self
            .agents
            .permission_changes
            .contains_key(&self.agents.active)
        {
            self.state.notice = Some(
                "Wait for this worker's access change to finish before changing mode again.".into(),
            );
            return;
        }
        self.worker_upgrade_pending = None;
        self.bypass_pending = None;
        if self.readonly_worker() && mode != PermissionMode::Plan {
            self.worker_upgrade_pending = Some((self.permission_scope(), mode));
        } else if mode == PermissionMode::Bypass && self.active_permission() != mode {
            self.bypass_pending = Some(self.permission_scope());
        } else {
            self.set_permission(mode);
        }
    }
    pub(super) fn propagate_settings(&mut self, before: &Settings) {
        if *before != self.prefs.settings
            && !self.demo
            && !self.remote_action(sync::Action::Settings(
                self.agent_settings(&self.agents.active),
            ))
            && before.payload() != self.prefs.settings.payload()
        {
            self.apply_settings();
        }
    }
    pub(super) fn permission_button(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        let before = self.prefs.settings.clone();
        let label = if self.readonly_worker() {
            "Read-only"
        } else {
            self.active_permission().short_label()
        };
        let color = if !self.readonly_worker() && self.active_permission() == PermissionMode::Bypass
        {
            colors.red
        } else {
            colors.muted
        };
        let galley = ui
            .painter()
            .layout_no_wrap(label.into(), FontId::proportional(11.5), color);
        let (rect, button) =
            ui.allocate_exact_size(Vec2::new(galley.size().x + 21.0, 28.0), Sense::click());
        ui.painter().galley(
            rect.left_center() - egui::vec2(0.0, galley.size().y / 2.0),
            galley,
            color,
        );
        icons::chevron(
            ui,
            egui::Rect::from_center_size(
                rect.right_center() - egui::vec2(8.0, 0.0),
                Vec2::splat(12.0),
            ),
            false,
            color,
        );
        button.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Agent mode: {label}"),
            )
        });
        let button = button
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(if self.readonly_worker() {
                "Read-only worker · choosing a writable mode requires an explicit isolated-worktree upgrade"
            } else {
                "Agent permission mode · applies to subsequent tool calls in this chat"
            });
        egui::Popup::menu(&button)
            .align(egui::emath::RectAlign::TOP_START)
            .align_alternatives(&[])
            .width(275.0)
            .frame(prompt_ui::popup(ui.ctx()))
            .show(|ui| {
                theme::menu_controls(ui);
                ui.set_width(255.0);
                ui.label(theme::small(ui, "Agent mode · this chat"));
                if self.readonly_worker() {
                    ui.label(theme::small(ui, "Read-only tools. Writable modes require a reconnect in an isolated Git worktree."));
                }
                for mode in PermissionMode::ALL {
                    if ui
                        .add(
                            egui::Button::new(if self.readonly_worker() && mode == PermissionMode::Plan { "Read-only" } else { mode.label() })
                                .selected(self.active_permission() == mode)
                                .fill(if self.active_permission() == mode {
                                    colors.accent_bg
                                } else {
                                    prompt_ui::background(ui.ctx())
                                })
                                .stroke(Stroke::NONE),
                        )
                        .on_hover_text(mode.description())
                        .clicked()
                    {
                        self.choose_permission(mode);
                        ui.close();
                    }
                }
            });
        self.propagate_settings(&before);
    }
    pub(super) fn permission_confirmation(&mut self, ui: &mut egui::Ui) {
        self.worker_upgrade_confirmation(ui);
        let Some(scope) = self.bypass_pending.as_ref() else {
            return;
        };
        if *scope != self.permission_scope() {
            self.bypass_pending = None;
            return;
        }
        let frame = ui.ctx().cumulative_frame_nr();
        let first = ui.ctx().data_mut(|data| {
            let id = egui::Id::new("bypass-confirmation-focus");
            let previous = data.get_temp::<(String, u64)>(id);
            data.insert_temp(id, (scope.clone(), frame));
            previous.is_none_or(|(previous_scope, previous_frame)| {
                previous_scope != *scope || previous_frame.saturating_add(1) < frame
            })
        });
        prompt_ui::inline(ui.ctx()).show(ui, |ui| {
            prompt_ui::style(ui);
            confirmations::heading(ui, "Bypass all permissions?", "Only enable this for work you trust.", true);
            ui.add_space(14.0);
            confirmations::note(ui, "This chat can run commands, change or delete files, access credentials and use the network without asking. Other workers keep their own modes. This does not override OS permissions.");
            ui.add_space(18.0);
            let choice = if prompt_ui::keyboard_available(ui) && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                Some(confirmations::Choice::Cancel)
            } else {
                confirmations::actions(ui, "Cancel", "Enable bypass", true, true, first)
            };
            match choice {
                Some(confirmations::Choice::Confirm) => {
                    self.set_permission(PermissionMode::Bypass);
                    self.bypass_pending = None;
                }
                Some(confirmations::Choice::Cancel) => self.bypass_pending = None,
                None => {}
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn texts(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => out.push((
                t.galley.job.text.clone(),
                egui::Rect::from_min_size(t.pos, t.galley.size()),
            )),
            egui::Shape::Vec(v) => {
                for s in v {
                    texts(s, out);
                }
            }
            _ => {}
        }
    }
    #[test]
    fn composer_picker_changes_mode_while_running_without_losing_the_draft() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.state.busy = true;
        app.draft = "Keep my message".into();
        let frame = |app: &mut Pui, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.add_space(500.0);
                        app.composer(ui, ctx);
                    });
                },
            )
        };
        frame(&mut app, vec![]);
        let mut text = Vec::new();
        for shape in frame(&mut app, vec![]).shapes {
            texts(&shape.shape, &mut text);
        }
        let button = text
            .iter()
            .find(|(t, _)| t == "Auto")
            .expect("Composer mode button")
            .1;
        let click = |app: &mut Pui, pos| {
            for pressed in [true, false] {
                frame(
                    app,
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
        click(&mut app, button.center());
        text.clear();
        for shape in frame(&mut app, vec![]).shapes {
            texts(&shape.shape, &mut text);
        }
        let plan = text
            .iter()
            .find(|(t, _)| t == "Plan")
            .expect("Plan choice")
            .1;
        assert!(plan.bottom() < button.top(), "Menu must open upward");
        assert!(text.iter().any(|(t, _)| t == "Bypass all permissions"));
        let manual = text.iter().find(|(t, _)| t == "Manual").unwrap().1;
        let accept = text.iter().find(|(t, _)| t == "Accept edits").unwrap().1;
        assert!(
            accept.center().y - manual.center().y <= 27.0,
            "Mode rows should be compact"
        );
        click(&mut app, plan.center());
        assert!(app.prefs.settings.permission == PermissionMode::Plan);
        assert!(app.state.busy);
        assert_eq!(app.draft, "Keep my message");
    }
    #[test]
    fn bypass_requires_confirmation_and_cannot_follow_a_chat_switch() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.choose_permission(PermissionMode::Bypass);
        assert!(app.prefs.settings.permission == PermissionMode::Auto);
        assert!(app.bypass_pending.is_some());
        app.state.session_file = Some("/different/chat".into());
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.permission_confirmation(ui));
        });
        assert!(app.bypass_pending.is_none());
        assert!(app.prefs.settings.permission == PermissionMode::Auto);
        app.choose_permission(PermissionMode::Bypass);
        for _ in 0..3 {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.permission_confirmation(ui));
            });
        }
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.permission_confirmation(ui));
            },
        );
        assert!(app.bypass_pending.is_none());
        assert!(app.prefs.settings.permission == PermissionMode::Auto);
        app.choose_permission(PermissionMode::Plan);
        assert!(app.prefs.settings.permission == PermissionMode::Plan);
        assert_eq!(PermissionMode::Bypass.key(), "bypassPermissions");
    }
}
