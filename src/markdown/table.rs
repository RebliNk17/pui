use super::{Link, pr_capsule, show_galley};
use crate::theme;
use eframe::egui::{self, Ui, text::LayoutJob};
use pulldown_cmark::Alignment;

#[derive(Clone)]
struct Cell {
    job: LayoutJob,
    links: Vec<Link>,
}

#[derive(Clone, Default)]
pub(super) struct Table {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<Cell>>,
}

fn expanded_table_id() -> egui::Id {
    egui::Id::new("pui-expanded-markdown-table")
}

pub(super) fn show_expanded_modal(ctx: &egui::Context) {
    let table: Option<std::sync::Arc<Table>> = ctx.data(|d| d.get_temp(expanded_table_id()));
    let Some(table) = table else {
        return;
    };
    let mut open = true;
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = false;
    }
    let max_size = ctx.available_rect().size() * 0.85;
    egui::Window::new("Table")
        .id(egui::Id::new("pui-expanded-markdown-table-window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .default_width(max_size.x.min(960.0))
        .max_size(max_size)
        .show(ctx, |ui| {
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    table.render(ui, false);
                });
        });
    if !open {
        ctx.data_mut(|d| d.remove_temp::<std::sync::Arc<Table>>(expanded_table_id()));
    }
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

    pub(super) fn push_cell(&mut self, mut job: LayoutJob, links: Vec<Link>) {
        job.align_baseline = true;
        self.rows.last_mut().unwrap().push(Cell { job, links });
    }

    pub(super) fn show(self, ui: &mut Ui) {
        self.render(ui, true);
    }

    fn render(&self, ui: &mut Ui, allow_expand: bool) {
        const PADDING: egui::Vec2 = egui::vec2(10.0, 6.0);
        let columns = self.alignments.len();
        if columns == 0 {
            return;
        }
        // Measure before painting: no Grid sizing pass or previous-frame geometry.
        let mut widths = vec![80.0_f32; columns];
        for row in &self.rows {
            for (column, cell) in row.iter().enumerate().take(columns) {
                let width = pr_capsule::layout(ui, cell.job.clone(), &cell.links)
                    .size()
                    .x;
                widths[column] = widths[column].max(width.min(320.0));
            }
        }
        let available = ui.available_width() - 2.0 * PADDING.x * columns as f32;
        let preferred: f32 = widths.iter().sum();
        let minimum = 80.0 * columns as f32;
        let is_wide = preferred > available;
        if is_wide && preferred > minimum {
            let scale = ((available - minimum) / (preferred - minimum)).clamp(0.0, 1.0);
            for width in &mut widths {
                *width = 80.0 + (*width - 80.0) * scale;
            }
        }
        let width = widths.iter().sum::<f32>() + 2.0 * PADDING.x * columns as f32;
        let colors = theme::palette(ui.ctx());
        let stroke = egui::Stroke::new(1.0, colors.border);
        let scroll_out = egui::ScrollArea::horizontal()
            .id_salt(ui.next_auto_id())
            .show(ui, |ui| {
                let mut rows = Vec::new();
                let mut height = 0.0;
                for row in &self.rows {
                    let mut row_height = 24.0_f32;
                    let cells: Vec<_> = row
                        .iter()
                        .enumerate()
                        .take(columns)
                        .map(|(column, cell)| {
                            let mut job = cell.job.clone();
                            job.wrap.max_width = widths[column];
                            job.halign = match self.alignments[column] {
                                Alignment::Center => egui::Align::Center,
                                Alignment::Right => egui::Align::Max,
                                _ => egui::Align::Min,
                            };
                            let galley = pr_capsule::layout(ui, job, &cell.links);
                            row_height = row_height.max(galley.size().y);
                            (galley, cell.links.clone())
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
        if allow_expand && is_wide {
            let btn_rect = egui::Rect::from_min_size(
                egui::pos2(
                    scroll_out.inner_rect.right() - 22.0,
                    scroll_out.inner_rect.top() + 3.0,
                ),
                egui::vec2(19.0, 19.0),
            );
            let resp = ui
                .interact(btn_rect, ui.id().with("expand-table"), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Expand wide table");
            ui.painter().rect(
                btn_rect,
                4.0,
                if resp.hovered() {
                    colors.hover
                } else {
                    colors.surface
                },
                egui::Stroke::new(1.0_f32, colors.border),
                egui::StrokeKind::Inside,
            );
            crate::icons::draw(
                ui,
                btn_rect.shrink(3.0),
                crate::icons::Icon::Expand,
                colors.muted,
            );
            if resp.clicked() {
                let arc = std::sync::Arc::new(self.clone());
                ui.ctx()
                    .data_mut(|d| d.insert_temp(expanded_table_id(), arc));
            }
        }
    }
}
