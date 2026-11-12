//! Bounded CSV/TSV indexing/decoding on the file worker; UI lays out visible cells only.
use super::*;
use std::{borrow::Cow, ops::Range};
mod grid;

const MAX_RECORDS: usize = 10_000;
const MAX_COLUMNS: usize = 128;
const MAX_CELLS: usize = 100_000;
const ROW_HEIGHT: f32 = 30.0;
const COLUMN_WIDTH: f32 = 170.0;
const NUMBER_WIDTH: f32 = 54.0;

pub(super) struct Preview {
    pub source: String,
    index: Result<Index, String>,
    line_starts: Vec<u32>,
    header: bool,
    grid: grid::Grid,
    target_line: std::cell::Cell<Option<usize>>,
}
#[derive(Debug)]
struct Index {
    records: Vec<Range<usize>>,
    cells: Vec<Cell>,
    columns: usize,
    ragged: bool,
}
#[derive(Debug)]
struct Cell {
    range: Range<usize>,
    decoded: Option<Box<str>>,
}
impl Cell {
    fn value<'a>(&'a self, source: &'a str) -> &'a str {
        self.decoded
            .as_deref()
            .unwrap_or(&source[self.range.clone()])
    }
}

impl Preview {
    pub(super) fn new(source: String, delimiter: u8) -> Self {
        let index = parse(&source, delimiter);
        // The file reader already enforces MAX_TEXT; offsets need only 32 bits.
        let line_starts = std::iter::once(0)
            .chain(
                source
                    .bytes()
                    .enumerate()
                    .filter(|(_, byte)| *byte == b'\n')
                    .map(|(at, _)| (at + 1) as u32),
            )
            .collect();
        Self {
            source,
            index,
            line_starts,
            header: true,
            grid: Default::default(),
            target_line: std::cell::Cell::new(None),
        }
    }

    pub(super) fn retained_bytes(&self) -> usize {
        let source =
            self.source.capacity() + self.line_starts.capacity() * std::mem::size_of::<u32>();
        source
            + match &self.index {
                Ok(index) => {
                    index.records.capacity() * std::mem::size_of::<Range<usize>>()
                        + index.cells.capacity() * std::mem::size_of::<Cell>()
                        + index
                            .cells
                            .iter()
                            .filter_map(|cell| cell.decoded.as_ref())
                            .map(|value| value.len())
                            .sum::<usize>()
                }
                Err(error) => error.capacity(),
            }
    }

    pub(super) fn header(&self) -> bool {
        self.header
    }
    pub(super) fn set_header(&mut self, header: bool) {
        if self.header != header {
            self.header = header;
            self.grid.header_changed();
        }
    }

    pub(super) fn target_line(&self, line: usize) -> usize {
        let count = self.line_starts.len() - usize::from(self.source.ends_with('\n'));
        let line = line.clamp(1, count.max(1));
        self.target_line.set(Some(line));
        line
    }

