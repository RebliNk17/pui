use super::*;

#[cfg(test)]
mod safety_tests;
#[cfg(test)]
mod tests;

pub(super) fn model_label(state: &State) -> String {
    if state.model.is_empty() && !state.model_hint.is_empty() {
        return format!("{} · recent", state.model_hint);
    }
    let current = state
        .models
        .iter()
        .find(|m| m.id == state.model && m.provider == state.provider);
    let name = current
        .map(|m| m.name.as_str())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            if !state.model_name.is_empty() {
                &state.model_name
            } else if !state.model.is_empty() {
                &state.model
            } else {
                "Loading models…"
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
    if ambiguous && !state.provider.is_empty() {
        // Several accounts offer the same model; name the account (`anthropic · Work`).
        format!(
            "{name} · {}",
            crate::accounts::display_provider(&state.provider)
        )
    } else {
        name.into()
    }
}

// Settings still uses a combined summary; the composer has two independent controls.
pub(super) fn selected_label(state: &State) -> String {
    let name = model_label(state);
    if state.model.is_empty() && !state.model_hint.is_empty() {
        return name;
    }
    if state.thinking.is_empty() {
        name
    } else {
        format!("{name} · {}", thinking_label(&state.thinking))
    }
}

fn paint_text(ui: &egui::Ui, text: &str, rect: egui::Rect, size: f32, color: Color32) {
    let mut job =
        egui::text::LayoutJob::simple_singleline(text.into(), FontId::proportional(size), color);
    job.wrap.max_width = rect.width().max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        rect.left_center() - egui::vec2(0.0, galley.size().y / 2.0),
        galley,
        color,
    );
}

// The price column ends before the independent selected-model check gutter.
const MODEL_PRICE_RIGHT_INSET: f32 = 20.0;
const MODEL_SCROLL_RAIL: f32 = 12.0;

fn provider_filter(
    ui: &mut egui::Ui,
    salt: &str,
    provider: &mut String,
    providers: &std::collections::BTreeSet<String>,
    width: f32,
) {
    let colors = theme::palette(ui.ctx());
    ui.scope(|ui| {
        ui.set_width(width);
        ui.set_max_width(width);
        ui.spacing_mut().button_padding = egui::vec2(3.0, 0.0);
        ui.spacing_mut().interact_size.y = 20.0;
        ui.spacing_mut().icon_width = 12.0;
        ui.spacing_mut().icon_spacing = 6.0;
        let widgets = &mut ui.style_mut().visuals.widgets;
        for visuals in [&mut widgets.inactive, &mut widgets.hovered] {
            visuals.bg_stroke = Stroke::NONE;
            visuals.weak_bg_fill = Color32::TRANSPARENT;
        }
        // ponytail: egui Areas remember their last size and clamp the next frame to it,
        // so a popup first measured with fewer providers would stay clipped. Re-key on count.
        let response = egui::ComboBox::from_id_salt((salt, "provider-filter", providers.len()))
            .width(width)
            .truncate()
            .selected_text(
                RichText::new(if provider.is_empty() {
                    "All providers".to_owned()
                } else {
                    crate::accounts::display_provider(provider)
                })
                .size(11.0),
            )
            .icon(move |ui, rect, _, open| {
                let center = rect.center();
                let sign = if open { -1.0 } else { 1.0 };
                ui.painter().add(egui::Shape::line(
                    vec![
                        center + egui::vec2(-3.0, -1.5 * sign),
                        center + egui::vec2(0.0, 1.5 * sign),
                        center + egui::vec2(3.0, -1.5 * sign),
                    ],
                    Stroke::new(1.2, colors.dim),
                ));
                #[cfg(test)]
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new("model-provider-chevron"), center)
                });
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(provider, String::new(), "All providers");
                for value in providers {
                    ui.selectable_value(
                        provider,
                        value.clone(),
                        crate::accounts::display_provider(value),
                    );
                }
            })
            .response;
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("model-provider-control"), response.rect)
        });
        response.on_hover_text(if provider.is_empty() {
            "Filter models by provider"
        } else {
            provider.as_str()
        });
    });
}

fn check(ui: &egui::Ui, center: egui::Pos2) {
    let color = theme::palette(ui.ctx()).accent;
    ui.painter().line_segment(
        [
            center + egui::vec2(-4.0, 0.0),
            center + egui::vec2(-1.0, 3.0),
        ],
        Stroke::new(1.4, color),
    );
    ui.painter().line_segment(
        [
            center + egui::vec2(-1.0, 3.0),
            center + egui::vec2(5.0, -4.0),
        ],
        Stroke::new(1.4, color),
    );
}

