//! Explicit, read-only projection: original record IDs only, never cell copies.
use super::*;
use std::sync::atomic::AtomicBool;

const QUERY_CHARS: usize = 128;
// One current map plus one worker/completed result. Fixed reservation at file
// admission avoids racing the file worker's single-writer shared payload lease.
const RESERVED_BYTES: usize =
    2 * MAX_RECORDS * std::mem::size_of::<u32>() + 4 * QUERY_CHARS * 4 + 2048;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Order {
    #[default]
    Original,
    Ascending,
    Descending,
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Sort {
    column: usize,
    order: Order,
}
struct Map {
    records: Box<[u32]>,
    zero: Option<usize>,
}
impl Map {
    fn build(source: &str, index: &Index, query: &str, sort: Sort) -> Self {
        // Reserve exactly the known upper bound; sorting in place uses original
        // record as the tie-breaker, so even descending equal values stay stable.
        let mut records = Vec::with_capacity(index.records.len());
        for record in 0..index.records.len() {
            if query.is_empty()
                || index.cells[index.records[record].clone()]
                    .iter()
                    .any(|cell| cell.value(source).contains(query))
            {
                records.push(record as u32);
            }
        }
        if sort.order != Order::Original {
            records.sort_unstable_by(|a, b| {
                let order = index
                    .value(source, *a as usize, sort.column)
                    .cmp(index.value(source, *b as usize, sort.column));
                let order = if sort.order == Order::Descending {
                    order.reverse()
                } else {
                    order
                };
                order.then_with(|| a.cmp(b))
            });
        }
        let zero = records.iter().position(|record| *record == 0);
        Self {
            records: records.into_boxed_slice(),
            zero,
        }
    }
}

/// Cheap view over the sparse map. A real header remains pinned and keeps its
/// original identity even when it doesn't match the filter; headerless views
/// include record zero in ordinary text sort/filter order. No toggle scan.
#[derive(Clone, Copy)]
pub(super) struct Rows<'a> {
    map: Option<&'a Map>,
    records: usize,
    header: bool,
}
impl Rows<'_> {
    pub fn len(self) -> usize {
        self.map.map_or(self.records, |map| {
            map.records.len() + usize::from(self.header && map.zero.is_none())
        })
    }
    pub fn record(self, position: usize) -> Option<usize> {
        if position >= self.len() {
            return None;
        }
        if self.header && position == 0 {
            return Some(0);
        }
        self.map.map_or(Some(position), |map| {
            let at = position - usize::from(self.header);
            let at = at + usize::from(self.header && map.zero.is_some_and(|zero| zero <= at));
            map.records.get(at).map(|record| *record as usize)
        })
    }
    pub fn position(self, record: usize) -> Option<usize> {
        if self.header && record == 0 {
            return (self.records > 0).then_some(0);
        }
        self.map.map_or_else(
            || (record < self.records).then_some(record),
            |map| {
                map.records
                    .iter()
                    .position(|original| *original as usize == record)
                    .map(|at| {
                        at + usize::from(self.header)
                            - usize::from(self.header && map.zero.is_some_and(|zero| zero < at))
                    })
            },
        )
    }
}
struct Pending {
    rx: Receiver<Result<Map, &'static str>>,
    cancelled: Arc<AtomicBool>,
    query: String,
    sort: Sort,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
struct Input {
    source: Arc<String>,
    index: Arc<Index>,
    // Field order drops shared bodies before the final admitted lease.
    _budget: Option<Arc<budget::Lease>>,
}
impl Input {
    fn build(&self, query: &str, sort: Sort) -> Map {
        Map::build(&self.source, &self.index, query, sort)
    }
}
struct Permit(Arc<AtomicBool>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
#[derive(Default)]
pub(super) struct Projection {
    map: Option<Map>,
    draft: String,
    applied: String,
    sort: Sort,
    applied_sort: Sort,
    pending: Option<Pending>,
    expanded: bool,
    notice: Option<&'static str>,
    controls: Vec<egui::Id>,
    column_menu: Option<egui::Id>,
    menu_pass: Option<u64>,
    budget: Option<Arc<budget::Lease>>,
    #[cfg(test)]
    started: usize,
    #[cfg(test)]
    before_scan: Option<Box<dyn FnOnce() + Send>>,
}
impl Projection {
    pub fn demo(&mut self) {
        self.expanded = true;
        self.draft = "Row".into();
        self.sort.order = Order::Ascending;
        self.notice = Some("Synthetic sort/filter preview · nothing applied or executed.");
    }
    pub fn reserved_bytes() -> usize {
        RESERVED_BYTES
    }
    pub fn rows(&self, records: usize, header: bool) -> Rows<'_> {
        Rows {
            map: self.map.as_ref(),
            records,
            header,
        }
    }
    pub fn reconcile_header(
        &mut self,
        ctx: &egui::Context,
        grid: &mut grid::Grid,
        records: usize,
        header: bool,
    ) {
        if grid.reconcile_header(ctx, self.rows(records, header)) {
            self.notice = Some(
                "The selected record is filtered out after changing the header view. Selection moved to the first visible record.",
            );
        }
    }
    pub fn attach_budget(&mut self, lease: &Arc<budget::Lease>) {
        self.budget = Some(lease.clone());
    }
    fn start(&mut self, ctx: &egui::Context, source: Arc<String>, index: Arc<Index>) -> bool {
        if self.pending.is_some() {
            return false;
        }
        if self.draft.chars().count() > QUERY_CHARS {
            self.notice = Some("Filter is limited to 128 Unicode characters. Nothing was applied.");
            return false;
        }
        let busy = ctx.data_mut(|data| {
            data.get_temp_mut_or_default::<Arc<AtomicBool>>(egui::Id::new(
                "table-projection-worker",
            ))
            .clone()
        });
        if busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            self.notice =
                Some("Another table is applying a view. Try Apply again when it finishes.");
            return false;
        }
        let permit = Permit(busy);
        let (tx, rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = cancelled.clone();
        let query = self.draft.clone();
        let worker_query = query.clone();
        let sort = self.sort;
        let ctx = ctx.clone();
        // Retain the admitted lease as long as a worker owns source/index, even
        // after its tab closes. No UI or projection worker writes to the lease.
        let job = Input {
            source,
            index,
            _budget: self.budget.clone(),
        };
        #[cfg(test)]
        let before_scan = self.before_scan.take();
        let spawn = std::thread::Builder::new()
            .name("pui-table-view".into())
            .spawn(move || {
                let _permit = permit;
                let input = job; // drop shared bodies/lease before releasing permit on every exit
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    #[cfg(test)]
                    if let Some(before_scan) = before_scan {
                        before_scan();
                    }
                    if cancel.load(Ordering::Relaxed) {
                        return None;
                    }
                    let map = input.build(&worker_query, sort);
                    (!cancel.load(Ordering::Relaxed)).then_some(map)
                }));
                let result = match result {
                    Ok(Some(map)) => Ok(map),
                    Ok(None) => return,
                    Err(_) => {
                        Err("Cannot apply this table view. Try Apply again; Source is unchanged.")
                    }
                };
                if !cancel.load(Ordering::Relaxed) {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                } else {
                    drop(result);
                }
                drop(input); // includes lease, only after result transferred/dropped
            });
        if spawn.is_err() {
            self.notice =
                Some("Cannot start table view worker. Try Apply again; Source is unchanged.");
            return false;
        }
        self.pending = Some(Pending {
            rx,
            cancelled,
            query,
            sort,
        });
        self.notice = None;
        #[cfg(test)]
        {
            self.started += 1;
        }
        true
    }
    /// Only an explicit Apply launches work. A completed worker wakes egui once;
    /// try_recv here never requests a recurring repaint or performs a scan.
    pub fn poll(
        &mut self,
        ctx: &egui::Context,
        grid: &mut grid::Grid,
        records: usize,
        header: bool,
    ) {
        let result = match self.pending.as_ref().map(|pending| pending.rx.try_recv()) {
            Some(Ok(result)) => result,
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                Err("Table view worker stopped. Try Apply again; Source is unchanged.")
            }
            _ => return,
        };
        let pending = self.pending.take().unwrap();
        match result {
            Ok(map) => {
                grid.clear_menu(ctx);
                self.map = Some(map);
                self.applied = pending.query.clone();
                self.applied_sort = pending.sort;
                let clamped = grid.projection_changed(ctx, self.rows(records, header));
                self.notice = clamped.then_some("The selected record is filtered out. Selection moved to the first visible record.");
            }
            Err(error) => self.notice = Some(error),
        }
    }
    fn reset(&mut self, ctx: &egui::Context, grid: &mut grid::Grid, records: usize, header: bool) {
        self.pending = None; // cancels stale result; permit stays held until worker actually exits
        self.map = None;
        self.draft.clear();
        self.applied.clear();
        self.sort = Sort::default();
        self.applied_sort = Sort::default();
        self.notice = None;
        grid.projection_changed(ctx, self.rows(records, header));
    }
    pub(super) fn close_menu(&mut self, ctx: &egui::Context) {
        if let Some(id) = self.column_menu.take() {
            egui::Popup::close_id(ctx, id);
        }
    }
    pub fn hidden(&mut self) {
        // Hidden/Source/same-position replacement cannot reuse stale controls.
        self.controls.clear();
        self.menu_pass = None;
    }
    pub(super) fn menu_owns_keyboard(&self, ctx: &egui::Context) -> bool {
        self.menu_pass == Some(ctx.cumulative_pass_nr())
            || self
                .column_menu
                .is_some_and(|id| egui::Popup::is_id_open(ctx, id))
    }
    pub fn control_inputs(&mut self, ctx: &egui::Context, enabled: bool) -> bool {
        if ctx.memory(|memory| memory.top_modal_layer().is_some()) {
            return false;
        }
        if self
            .column_menu
            .is_some_and(|id| egui::Popup::is_id_open(ctx, id))
        {
            self.menu_pass = Some(ctx.cumulative_pass_nr());
            let escape = ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
            });
            if escape {
                if let Some(id) = self.column_menu {
                    egui::Popup::close_id(ctx, id);
                }
            }
            return true;
        }
        if egui::Popup::is_any_open(ctx) {
            return false;
        }
        let focus = ctx.memory(|memory| memory.focused());

        // read_response uses the previous live pass, just as native egui
        // pointer interaction does. Hidden/Source previews never call this hook.
        let pointer = ctx.input(|input| {
            (input.pointer.button_pressed(egui::PointerButton::Primary)
                || input.pointer.button_released(egui::PointerButton::Primary))
            .then(|| input.pointer.interact_pos())
            .flatten()
        });
        let escape_pressed = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let owner = self.controls.iter().copied().find(|id| {
            ctx.read_response(*id).is_some_and(|response| {
                response.enabled()
                    && (focus == Some(*id)
                        || focus.is_none()
                            && escape_pressed
                            && ctx.memory(|memory| memory.focus_lost_by_escape(*id))
                        || response.sense.senses_click()
                            && (response.is_pointer_button_down_on()
                                || response.clicked_by(egui::PointerButton::Primary)
                                || pointer.is_some_and(|position| {
                                    response.interact_rect.contains(position)
                                        && ctx.layer_id_at(position) == Some(response.layer_id)
                                })))
            })
        });
        let Some(owner) = owner else {
            return false;
        };
        let pointer_action = pointer.is_some_and(|position| {
            self.controls.iter().any(|id| {
                ctx.read_response(*id).is_some_and(|response| {
                    response.enabled()
                        && response.sense.senses_click()
                        && response.interact_rect.contains(position)
                        && ctx.layer_id_at(position) == Some(response.layer_id)
                })
            })
        });
        let mut escape = false;
        ctx.input_mut(|input| {
            input.events.retain(|event| match event {
                egui::Event::Key { key, modifiers, .. } => {
                    if modifiers.command && matches!(key, egui::Key::Enter | egui::Key::Backspace) {
                        return false;
                    }
                    if *key == egui::Key::Escape && *modifiers == egui::Modifiers::NONE {
                        escape = true;
                        return false;
                    }
                    // Static pointer activation cannot also submit the old
                    // composer's Enter/Space in the same input batch.
                    if pointer_action && matches!(key, egui::Key::Enter | egui::Key::Space) {
                        return false;
                    }
                    // A newly arriving question must not reinterpret a previous
                    // toolbar Space/text/navigation as an answer. Tab stays native.
                    enabled || *key == egui::Key::Tab
                }
                egui::Event::Text(_) | egui::Event::Paste(_) if !enabled || pointer_action => false,
                _ => true,
            })
        });
        if escape {
            ctx.memory_mut(|memory| memory.surrender_focus(owner));
        }
        true
    }
    pub(super) fn remember(&mut self, response: egui::Response) -> egui::Response {
        if !self.controls.contains(&response.id) {
            self.controls.push(response.id);
        }
        if response.has_focus() {
            response.ctx.memory_mut(|memory| {
                memory.set_focus_lock_filter_current(
                    response.id,
                    egui::EventFilter {
                        escape: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..Default::default()
                    },
                )
            });
        }
        response
    }
    pub fn remember_scrollbars(&mut self, ctx: &egui::Context, id: egui::Id) {
        for axis in [0usize, 1usize] {
            if let Some(response) = ctx.read_response(id.with(axis)) {
                self.remember(response);
            }
        }
    }
    pub(super) fn begin_controls(&mut self) {
        self.controls.clear();
    }
    pub(super) fn expanded(&self) -> bool {
        self.expanded
    }
    pub(super) fn toggle(&mut self) {
        self.expanded = !self.expanded;
    }
    pub(super) fn is_applied(&self) -> bool {
        self.map.is_some()
    }
    pub fn toolbar(
        &mut self,
        ui: &mut egui::Ui,
        source: &Arc<String>,
        index: &Arc<Index>,
        grid: &mut grid::Grid,
        header: bool,
    ) {
        if self.expanded {
            let colors = theme::palette(ui.ctx());
            egui::Frame::new()
                .fill(colors.surface)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    let output = egui::ScrollArea::vertical()
                        .id_salt("table-view-controls")
                        .max_height(112.0)
                        .show(ui, |ui| {
                            ui.spacing_mut().interact_size.y = 26.0;
                            ui.spacing_mut().button_padding = Vec2::new(7.0, 4.0);
                            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                            ui.style_mut().override_font_id = Some(FontId::proportional(12.0));
                            self.settings(ui, source, index, grid, header);
                        });
                    self.remember_scrollbars(ui.ctx(), output.id);
                });
            ui.add_space(6.0);
        }
        if let Some(notice) = self.notice {
            ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
        }
    }
    fn settings(
        &mut self,
        ui: &mut egui::Ui,
        source: &Arc<String>,
        index: &Arc<Index>,
        grid: &mut grid::Grid,
        header: bool,
    ) {
        ui.horizontal_wrapped(|ui| {
            let edit = ui.add(egui::TextEdit::singleline(&mut self.draft)
                .id_salt("table-filter-query").hint_text("Filter rows…")
                .font(FontId::proportional(12.0)).margin(egui::Margin::symmetric(8, 5))
                .char_limit(QUERY_CHARS).desired_width((ui.available_width()-125.0).clamp(70.0, 240.0)));
            let edit = edit.on_hover_text("Literal, case-sensitive text in any cell. Up to 128 characters. Apply updates the view; Source is unchanged.");
            if edit.changed() { self.draft.shrink_to(QUERY_CHARS * 4); }
            self.remember(edit);
            let response = ui.add_enabled(self.pending.is_none(), egui::Button::new(RichText::new("Apply").size(12.0)).fill(theme::palette(ui.ctx()).accent_bg).stroke(Stroke::NONE).corner_radius(6));
            if self.remember(response).clicked() && self.start(ui.ctx(), source.clone(), index.clone()) { grid.clear_menu(ui.ctx()); }
            let response = toolbar::button(ui, "Reset view", false).on_hover_text("Restore original order and all records. No file data changes.");
            if self.remember(response).clicked() { self.reset(ui.ctx(), grid, index.records.len(), header); }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::small(ui, "Sort"));
            let selected = if header {
                index
                    .value(source, 0, self.sort.column)
                    .chars()
                    .take(24)
                    .map(|ch| if ch.is_control() { ' ' } else { ch })
                    .collect::<String>()
            } else {
                format!("Column {}", self.sort.column + 1)
            };
            let response = egui::ComboBox::from_id_salt("table-sort-column")
                .selected_text(if selected.is_empty() {
                    format!("Column {}", self.sort.column + 1)
                } else {
                    selected
                })
                .width(112.0)
                .show_ui(ui, |ui| {
                    theme::menu_controls(ui);
                    for column in 0..index.columns {
                        let name = if header {
                            index
                                .value(source, 0, column)
                                .chars()
                                .take(40)
                                .map(|ch| if ch.is_control() { ' ' } else { ch })
                                .collect::<String>()
                        } else {
                            String::new()
                        };
                        let label = if name.is_empty() {
                            format!("Column {}", column + 1)
                        } else {
                            name
                        };
                        ui.selectable_value(&mut self.sort.column, column, label);
                    }
                })
                .response;
            self.column_menu = Some(response.id.with("popup"));
            self.remember(response);
            for (order, label) in [
                (Order::Original, "Original"),
                (Order::Ascending, "A–Z"),
                (Order::Descending, "Z–A"),
            ] {
                let response = toolbar::button(ui, label, self.sort.order == order);
                if self.remember(response).clicked() {
                    self.sort.order = order;
                }
            }
        });
        if self.pending.is_some() {
            ui.label(theme::small(ui, "Applying…"));
        } else if self.draft != self.applied || self.sort != self.applied_sort {
            ui.label(theme::small(ui, "Unapplied changes"));
        }
    }
}

#[cfg(test)]
mod input_regressions;
#[cfg(test)]
pub(super) mod tests;
#[cfg(test)]
mod ui_tests;
