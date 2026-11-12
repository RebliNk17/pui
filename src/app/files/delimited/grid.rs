//! One keyboard focus target per virtual table, never one widget per hidden cell.
use super::*;

pub(super) struct Grid {
    identity: u64,
    pub active: (usize, usize), // original zero-based record and column, including a real header
    reveal: bool,
    viewport: Vec2,
    offset: Vec2,
    changed: bool,
    menu: Option<(egui::Id, u64)>,
}

impl Default for Grid {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            identity: NEXT.fetch_add(1, Ordering::Relaxed),
            active: (0, 0),
            reveal: false,
            viewport: Vec2::ZERO,
            offset: Vec2::ZERO,
            changed: false,
            menu: None,
        }
    }
}
fn clear_selection(ctx: &egui::Context) {
    let mut state = egui::text_selection::LabelSelectionState::load(ctx);
    state.clear_selection();
    state.store(ctx);
}
impl Grid {
    fn id(&self, path: &Path) -> egui::Id {
        egui::Id::new(("delimited-grid", path, self.identity))
    }
    pub fn focus(&mut self, ctx: &egui::Context, path: &Path) {
        ctx.memory_mut(|memory| memory.request_focus(self.id(path)));
        self.reveal = true;
        ctx.request_repaint(); // one follow-up installs the focus key filter
        clear_selection(ctx);
    }
    pub fn select(&mut self, ctx: &egui::Context, path: &Path, record: usize, column: usize) {
        self.active = (record, column);
        self.changed = true;
        // Do not clear a pointer-selected substring when merely clicking its label.
        ctx.memory_mut(|memory| memory.request_focus(self.id(path)));
        ctx.request_repaint();
    }
    fn bounded(&mut self, index: &Index) {
        self.active.0 = self.active.0.min(index.records.len().saturating_sub(1));
        self.active.1 = self.active.1.min(index.columns.saturating_sub(1));
    }
    pub fn inputs(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        index: &Index,
        source: &str,
        enabled: bool,
    ) {
        if let Some((menu, _)) = self.menu {
            if egui::Popup::is_id_open(ctx, menu)
                && ctx.memory(|memory| memory.top_modal_layer().is_none())
            {
                let escape = ctx.input_mut(|input| {
                    input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                    input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                });
                if escape {
                    egui::Popup::close_id(ctx, menu);
                    self.menu = None;
                }
                return;
            }
            self.menu = None;
        }
        if !ctx.memory(|memory| memory.has_focus(self.id(path)))
            || ctx.memory(|memory| memory.top_modal_layer().is_some())
            || egui::Popup::is_any_open(ctx)
            || index.records.is_empty()
        {
            return;
        }
        if !enabled {
            // A newly-arrived inline approval must not reinterpret keys typed
            // into the previously focused table as an approval decision.
            ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            });
            return;
        }
        self.bounded(index);
        let has_selection = egui::text_selection::LabelSelectionState::load(ctx).has_selection();
        let mut moved = false;
        let mut copy = None;
        let mut clear_pointer_selection = false;
        let mut escape = false;
        // Preserve event order/repeats. Modified approval keys belong to neither
        // the cell nor the approval below it; copying is never a capture/send.
        ctx.input_mut(|input| {
            input.events.retain(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if modifiers.command && matches!(key, egui::Key::Enter | egui::Key::Backspace) {
                        return false;
                    }
                    let whole = modifiers.command || modifiers.ctrl;
                    if *key == egui::Key::Escape && *modifiers == egui::Modifiers::NONE {
                        escape = true;
                        return false;
                    }
                    if *key == egui::Key::C
                        && modifiers.command
                        && !modifiers.shift
                        && !modifiers.alt
                    {
                        if !has_selection || clear_pointer_selection {
                            self.bounded(index);
                            copy = Some(self.active);
                            return false;
                        }
                        return true;
                    }
                    if modifiers.alt
                        || modifiers.shift
                        || (whole && !matches!(key, egui::Key::Home | egui::Key::End))
                    {
                        return true;
                    }
                    let (row, column) = &mut self.active;
                    match key {
                        egui::Key::ArrowUp => *row = row.saturating_sub(1),
                        egui::Key::ArrowDown => *row = row.saturating_add(1),
                        egui::Key::ArrowLeft => *column = column.saturating_sub(1),
                        egui::Key::ArrowRight => *column = column.saturating_add(1),
                        egui::Key::Home => {
                            *column = 0;
                            if whole {
                                *row = 0;
                            }
                        }
                        egui::Key::End => {
                            *column = index.columns - 1;
                            if whole {
                                *row = index.records.len() - 1;
                            }
                        }
                        egui::Key::PageUp => {
                            *row = row.saturating_sub(
                                ((self.viewport.y / ROW_HEIGHT) as usize)
                                    .saturating_sub(1)
                                    .max(1),
                            )
                        }
                        egui::Key::PageDown => {
                            *row = row.saturating_add(
                                ((self.viewport.y / ROW_HEIGHT) as usize)
                                    .saturating_sub(1)
                                    .max(1),
                            )
                        }
                        _ => return true,
                    }
                    self.bounded(index);
                    moved = true;
                    clear_pointer_selection = true;
                    false
                }
                egui::Event::Copy if !has_selection || clear_pointer_selection => {
                    self.bounded(index);
                    copy = Some(self.active);
                    false
                }
                _ => true,
            })
        });
        self.bounded(index);
        if moved {
            self.reveal = true;
            self.changed = true;
            clear_selection(ctx);
        }
        if let Some((record, column)) = copy {
            ctx.copy_text(index.value(source, record, column).into());
        }
        if escape {
            ctx.memory_mut(|memory| memory.surrender_focus(self.id(path)));
        }
    }
    pub fn scroll(&mut self, mut scroll: egui::ScrollArea, header: bool) -> egui::ScrollArea {
        // Initial layout has no measured viewport yet. Retain the one-shot
        // reveal until the next pass instead of scrolling past its target.
        if self.viewport.x <= 0.0 || self.viewport.y <= 0.0 {
            return scroll;
        }
        if std::mem::take(&mut self.reveal) {
            let (record, column) = self.active;
            let x = NUMBER_WIDTH + column as f32 * COLUMN_WIDTH;
            let y = (record + 1 - usize::from(header)) as f32 * ROW_HEIGHT;
            let mut offset = self.offset;
            if x < offset.x {
                offset.x = x;
            }
            if x + COLUMN_WIDTH > offset.x + self.viewport.x {
                offset.x = (x + COLUMN_WIDTH - self.viewport.x).clamp(0.0, x);
            }
            if !(header && record == 0) {
                if y < offset.y + ROW_HEIGHT {
                    offset.y = (y - ROW_HEIGHT).max(0.0);
                }
                if y + ROW_HEIGHT > offset.y + self.viewport.y {
                    offset.y = (y + ROW_HEIGHT - self.viewport.y).max(0.0);
                }
            }
            scroll = scroll.scroll_offset(offset);
        }
        scroll
    }
    #[expect(clippy::too_many_arguments)]
    pub fn finish(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        rect: egui::Rect,
        offset: Vec2,
        index: &Index,
        source: &str,
        header: bool,
    ) {
        if self
            .menu
            .is_some_and(|(_, pass)| pass != ui.ctx().cumulative_pass_nr())
        {
            self.clear_menu(ui.ctx()); // virtualized owner scrolled out/disappeared
        }
        self.bounded(index);
        if (self.viewport - rect.size()).length_sq() > 1.0
            && ui.memory(|memory| memory.has_focus(self.id(path)))
        {
            self.reveal = true;
        }
        self.viewport = rect.size();
        self.offset = offset;
        if self.reveal && rect.is_positive() {
            ui.ctx().request_repaint();
        }
        let mut response = ui.interact(rect, self.id(path), Sense::focusable_noninteractive());
        if response.gained_focus() {
            self.reveal = true;
            ui.ctx().request_repaint();
        }
        if response.has_focus() {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                        ..Default::default()
                    },
                )
            });
        }
        if std::mem::take(&mut self.changed) {
            response.mark_changed();
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                ui.is_enabled(),
                self.description(index, source, header),
            )
        });
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("delimited-grid-focus"), response.id);
            data.insert_temp(egui::Id::new("delimited-grid-viewport"), rect);
            data.insert_temp(
                egui::Id::new("delimited-grid-description"),
                self.description(index, source, header),
            );
        });
    }
    pub fn paint(
        &self,
        ui: &egui::Ui,
        path: &Path,
        rect: egui::Rect,
        record: usize,
        column: usize,
    ) {
        if self.active == (record, column) && ui.memory(|memory| memory.has_focus(self.id(path))) {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                3.0,
                Stroke::new(2.0, theme::palette(ui.ctx()).accent),
                egui::StrokeKind::Inside,
            );
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("delimited-grid-active-rect"),
                    rect.intersect(ui.clip_rect()),
                )
            });
        }
    }
    pub fn menu(&mut self, response: &egui::Response) {
        if response.context_menu_opened() {
            self.menu = Some((
                egui::Popup::default_response_id(response),
                response.ctx.cumulative_pass_nr(),
            ));
        }
    }
    fn clear_menu(&mut self, ctx: &egui::Context) {
        if let Some((menu, _)) = self.menu.take() {
            egui::Popup::close_id(ctx, menu);
        }
    }
    pub fn header_changed(&mut self) {
        self.reveal = true;
    }

    pub fn description(&self, index: &Index, source: &str, header: bool) -> String {
        let (record, column) = self.active;
        let missing = index
            .records
            .get(record)
            .is_none_or(|fields| column >= fields.len());
        let value = index.value(source, record, column);
        let display: String = value
            .chars()
            .take(256)
            .map(|ch| if ch.is_control() { ' ' } else { ch })
            .collect();
        format!(
            "Read-only table · {}record {} of {}, column {} of {} · {}{}",
            if header && record == 0 {
                "header · "
            } else {
                ""
            },
            record + 1,
            index.records.len(),
            column + 1,
            index.columns,
            if missing {
                "missing cell"
            } else if value.is_empty() {
                "empty cell"
            } else {
                &display
            },
            if value.chars().nth(256).is_some() {
                "… (label shortened; Copy cell keeps full value)"
            } else {
                ""
            }
        )
    }
}

#[cfg(test)]
mod safety_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;

impl Preview {
    pub(in crate::app::files) fn demo_grid(ctx: &egui::Context, path: &Path) -> Self {
        let source = "Name,Note,Literal formula,Unicode\n".to_owned()
            + &(1..=200)
                .map(|row| format!("Row {row},Read-only cell {row},=1+2,שלום 世界\n"))
                .collect::<String>();
        let mut preview = Self::new(source, b',');
        preview.grid.active = (7, 1);
        preview.grid.focus(ctx, path);
        preview
    }

    pub(in crate::app::files) fn clear_grid_menu(&mut self, ctx: &egui::Context) {
        self.grid.clear_menu(ctx);
    }

    pub(in crate::app::files) fn grid_inputs(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        enabled: bool,
    ) {
        if let Ok(index) = &self.index {
            self.grid.inputs(ctx, path, index, &self.source, enabled);
        }
    }
}
