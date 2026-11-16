//! Data-first table chrome: one toolbar, contextual actions, one quiet footer.
use super::*;

#[derive(Default)]
pub(super) struct Toolbar {
    controls: [Option<egui::Id>; 5],
    popup: Option<(egui::Id, egui::Id)>, // popup and exact trigger
    keyboard_pass: Option<u64>,
}

pub(super) fn button(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    ui.add(
        egui::Button::new(RichText::new(label).size(12.0).color(if selected {
            colors.accent
        } else {
            colors.muted
        }))
        .fill(if selected {
            colors.accent_bg
        } else {
            Color32::TRANSPARENT
        })
        .stroke(Stroke::NONE)
        .corner_radius(6)
        .min_size(Vec2::new(0.0, 28.0)),
    )
}

impl Toolbar {
    pub(super) fn hidden(&mut self, ctx: &egui::Context) {
        if let Some((popup, _)) = self.popup.take() {
            egui::Popup::close_id(ctx, popup);
        }
        self.controls = [None; 5];
        self.keyboard_pass = None;
    }
    pub(super) fn owns_keyboard(&self, ctx: &egui::Context) -> bool {
        self.keyboard_pass == Some(ctx.cumulative_pass_nr())
    }
    pub(super) fn inputs(&mut self, ctx: &egui::Context) -> bool {
        if ctx.memory(|memory| memory.top_modal_layer().is_some()) {
            return false;
        }
        if let Some((popup, trigger)) = self.popup {
            if egui::Popup::is_id_open(ctx, popup) {
                self.keyboard_pass = Some(ctx.cumulative_pass_nr());
                let escape = ctx.input_mut(|input| {
                    input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                    input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
                    input.consume_key(egui::Modifiers::CTRL, egui::Key::Enter);
                    input.consume_key(egui::Modifiers::CTRL, egui::Key::Backspace);
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                });
                if escape {
                    egui::Popup::close_id(ctx, popup);
                    self.popup = None;
                    ctx.memory_mut(|memory| memory.request_focus(trigger));
                }
                return true;
            }
            self.popup = None;
        }
        if egui::Popup::is_any_open(ctx) {
            return false;
        }
        let focus = ctx.memory(|memory| memory.focused());
        let pointer = ctx.input(|input| {
            (input.pointer.primary_pressed() || input.pointer.primary_released())
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        let owns = self.controls.iter().flatten().any(|id| {
            ctx.read_response(*id).is_some_and(|response| {
                response.enabled()
                    && (focus == Some(*id)
                        || focus.is_none() && ctx.memory(|memory| memory.focus_lost_by_escape(*id))
                        || pointer.is_some_and(|pos| {
                            response.interact_rect.contains(pos)
                                && ctx.layer_id_at(pos) == Some(response.layer_id)
                        }))
            })
        });
        if owns {
            self.keyboard_pass = Some(ctx.cumulative_pass_nr());
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(event, egui::Event::Key {
                        key: egui::Key::Enter | egui::Key::Backspace, modifiers, ..
                    } if modifiers.command || modifiers.ctrl)
                })
            });
        }
        owns
    }
    fn remember(
        &mut self,
        slot: usize,
        response: egui::Response,
        projection: &mut projection::Projection,
    ) -> egui::Response {
        let response = projection.remember(response);
        self.controls[slot] = Some(response.id);
        if response.enabled()
            && (response.has_focus() || response.clicked() || response.is_pointer_button_down_on())
        {
            self.keyboard_pass = Some(response.ctx.cumulative_pass_nr());
        }
        #[cfg(test)]
        response
            .ctx
            .data_mut(|data| data.insert_temp(egui::Id::new(("table-toolbar", slot)), response.id));
        response
    }
    fn popup<R>(&mut self, response: &egui::Response, content: impl FnOnce(&mut egui::Ui) -> R) {
        let id = response.id.with("table-menu");
        let width = 236.0_f32.min((response.ctx.screen_rect().width() - 32.0).max(120.0));
        let shown = egui::Popup::menu(response)
            .id(id)
            .frame(prompt_ui::popup(&response.ctx))
            .width(width)
            .show(content);
        if shown.is_some() {
            self.popup = Some((id, response.id));
            self.keyboard_pass = Some(response.ctx.cumulative_pass_nr());
        }
    }
    fn own_popup(&self, ctx: &egui::Context) -> bool {
        self.popup
            .is_some_and(|(id, _)| egui::Popup::is_id_open(ctx, id))
    }
    pub(super) fn finish(&self, ctx: &egui::Context) {
        if self.owns_keyboard(ctx)
            && ctx.memory(|memory| memory.top_modal_layer().is_none())
            && (!egui::Popup::is_any_open(ctx) || self.own_popup(ctx))
        {
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Key { .. }
                            | egui::Event::Text(_)
                            | egui::Event::Paste(_)
                            | egui::Event::Ime(_)
                    )
                })
            });
        }
    }
}

#[cfg(test)]
mod tests;

