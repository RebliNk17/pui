use super::*;

pub(super) fn selected_label(state: &State) -> String {
    let current = state
        .models
        .iter()
        .find(|m| m.id == state.model && m.provider == state.provider);
    let name = current
        .map(|m| m.name.as_str())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            if state.model_name.is_empty() {
                if state.model.is_empty() {
                    "Loading models…"
                } else {
                    &state.model
                }
            } else {
                &state.model_name
            }
        });
    let ambiguous = state
        .models
        .iter()
        .filter(|m| m.id == state.model || m.name == name)
        .map(|m| m.provider.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        > 1;
    let mut label = name.to_owned();
    if ambiguous && !state.provider.is_empty() {
        label.push_str(&format!(" · {}", state.provider));
    }
    if !state.thinking.is_empty() {
        label.push_str(&format!(" · {}", thinking_label(&state.thinking)));
    }
    label
}
fn effort_slider(
    ui: &mut egui::Ui,
    index: &mut usize,
    levels: &[String],
    enabled: bool,
) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = ui.available_width();
        ui.style_mut().visuals.widgets.inactive.bg_fill = colors.border;
        ui.style_mut().visuals.selection.bg_fill = colors.accent;
        let response = ui.add_enabled(
            enabled,
            egui::Slider::new(index, 0..=levels.len() - 1)
                .integer()
                .trailing_fill(true)
                .show_value(false),
        );
        let label = thinking_label(&levels[*index]);
        response.widget_info(|| {
            egui::WidgetInfo::slider(
                enabled && ui.is_enabled(),
                *index as f64,
                format!("Thinking effort: {label}"),
            )
        });
        response.on_hover_text(label)
    })
    .inner
}
impl Pui {
    fn model_change_blocked(&self) -> Option<&'static str> {
        if self.demo {
            Some("Design preview only. Start a real chat to change models.")
        } else if !self.connected || (!self.is_mirror() && self.client.is_none()) {
            Some("Connect to Pi before changing models.")
        } else if self.loading_chat {
            Some("Wait for this chat to finish loading.")
        } else if self.state.compacting {
            Some("Wait for context compaction to finish before changing models.")
        } else if self.history_pending.is_some() || self.restoring_code() {
            Some("Wait for history navigation to finish before changing models.")
        } else {
            None
        }
    }
    pub(super) fn model_button(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        let label = format!(
            "{}{}",
            selected_label(&self.state),
            if self.ultracode { " · Ultracode" } else { "" }
        );
        let width = (ui
            .painter()
            .layout_no_wrap(label.clone(), FontId::proportional(11.0), colors.muted)
            .size()
            .x
            + 22.0)
            .min((ui.available_width() - 170.0).max(65.0));
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let mut job = egui::text::LayoutJob::simple_singleline(
            label.clone(),
            FontId::proportional(11.0),
            colors.muted,
        );
        job.wrap.max_width = (width - 20.0).max(20.0);
        job.wrap.max_rows = 1;
        let galley = ui.painter().layout_job(job);
        let response = if galley.elided {
            response.on_hover_text(label)
        } else {
            response
        };
        ui.painter().galley(
            rect.left_center() - egui::vec2(0.0, galley.size().y / 2.0),
            galley,
            colors.muted,
        );
        icons::chevron(
            ui,
            egui::Rect::from_center_size(
                rect.right_center() - egui::vec2(8.0, 0.0),
                Vec2::splat(12.0),
            ),
            false,
            colors.dim,
        );
        if response.clicked() {
            self.model_picker_open = !self.model_picker_open;
            if self.model_picker_open {
                self.model_query.clear();
                self.effort_draft = None;
                self.model_picker_focus = true;
            }
        }
        self.model_anchor = Some((rect, ui.layer_id()));
    }
    pub(super) fn model_picker(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        if !self.model_picker_open {
            return;
        }
        let Some((anchor, layer)) = self.model_anchor else {
            return;
        };
        let mut open = true;
        let mut selection = None;
        let mut effort = None;
        let mut ultra = None;
        let blocked = self.model_change_blocked();
        let width = 400.0_f32.min(ctx.screen_rect().width() - 24.0);
        egui::Popup::new(
            egui::Id::new("model-picker-dropdown"),
            ctx.clone(),
            anchor,
            layer,
        )
        .open_bool(&mut open)
        .align(egui::emath::RectAlign::TOP_START)
        .align_alternatives(&[])
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(width)
        .frame(prompt_ui::popup(ctx))
        .gap(12.0)
        .show(|ui| {
            theme::menu_controls(ui);
            ui.set_width(width - 20.0);
            ui.label(RichText::new("Model").size(12.0).color(colors.muted));
            let search=egui::Frame::new().fill(colors.bg).stroke(Stroke::new(1.0,colors.border)).corner_radius(8).inner_margin(egui::Margin::symmetric(9,7)).show(ui,|ui|{
                ui.horizontal(|ui|{ui.spacing_mut().item_spacing.x=7.0;icons::inline(ui,Icon::Search,colors.dim);ui.add(egui::TextEdit::singleline(&mut self.model_query).id_salt("model-search").font(FontId::proportional(12.0)).frame(false).hint_text("Search models…").desired_width(ui.available_width()))}).inner
            }).inner;
            if self.model_picker_focus {
                search.request_focus();
                self.model_picker_focus = false;
            }
            let providers: std::collections::BTreeSet<_> = self
                .state
                .models
                .iter()
                .map(|m| m.provider.clone())
                .collect();
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("provider-filter")
                    .selected_text(if self.model_provider.is_empty() {
                        "All providers"
                    } else {
                        &self.model_provider
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.model_provider,
                            String::new(),
                            "All providers",
                        );
                        for provider in providers {
                            ui.selectable_value(
                                &mut self.model_provider,
                                provider.clone(),
                                provider,
                            );
                        }
                    });
                ui.label(theme::small(ui, format!("{} models", self.state.models.len())));
            });
            let query = self.model_query.to_lowercase();
            let filtered: Vec<_> = self
                .state
                .models
                .iter()
                .filter(|m| {
                    (self.model_provider.is_empty() || m.provider == self.model_provider)
                        && format!("{} {} {}", m.id, m.name, m.provider)
                            .to_lowercase()
                            .contains(&query)
                })
                .collect();
            let count = filtered.len();
            if let Some(reason) = blocked {
                ui.add(egui::Label::new(theme::small(ui, reason)).wrap());
            } else if !self.state.ready() {
                ui.add(egui::Label::new(theme::small(ui, "Changes apply to subsequent model requests. The response in progress is not restarted.")).wrap());
            }
            if let Some(error) = &self.state.error { ui.add(egui::Label::new(RichText::new(error).size(11.0).color(colors.red)).wrap()); }
            egui::ScrollArea::vertical()
                .id_salt("model-results")
                .max_height((anchor.top() - 210.0).clamp(90.0, 260.0))
                .auto_shrink([false, true])
                .show_rows(ui, 38.0, count, |ui, range| {
                    for index in range {
                        let model = filtered[index];
                        let selected =
                            self.state.model == model.id && self.state.provider == model.provider;
                        let (rect, response) = ui.add_enabled_ui(blocked.is_none(), |ui| ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 38.0), Sense::click(),
                        )).inner;
                        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, blocked.is_none(), selected, &model.name));
                        let response = response.on_hover_cursor(if blocked.is_none() { egui::CursorIcon::PointingHand } else { egui::CursorIcon::NotAllowed });
                        if selected || response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                7,
                                if selected { colors.hover } else { colors.surface },
                            );
                        }
                        ui.painter().text(
                            rect.min + egui::vec2(10.0, 11.0),
                            egui::Align2::LEFT_CENTER,
                            &model.name,
                            FontId::proportional(13.0),
                            if blocked.is_none() { colors.text } else { colors.dim },
                        );
                        ui.painter().text(
                            rect.min + egui::vec2(10.0, 27.0),
                            egui::Align2::LEFT_CENTER,
                            format!(
                                "{} · {} context · ${} in · ${} out",
                                model.provider,
                                crate::context::number(model.context_window as f64),
                                model.input_cost,
                                model.output_cost
                            ),
                            FontId::proportional(10.0),
                            colors.muted,
                        );
                        if response.clicked() && blocked.is_none() {
                            selection = Some((model.provider.clone(), model.id.clone()));
                        }
                    }
                });
            if count == 0 {
                ui.label(theme::small(ui, if self.state.models.is_empty() {
                    "Pi has not returned available models yet."
                } else {
                    "No matching models."
                }));
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Thinking effort").size(12.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(theme::small(ui, &self.state.model_name));
                });
            });
            if self.state.levels.is_empty(){ui.label(theme::small(ui, "Loading model-supported levels…"));}else{
                let model_key=format!("{}/{}",self.state.provider,self.state.model);
                let mut index=self.effort_draft.as_ref().filter(|(key,_)|*key==model_key).map(|(_,index)|*index).unwrap_or_else(||self.state.levels.iter().position(|level|*level==self.state.thinking).unwrap_or(0)).min(self.state.levels.len()-1);
                let count=self.state.levels.len();
                let response=effort_slider(ui,&mut index,&self.state.levels,self.state.ready()&&!self.loading_chat);
                if response.changed(){self.effort_draft=Some((model_key.clone(),index));}
                if response.drag_stopped() || (response.changed()&&!response.dragged()){effort=Some(self.state.levels[index].clone());}
                ui.horizontal(|ui|{ui.label(RichText::new(thinking_label(&self.state.levels[0])).size(10.0).color(colors.dim));ui.with_layout(Layout::right_to_left(Align::Center),|ui|{ui.label(RichText::new(thinking_label(&self.state.levels[count-1])).size(10.0).color(colors.dim));});});
            }
            let mut enabled=self.ultracode;
            let readonly=self.agents.nodes.get(&self.agents.active).is_some_and(|n|n.access=="read-only");
            let response=ui.add_enabled(!readonly&&self.ultracode_ready&&self.state.ready()&&!self.loading_chat,egui::Checkbox::new(&mut enabled,"Ultracode · dynamic workflow"));
            if response.changed(){ultra=Some(enabled);}
            response.on_hover_text("Separate from model reasoning. Delegate the fewest bounded workers that help; parent reviews results. Read-only workers or isolated Git worktrees.");
            ui.horizontal(|ui|{ui.label(theme::small(ui, "Worker concurrency ceiling"));ui.add(egui::DragValue::new(&mut self.prefs.agent_concurrency).range(1..=16));});
            if !self.ultracode_ready{ui.label(theme::small(ui, "Waiting for Pi's Pui adapter…"));}
            ui.label(
                RichText::new("Prices per 1M tokens · input / output")
                    .size(10.0)
                    .color(colors.dim),
            );
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        self.model_picker_open = open;
        if let Some((provider, id)) = selection {
            self.effort_draft = None;
            self.send(json!({"type":"set_model","provider":provider,"modelId":id}));
            self.model_picker_open = false;
        }
        if let Some(level) = effort {
            self.send(json!({"type":"set_thinking_level","level":level}));
        }
        if let Some(enabled) = ultra {
            self.ensure_agent();
            self.send(json!({"type":"prompt","message":if enabled{"/pui-ultracode on"}else{"/pui-ultracode off"}}));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn busy_chat_can_select_gemini_and_only_pi_acknowledgment_changes_model() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.connected = true;
        app.loading_chat = false;
        let (client, events, requests) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.busy = true;
        app.state.model = "old".into();
        app.state.provider = "litellm".into();
        app.state.model_name = "Old model".into();
        app.state.models = vec![protocol::Model {
            id: "gemini-4-argon".into(),
            name: "Gemini 4 Argon".into(),
            provider: "litellm".into(),
            context_window: 600000,
            input_cost: 2.0,
            output_cost: 10.0,
        }];
        app.state.levels = vec!["low".into(), "high".into()];
        app.draft = "keep draft".into();
        app.model_picker_open = true;
        app.model_query = "arg".into();
        app.model_anchor = Some((
            egui::Rect::from_min_size(egui::pos2(220.0, 800.0), egui::vec2(150.0, 28.0)),
            egui::LayerId::background(),
        ));
        fn locate(shape: &egui::Shape) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(t) if t.galley.job.text == "Gemini 4 Argon" => {
                    Some(t.pos + t.galley.size() / 2.0)
                }
                egui::Shape::Vec(v) => v.iter().find_map(locate),
                _ => None,
            }
        }
        let render = |app: &mut Pui, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| app.model_picker(ctx),
            )
        };
        for _ in 0..3 {
            render(&mut app, vec![]);
        }
        let output = render(&mut app, vec![]);
        let point = output.shapes.iter().find_map(|s| locate(&s.shape)).unwrap();
        for pressed in [true, false] {
            render(
                &mut app,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(!app.model_picker_open, "Busy chat must accept the click");
        assert_eq!(app.state.model, "old");
        assert_eq!(app.state.levels.len(), 2);
        let sent = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(sent["type"], "set_model");
        assert_eq!(sent["provider"], "litellm");
        assert_eq!(sent["modelId"], "gemini-4-argon");
        assert!(
            requests.try_recv().is_err(),
            "Exactly one model change, no abort or prompt"
        );
        events.send(rpc::Event::Record(json!({"type":"response","command":"set_model","success":true,"data":{"id":"gemini-4-argon","name":"Gemini 4 Argon","provider":"litellm"}}))).unwrap();
        app.poll_rpc();
        assert_eq!(app.state.model, "gemini-4-argon");
        assert_eq!(app.state.model_name, "Gemini 4 Argon");
        assert!(app.state.levels.is_empty());
        assert!(app.state.busy);
        assert_eq!(app.draft, "keep draft");
    }
    #[test]
    fn rejected_or_unavailable_model_changes_preserve_current_model_and_levels() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.state.model = "old".into();
        app.state.levels = vec!["high".into()];
        assert!(app.model_change_blocked().unwrap().contains("Connect"));
        app.connected = true;
        app.client = Some(rpc::Client::stub());
        app.loading_chat = false;
        app.state.compacting = true;
        assert!(app.model_change_blocked().unwrap().contains("compaction"));
        app.state.apply(json!({"type":"response","command":"set_model","success":false,"error":"No API key for litellm/gemini-4-argon"}));
        assert_eq!(app.state.model, "old");
        assert_eq!(app.state.levels, vec!["high"]);
        assert!(app.state.error.as_ref().unwrap().contains("No API key"));
    }
    #[test]
    fn effort_slider_uses_the_full_column_without_an_inline_value() {
        for width in [260.0, 400.0] {
            let ctx = egui::Context::default();
            let levels = ["low", "medium", "high", "xhigh"].map(str::to_owned);
            let mut index = 2;
            let mut rect = egui::Rect::NOTHING;
            let mut column = egui::Rect::NOTHING;
            let mut id = egui::Id::NULL;
            let render = |index: &mut usize,
                          events,
                          rect: &mut egui::Rect,
                          column: &mut egui::Rect,
                          id: &mut egui::Id| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 200.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            *column = ui.available_rect_before_wrap();
                            let old_width = ui.spacing().slider_width;
                            let response = effort_slider(ui, index, &levels, true);
                            *rect = response.rect;
                            *id = response.id;
                            assert_eq!(
                                ui.spacing().slider_width,
                                old_width,
                                "Slider styling must stay local"
                            );
                        });
                    },
                )
            };
            render(&mut index, vec![], &mut rect, &mut column, &mut id);
            let output = render(&mut index, vec![], &mut rect, &mut column, &mut id);
            assert!((rect.left() - column.left()).abs() < 0.1);
            assert!(
                (rect.right() - column.right()).abs() < 0.1,
                "Slider must use the whole column at {width}px: {rect:?} / {column:?}"
            );
            fn has_text(shape: &egui::Shape) -> bool {
                match shape {
                    egui::Shape::Text(_) => true,
                    egui::Shape::Vec(v) => v.iter().any(has_text),
                    _ => false,
                }
            }
            assert!(
                !output.shapes.iter().any(|s| has_text(&s.shape)),
                "No permanent value should take space beside the slider"
            );
            let pos = egui::pos2(rect.right() - 8.0, rect.center().y);
            for pressed in [true, false] {
                render(
                    &mut index,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    &mut rect,
                    &mut column,
                    &mut id,
                );
            }
            assert_eq!(
                index, 3,
                "The rightmost supported level must remain selectable"
            );
            ctx.memory_mut(|m| m.request_focus(id));
            render(
                &mut index,
                vec![egui::Event::Key {
                    key: egui::Key::ArrowLeft,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                &mut rect,
                &mut column,
                &mut id,
            );
            assert_eq!(
                index, 2,
                "Keyboard stepping must preserve discrete model-supported levels"
            );
        }
    }
    #[test]
    fn picker_contains_models_and_effort_in_one_view() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.model_picker_open = true;
        app.model_anchor = Some((
            egui::Rect::from_min_size(egui::pos2(300.0, 800.0), egui::vec2(160.0, 28.0)),
            egui::LayerId::background(),
        ));
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 900.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input(), |ctx| app.model_picker(ctx));
        let output = ctx.run(input(), |ctx| app.model_picker(ctx));
        fn text(shape: &egui::Shape, out: &mut String) {
            match shape {
                egui::Shape::Text(s) => out.push_str(&s.galley.job.text),
                egui::Shape::Vec(v) => {
                    for s in v {
                        text(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut labels = String::new();
        for shape in output.shapes {
            text(&shape.shape, &mut labels);
        }
        assert!(labels.contains("GPT-6.1 Sol"));
        assert!(labels.contains("Thinking effort"));
        assert!(labels.contains("Low"));
        assert_eq!(
            labels.matches("Extra high").count(),
            1,
            "The upper endpoint should be shown only once"
        );
        assert!(
            !labels.contains("High"),
            "The current effort must not be repeated beside the slider"
        );
        assert!(labels.contains("Search models"));
    }
    #[test]
    fn dropdown_is_anchored_above_model_button_and_dismisses_on_escape() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.model_picker_open = true;
        let anchor = egui::Rect::from_min_size(egui::pos2(300.0, 800.0), egui::vec2(150.0, 28.0));
        app.model_anchor = Some((anchor, egui::LayerId::background()));
        let input = |events| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            events,
            ..Default::default()
        };
        // Popup/ScrollArea geometry needs several passes to settle after sizing.
        for _ in 0..8 {
            let _ = ctx.run(input(vec![]), |ctx| app.model_picker(ctx));
        }
        let popup = ctx
            .read_response(egui::Id::new("model-picker-dropdown"))
            .unwrap();
        assert!(
            popup.rect.bottom() <= anchor.top(),
            "Dropdown must open up: {:?}",
            popup.rect
        );
        assert!((popup.rect.left() - anchor.left()).abs() < 2.0);
        let _ = ctx.run(
            input(vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]),
            |ctx| app.model_picker(ctx),
        );
        assert!(!app.model_picker_open);
    }
    #[test]
    fn slider_never_offers_levels_missing_from_current_pi_response() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.state.levels = vec!["off".into()];
        app.state.thinking = "off".into();
        app.model_picker_open = true;
        app.ultracode_ready = true;
        app.model_anchor = Some((
            egui::Rect::from_min_size(egui::pos2(300.0, 800.0), egui::vec2(150.0, 28.0)),
            egui::LayerId::background(),
        ));
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input(), |ctx| app.model_picker(ctx));
        let output = ctx.run(input(), |ctx| app.model_picker(ctx));
        fn labels(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                egui::Shape::Vec(v) => {
                    for s in v {
                        labels(s, out)
                    }
                }
                _ => {}
            }
        }
        let mut text = Vec::new();
        for s in output.shapes {
            labels(&s.shape, &mut text);
        }
        assert!(text.iter().any(|s| s == "Off"));
        assert!(
            !text
                .iter()
                .any(|s| s == "Extra high" || s == "Medium" || s == "High")
        );
        assert!(text.iter().any(|s| s.contains("Ultracode")));
    }
    #[test]
    fn label_shows_name_provider_only_for_ambiguity_and_capitalized_effort() {
        let mut state = State {
            model: "test".into(),
            model_name: "Friendly Model".into(),
            provider: "one".into(),
            thinking: "high".into(),
            ..Default::default()
        };
        let model = |provider: &str| crate::protocol::Model {
            id: "test".into(),
            name: "Friendly Model".into(),
            provider: provider.into(),
            context_window: 1000,
            input_cost: 1.0,
            output_cost: 2.0,
        };
        state.models = vec![model("one")];
        assert_eq!(selected_label(&state), "Friendly Model · High");
        state.models.push(model("two"));
        assert_eq!(selected_label(&state), "Friendly Model · one · High");
    }
}