    pub(super) fn show_source(&self, ui: &mut egui::Ui, path: &Path) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Copy source")
                .on_hover_text(
                    "Copy the exact original UTF-8 text, including quotes and line endings",
                )
                .clicked()
            {
                ui.ctx().copy_text(self.source.clone());
            }
            ui.label(theme::small(
                ui,
                "Read-only · long lines are shortened visually; Copy source keeps the full text.",
            ));
        });
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace).max(18.0);
        let mut scroll = egui::ScrollArea::both()
            .id_salt(("delimited-source", path))
            .auto_shrink([false, false]);
        if let Some(line) = self.target_line.take() {
            scroll = scroll.vertical_scroll_offset(
                (line - 1) as f32 * (row_height + ui.spacing().item_spacing.y),
            );
        }
        scroll.show_rows(ui, row_height, self.line_starts.len(), |ui, rows| {
            for row in rows {
                let start = self.line_starts[row] as usize;
                let end = self
                    .line_starts
                    .get(row + 1)
                    .map_or(self.source.len(), |at| *at as usize);
                let line = self.source[start..end].trim_end_matches(['\r', '\n']);
                ui.push_id(("source-line", path, row), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{:>6}", row + 1))
                                .monospace()
                                .size(12.0)
                                .color(theme::palette(ui.ctx()).dim),
                        );
                        let display: String = line.chars().take(4096).collect();
                        let response = ui.add(
                            egui::Label::new(RichText::new(display).monospace().size(12.0))
                                .extend()
                                .selectable(true),
                        );
                        context_capture::offer_source_selection(&response, path, line, row + 1);
                        if line.chars().nth(4096).is_some() {
                            ui.label(theme::small(
                                ui,
                                "… long line · excerpt limited to 4,096 characters",
                            ));
                        }
                    });
                });
            }
        });
    }

    pub(super) fn show(&mut self, ui: &mut egui::Ui, path: &Path) {
        let index = match &self.index {
            Ok(index) => index,
            Err(error) => {
                ui.add(
                    egui::Label::new(theme::small(ui, format!("Table unavailable: {error}")))
                        .wrap(),
                );
                ui.label(theme::small(
                    ui,
                    "Choose Source to inspect the unchanged file.",
                ));
                return;
            }
        };
        if index.records.is_empty() {
            ui.label(theme::small(ui, "Empty file · no records to preview."));
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if ui.checkbox(&mut self.header, "First record is header")
                .on_hover_text("Remembered for this file in this workspace (up to 256 headerless overrides). No file data is changed.")
                .changed() { self.grid.header_changed(); }
            if ui.button("Navigate cells").on_hover_text("Focus the table. Arrows move; Home/End choose a column; Ctrl/⌘ Home/End choose first/last cell; Page Up/Down move a page; Tab leaves.").clicked() {
                self.grid.focus(ui.ctx(), path);
            }
            if ui.button("Copy current cell").on_hover_text("Copy the complete current cell value, not its shortened display").clicked() {
                ui.ctx().copy_text(index.value(&self.source, self.grid.active.0, self.grid.active.1).into());
            }
            let count = index.records.len() - usize::from(self.header);
            ui.label(theme::small(
                ui,
                format!("{count} data records · {} columns", index.columns),
            ));
        });
        if index.ragged {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    "Record lengths differ. Missing cells are blank; no values were discarded.",
                ))
                .wrap(),
            );
        }
        ui.add(egui::Label::new(theme::small(ui, "Arrows move · Home/End · Page Up/Down · ⌘/Ctrl+C copy · Tab leaves")).wrap())
            .on_hover_text("Read-only. Navigate cells or click a cell to focus the table. Copy keeps the complete cell value, or a pointer-selected substring. Ctrl/⌘ Home/End reaches first/last cell. Right-click a cell to copy or quote it.");
        let description = self.grid.description(index, &self.source, self.header);
        ui.add(egui::Label::new(theme::small(ui, &description)).truncate())
            .on_hover_text(description);
        ui.add_space(6.0);
        let colors = theme::palette(ui.ctx());
        let first = usize::from(self.header);
        let count = index.records.len() - first;
        let width = NUMBER_WIDTH + index.columns as f32 * COLUMN_WIDTH;
        let height = (count + 1) as f32 * ROW_HEIGHT;
        let mut painted = 0_usize;
        let scroll = egui::ScrollArea::both()
            .id_salt(("delimited-table", path, self.header))
            .auto_shrink([false, false]);
        let output = self
            .grid
            .scroll(scroll, self.header)
            .show_viewport(ui, |ui, viewport| {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
                let visible_columns = visible_columns(viewport, index.columns);
                let start = (viewport.top() / ROW_HEIGHT).floor() as usize;
                let end = ((viewport.bottom() / ROW_HEIGHT).ceil() as usize).min(count);
                // Sticky header is painted last; data never paints or receives input beneath it.
                let header_top = rect.top() + viewport.top();
                let data_clip = ui.clip_rect().intersect(egui::Rect::from_min_max(
                    egui::pos2(ui.clip_rect().left(), header_top + ROW_HEIGHT),
                    ui.clip_rect().max,
                ));
                let mut body =
                    ui.new_child(egui::UiBuilder::new().id_salt("table-body").max_rect(rect));
                body.set_clip_rect(data_clip);
                for row in start..end {
                    let record = row + first;
                    let y = rect.top() + (row + 1) as f32 * ROW_HEIGHT;
                    let row_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.left(), y),
                        Vec2::new(width, ROW_HEIGHT),
                    );
                    if !body.is_rect_visible(row_rect) {
                        continue;
                    }
                    if row % 2 == 1 {
                        body.painter().rect_filled(row_rect, 0, colors.surface);
                    }
                    number(&mut body, row_rect, record + 1);
                    for column in visible_columns.clone() {
                        let cell_rect = cell_rect(rect.left(), y, column);
                        let value = index.value(&self.source, record, column);
                        cell(
                            &mut body,
                            cell_rect,
                            value,
                            false,
                            path,
                            record + 1,
                            column + 1,
                            &mut self.grid,
                        );
                        painted += 1;
                    }
                    body.painter().hline(
                        row_rect.x_range(),
                        row_rect.bottom(),
                        Stroke::new(1.0, colors.border),
                    );
                }
                let header_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.left(), header_top),
                    Vec2::new(width, ROW_HEIGHT),
                );
                ui.painter().rect_filled(header_rect, 0, colors.sidebar);
                number(ui, header_rect, 0);
                for column in visible_columns {
                    let value = if self.header {
                        Cow::Borrowed(index.value(&self.source, 0, column))
                    } else {
                        Cow::Owned(format!("Column {}", column + 1))
                    };
                    cell(
                        ui,
                        cell_rect(rect.left(), header_top, column),
                        &value,
                        true,
                        path,
                        usize::from(self.header),
                        column + 1,
                        &mut self.grid,
                    );
                    painted += 1;
                }
                ui.painter().hline(
                    header_rect.x_range(),
                    header_rect.bottom(),
                    Stroke::new(1.0, colors.border),
                );
            });
        self.grid.finish(
            ui,
            path,
            output.inner_rect,
            output.state.offset,
            index,
            &self.source,
            self.header,
        );
        #[cfg(test)]
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new("delimited-painted-cells"), painted));
        let _ = painted;
    }
}