impl Preview {
    pub(in crate::app::files) fn show_view(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        source: &mut bool,
    ) {
        let id = self.grid.id(path).with("presentation");
        ui.push_id(id, |ui| {
            self.projection.begin_controls();
            let colors = theme::palette(ui.ctx());
            #[cfg(test)]
            let toolbar_top = ui.cursor().top();
            let width = ui.available_width();
            let available = prompt_ui::keyboard_available(ui)
                && ui.memory(|memory| memory.top_modal_layer().is_none())
                && (!egui::Popup::is_any_open(ui.ctx()) || self.toolbar.own_popup(ui.ctx()) || self.projection.menu_owns_keyboard(ui.ctx()));
            ui.add_enabled_ui(available, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(2, 2)).show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                ui.spacing_mut().interact_size.y = 26.0;
                ui.spacing_mut().button_padding = Vec2::new(9.0, 4.0);
                ui.horizontal_wrapped(|ui| {
                    egui::Frame::new().fill(colors.surface).corner_radius(7).inner_margin(2).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            for (slot, label, value) in [(0, "Table", false), (1, "Source", true)] {
                                let response = button(ui, label, *source == value);
                                if self.toolbar.remember(slot, response, &mut self.projection).clicked() { *source = value; }
                            }
                        });
                    });
                    if width >= 330.0 { ui.add_space((width - 304.0).max(0.0)); }
                    let response = ui.add_enabled_ui(!*source && self.index.is_ok(), |ui| {
                        button(ui, "Filter", self.projection.expanded())
                    }).inner.on_hover_text("Filter and sort this local view");
                    if self.toolbar.remember(2, response, &mut self.projection).clicked() { self.projection.toggle(); }
                    let response = button(ui, "Copy ▾", false);
                    let response = self.toolbar.remember(3, response, &mut self.projection);
                    let index = self.index.as_ref().ok().map(Arc::clone);
                    self.toolbar.popup(&response, |ui| {
                        theme::menu_controls(ui);
                        ui.label(theme::caption(ui, "COPY"));
                        self.export.show(ui, &self.source, index.as_deref(), self.header, &mut self.projection, &self.grid, *source,path);
                    });
                    let response = super::super::super::webview::chrome::icon_button(ui, Icon::Sliders, "Table options", false);
                    let response = self.toolbar.remember(4, response, &mut self.projection);
                    let mut focus = false;
                    let old_header = self.header;
                    self.toolbar.popup(&response, |ui| {
                        theme::menu_controls(ui);
                        ui.label(theme::caption(ui, "TABLE OPTIONS"));
                        if ui.checkbox(&mut self.header, "First record is header").changed() { ui.close(); }
                        if ui.button("Navigate cells").clicked() { focus = true; ui.close(); }
                        ui.separator();
                        ui.add(egui::Label::new(theme::small(ui, "Read-only. Arrow keys move between cells; ⌘/Ctrl+C copies. Right-click a cell to quote it to chat.")).wrap());
                    });
                    if old_header != self.header { self.grid.header_changed(); }
                    if focus { self.grid.focus(ui.ctx(), path); }
                });
            });
            });
            #[cfg(test)]
            ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("table-toolbar-height"), ui.cursor().top()-toolbar_top));
            ui.add_space(4.0);
            if *source {
                self.show_source(ui, path);
            } else {
                self.show_table(ui, path);
            }
            if self.projection.menu_owns_keyboard(ui.ctx()) {
                self.toolbar.keyboard_pass = Some(ui.ctx().cumulative_pass_nr());
            }
            self.toolbar.finish(ui.ctx());
        });
    }
    pub(super) fn footer(&self, ui: &mut egui::Ui, index: &Index) {
        let rows = self.projection.rows(index.records.len(), self.header);
        let count = rows.len().saturating_sub(usize::from(self.header));
        let filtered = self.projection.is_applied();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(theme::small(
                ui,
                if filtered {
                    format!(
                        "{count} / {} rows",
                        index.records.len().saturating_sub(usize::from(self.header))
                    )
                } else {
                    format!("{count} rows")
                },
            ));
            ui.label(theme::small(ui, format!("· {} columns", index.columns)));
            if let Some((first,last))=self.grid.selection(){ui.label(theme::small(ui,format!("· {}×{} selected",last.0-first.0+1,last.1-first.1+1))).on_hover_text("Shift+arrows/Home/End/Page extends a rectangle in the applied view. Copy exports TSV up to 512 KiB; oversize leaves clipboard unchanged. Missing cells become empty fields; formulas unchanged and may evaluate in spreadsheets. Escape clears selection. Sort/filter/header or pointer selection clears the range.");}
            if let Some(notice) = self.grid.range_notice.or(self.export.notice()) {
                let label = if notice.starts_with("Exported ") { "· exported" } else if notice.starts_with("Writing ") { "· exporting" } else if notice.starts_with("Copied ") {
                    "· copied"
                } else {
                    "· not copied"
                };
                ui.add(egui::Label::new(theme::small(ui, label)).truncate())
                    .on_hover_text(notice);
            }
            if index.ragged {
                ui.label(theme::small(ui, "· uneven rows")).on_hover_text(
                    "Record lengths differ. No values are discarded; missing cells appear blank.",
                );
            }
            if ui.available_width() > 110.0 {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(theme::small(
                        ui,
                        format!("R{} · C{}", self.grid.active.0 + 1, self.grid.active.1 + 1),
                    ))
                    .on_hover_text(self.grid.description(
                        index,
                        &self.source,
                        self.header,
                    ));
                });
            }
        });
    }
}