fn menu_row(
    ui: &mut egui::Ui,
    id: egui::Id,
    height: f32,
    selected: bool,
    enabled: bool,
    label: &str,
) -> egui::Response {
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
            ui.interact(rect, id, Sense::click())
        })
        .inner;
    let colors = theme::palette(ui.ctx());
    if selected || response.hovered() || response.has_focus() {
        ui.painter().rect_filled(response.rect, 5, colors.hover);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.shrink(0.5),
            5,
            Stroke::new(1.0, colors.accent),
            egui::StrokeKind::Inside,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            enabled && ui.is_enabled(),
            selected,
            label,
        )
    });
    response.on_hover_cursor(if enabled {
        egui::CursorIcon::PointingHand
    } else {
        egui::CursorIcon::NotAllowed
    })
}

fn price(value: f64) -> String {
    if value.is_finite() && value >= 0.0 {
        format!("${value}")
    } else {
        "—".into()
    }
}

/// One choice from the shared model list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Pick {
    /// The optional leading "Automatic" entry (e.g. the cheapest classifier).
    Automatic,
    Model { provider: String, id: String },
}

/// Filter state owned by each picker so independent pickers never share a query.
#[derive(Default)]
pub(super) struct ListState {
    pub query: String,
    pub provider: String,
    pub focus: bool,
}

pub(super) struct ModelList<'a> {
    /// Unique per picker: scopes widget IDs and the provider filter popup.
    pub salt: &'a str,
    pub models: &'a [&'a protocol::Model],
    pub selected: Option<(&'a str, &'a str)>,
    pub enabled: bool,
    /// Optional first row: label, description and whether it is selected.
    pub automatic: Option<(&'a str, &'a str, bool)>,
}

