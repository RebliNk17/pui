use super::{Link, show_galley};
use crate::theme;
use eframe::egui::{self, Ui, text::LayoutJob};
use pulldown_cmark::Alignment;

struct Cell {
    job: LayoutJob,
    links: Vec<Link>,
}

pub(super) struct Table {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<Cell>>,
}

impl Table {
    pub(super) fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            alignments,
            rows: vec![Vec::new()], // TableHead contains cells directly, without TableRow.
        }
    }

    pub(super) fn start_row(&mut self) {
        self.rows.push(Vec::new());
    }

    pub(super) fn push_cell(&mut self, job: LayoutJob, links: Vec<Link>) {
        self.rows.last_mut().unwrap().push(Cell { job, links });
    }

    pub(super) fn show(self, ui: &mut Ui) {
        const PADDING: egui::Vec2 = egui::vec2(10.0, 6.0);
        let columns = self.alignments.len();
        if columns == 0 {
            return;
        }
        // Measure before painting: no Grid sizing pass or previous-frame geometry.
        let mut widths = vec![80.0_f32; columns];
        for row in &self.rows {
            for (column, cell) in row.iter().enumerate().take(columns) {
                let width = ui.fonts(|fonts| fonts.layout_job(cell.job.clone()).size().x);
                widths[column] = widths[column].max(width.min(320.0));
            }
        }
        let available = ui.available_width() - 2.0 * PADDING.x * columns as f32;
        let preferred: f32 = widths.iter().sum();
        let minimum = 80.0 * columns as f32;
        if preferred > available && preferred > minimum {
            let scale = ((available - minimum) / (preferred - minimum)).clamp(0.0, 1.0);
            for width in &mut widths {
                *width = 80.0 + (*width - 80.0) * scale;
            }
        }
        let width = widths.iter().sum::<f32>() + 2.0 * PADDING.x * columns as f32;
        let colors = theme::palette(ui.ctx());
        let stroke = egui::Stroke::new(1.0, colors.border);
        egui::ScrollArea::horizontal()
            .id_salt(ui.next_auto_id())
            .show(ui, |ui| {
                let mut rows = Vec::new();
                let mut height = 0.0;
                for row in self.rows {
                    let mut row_height = 24.0_f32;
                    let cells: Vec<_> = row
                        .into_iter()
                        .enumerate()
                        .take(columns)
                        .map(|(column, mut cell)| {
                            cell.job.wrap.max_width = widths[column];
                            cell.job.halign = match self.alignments[column] {
                                Alignment::Center => egui::Align::Center,
                                Alignment::Right => egui::Align::Max,
                                _ => egui::Align::Min,
                            };
                            let galley = ui.fonts(|fonts| fonts.layout_job(cell.job));
                            row_height = row_height.max(galley.size().y);
                            (galley, cell.links)
                        })
                        .collect();
                    row_height += 2.0 * PADDING.y;
                    height += row_height;
                    rows.push((row_height, cells));
                }
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
                let mut y = rect.top();
                for (row_index, (row_height, cells)) in rows.into_iter().enumerate() {
                    if row_index == 0 {
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(
                                egui::pos2(rect.left(), y),
                                egui::vec2(width, row_height),
                            ),
                            0,
                            colors.surface,
                        );
                    } else {
                        ui.painter().hline(rect.x_range(), y, stroke);
                    }
                    let mut x = rect.left();
                    for (column, (galley, links)) in cells.into_iter().enumerate() {
                        let content_rect = egui::Rect::from_min_size(
                            egui::pos2(x, y) + PADDING,
                            egui::vec2(widths[column], row_height - 2.0 * PADDING.y),
                        );
                        let mut cell_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .id_salt((row_index, column))
                                .max_rect(content_rect)
                                .layout(egui::Layout::top_down(galley.job.halign)),
                        );
                        show_galley(&mut cell_ui, galley, &links);
                        x += widths[column] + 2.0 * PADDING.x;
                    }
                    y += row_height;
                }
                let mut x = rect.left();
                for width in widths.iter().take(columns - 1) {
                    x += width + 2.0 * PADDING.x;
                    ui.painter().vline(x, rect.y_range(), stroke);
                }
                ui.painter()
                    .rect_stroke(rect, 0, stroke, egui::StrokeKind::Inside);
            });
    }
}
