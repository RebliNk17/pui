//! Explicit bounded clipboard serialization of the applied view, never a file write.
use super::*;

const MAX_OUTPUT: usize = 512 * 1024;

#[derive(Default)]
pub(super) struct Export {
    controls: Box<[Option<egui::Id>; 9]>, // bounded chrome, keep Content enum compact
    keyboard_pass: Option<u64>,
    notice: Option<String>,
}
fn encoded_len(value: &str, delimiter: u8) -> Option<usize> {
    let quoted = value.is_empty()
        || value
            .bytes()
            .any(|byte| matches!(byte, b'"' | b'\r' | b'\n') || byte == delimiter);
    value.len().checked_add(if quoted {
        2 + value.bytes().filter(|byte| *byte == b'"').count()
    } else {
        0
    })
}
fn serialize(
    source: &str,
    index: &Index,
    rows: projection::Rows<'_>,
    delimiter: u8,
) -> Result<String, &'static str> {
    if !matches!(delimiter, b',' | b'\t') {
        return Err("Unsupported clipboard delimiter.");
    }
    serialize_records(
        source,
        index,
        (0..rows.len()).map(|position| rows.record(position)),
        delimiter,
    )
}
fn serialize_records(
    source: &str,
    index: &Index,
    records: impl Iterator<Item = Option<usize>> + Clone,
    delimiter: u8,
) -> Result<String, &'static str> {
    if !matches!(delimiter, b',' | b'\t') {
        return Err("Unsupported clipboard delimiter.");
    }
    // Exact preflight rejects whole output before allocating a clipboard body.
    let mut bytes = 0usize;
    for record in records.clone() {
        let record = record.ok_or("Table view is unavailable; nothing copied.")?;
        let cells = index
            .records
            .get(record)
            .ok_or("Table view is unavailable.")?;
        bytes = bytes
            .checked_add(1 + cells.len().saturating_sub(1))
            .ok_or("Copy exceeds 512 KiB; nothing copied.")?;
        for cell in &index.cells[cells.clone()] {
            bytes = bytes
                .checked_add(
                    encoded_len(cell.value(source), delimiter)
                        .ok_or("Copy exceeds 512 KiB; nothing copied.")?,
                )
                .ok_or("Copy exceeds 512 KiB; nothing copied.")?;
            if bytes > MAX_OUTPUT {
                return Err(
                    "Copy exceeds 512 KiB; nothing copied. Use Copy current cell or Copy source.",
                );
            }
        }
    }
    if bytes > MAX_OUTPUT {
        return Err("Copy exceeds 512 KiB; nothing copied.");
    }
    let mut output = String::with_capacity(bytes);
    for record in records {
        let record = record.unwrap(); // verified exact iterator in the preflight
        for (column, cell) in index.cells[index.records[record].clone()]
            .iter()
            .enumerate()
        {
            if column > 0 {
                output.push(char::from(delimiter));
            }
            let value = cell.value(source);
            let quoted = value.is_empty()
                || value
                    .bytes()
                    .any(|byte| matches!(byte, b'"' | b'\r' | b'\n') || byte == delimiter);
            if quoted {
                output.push('"');
                for ch in value.chars() {
                    if ch == '"' {
                        output.push('"');
                    }
                    output.push(ch);
                }
                output.push('"');
            } else {
                output.push_str(value);
            }
        }
        output.push('\n');
    }
    debug_assert_eq!(output.len(), bytes);
    Ok(output)
}
impl Export {
    pub(super) fn hidden(&mut self) {
        *self.controls = [None; 9];
        self.keyboard_pass = None;
    }
    pub(super) fn owns_keyboard(&self, ctx: &egui::Context) -> bool {
        self.keyboard_pass == Some(ctx.cumulative_pass_nr())
    }
    pub(super) fn inputs(&mut self, ctx: &egui::Context) -> bool {
        if egui::Popup::is_any_open(ctx) || ctx.memory(|memory| memory.top_modal_layer().is_some())
        {
            return false;
        }
        let focus = ctx.memory(|memory| memory.focused());
        let pointer = ctx.input(|input| {
            (input.pointer.primary_pressed() || input.pointer.primary_released())
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        if self.controls.iter().flatten().any(|id| {
            ctx.read_response(*id).is_some_and(|response| {
                response.enabled()
                    && (focus == Some(*id)
                        || focus.is_none() && ctx.memory(|memory| memory.focus_lost_by_escape(*id))
                        || pointer.is_some_and(|pos| {
                            response.interact_rect.contains(pos)
                                && ctx.layer_id_at(pos) == Some(response.layer_id)
                        }))
            })
        }) {
            self.keyboard_pass = Some(ctx.cumulative_pass_nr());
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(event, egui::Event::Key {
                        key: egui::Key::Enter | egui::Key::Backspace,
                        modifiers, ..
                    } if modifiers.command || modifiers.ctrl)
                })
            });
            return true;
        }
        false
    }
    pub(super) fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }
    #[expect(clippy::too_many_arguments)]
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &str,
        index: Option<&Index>,
        header: bool,
        projection: &mut projection::Projection,
        grid: &grid::Grid,
        source_view: bool,
        path: &Path,
    ) {
        let available = ui.is_enabled()
            && prompt_ui::keyboard_available(ui)
            && ui.memory(|memory| memory.top_modal_layer().is_none());
        let count = index.map_or(0, |index| {
            projection.rows(index.records.len(), header).len()
        });
        let record = index
            .and_then(|index| grid.current_record(projection.rows(index.records.len(), header)));
        if let Some(index) = index {
            if ui
                .add_enabled(
                    !source_view && record.is_some(),
                    egui::Button::new("Copy current cell"),
                )
                .clicked()
            {
                ui.ctx()
                    .copy_text(index.value(source, grid.active.0, grid.active.1).into());
                ui.close();
            }
        }
        if ui
            .button("Copy source")
            .on_hover_text("Exact original UTF-8 text, including quotes and line endings")
            .clicked()
        {
            ui.ctx().copy_text(source.to_owned());
            ui.close();
        }
        ui.separator();
        ui.vertical(|ui| {
            for (slot, label, delimiter, mode) in [
                (0, "Copy CSV", b',', 0),
                (1, "Copy TSV", b'\t', 0),
                (2, "Copy record as CSV", b',', 1),
                (3, "Copy record as TSV", b'\t', 1),
                (4, "Copy column as CSV", b',', 2),
                (5, "Copy column as TSV", b'\t', 2),
                (6, "Copy selection as CSV", b',', 3),
                (7, "Copy selection as TSV", b'\t', 3),
            ] {
                let selection=grid.selection();
                let rectangle=mode==3;
                let one_record=mode==1;
                let one_column=mode==2;
                if slot==2 {ui.separator();}
                let target_available=if rectangle{!source_view && record.is_some() && selection.is_some()}else if mode!=0 {!source_view && record.is_some() && index.is_some_and(|index|grid.active.1<index.columns)}else{count>0};
                // Record identity prevents async selection changes inheriting a press.
                let response = ui.push_id(("table-copy",slot,(mode!=0).then_some(record),one_column.then_some(grid.active.1),rectangle.then_some(selection)),|ui| ui
                    .add_enabled(available && target_available, egui::Button::new(label)))
                    .inner
                    .on_hover_text(if rectangle {
                        "Copy only the current applied-view rectangle, up to 512 KiB, whole or reject. No added header; ragged missing cells become empty fields. Formula values unchanged; spreadsheets may evaluate them. No file is written.".into()
                    }else if one_column {
                        format!("Copy active column {} across the applied view, including the real header if enabled, up to 512 KiB. Missing ragged cells become empty fields. Original values/formulas unchanged; spreadsheets may evaluate formulas. No file is written.",grid.active.1+1)
                    }else if one_record {
                        format!("Copy all original fields of active record {}, without an added header, up to 512 KiB. The real header is copied only when it is the active record. Ragged/empty values and literal formulas remain; spreadsheets may evaluate formulas. No file is written.",record.map_or_else(||"(unavailable)".into(),|record|(record+1).to_string()))
                    }else{concat!(
                        "Copy the applied sorted/filtered view, including the real header if enabled, ",
                        "up to 512 KiB. Original cell values and ragged record lengths remain. ",
                        "Quotes/line endings are normalized, not exact Source. Formula-looking ",
                        "values are unchanged: pasting into a spreadsheet may evaluate them. ",
                        "No file is written.",
                    ).into()});
                let response = projection.remember(response);
                self.controls[slot] = Some(response.id);
                if response.enabled()
                    && (response.has_focus() || response.is_pointer_button_down_on() || response.clicked())
                {
                    self.keyboard_pass = Some(ui.ctx().cumulative_pass_nr());
                }
                if response.clicked() {
                    let index = index.unwrap();
                    let result=if rectangle {
                        selection.ok_or("Selection unavailable; nothing copied.").and_then(|(first,last)|rectangle::serialize_as(source,index,projection.rows(index.records.len(),header),first,last,delimiter))
                    }else if one_column {
                        column::serialize(source,index,projection.rows(index.records.len(),header),grid.active.1,delimiter)
                    }else if one_record {
                        record.ok_or("Active record is unavailable; nothing copied.").and_then(|record|serialize_records(source,index,std::iter::once(Some(record)),delimiter))
                    }else {serialize(source, index, projection.rows(index.records.len(), header), delimiter)};
                    match result {
                        Ok(output) => {
                            let bytes = output.len();
                            ui.ctx().copy_text(output);
                            ui.close();
                            let format = if delimiter == b',' { "CSV" } else { "TSV" };
                            self.notice = Some(if rectangle {
                                format!("Copied selected cells · {bytes} bytes · {format}. Formula values unchanged.")
                            }else if one_column {
                                format!("Copied column {} · {count} records · {bytes} bytes · {format}. Formula values unchanged.",grid.active.1+1)
                            }else if one_record {
                                format!("Copied original record {} · {bytes} bytes · {format}. Formula values unchanged.",record.unwrap()+1)
                            }else{format!(
                                "Copied {count} original records · {bytes} bytes · {format}. Formula values unchanged."
                            )});
                        }
                        Err(error) => self.notice = Some(error.into()),
                    }
                }
                #[cfg(test)]
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new(("table-export", slot)), response.id)
                });
            }
        });
        let selection = grid.selection();
        let add=ui.push_id(("table-selection-draft",selection),|ui|ui.add_enabled(available && !source_view && record.is_some() && selection.is_some(),egui::Button::new("Add selected cells to draft"))).inner.on_hover_text("Complete applied-view rectangle as TSV, up to 24 KiB. No added header; ragged cells empty; original formulas unchanged. Never sends or writes a file.");
        let add = projection.remember(add);
        self.controls[8] = Some(add.id);
        if add.enabled() && (add.has_focus() || add.is_pointer_button_down_on() || add.clicked()) {
            self.keyboard_pass = Some(ui.ctx().cumulative_pass_nr());
        }
        #[cfg(test)]
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new(("table-export", 8usize)), add.id));
        if add.clicked() {
            let index = index.unwrap();
            let (first, last) = selection.unwrap();
            let rows = projection.rows(index.records.len(), header);
            match rectangle::serialize_limited(
                source,
                index,
                rows,
                first,
                last,
                b'\t',
                24 * 1024 - 1,
            ) {
                Ok(text) => {
                    let label = format!(
                        "{} · applied view rows {}–{}, columns {}–{} · TSV, original record order may be filtered/sorted",
                        path.display(),
                        first.0 + 1,
                        last.0 + 1,
                        first.1 + 1,
                        last.1 + 1
                    );
                    if context_capture::queue_file_selection(ui.ctx(), &label, &text) {
                        self.notice = Some(
                            "Selection queued for draft · TSV · never sent automatically.".into(),
                        );
                        ui.close();
                    } else {
                        self.notice = Some("Draft owner unavailable; nothing added.".into());
                    }
                }
                Err(_) => {
                    self.notice = Some(
                        "Selection exceeds 24 KiB or is unavailable; nothing added to draft."
                            .into(),
                    )
                }
            }
        }
        ui.add_space(4.0);
        if let Some(notice) = self.notice.as_deref() {
            ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
        } else {
            ui.add(egui::Label::new(theme::small(ui, "Applied view · up to 512 KiB. Formula values are unchanged; spreadsheets may evaluate them.")).wrap());
        }
        if available && self.owns_keyboard(ui.ctx()) {
            ui.input_mut(|input| {
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
mod capture_tests;
mod column;
#[cfg(test)]
mod column_tests;
#[cfg(test)]
mod record_tests;
#[cfg(test)]
mod record_ui_tests;
pub(super) mod rectangle;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