/// The composer's searchable, provider-filtered and priced model list, shared
/// with Settings. `notes` renders status text between the header and results;
/// `results_height` maps the measured header height to the viewport height.
pub(super) fn model_list(
    ui: &mut egui::Ui,
    state: &mut ListState,
    list: ModelList<'_>,
    notes: impl FnOnce(&mut egui::Ui),
    results_height: impl FnOnce(f32) -> f32,
    empty_message: &str,
) -> Option<Pick> {
    let ctx = ui.ctx().clone();
    let colors = theme::palette(&ctx);
    let providers = list
        .models
        .iter()
        .map(|m| m.provider.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let search_frame = egui::Frame::new().fill(colors.bg).stroke(Stroke::new(1.0, colors.border)).corner_radius(6)
        .inner_margin(egui::Margin::symmetric(7, 5)).show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::inline(ui, Icon::Search, colors.dim);
                let provider_width = (ui.available_width() * 0.44).clamp(44.0, 115.0);
                let search_width = (ui.available_width() - provider_width - 2.0 * ui.spacing().item_spacing.x - 1.0).max(8.0);
                let search = ui.add(egui::TextEdit::singleline(&mut state.query).id_salt((list.salt, "model-search"))
                    .font(FontId::proportional(11.0)).frame(false).margin(0).hint_text("Search models…").vertical_align(Align::Center).desired_width(search_width).min_size(egui::vec2(0.0, 20.0)));
                let (divider, _) = ui.allocate_exact_size(egui::vec2(1.0, 18.0), Sense::hover());
                ui.painter().line_segment([divider.center_top(), divider.center_bottom()], Stroke::new(1.0, colors.border));
                provider_filter(ui, list.salt, &mut state.provider, &providers, provider_width);
                search
            }).inner
        });
    let search = search_frame.inner;
    #[cfg(test)]
    ctx.data_mut(|data| { data.insert_temp(egui::Id::new("model-search-row"), search_frame.response.rect); data.insert_temp(egui::Id::new("model-search-input"), search.rect); });
    if state.focus && !ui.is_sizing_pass() { search.request_focus(); state.focus = false; }
    let results_width = (ui.available_width() - MODEL_SCROLL_RAIL).max(0.0);
    ui.allocate_ui_with_layout(egui::vec2(results_width, 16.0), Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(MODEL_PRICE_RIGHT_INSET);
        let _label = ui.label(RichText::new("In / Out ⓘ").size(9.0).color(colors.dim))
            .on_hover_text("Input / output prices per 1 million tokens, as reported by Pi. Zero may be a provider placeholder.");
        #[cfg(test)]
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("model-price-heading"), _label.rect));
    });
    notes(ui);
    let query = state.query.to_lowercase();
    let filtered = list
        .models
        .iter()
        .copied()
        .filter(|m| {
            (state.provider.is_empty() || m.provider == state.provider)
                && format!("{} {} {}", m.id, m.name, m.provider)
                    .to_lowercase()
                    .contains(&query)
        })
        .collect::<Vec<_>>();
    // The Automatic row is a real entry: searchable only when the query is empty.
    let automatic = list.automatic.filter(|_| query.is_empty() && state.provider.is_empty());
    let offset = usize::from(automatic.is_some());
    let mut ids = Vec::with_capacity(filtered.len() + offset);
    if automatic.is_some() {
        ids.push(ui.id().with((list.salt, "automatic")));
    }
    ids.extend(filtered.iter().map(|m| ui.id().with((list.salt, "model", &m.provider, &m.id))));
    let selected_index = if automatic.is_some_and(|(_, _, selected)| selected) {
        0
    } else {
        filtered
            .iter()
            .position(|m| list.selected == Some((m.provider.as_str(), m.id.as_str())))
            .map(|i| i + offset)
            .unwrap_or(0)
    };
    let target = navigate(ui, &ids, selected_index, list.enabled);
    if filtered.is_empty() && automatic.is_none() {
        ui.add(egui::Label::new(theme::small(ui, empty_message)).wrap());
    }
    ui.spacing_mut().item_spacing.y = 0.0;
    let height = results_height(ui.min_rect().height() + 20.0);
    let mut scroll = egui::ScrollArea::vertical().id_salt((list.salt, "model-results"))
        .max_height(height).min_scrolled_height(0.0).auto_shrink([false, false]);
    if let Some(index) = target { scroll = scroll.vertical_scroll_offset(index as f32 * 32.0); }
    let mut pick = None;
    scroll.show_rows(ui, 32.0, ids.len(), |ui, range| {
        ui.set_width(results_width);
        ui.spacing_mut().item_spacing.y = 0.0;
        for index in range {
            let text_color = if list.enabled { colors.text } else { colors.dim };
            if let (Some((label, description, selected)), 0) = (automatic, index) {
                let response = menu_row(ui, ids[0], 32.0, selected, list.enabled, label);
                let rect = response.rect;
                #[cfg(test)]
                ctx.data_mut(|data| data.insert_temp(egui::Id::new((list.salt, "automatic-rect")), rect));
                icons::draw(ui, egui::Rect::from_center_size(egui::pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(14.0)), Icon::Spark, colors.accent);
                paint_text(ui, label, egui::Rect::from_min_max(rect.min + egui::vec2(26.0, 0.0), egui::pos2(rect.right() - 24.0, rect.top() + 17.0)), 11.5, text_color);
                paint_text(ui, description, egui::Rect::from_min_max(rect.min + egui::vec2(26.0, 17.0), egui::pos2(rect.right() - 24.0, rect.bottom())), 9.0, colors.dim);
                if selected { check(ui, rect.right_center() - egui::vec2(9.0, 0.0)); }
                if target == Some(0) { response.scroll_to_me(Some(Align::Center)); }
                if response.on_hover_text(description).clicked() && list.enabled { pick = Some(Pick::Automatic); }
                continue;
            }
            let model = filtered[index - offset];
            let selected = list.selected == Some((model.provider.as_str(), model.id.as_str()))
                && !automatic.is_some_and(|(_, _, s)| s);
            let name = if model.name.is_empty() { &model.id } else { &model.name };
            let prices = format!("{} / {}", price(model.input_cost), price(model.output_cost));
            let metadata = format!("{} context · {}", crate::context::number(model.context_window as f64), crate::accounts::display_provider(&model.provider));
            let response = menu_row(ui, ids[index], 32.0, selected, list.enabled, &format!("{name}, {metadata}, {prices} per million tokens"));
            let rect = response.rect;
            let price_width = ui.painter().layout_no_wrap(prices.clone(), FontId::proportional(10.0), colors.muted).size().x.min(rect.width() * 0.45);
            let text_right = rect.right() - price_width - MODEL_PRICE_RIGHT_INSET - 6.0;
            icons::draw_provider(
                ui,
                egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 14.0, rect.center().y),
                    Vec2::splat(14.0),
                ),
                &model.provider,
            );
            paint_text(ui, name, egui::Rect::from_min_max(rect.min + egui::vec2(26.0, 0.0), egui::pos2(text_right, rect.top() + 17.0)), 11.5, text_color);
            paint_text(ui, &metadata, egui::Rect::from_min_max(rect.min + egui::vec2(26.0, 17.0), egui::pos2(text_right, rect.bottom())), 9.0, colors.dim);
            let price_rect = egui::Rect::from_min_max(egui::pos2(rect.right() - price_width - MODEL_PRICE_RIGHT_INSET, rect.top()), egui::pos2(rect.right() - MODEL_PRICE_RIGHT_INSET, rect.bottom()));
            paint_text(ui, &prices, price_rect, 10.0, colors.muted);
            #[cfg(test)]
            ctx.data_mut(|data| data.insert_temp(egui::Id::new(("model-price-column", index - offset)), price_rect));
            if selected { check(ui, rect.right_center() - egui::vec2(9.0, 0.0)); }
            if target == Some(index) { response.scroll_to_me(Some(Align::Center)); }
            if response.on_hover_text(format!("{name}\n{metadata}\n{prices} input / output per 1 million tokens")).clicked() && list.enabled {
                pick = Some(Pick::Model { provider: model.provider.clone(), id: model.id.clone() });
            }
        }
    });
    pick
}

