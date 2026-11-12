//! Explicit, frozen terminal viewport snapshots. No command inference or scrollback scan.
use super::*;
use std::sync::Arc;

const LIMIT: usize = 24 * 1024;

#[derive(Clone)]
struct Snapshot {
    label: String,
    text: String,
    selection: bool,
}

#[derive(Clone)]
struct Capture {
    snapshot: Arc<Snapshot>,
    owner: egui::Id,
    trigger: egui::Id,
    root: PathBuf,
    pass: u64,
}

fn key() -> egui::Id {
    egui::Id::new("terminal-context-snapshot")
}

pub(super) fn clear(ctx: &egui::Context) {
    if let Some(captured) = ctx.data_mut(|data| {
        let saved = data.get_temp::<Capture>(key());
        data.remove::<Capture>(key());
        saved
    }) {
        egui::Popup::close_id(ctx, captured.owner);
    }
}

pub(super) fn inputs(ctx: &egui::Context) {
    if ctx.memory(|memory| memory.top_modal_layer().is_some()) {
        clear(ctx);
        return;
    }
    let Some(captured) = ctx.data(|data| data.get_temp::<Capture>(key())) else {
        return;
    };
    if !egui::Popup::is_id_open(ctx, captured.owner) {
        clear(ctx);
        return;
    }
    let escape = ctx.input_mut(|input| {
        input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
        input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
    });
    if escape {
        clear(ctx);
        ctx.memory_mut(|memory| memory.request_focus(captured.trigger));
    }
}

/// Build at most 24 KiB + one UTF-8 scalar; queue() owns the shared truncation marker.
/// vt100 extracts one bounded row at a time, never the entire history/screen.
fn snapshot(screen: &vt100::Screen, selection: Selection, root: &Path) -> Snapshot {
    let (rows, cols) = screen.size();
    let rows = rows.get().min(200);
    let cols = cols.get().min(400);
    let point = |(row, col): (u16, u16)| (row.min(rows - 1), col.min(cols));
    let range = selection
        .map(|(a, b)| {
            let (a, b) = (point(a), point(b));
            if a <= b { (a, b) } else { (b, a) }
        })
        .filter(|(a, b)| a != b);
    let (start, end) = range.unwrap_or(((0, 0), (rows - 1, cols)));
    let mut text = String::new();
    'rows: for (row, line) in screen
        .rows(0, cols)
        .enumerate()
        .take(usize::from(end.0) + 1)
        .skip(usize::from(start.0))
    {
        let row = row as u16;
        let first = if row == start.0 { start.1 } else { 0 };
        let last = if row == end.0 { end.1 } else { cols };
        let line = if first != 0 || last != cols {
            screen.contents_between(row, first, row, last)
        } else {
            line
        };
        for character in line
            .chars()
            .chain((row != end.0 && !screen.row_wrapped(row)).then_some('\n'))
        {
            text.push(character);
            if text.len() > LIMIT {
                break 'rows;
            }
        }
    }
    if range.is_none() && text.len() <= LIMIT {
        // Empty viewport padding is not terminal output. Selected newlines are kept.
        text.truncate(text.trim_end_matches('\n').len());
    }
    let label = format!(
        "Terminal {} · viewport {}:{}–{}:{} · scrollback offset {} · project {}",
        if range.is_some() {
            "selection"
        } else {
            "visible output"
        },
        start.0 + 1,
        start.1 + 1,
        end.0 + 1,
        end.1 + 1,
        screen.scrollback(),
        root.to_string_lossy()
            .chars()
            .filter(|character| !character.is_control())
            .take(512)
            .collect::<String>()
    );
    Snapshot {
        label,
        text,
        selection: range.is_some(),
    }
}

pub(super) fn offer(
    response: &egui::Response,
    screen: &vt100::Screen,
    selection: Selection,
    root: &Path,
    toolbar: bool,
) {
    let ctx = &response.ctx;
    let id = response.id.with("terminal-context");
    if ctx.data(|data| {
        data.get_temp::<Capture>(key())
            .is_some_and(|saved| saved.root != root)
    }) {
        clear(ctx);
    }
    // Standalone rendering also protects inline approvals/composer; the app calls this early.
    inputs(ctx);
    let popup = if toolbar {
        egui::Popup::menu(response)
    } else {
        egui::Popup::context_menu(response)
    };
    popup.id(id).frame(prompt_ui::popup(ctx)).show(|ui| {
        theme::menu_controls(ui);
        ui.set_width(250.0_f32.min((ctx.screen_rect().width() - 40.0).max(120.0)));
        let pass = ctx.cumulative_pass_nr();
        let key = key();
        // Refresh only on opening/reopening. Output can keep streaming underneath a menu.
        let refresh = ctx.data(|data| data.get_temp::<Capture>(key).is_none_or(|saved| saved.owner != id || saved.pass + 1 < pass))
            || (toolbar && response.clicked()) || (!toolbar && response.secondary_clicked());
        if refresh { ctx.data_mut(|data| data.insert_temp(key, Capture { snapshot: Arc::new(snapshot(screen, selection, root)), owner: id, trigger: response.id, root: root.into(), pass })); }
        let captured = ctx.data_mut(|data| {
            let mut captured = data.get_temp::<Capture>(key).expect("snapshot initialized");
            captured.pass = pass;
            let snapshot = captured.snapshot.clone();
            data.insert_temp(key, captured);
            snapshot
        });
        let label = if captured.selection { "Add terminal selection to chat" } else { "Add visible terminal output to chat" };
        ui.add(egui::Label::new(theme::small(ui, if captured.selection { "Selected screen text · snapshot at menu open" } else { "Visible viewport only · not full scrollback" })).wrap());
        let add = ui.add_enabled(!captured.text.is_empty(), egui::Button::new(label).wrap());
        if refresh && !captured.text.is_empty() { add.request_focus(); }
        if add.clicked() {
            context_capture::queue(ctx, &captured.label, &captured.text);
            ui.close();
        }
        ui.add(egui::Label::new(theme::small(ui, "Adds up to 24 KiB to your draft. Review before sending; terminal output may contain secrets.")).wrap());
        #[cfg(test)]
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("terminal-context-add"), add.rect));
    });
    if !egui::Popup::is_id_open(ctx, id)
        && ctx.data(|data| {
            data.get_temp::<Capture>(key())
                .is_some_and(|saved| saved.owner == id)
        })
    {
        clear(ctx);
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