impl Index {
    fn value<'a>(&'a self, source: &'a str, record: usize, column: usize) -> &'a str {
        self.records
            .get(record)
            .and_then(|row| self.cells[row.clone()].get(column))
            .map_or("", |cell| cell.value(source))
    }
}
fn visible_columns(viewport: egui::Rect, columns: usize) -> Range<usize> {
    let first = ((viewport.left() - NUMBER_WIDTH).max(0.0) / COLUMN_WIDTH).floor() as usize;
    let last = ((viewport.right() - NUMBER_WIDTH).max(0.0) / COLUMN_WIDTH).ceil() as usize;
    first.min(columns)..last.min(columns)
}
fn cell_rect(left: f32, top: f32, column: usize) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(left + NUMBER_WIDTH + column as f32 * COLUMN_WIDTH, top),
        Vec2::new(COLUMN_WIDTH, ROW_HEIGHT),
    )
}
fn number(ui: &mut egui::Ui, rect: egui::Rect, record: usize) {
    ui.painter().text(
        egui::pos2(rect.left() + NUMBER_WIDTH - 10.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        if record == 0 {
            "#".into()
        } else {
            record.to_string()
        },
        FontId::monospace(11.0),
        theme::palette(ui.ctx()).dim,
    );
}
#[expect(clippy::too_many_arguments)]
fn cell(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    value: &str,
    header: bool,
    path: &Path,
    record: usize,
    column: usize,
    grid: &mut grid::Grid,
) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let colors = theme::palette(ui.ctx());
    // Bound glyph layout even if a single field occupies the entire 512 KiB file.
    let mut display: String = value
        .chars()
        .take(256)
        .map(|ch| {
            if matches!(ch, '\n' | '\r' | '\t') {
                ' '
            } else {
                ch
            }
        })
        .collect();
    if value.chars().nth(256).is_some() {
        display.push('…');
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt((record, column, header))
            .max_rect(rect.shrink2(egui::vec2(8.0, 4.0))),
    );
    let mut text = RichText::new(display).size(12.0).color(colors.text);
    if header {
        text = text.strong();
    }
    let label = child.add(
        egui::Label::new(text)
            .truncate()
            .selectable(true)
            .sense(Sense::hover())
            .halign(Align::Min),
    );
    // Empty values still have a discoverable context-menu target.
    let response = child.interact(
        rect,
        label.id,
        Sense::click_and_drag() - Sense::focusable_noninteractive(),
    );
    if record > 0 {
        if response.clicked_by(egui::PointerButton::Primary) {
            grid.select(ui.ctx(), path, record - 1, column - 1);
        }
        grid.paint(ui, path, rect, record - 1, column - 1);
    }
    #[cfg(test)]
    if record <= 2 {
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new(("delimited-cell", record, column, header)),
                response.rect,
            )
        });
    }
    response.clone().on_hover_ui(|ui| {
        ui.set_max_width(420.0);
        ui.add(egui::Label::new(value.chars().take(4096).collect::<String>()).wrap());
        if value.chars().nth(4096).is_some() {
            ui.label(theme::small(
                ui,
                "Tooltip limited; Copy cell keeps the full value.",
            ));
        }
    });
    response.context_menu(|ui| {
        theme::menu_controls(ui);
        if ui.button("Copy cell").clicked() {
            ui.ctx().copy_text(value.to_owned());
            ui.close();
        }
        if ui.button("Add cell to chat").clicked() {
            context_capture::queue(
                ui.ctx(),
                &format!("{} · record {record}, column {column}", path.display()),
                value,
            );
            ui.close();
        }
    });
    grid.menu(&response);
    ui.painter().vline(
        rect.right(),
        rect.y_range(),
        Stroke::new(1.0, colors.border),
    );
}