/// Arrow keys move through real supported entries. Tab/Enter/Space remain native widget actions.
fn navigate(ui: &mut egui::Ui, ids: &[egui::Id], selected: usize, enabled: bool) -> Option<usize> {
    if ids.is_empty()
        || ui.is_sizing_pass()
        || !enabled
        || !prompt_ui::keyboard_available(ui)
        || egui::Popup::is_any_open(ui.ctx())
    {
        return None;
    }
    let current = ui
        .memory(|m| m.focused())
        .and_then(|id| ids.iter().position(|i| *i == id));
    let target = ui.input_mut(|i| {
        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
            Some(
                current
                    .map(|n| (n + 1).min(ids.len() - 1))
                    .unwrap_or(selected),
            )
        } else if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
            Some(current.map(|n| n.saturating_sub(1)).unwrap_or(selected))
        } else {
            None
        }
    });
    if let Some(target) = target {
        ui.memory_mut(|m| {
            m.request_focus_with_filter(
                ids[target],
                egui::EventFilter {
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
    }
    if let Some(id) = ui.memory(|m| m.focused()).filter(|id| ids.contains(id)) {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
    }
    target
}

impl Pui {
    pub(super) fn model_change_blocked(&self) -> Option<&'static str> {
        if self.demo {
            Some("Design preview only. Start a real chat to change models.")
        } else if !self.connected || (!self.is_mirror() && self.client.is_none()) {
            Some("Connect to Pi before changing models.")
        } else if self.loading_chat || self.resource_reload.pending() {
            Some("Wait for this chat to finish loading.")
        } else if self.state.compacting {
            Some("Wait for context compaction to finish before changing models.")
        } else if self.history_pending.is_some() || self.restoring_code() {
            Some("Wait for history navigation to finish before changing models.")
        } else {
            None
        }
    }

    pub(super) fn close_model_popovers(&mut self) {
        self.model_picker_open = false;
        self.effort_picker_open = false;
    }

    // Handle Escape before inline approvals, composer submission, or the global Stop shortcut.
    pub(super) fn model_picker_escape(&mut self, ctx: &egui::Context) -> bool {
        if self.model_picker_open || self.effort_picker_open {
            ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
            });
        }
        if !(self.model_picker_open || self.effort_picker_open)
            || ctx.memory(|m| m.top_modal_layer().is_some())
            || egui::Popup::is_any_open(ctx)
        {
            return false;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            let id = if self.effort_picker_open {
                "composer-effort"
            } else {
                "composer-model"
            };
            self.close_model_popovers();
            ctx.memory_mut(|m| m.request_focus(egui::Id::new(id)));
            return true;
        }
        false
    }

    fn composer_choice(
        &self,
        ui: &mut egui::Ui,
        label: &str,
        id: &str,
        width: f32,
        icon: Option<Icon>,
        open: bool,
    ) -> egui::Response {
        let colors = theme::palette(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::hover());
        let response = ui.interact(rect, egui::Id::new(id), Sense::click());
        if open || response.hovered() || response.has_focus() {
            ui.painter().rect_filled(rect, 5, colors.hover);
        }
        let mut text = rect.shrink2(egui::vec2(4.0, 0.0));
        if let Some(icon) = icon {
            icons::draw(
                ui,
                egui::Rect::from_center_size(
                    rect.left_center() + egui::vec2(10.0, 0.0),
                    Vec2::splat(14.0),
                ),
                icon,
                if self.ultracode {
                    colors.accent
                } else {
                    colors.muted
                },
            );
            text.min.x += 18.0;
        } else {
            let hint = if !self.state.provider.is_empty() {
                self.state.provider.as_str()
            } else {
                self.state.model.as_str()
            };
            if icons::has_provider_logo(hint) {
                icons::draw_provider(
                    ui,
                    egui::Rect::from_center_size(
                        rect.left_center() + egui::vec2(10.0, 0.0),
                        Vec2::splat(14.0),
                    ),
                    hint,
                );
                text.min.x += 18.0;
            }
        }
        text.max.x -= 14.0;
        paint_text(ui, label, text, 11.0, colors.muted);
        icons::chevron(
            ui,
            egui::Rect::from_center_size(
                rect.right_center() - egui::vec2(8.0, 0.0),
                Vec2::splat(10.0),
            ),
            false,
            colors.dim,
        );
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                5,
                Stroke::new(1.0, colors.accent),
                egui::StrokeKind::Inside,
            );
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!(
                    "{}: {label}",
                    if icon.is_some() {
                        "Thinking effort"
                    } else {
                        "Model"
                    }
                ),
            )
        });
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    pub(super) fn model_button(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        let effort = thinking_label(&self.state.thinking);
        let effort_width = ui
            .painter()
            .layout_no_wrap(effort, FontId::proportional(11.0), colors.muted)
            .size()
            .x
            + 40.0;
        let label = model_label(&self.state);
        let hint = if !self.state.provider.is_empty() {
            self.state.provider.as_str()
        } else {
            self.state.model.as_str()
        };
        let logo_pad = if icons::has_provider_logo(hint) {
            18.0
        } else {
            0.0
        };
        let natural = ui
            .painter()
            .layout_no_wrap(label.clone(), FontId::proportional(11.0), colors.muted)
            .size()
            .x
            + 22.0
            + logo_pad;
        let width = natural.min((ui.available_width() - effort_width - 170.0).max(45.0));
        let response = self
            .composer_choice(
                ui,
                &label,
                "composer-model",
                width,
                None,
                self.model_picker_open,
            )
            .on_hover_text(format!(
                "{label}\nChoose the model for subsequent requests."
            ));
        if response.clicked() {
            if response.has_focus() {
                ui.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Space);
                });
            }
            self.model_picker_open = !self.model_picker_open;
            self.effort_picker_open = false;
            if self.model_picker_open {
                self.model_query.clear();
                self.model_picker_focus = true;
            }
        }
        self.model_anchor = Some((response.rect, ui.layer_id()));
    }

    pub(super) fn effort_button(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        let label = thinking_label(&self.state.thinking);
        let width = ui
            .painter()
            .layout_no_wrap(label.clone(), FontId::proportional(11.0), colors.muted)
            .size()
            .x
            + 40.0;
        let response = self.composer_choice(ui, &label, "composer-effort", width, Some(Icon::Sliders), self.effort_picker_open)
            .on_hover_text(format!("Thinking effort: {label}{}\nApplies to subsequent model requests; the response in progress is unchanged.", if self.ultracode { " · Ultracode on" } else { "" }));
        if response.clicked() {
            if response.has_focus() {
                ui.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Space);
                });
            }
            self.effort_picker_open = !self.effort_picker_open;
            self.model_picker_open = false;
            self.effort_picker_focus = self.effort_picker_open;
        }
        self.effort_anchor = Some((response.rect, ui.layer_id()));
    }

    pub(super) fn model_picker(&mut self, ctx: &egui::Context) {
        if self.model_picker_escape(ctx) {
            return;
        }
        if self.model_picker_open {
            self.model_menu(ctx);
        }
        if self.effort_picker_open {
            self.effort_menu(ctx);
        }
    }

    fn model_menu(&mut self, ctx: &egui::Context) {
        let Some((anchor, layer)) = self.model_anchor else {
            self.model_picker_open = false;
            return;
        };
        let colors = theme::palette(ctx);
        let blocked = self.model_change_blocked();
        let width = 320.0_f32.min((ctx.screen_rect().width() - 16.0).max(120.0));
        let mut open = true;
        let mut selection = None;
        // The provider ComboBox is its own popup/layer. Its selection click
        // belongs to that child, not an outside click dismissing this parent.
        // Capture before rendering: selecting a provider closes the child.
        let close_behavior = if egui::Popup::is_any_open(ctx) {
            egui::PopupCloseBehavior::IgnoreClicks
        } else {
            egui::PopupCloseBehavior::CloseOnClickOutside
        };
        self.settings_view.providers.ensure_loaded(self.demo);
        egui::Popup::new(egui::Id::new("model-picker-dropdown"), ctx.clone(), anchor, layer)
            .open_bool(&mut open).align(egui::emath::RectAlign::TOP_START).align_alternatives(&[])
            .close_behavior(close_behavior).width(width)
            .frame(prompt_ui::popup(ctx).inner_margin(6)).gap(6.0).show(|ui| {
                theme::menu_controls(ui);
                ui.set_width(width - 12.0);
                let configured_models = self
                    .state
                    .models
                    .iter()
                    .filter(|m| {
                        self.settings_view
                            .providers
                            .is_provider_configured(&m.provider)
                    })
                    .collect::<Vec<_>>();
                let visible_models = configured_models
                    .iter()
                    .copied()
                    .filter(|m| {
                        !self
                            .prefs
                            .hidden_models
                            .contains(&format!("{}/{}", m.provider, m.id))
                    })
                    .collect::<Vec<_>>();
                let empty = if self.state.models.is_empty() {
                    "Pi has not returned available models yet."
                } else if configured_models.is_empty() {
                    "No configured providers. Add one in Settings → Providers."
                } else {
                    "No matching models."
                };
                if !self.model_provider.is_empty()
                    && !self
                        .settings_view
                        .providers
                        .is_provider_configured(&self.model_provider)
                {
                    self.model_provider.clear();
                }
                let mut list_state = ListState {
                    query: std::mem::take(&mut self.model_query),
                    provider: std::mem::take(&mut self.model_provider),
                    focus: self.model_picker_focus,
                };
                let (cached, error) = (self.state.models_cached, self.state.error.clone());
                let pick = model_list(
                    ui,
                    &mut list_state,
                    ModelList {
                        salt: "composer",
                        models: &visible_models,
                        selected: Some((self.state.provider.as_str(), self.state.model.as_str())),
                        enabled: blocked.is_none(),
                        automatic: None,
                    },
                    |ui| {
                        if cached { ui.add(egui::Label::new(theme::small(ui, if error.is_some() { "Cached models · live refresh unavailable" } else { "Cached models · refreshing from Pi…" })).wrap()); }
                        if let Some(reason) = blocked { ui.add(egui::Label::new(theme::small(ui, reason)).wrap()); }
                        if let Some(error) = &error { ui.add(egui::Label::new(RichText::new(error).size(11.0).color(colors.red)).wrap()); }
                    },
                    // Reserve a useful viewport even for a short catalog, bounded
                    // by the space above the composer anchor.
                    |header| (anchor.top() - ctx.screen_rect().top() - header).clamp(0.0, 226.0),
                    empty,
                );
                self.model_query = list_state.query;
                self.model_provider = list_state.provider;
                self.model_picker_focus = list_state.focus;
                if let Some(Pick::Model { provider, id }) = pick { selection = Some((provider, id)); }
            });
        self.model_picker_open = open;
        if let Some((provider, id)) = selection {
            self.send(json!({"type":"set_model","provider":provider,"modelId":id}));
            self.model_picker_open = false;
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("composer-model")));
        }
    }

    fn effort_menu(&mut self, ctx: &egui::Context) {
        let Some((anchor, layer)) = self.effort_anchor else {
            self.effort_picker_open = false;
            return;
        };
        let colors = theme::palette(ctx);
        let blocked = self.model_change_blocked();
        let width = 240.0_f32.min((ctx.screen_rect().width() - 16.0).max(120.0));
        let mut open = true;
        let mut effort = None;
        let mut ultra = None;
        egui::Popup::new(
            egui::Id::new("effort-picker-dropdown"),
            ctx.clone(),
            anchor,
            layer,
        )
        .open_bool(&mut open)
        .align(egui::emath::RectAlign::TOP_START)
        .align_alternatives(&[])
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(width)
        .frame(prompt_ui::popup(ctx).inner_margin(6))
        .gap(6.0)
        .show(|ui| {
            theme::menu_controls(ui);
            ui.set_width(width - 12.0);
            egui::ScrollArea::vertical()
                .id_salt("effort-options")
                .max_height((anchor.top() - 18.0).max(72.0))
                .min_scrolled_height(
                    ((self.state.levels.len() as f32 * 28.0) + 140.0)
                        .min((anchor.top() - 18.0).max(72.0)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Thinking effort").size(11.0));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&self.state.model_name)
                                        .size(10.0)
                                        .color(colors.dim),
                                )
                                .truncate(),
                            );
                        });
                    });
                    if let Some(reason) = blocked {
                        ui.add(egui::Label::new(theme::small(ui, reason)).wrap());
                    }
                    if self.state.levels.is_empty() {
                        ui.add(
                            egui::Label::new(theme::small(ui, "Loading model-supported levels…"))
                                .wrap(),
                        );
                    }
                    let ids = self
                        .state
                        .levels
                        .iter()
                        .map(|level| ui.id().with(("effort", level)))
                        .collect::<Vec<_>>();
                    let selected = self
                        .state
                        .levels
                        .iter()
                        .position(|level| *level == self.state.thinking)
                        .unwrap_or(0);
                    if self.effort_picker_focus && !ui.is_sizing_pass() {
                        if let Some(id) = ids.get(selected).filter(|_| blocked.is_none()) {
                            ui.memory_mut(|m| {
                                m.request_focus_with_filter(
                                    *id,
                                    egui::EventFilter {
                                        vertical_arrows: true,
                                        ..Default::default()
                                    },
                                )
                            });
                        }
                        self.effort_picker_focus = false;
                    }
                    let _ = navigate(ui, &ids, selected, blocked.is_none());
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (index, level) in self.state.levels.iter().enumerate() {
                        let label = thinking_label(level);
                        let response = menu_row(
                            ui,
                            ids[index],
                            28.0,
                            *level == self.state.thinking,
                            blocked.is_none(),
                            &label,
                        );
                        let mut text = response.rect.shrink2(egui::vec2(7.0, 0.0));
                        text.max.x -= 17.0;
                        paint_text(
                            ui,
                            &label,
                            text,
                            11.5,
                            if blocked.is_none() {
                                colors.text
                            } else {
                                colors.dim
                            },
                        );
                        if *level == self.state.thinking {
                            check(ui, response.rect.right_center() - egui::vec2(11.0, 0.0));
                        }
                        if response.clicked() && blocked.is_none() {
                            effort = Some(level.clone());
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    let mut enabled = self.ultracode;
                    let can_toggle = blocked.is_none()
                        && !self.readonly_worker()
                        && self.ultracode_ready
                        && self.state.ready();
                    ui.add_enabled_ui(can_toggle, |ui| {
                        prompt_ui::compact_toggle_row(ui, &mut enabled, "Ultracode");
                    })
                    .response
                    .on_disabled_hover_text(if self.readonly_worker() {
                        "Read-only workers cannot enable Ultracode."
                    } else if !self.ultracode_ready {
                        "Waiting for Pi's Pui adapter…"
                    } else {
                        "Change Ultracode when this chat is idle."
                    });
                    if enabled != self.ultracode {
                        ultra = Some(enabled);
                    }
                    if self.ultracode {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Max parallel workers")
                                    .size(10.0)
                                    .color(colors.muted),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add_enabled(
                                    !self.readonly_worker(),
                                    egui::DragValue::new(&mut self.prefs.agent_concurrency)
                                        .range(1..=16)
                                        .speed(1.0),
                                );
                            });
                        });
                    }
                });
        });
        self.effort_picker_open = open;
        if let Some(level) = effort {
            self.send(json!({"type":"set_thinking_level","level":level}));
            self.effort_picker_open = false;
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("composer-effort")));
        }
        if let Some(enabled) = ultra {
            self.ensure_agent();
            self.send(json!({"type":"prompt","message":if enabled { "/pui-ultracode on" } else { "/pui-ultracode off" }}));
        }
    }
}
