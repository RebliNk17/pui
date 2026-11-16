//! One keyboard focus target per virtual table, never one widget per hidden cell.
use super::*;
mod row_drag;

pub(super) struct Grid {
    identity: u64,
    pub active: (usize, usize), // original zero-based record and column, including a real header
    position: usize,            // projected visible position, not a source record number
    anchor: Option<(usize, usize)>, // applied position/column; cleared on projection/header/pointer
    pub(super) range_notice: Option<&'static str>,
    reveal: bool,
    viewport: Vec2,
    offset: Vec2,
    changed: bool,
    header_dirty: bool,
    menu: Option<(egui::Id, u64)>,
    row_drag: Option<(std::num::NonZeroUsize, u64)>, // applied anchor+1 and last visible input pass
}

impl Default for Grid {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            identity: NEXT.fetch_add(1, Ordering::Relaxed),
            active: (0, 0),
            position: 0,
            anchor: None,
            range_notice: None,
            reveal: false,
            viewport: Vec2::ZERO,
            offset: Vec2::ZERO,
            changed: false,
            header_dirty: false,
            menu: None,
            row_drag: None,
        }
    }
}
fn clear_selection(ctx: &egui::Context) {
    let mut state = egui::text_selection::LabelSelectionState::load(ctx);
    state.clear_selection();
    state.store(ctx);
}
impl Grid {
    pub(super) fn selection(&self) -> Option<((usize, usize), (usize, usize))> {
        self.anchor.map(|(row, column)| {
            (
                (row.min(self.position), column.min(self.active.1)),
                (row.max(self.position), column.max(self.active.1)),
            )
        })
    }
    pub(super) fn paint_selection(
        &self,
        ui: &egui::Ui,
        rect: egui::Rect,
        position: usize,
        column: usize,
    ) {
        if let Some((first, last)) = self.selection()
            && position >= first.0
            && position <= last.0
            && column >= first.1
            && column <= last.1
        {
            ui.painter()
                .rect_filled(rect.shrink(1.0), 2, theme::palette(ui.ctx()).accent_bg);
        }
    }
    pub(super) fn current_record(&self, rows: projection::Rows<'_>) -> Option<usize> {
        // The saved original record must still occupy this exact applied-view
        // position. Never copy a hidden/removed record after projection changes.
        (rows.record(self.position) == Some(self.active.0)).then_some(self.active.0)
    }
    pub(super) fn id(&self, path: &Path) -> egui::Id {
        egui::Id::new(("delimited-grid", path, self.identity))
    }
    pub fn focus(&mut self, ctx: &egui::Context, path: &Path) {
        ctx.memory_mut(|memory| {
            memory.request_focus_with_filter(
                self.id(path),
                egui::EventFilter {
                    escape: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
        self.reveal = true;
        ctx.request_repaint(); // one follow-up installs the focus key filter
        clear_selection(ctx);
    }
    pub(super) fn extend_to(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        record: usize,
        column: usize,
        position: usize,
    ) {
        let anchor = self.anchor.unwrap_or((self.position, self.active.1));
        self.select(ctx, path, record, column, position);
        self.anchor = Some(anchor);
        clear_selection(ctx); // an explicit rectangle is not a label substring
    }
    pub fn select(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        record: usize,
        column: usize,
        position: usize,
    ) {
        self.active = (record, column);
        self.anchor = None;
        self.position = position;
        self.changed = true;
        // Do not clear a pointer-selected substring when merely clicking its label.
        ctx.memory_mut(|memory| {
            memory.request_focus_with_filter(
                self.id(path),
                egui::EventFilter {
                    escape: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
        ctx.request_repaint();
    }
    fn bounded(&mut self, index: &Index, rows: projection::Rows<'_>) {
        if rows.record(self.position) != Some(self.active.0) {
            self.position = rows.position(self.active.0).unwrap_or(0);
        }
        self.position = self.position.min(rows.len().saturating_sub(1));
        self.active.0 = rows.record(self.position).unwrap_or(0);
        self.active.1 = self.active.1.min(index.columns.saturating_sub(1));
    }
    pub fn projection_changed(&mut self, ctx: &egui::Context, rows: projection::Rows<'_>) -> bool {
        self.anchor = None;
        self.row_drag = None;
        self.clear_menu(ctx);
        clear_selection(ctx);
        let position = rows.position(self.active.0);
        self.position = position.unwrap_or(0);
        self.active.0 = rows.record(self.position).unwrap_or(0);
        self.reveal = true;
        self.changed = true;
        position.is_none()
    }
    pub fn inputs(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        index: &Index,
        source: &str,
        enabled: bool,
        rows: projection::Rows<'_>,
    ) {
        self.row_drag_inputs(ctx, enabled);
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
        if ctx.memory(|memory| memory.top_modal_layer().is_some()) || egui::Popup::is_any_open(ctx)
        {
            return;
        }
        let id = self.id(path);
        let pointer = ctx.input(|input| {
            (input.pointer.primary_pressed() || input.pointer.primary_released())
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        let pointer_owns = ctx.read_response(id).is_some_and(|response| {
            response.enabled()
                && pointer.is_some_and(|pos| {
                    response.interact_rect.contains(pos)
                        && ctx.layer_id_at(pos) == Some(response.layer_id)
                })
        });
        let owns = ctx.memory(|memory| {
            memory.has_focus(id) || memory.focused().is_none() && memory.focus_lost_by_escape(id)
        });
        if !owns && !pointer_owns {
            return;
        }
        if pointer_owns {
            ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                input.consume_key(egui::Modifiers::NONE, egui::Key::Space);
                input
                    .events
                    .retain(|event| !matches!(event, egui::Event::Text(_) | egui::Event::Paste(_)));
            });
        }
        if !enabled {
            // A newly-arrived inline approval must not reinterpret keys typed
            // into the previously focused table as an approval decision.
            ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
                input.events.retain(|event| match event {
                    egui::Event::Text(_) | egui::Event::Paste(_) => false,
                    egui::Event::Key { key, .. } => *key == egui::Key::Tab,
                    _ => true,
                });
            });
            return;
        }
        if !owns {
            return;
        }
        if rows.len() == 0 {
            let escape = ctx.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Copy
                            | egui::Event::Key {
                                key: egui::Key::Enter | egui::Key::Backspace,
                                modifiers: egui::Modifiers { command: true, .. },
                                ..
                            }
                    )
                });
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
            });
            if escape {
                ctx.memory_mut(|memory| memory.surrender_focus(self.id(path)));
            }
            return;
        }
        self.bounded(index, rows);
        let has_selection = egui::text_selection::LabelSelectionState::load(ctx).has_selection();
        let mut moved = false;
        let mut copy = None;
        let mut copy_range = None;
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
                        if self.anchor.take().is_some() {
                            clear_pointer_selection = true;
                        } else {
                            escape = true;
                        }
                        return false;
                    }
                    if *key == egui::Key::Space
                        && !has_selection
                        && (*modifiers == egui::Modifiers::SHIFT
                            || *modifiers == egui::Modifiers::CTRL)
                    {
                        if *modifiers == egui::Modifiers::SHIFT {
                            self.anchor = Some((self.position, 0));
                            self.active.1 = index.columns - 1;
                        } else {
                            self.anchor = Some((0, self.active.1));
                            self.position = rows.len() - 1;
                            self.active.0 = rows.record(self.position).unwrap();
                        }
                        moved = true;
                        clear_pointer_selection = true;
                        return false;
                    }
                    if *key == egui::Key::A
                        && whole
                        && !modifiers.shift
                        && !modifiers.alt
                        && !has_selection
                    {
                        self.anchor = Some((0, 0));
                        self.position = rows.len() - 1;
                        self.active = (rows.record(self.position).unwrap(), index.columns - 1);
                        moved = true;
                        clear_pointer_selection = true;
                        return false;
                    }
                    if *key == egui::Key::C
                        && modifiers.command
                        && !modifiers.shift
                        && !modifiers.alt
                    {
                        if !has_selection || clear_pointer_selection {
                            self.bounded(index, rows);
                            copy_range = self.selection();
                            copy = Some(self.active);
                            return false;
                        }
                        return true;
                    }
                    if modifiers.alt || (whole && !matches!(key, egui::Key::Home | egui::Key::End))
                    {
                        return true;
                    }
                    if !matches!(
                        key,
                        egui::Key::ArrowUp
                            | egui::Key::ArrowDown
                            | egui::Key::ArrowLeft
                            | egui::Key::ArrowRight
                            | egui::Key::Home
                            | egui::Key::End
                            | egui::Key::PageUp
                            | egui::Key::PageDown
                    ) {
                        return true;
                    }
                    if modifiers.shift {
                        self.anchor.get_or_insert((self.position, self.active.1));
                    } else {
                        self.anchor = None;
                    }
                    let row = &mut self.position;
                    let column = &mut self.active.1;
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
                                *row = rows.len() - 1;
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
                    self.position = self.position.min(rows.len().saturating_sub(1));
                    self.active.0 = rows.record(self.position).unwrap_or(0);
                    self.active.1 = self.active.1.min(index.columns.saturating_sub(1));
                    moved = true;
                    clear_pointer_selection = true;
                    false
                }
                egui::Event::Text(text) if clear_pointer_selection && text == " " => false,
                egui::Event::Copy if !has_selection || clear_pointer_selection => {
                    self.bounded(index, rows);
                    copy_range = self.selection();
                    copy = Some(self.active);
                    false
                }
                _ => true,
            })
        });
        self.bounded(index, rows);
        if moved {
            self.reveal = true;
            self.changed = true;
            clear_selection(ctx);
        }
        if let Some((record, column)) = copy {
            if let Some((first, last)) = copy_range {
                // Freeze event-time bounds, even if later navigation clears the range.
                match super::export::rectangle::serialize(source, index, rows, first, last) {
                    Ok(text) => {
                        ctx.copy_text(text);
                        self.range_notice =
                            Some("Copied selected cells as TSV. Formula values unchanged.");
                    }
                    Err(error) => self.range_notice = Some(error),
                }
            } else {
                ctx.copy_text(index.value(source, record, column).into());
            }
        }
        if escape {
            ctx.memory_mut(|memory| memory.surrender_focus(self.id(path)));
        }
    }
    pub fn scroll(
        &mut self,
        mut scroll: egui::ScrollArea,
        header: bool,
        ctx: &egui::Context,
        path: &Path,
        records: usize,
        columns: usize,
    ) -> egui::ScrollArea {
        if self.row_drag.is_some() {
            scroll = scroll.scroll_source(egui::scroll_area::ScrollSource {
                drag: false,
                ..Default::default()
            });
        }
        // Initial layout has no measured viewport yet. Retain the one-shot
        // reveal until the next pass instead of scrolling past its target.
        if self.viewport.x <= 0.0 || self.viewport.y <= 0.0 {
            return scroll;
        }
        if let Some(offset) = self.row_drag_scroll(ctx, path, header, records, columns) {
            self.reveal = false;
            return scroll.scroll_offset(offset);
        }
        if std::mem::take(&mut self.reveal) {
            let (_, column) = self.active;
            let x = NUMBER_WIDTH + column as f32 * COLUMN_WIDTH;
            let y = (self.position + 1 - usize::from(header)) as f32 * ROW_HEIGHT;
            let mut offset = self.offset;
            if x < offset.x {
                offset.x = x;
            }
            if x + COLUMN_WIDTH > offset.x + self.viewport.x {
                offset.x = (x + COLUMN_WIDTH - self.viewport.x).clamp(0.0, x);
            }
            if !(header && self.position == 0) {
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
        rows: projection::Rows<'_>,
    ) {
        if self
            .menu
            .is_some_and(|(_, pass)| pass != ui.ctx().cumulative_pass_nr())
        {
            self.clear_menu(ui.ctx()); // virtualized owner scrolled out/disappeared
        }
        self.finish_row_drag(ui, path, rect, offset, index, header, rows);
        self.bounded(index, rows);
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
        let mut response = ui.interact(rect, self.id(path), Sense::focusable_noninteractive()).on_hover_text("Alt/Option-drag cells to select a rectangle. Plain drag selects text; drag record numbers for whole rows. Copy uses the applied view; no file edits.");
        if response.gained_focus() {
            self.reveal = true;
            ui.ctx().request_repaint();
        }
        if response.has_focus() {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter_current(
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
                if rows.len() == 0 {
                    "Read-only table · no matching records".into()
                } else {
                    self.description(index, source, header)
                },
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
    pub(super) fn clear_menu(&mut self, ctx: &egui::Context) {
        if let Some((menu, _)) = self.menu.take() {
            egui::Popup::close_id(ctx, menu);
        }
    }
    pub fn header_changed(&mut self) {
        self.row_drag = None;
        self.reveal = true;
        self.header_dirty = true;
    }
    pub fn reconcile_header(&mut self, ctx: &egui::Context, rows: projection::Rows<'_>) -> bool {
        std::mem::take(&mut self.header_dirty) && self.projection_changed(ctx, rows)
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
mod axis_tests;
#[cfg(test)]
mod range_tests;
#[cfg(test)]
mod safety_tests;
#[cfg(test)]
mod select_all_tests;
#[cfg(test)]
mod shift_click_tests;
#[cfg(test)]
pub(super) mod tests;
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
        self.projection.close_menu(ctx);
        self.projection.hidden();
        self.export.hidden();
        self.toolbar.hidden(ctx);
    }

    pub(in crate::app::files) fn clear_source_grid_menu(&mut self, ctx: &egui::Context) {
        self.grid.clear_menu(ctx);
        self.projection.close_menu(ctx);
    }

    pub(in crate::app::files) fn toolbar_inputs(
        &mut self,
        ctx: &egui::Context,
        enabled: bool,
    ) -> bool {
        let toolbar_input = self.toolbar.inputs(ctx);
        let export_input = self.export.inputs(ctx);
        let projection_input = self
            .projection
            .control_inputs(ctx, enabled || export_input || toolbar_input);
        toolbar_input || export_input || projection_input
    }

    pub(in crate::app::files) fn grid_inputs(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        enabled: bool,
    ) {
        if let Ok(index) = &self.index {
            self.projection
                .reconcile_header(ctx, &mut self.grid, index.records.len(), self.header);
        }
        let reverse_grid = ctx.memory(|memory| {
            memory.focused().is_none() && memory.focus_lost_by_escape(self.grid.id(path))
        });
        if reverse_grid
            && !egui::Popup::is_any_open(ctx)
            && ctx.memory(|memory| memory.top_modal_layer().is_none())
        {
            ctx.memory_mut(|memory| {
                memory.request_focus_with_filter(
                    self.grid.id(path),
                    egui::EventFilter {
                        escape: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..Default::default()
                    },
                )
            });
        }
        if self.toolbar_inputs(ctx, enabled) {
            return;
        }
        if let Ok(index) = &self.index {
            self.grid.inputs(
                ctx,
                path,
                index,
                &self.source,
                enabled,
                self.projection.rows(index.records.len(), self.header),
            );
        }
    }
}