/// Strict RFC-style quoting, comma/tab dialect only; never infer numeric/formula types.
fn parse(source: &str, delimiter: u8) -> Result<Index, String> {
    if source.len() > MAX_TEXT {
        return Err("File exceeds the 512 KiB preview limit.".into());
    }
    let bytes = source.as_bytes();
    let mut at = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        3
    } else {
        0
    };
    let mut index = Index {
        records: Vec::new(),
        cells: Vec::new(),
        columns: 0,
        ragged: false,
    };
    while at < bytes.len() {
        if index.records.len() >= MAX_RECORDS {
            return Err("Table exceeds 10,000 records; Source remains available.".into());
        }
        let row_start = index.cells.len();
        loop {
            if index.cells.len() - row_start >= MAX_COLUMNS {
                return Err("Table exceeds 128 columns; Source remains available.".into());
            }
            if index.cells.len() >= MAX_CELLS {
                return Err("Table exceeds 100,000 cells; Source remains available.".into());
            }
            let quoted = bytes.get(at) == Some(&b'"');
            let mut escaped = false;
            let start;
            let end;
            if quoted {
                at += 1;
                start = at;
                loop {
                    match bytes.get(at) {
                        None => {
                            return Err(format!(
                                "Unclosed quoted field in record {}.",
                                index.records.len() + 1
                            ));
                        }
                        Some(b'"') if bytes.get(at + 1) == Some(&b'"') => {
                            escaped = true;
                            at += 2;
                        }
                        Some(b'"') => {
                            end = at;
                            at += 1;
                            break;
                        }
                        _ => at += 1,
                    }
                }
                if bytes
                    .get(at)
                    .is_some_and(|b| !matches!(*b, b'\r' | b'\n') && *b != delimiter)
                {
                    return Err(format!(
                        "Unexpected text after closing quote in record {}.",
                        index.records.len() + 1
                    ));
                }
            } else {
                start = at;
                while bytes
                    .get(at)
                    .is_some_and(|b| !matches!(*b, b'\r' | b'\n') && *b != delimiter)
                {
                    if bytes[at] == b'"' {
                        return Err(format!(
                            "Quote in an unquoted field in record {}.",
                            index.records.len() + 1
                        ));
                    }
                    at += 1;
                }
                end = at;
            }
            let decoded =
                escaped.then(|| source[start..end].replace("\"\"", "\"").into_boxed_str());
            index.cells.push(Cell {
                range: start..end,
                decoded,
            });
            if bytes.get(at) == Some(&delimiter) {
                at += 1;
                continue;
            }
            if bytes.get(at) == Some(&b'\r') {
                at += 1;
                if bytes.get(at) == Some(&b'\n') {
                    at += 1;
                }
            } else if bytes.get(at) == Some(&b'\n') {
                at += 1;
            }
            break;
        }
        let columns = index.cells.len() - row_start;
        if let Some(first) = index.records.first() {
            index.ragged |= first.len() != columns;
        }
        index.columns = index.columns.max(columns);
        index.records.push(row_start..index.cells.len());
    }
    Ok(index)
}

#[cfg(test)]
mod tests;
