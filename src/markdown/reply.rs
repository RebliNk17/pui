//! Assistant-only list geometry. Parser boundaries, never prose heuristics, define items.
use super::*;

pub(super) struct Item {
    marker: String,
    gutter: f32,
}

impl Item {
    pub(super) fn flush(&mut self, ui: &mut Ui, job: &mut LayoutJob, links: &mut Vec<Link>) {
        if self.marker.is_empty() {
            flush(ui, job, links);
            return;
        }
        let marker_len = self.marker.len();
        let mut marked = LayoutJob::default();
        let mut marker_format = format(ui.ctx(), false, false, false, false, false);
        marker_format.line_height = Some(21.0);
        marker_format.color = theme::palette(ui.ctx()).muted;
        marked.append(&self.marker, 0.0, marker_format);
        marked.text.push_str(&job.text);
        marked
            .sections
            .extend(job.sections.drain(..).map(|mut section| {
                section.byte_range.start += marker_len;
                section.byte_range.end += marker_len;
                section
            }));
        for link in links.iter_mut() {
            link.range.start += marker_len;
            link.range.end += marker_len;
        }
        marked.wrap.max_width = ui.available_width().max(1.0);
        marked.align_baseline = true;
        *job = LayoutJob::default();
        let mut galley = ui.fonts(|fonts| fonts.layout_job(marked));
        // Keep marker and body in ONE selectable label. Only move placed rows:
        // bytes, sections, bidi cursors and baseline boxes survive unchanged.
        let galley_mut = Arc::make_mut(&mut galley);
        let mut rect = egui::Rect::ZERO;
        for (index, row) in galley_mut.rows.iter_mut().enumerate() {
            if index > 0 {
                row.pos.x += self.gutter;
            }
            rect |= row.rect();
        }
        galley_mut.rect = rect;
        galley_mut.mesh_bounds.max.x += self.gutter;
        let rect = egui::Rect::from_min_max(
            ui.cursor().min - egui::vec2(self.gutter, 0.0),
            ui.max_rect().max,
        );
        let mut label_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        show_galley(&mut label_ui, galley, links);
        // Advancing with the label's gutter would expand this Ui leftwards,
        // causing every later paragraph and nested list to lose its inset.
        let mut used = label_ui.min_rect();
        used.min.x = ui.max_rect().left();
        ui.advance_cursor_after_rect(used);
        links.clear();
        self.marker.clear();
    }
}

struct List {
    number: Option<u64>,
    siblings: usize,
    indent: f32,
}
struct ActiveItem {
    marker: String,
    gutter: f32,
}

pub(super) fn list(ui: &mut Ui, events: &[Event<'static>], top_level: bool) {
    let mut lists: Vec<List> = Vec::new();
    let mut items: Vec<ActiveItem> = Vec::new();
    let mut block_start = 0;
    let mut quote_depth = 0usize;
    // An explicit stack keeps arbitrary model-supplied nesting off the call
    // stack. Once narrow, indentation stops growing, never content or markers.
    for (index, event) in events.iter().enumerate() {
        // Quote/callout renderers own their complete subtree. Nested list
        // boundaries there must not split the enclosing quote frame.
        match event {
            Event::Start(Tag::BlockQuote(_)) => quote_depth += 1,
            Event::End(TagEnd::BlockQuote(_)) => quote_depth = quote_depth.saturating_sub(1),
            _ => {}
        }
        if quote_depth > 0 || matches!(event, Event::End(TagEnd::BlockQuote(_))) {
            continue;
        }
        if !matches!(
            event,
            Event::Start(Tag::List(_) | Tag::Item) | Event::End(TagEnd::List(_) | TagEnd::Item)
        ) {
            continue;
        }
        if let Some(item) = items.last_mut() {
            render_block(
                ui,
                &events[block_start..index],
                item,
                lists.last().unwrap().indent,
            );
        }
        match event {
            Event::Start(Tag::List(number)) => {
                let indent = lists.last().map_or(0.0, |list| list.indent)
                    + items.last().map_or(0.0, |item| item.gutter);
                lists.push(List {
                    number: *number,
                    siblings: 0,
                    indent,
                });
            }
            Event::Start(Tag::Item) => {
                let list_depth = lists.len();
                let list = lists.last_mut().unwrap();
                if top_level && list_depth == 1 && list.number.is_some() && list.siblings > 0 {
                    ui.add_space(4.0);
                    let y = ui.cursor().top();
                    ui.painter().hline(
                        ui.max_rect().x_range(),
                        y,
                        egui::Stroke::new(1.0, theme::palette(ui.ctx()).border.gamma_multiply(0.5)),
                    );
                    ui.add_space(9.0);
                }
                let marker = list
                    .number
                    .map_or_else(|| "•  ".into(), |number| format!("{number}. "));
                list.number = list.number.map(|number| number.saturating_add(1));
                list.siblings += 1;
                let format = format(ui.ctx(), false, false, false, false, false);
                let gutter = ui.fonts(|fonts| {
                    fonts
                        .layout_no_wrap(marker.clone(), format.font_id, format.color)
                        .size()
                        .x
                });
                items.push(ActiveItem { marker, gutter });
            }
            Event::End(TagEnd::Item) => {
                items.pop();
            }
            Event::End(TagEnd::List(_)) => {
                lists.pop();
                ui.add_space(4.0);
            }
            _ => unreachable!(),
        }
        block_start = index + 1;
    }
}

fn render_block(ui: &mut Ui, events: &[Event<'static>], item: &mut ActiveItem, indent: f32) {
    if events.is_empty() && item.marker.is_empty() {
        return;
    }
    let indent = indent.min((ui.available_width() - 80.0).max(0.0));
    let gutter = item
        .gutter
        .min((ui.available_width() - indent - 64.0).max(0.0));
    let rect = egui::Rect::from_min_max(
        ui.cursor().min + egui::vec2(indent + gutter, 0.0),
        ui.max_rect().max,
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        show_events(
            ui,
            events,
            true,
            Some(Item {
                marker: std::mem::take(&mut item.marker),
                gutter,
            }),
        );
    });
}
