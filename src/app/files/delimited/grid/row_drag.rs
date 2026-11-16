//! Record-number row gestures and Alt/Option cell rectangles; plain text dragging stays native.
const CELL_DRAG: usize = 1usize << (usize::BITS - 1);
// High bit distinguishes cell gestures. Payload = applied row*128+column;
// table bounds (10,000 records/128 columns) keep it below the tag even on 32-bit.
fn decode(anchor: usize) -> (usize, Option<usize>) {
    if anchor & CELL_DRAG != 0 {
        let value = anchor & !CELL_DRAG;
        (value / 128, Some(value % 128))
    } else {
        (anchor - 1, None)
    }
}
use super::*;
#[cfg(test)]
mod cell_tests;
#[cfg(test)]
mod horizontal_tests;
#[cfg(test)]
mod tests;
impl Grid {
    pub(super) fn row_drag_scroll(
        &self,
        ctx: &egui::Context,
        path: &Path,
        header: bool,
        records: usize,
        columns: usize,
    ) -> Option<Vec2> {
        let (anchor, pass) = self.row_drag?;
        if ctx.cumulative_pass_nr().saturating_sub(pass) > 2
            || !ctx
                .input(|i| i.focused && i.pointer.primary_down() && !i.pointer.primary_released())
            || ctx.memory(|m| m.top_modal_layer().is_some())
            || egui::Popup::is_any_open(ctx)
        {
            return None;
        }
        let pos = ctx.input(|i| i.pointer.interact_pos())?;
        let response = ctx.read_response(self.id(path)).filter(|r| {
            r.enabled() && r.interact_rect.contains(pos) && ctx.layer_id_at(pos) == Some(r.layer_id)
        })?;
        let rect = response.interact_rect;
        let direction = if pos.y >= rect.bottom() - ROW_HEIGHT {
            -1.0
        } else if pos.y >= rect.top() + ROW_HEIGHT && pos.y < rect.top() + 2.0 * ROW_HEIGHT {
            1.0
        } else {
            0.0
        };
        // At most 480 logical points/second, with a 50 ms frame cap. No idle timer.
        let step = 480.0 * ctx.input(|i| i.stable_dt.clamp(0.0, 0.05));
        let max = ((records.saturating_sub(usize::from(header)) + 1) as f32 * ROW_HEIGHT
            - self.viewport.y)
            .max(0.0);
        let y = (self.offset.y - direction * step).clamp(0.0, max);
        let horizontal = if decode(anchor.get()).1.is_some() && pos.y >= rect.top() + ROW_HEIGHT {
            if pos.x >= rect.right() - ROW_HEIGHT {
                1.0
            } else if pos.x < rect.left() + ROW_HEIGHT {
                -1.0
            } else {
                0.0
            }
        } else {
            0.0
        };
        let max_x = (NUMBER_WIDTH + columns as f32 * COLUMN_WIDTH - self.viewport.x).max(0.0);
        let x = (self.offset.x + horizontal * step).clamp(0.0, max_x);
        if (y - self.offset.y).abs() < 0.01 && (x - self.offset.x).abs() < 0.01 {
            return None;
        }
        ctx.request_repaint();
        Some(egui::vec2(x, y))
    }
    pub(super) fn row_drag_inputs(&mut self, ctx: &egui::Context, enabled: bool) {
        let Some((_, pass)) = self.row_drag else {
            return;
        };
        if !enabled
            || !ctx.input(|i| i.focused)
            || ctx.cumulative_pass_nr().saturating_sub(pass) > 2
            || ctx.memory(|m| m.top_modal_layer().is_some())
            || egui::Popup::is_any_open(ctx)
        {
            self.row_drag = None;
            return;
        }
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        ctx.input_mut(|i| {
            i.events.retain(|event| {
                !matches!(
                    event,
                    egui::Event::Text(_)
                        | egui::Event::Paste(_)
                        | egui::Event::Key {
                            key: egui::Key::Enter
                                | egui::Key::Backspace
                                | egui::Key::Escape
                                | egui::Key::Space,
                            ..
                        }
                )
            })
        });
        if escape {
            self.row_drag = None;
        }
    }
    #[expect(clippy::too_many_arguments)]
    pub(super) fn finish_row_drag(
        &mut self,
        ui: &egui::Ui,
        path: &Path,
        rect: egui::Rect,
        offset: Vec2,
        index: &Index,
        header: bool,
        rows: projection::Rows<'_>,
    ) {
        let ctx = ui.ctx();
        let id = self.id(path);
        let gutter = egui::Rect::from_min_max(
            rect.min + egui::vec2(0.0, ROW_HEIGHT),
            egui::pos2(
                (rect.left() - offset.x + NUMBER_WIDTH).max(rect.left()),
                rect.bottom(),
            ),
        )
        .intersect(ui.clip_rect());
        if gutter.is_positive() {
            ui.interact(gutter,id.with("row-drag-hint"),Sense::hover()).on_hover_text("Drag record numbers for complete rows, or Alt/Option-drag cells for a rectangle. Plain cell dragging keeps substring selection. Hold near a body edge to scroll; release or Escape stops. No file edits.");
        }
        if !ui.is_enabled()
            || !prompt_ui::keyboard_available(ui)
            || ctx.memory(|m| m.top_modal_layer().is_some())
            || egui::Popup::is_any_open(ctx)
        {
            self.row_drag = None;
            return;
        }
        let (pos, pressed, down, released, modifiers) = ctx.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.modifiers,
            )
        });
        if !down && !released {
            self.row_drag = None;
            return;
        }
        let Some(pos) = pos else {
            self.row_drag = None;
            return;
        };
        let visible = rect.intersect(ui.clip_rect());
        let owns = visible.contains(pos) && ctx.layer_id_at(pos) == Some(ui.layer_id());
        let first = usize::from(header);
        let body = pos.y >= rect.top() + ROW_HEIGHT;
        let position = (((pos.y - rect.top() + offset.y) / ROW_HEIGHT).floor() as usize)
            .saturating_sub(1)
            + first;
        let position = position.min(rows.len().saturating_sub(1));
        let column = ((pos.x - rect.left() + offset.x - NUMBER_WIDTH).max(0.0) / COLUMN_WIDTH)
            .floor() as usize;
        let alt_cell = modifiers.alt
            && !modifiers.shift
            && !modifiers.ctrl
            && !modifiers.command
            && pos.x >= rect.left() - offset.x + NUMBER_WIDTH
            && column < index.columns;
        let plain_gutter = !modifiers.any()
            && pos.x >= rect.left() - offset.x
            && pos.x < rect.left() - offset.x + NUMBER_WIDTH;
        if pressed
            && self.row_drag.is_none()
            && owns
            && body
            && (plain_gutter || alt_cell)
            && ctx
                .read_response(id)
                .is_some_and(|r| r.enabled() && r.interact_rect.contains(pos))
            && rows.len() > first
        {
            let encoded = if alt_cell {
                CELL_DRAG | (position * 128 + column)
            } else {
                position + 1
            };
            self.row_drag = std::num::NonZeroUsize::new(encoded)
                .map(|anchor| (anchor, ctx.cumulative_pass_nr()));
        }
        if let Some((anchor, _)) = self.row_drag {
            let (anchor_row, anchor_column) = decode(anchor.get());
            let target_column = anchor_column.map_or(index.columns.saturating_sub(1), |_| {
                column.min(index.columns.saturating_sub(1))
            });
            let target_anchor = (anchor_row, anchor_column.unwrap_or(0));
            if owns
                && body
                && let Some(record) = rows.record(position)
            {
                if self.active != (record, target_column) || self.anchor != Some(target_anchor) {
                    self.select(ctx, path, record, target_column, position);
                }
                self.anchor = Some(target_anchor);
                clear_selection(ctx);
            }
            ui.output_mut(|out| {
                out.cursor_icon = if anchor_column.is_some() {
                    egui::CursorIcon::Crosshair
                } else {
                    egui::CursorIcon::ResizeVertical
                }
            });
            self.row_drag = if released {
                None
            } else {
                Some((anchor, ctx.cumulative_pass_nr()))
            };
        }
        #[cfg(test)]
        ctx.data_mut(|d| {
            d.insert_temp(
                egui::Id::new("delimited-row-gutter"),
                egui::Rect::from_min_size(
                    rect.min + egui::vec2(0.0, ROW_HEIGHT),
                    Vec2::new(NUMBER_WIDTH, ROW_HEIGHT),
                ),
            );
        });
    }
}
