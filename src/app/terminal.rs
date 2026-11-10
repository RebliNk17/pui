//! Project-local terminals dock below the chat; hiding never kills the shell.
use super::*;
use crate::terminal::Session;
use std::{collections::BTreeMap, path::Path};

type Selection = Option<((u16, u16), (u16, u16))>;

#[derive(Default)]
pub(super) struct Terminals {
    open: bool,
    expanded: bool,
    root: PathBuf,
    projects: BTreeMap<PathBuf, Project>,
    focus: bool,
    confirm_stop: bool,
}
#[derive(Default)]
struct Project {
    session: Option<Session>,
    starting: Option<Receiver<Result<Session, String>>>,
    error: Option<String>,
    selection: Selection,
    scroll: f32,
    demo: Option<vt100::Parser>,
}

fn terminal_id() -> egui::Id {
    egui::Id::new("project-terminal-input")
}

impl Terminals {
    pub(super) fn is_open(&self) -> bool {
        self.open
    }
    pub(super) fn confirming_stop(&self) -> bool {
        self.confirm_stop
    }
    pub(super) fn focused(&self, ctx: &egui::Context) -> bool {
        self.open && ctx.memory(|m| m.has_focus(terminal_id()))
    }
    pub(super) fn running(&self) -> bool {
        self.projects
            .values()
            .any(|p| p.starting.is_some() || p.session.as_ref().is_some_and(Session::running))
    }
    pub(super) fn shutdown(&mut self) {
        self.projects.clear();
    }
    pub(super) fn toggle(&mut self, ctx: &egui::Context, root: &Path, demo: bool) {
        self.open = !self.open;
        if self.open {
            self.workspace(ctx, root, demo);
            self.focus = true;
        } else {
            ctx.memory_mut(|m| m.surrender_focus(terminal_id()));
        }
    }
    fn workspace(&mut self, ctx: &egui::Context, root: &Path, demo: bool) {
        if self.root != root {
            self.root = root.to_path_buf();
            self.focus = true;
            self.confirm_stop = false;
        }
        if !self.projects.contains_key(root) {
            self.start(ctx, demo);
        }
    }
    fn start(&mut self, ctx: &egui::Context, demo: bool) {
        let mut project = Project::default();
        if demo {
            let mut parser =
                vt100::Parser::new(16.try_into().unwrap(), 90.try_into().unwrap(), 100);
            parser.process(b"\x1b[32mproject\x1b[0m \x1b[36m$\x1b[0m pwd\r\n");
            parser.process(self.root.to_string_lossy().as_bytes());
            parser.process(b"\r\n\x1b[32mproject\x1b[0m \x1b[36m$\x1b[0m ");
            project.demo = Some(parser);
        } else {
            let (tx, rx) = mpsc::channel();
            project.starting = Some(rx);
            let root = self.root.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = Session::start(&ctx, &root, portable_pty::PtySize::default());
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        }
        self.projects.insert(self.root.clone(), project);
    }
    pub(super) fn show(&mut self, ctx: &egui::Context, root: &Path, demo: bool) -> bool {
        self.workspace(ctx, root, demo);
        let colors = theme::palette(ctx);
        let height = ctx.available_rect().height();
        let frame = egui::Frame::new()
            .fill(colors.bg)
            .inner_margin(egui::Margin::symmetric(16, 8))
            .stroke(Stroke::new(1.0, colors.hover));
        let expanded = self.expanded;
        if expanded {
            egui::CentralPanel::default()
                .frame(frame)
                .show(ctx, |ui| self.content(ui, demo));
        } else {
            egui::TopBottomPanel::bottom("project-terminal")
                .resizable(true)
                .default_height((height * 0.38).clamp(180.0, 320.0))
                .height_range(140.0..=(height * 0.75).max(140.0))
                .frame(frame)
                .show(ctx, |ui| self.content(ui, demo));
        }
        if self.confirm_stop {
            let choice = confirmations::Dialog::new(
                "stop-project-terminal",
                "Stop this terminal?",
                "This ends its shell and interrupts any commands still running.",
                "Keep running",
                "Stop terminal",
            )
            .show(ctx, |ui| {
                confirmations::note(
                    ui,
                    &format!(
                        "Project: {}\nFiles and chat sessions will be kept.",
                        self.root.display()
                    ),
                );
            });
            match choice {
                Some(confirmations::Choice::Cancel) => self.confirm_stop = false,
                Some(confirmations::Choice::Confirm) => {
                    self.projects.remove(&self.root);
                    self.confirm_stop = false;
                    self.open = false;
                    ctx.memory_mut(|m| m.surrender_focus(terminal_id()));
                }
                None => {}
            }
        }
        expanded
    }
    fn content(&mut self, ui: &mut egui::Ui, demo: bool) {
        let colors = theme::palette(ui.ctx());
        let mut restart = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Terminal").size(12.0).color(colors.text));
            let path = self.root.to_string_lossy();
            ui.add_sized(
                Vec2::new((ui.available_width() - 116.0).max(20.0), 28.0),
                egui::Label::new(RichText::new(path.as_ref()).size(11.0).color(colors.muted))
                    .truncate(),
            )
            .on_hover_text(path.as_ref());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, Icon::Close, "Hide terminal · commands keep running").clicked()
                {
                    self.open = false;
                    ui.ctx().memory_mut(|m| m.surrender_focus(terminal_id()));
                }
                if icons::button(
                    ui,
                    if self.expanded {
                        Icon::Collapse
                    } else {
                        Icon::Expand
                    },
                    if self.expanded {
                        "Restore terminal"
                    } else {
                        "Expand terminal"
                    },
                )
                .clicked()
                {
                    self.expanded = !self.expanded;
                }
                let running = self.projects.get(&self.root).is_some_and(|p| {
                    p.starting.is_some() || p.session.as_ref().is_some_and(Session::running)
                });
                if running {
                    if icons::button(ui, Icon::Stop, "Stop terminal…").clicked() {
                        self.confirm_stop = true;
                    }
                } else if icons::button(ui, Icon::Refresh, "Start a new terminal").clicked() {
                    restart = true;
                }
            });
        });
        ui.add_space(4.0);
        if restart {
            self.start(ui.ctx(), demo);
            self.focus = true;
        }
        let Some(project) = self.projects.get_mut(&self.root) else {
            return;
        };
        if let Some(rx) = &project.starting {
            match rx.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(session) => project.session = Some(session),
                        Err(error) => project.error = Some(error),
                    }
                    project.starting = None;
                    self.focus = true;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    project.starting = None;
                    project.error = Some("Could not start terminal.".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(error) = &project.error {
            ui.label(RichText::new(error).size(11.0).color(colors.muted));
        }
        if project.starting.is_some() {
            ui.label(theme::small(ui, "Starting shell…"));
        }
        if let Some(session) = project.session.as_mut() {
            let (status, error) = {
                let state = session.state.lock().unwrap();
                (state.exited.clone(), state.error.clone())
            };
            if let Some(status) = status.or(error) {
                ui.label(theme::small(ui, &status));
            }
        }
        let font = FontId::monospace(12.0);
        let cell = ui.fonts(|f| Vec2::new(f.glyph_width(&font, 'M'), f.row_height(&font).ceil()));
        let size = ui.available_size().max(Vec2::splat(1.0));
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let response = ui.interact(rect, terminal_id(), Sense::click_and_drag());
        if self.focus || response.clicked() || response.drag_started() {
            response.request_focus();
            self.focus = false;
        }
        if response.has_focus() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    terminal_id(),
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }
        let rows = ((rect.height() / cell.y) as u16).clamp(1, 200);
        let cols = ((rect.width() / cell.x) as u16).clamp(1, 400);
        if let Some(parser) = project.demo.as_mut() {
            parser.set_size(rows.try_into().unwrap(), cols.try_into().unwrap());
            interact(
                ui,
                &response,
                rect,
                cell,
                parser,
                &mut project.selection,
                &mut project.scroll,
            );
            paint(
                ui,
                rect,
                cell,
                &font,
                parser.screen(),
                project.selection,
                response.has_focus(),
            );
        }
        if let Some(session) = project.session.as_mut() {
            session.resize(rows, cols);
            let mut state = session.state.lock().unwrap();
            let input = interact(
                ui,
                &response,
                rect,
                cell,
                &mut state.parser,
                &mut project.selection,
                &mut project.scroll,
            );
            paint(
                ui,
                rect,
                cell,
                &font,
                state.parser.screen(),
                project.selection,
                response.has_focus(),
            );
            drop(state);
            if let Err(error) = session.send(input) {
                project.error = Some(error);
            }
        }
    }
}

fn interact<CB: vt100::Callbacks>(
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
    cell: Vec2,
    parser: &mut vt100::Parser<CB>,
    selection: &mut Selection,
    scroll: &mut f32,
) -> Vec<u8> {
    let screen = parser.screen_mut();
    let (rows, cols) = screen.size();
    if response.hovered() {
        *scroll += ui.input(|i| i.smooth_scroll_delta.y) / cell.y;
        let lines = scroll.trunc() as isize;
        if lines != 0 {
            screen.set_scrollback(screen.scrollback().saturating_add_signed(lines));
            *scroll -= lines as f32;
            *selection = None;
        }
    }
    if let Some(pos) = response.interact_pointer_pos() {
        let point = (
            (((pos.y - rect.top()) / cell.y) as u16).min(rows.get() - 1),
            (((pos.x - rect.left()) / cell.x) as u16).min(cols.get() - 1),
        );
        if response.drag_started() {
            *selection = Some((point, point));
        } else if response.dragged() {
            if let Some((_, end)) = selection {
                *end = (point.0, (point.1 + 1).min(cols.get()));
            }
        } else if response.clicked() {
            *selection = None;
        }
    }
    let mut input = Vec::new();
    if response.has_focus() {
        let mut copied = None;
        ui.input_mut(|i| {
            let interrupt = !cfg!(target_os = "macos") && i.modifiers.ctrl && !i.modifiers.shift;
            i.events.retain(|event| {
                if matches!(event, egui::Event::Copy) {
                    if interrupt {
                        input.push(3);
                        return false;
                    }
                    let text = if let Some((a, b)) = *selection {
                        let (a, b) = if a <= b { (a, b) } else { (b, a) };
                        screen.contents_between(a.0, a.1, b.0, b.1)
                    } else {
                        String::new()
                    };
                    if !text.is_empty() {
                        copied = Some(text);
                    }
                    return false;
                }
                if let Some(bytes) =
                    encode(event, screen.application_cursor(), screen.bracketed_paste())
                {
                    input.extend(bytes);
                    return false;
                }
                true
            });
        });
        if let Some(text) = copied {
            ui.ctx().copy_text(text);
        }
        if !input.is_empty() {
            screen.set_scrollback(0);
            *selection = None;
        }
    }
    input
}

fn encode(event: &egui::Event, application_cursor: bool, bracketed: bool) -> Option<Vec<u8>> {
    use egui::{Event, Key};
    match event {
        Event::Text(text) => Some(text.as_bytes().to_vec()),
        Event::Paste(text) => {
            // Strip terminal controls so a paste cannot terminate its own bracket.
            let text: String = text
                .chars()
                .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
                .collect();
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            Some(if bracketed {
                format!("\x1b[200~{text}\x1b[201~").into_bytes()
            } else {
                text.replace('\n', "\r").into_bytes()
            })
        }
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => {
            if modifiers.mac_cmd {
                return None;
            }
            if modifiers.ctrl
                && !modifiers.alt
                && matches!(
                    key,
                    Key::A
                        | Key::B
                        | Key::C
                        | Key::D
                        | Key::E
                        | Key::F
                        | Key::G
                        | Key::H
                        | Key::I
                        | Key::J
                        | Key::K
                        | Key::L
                        | Key::M
                        | Key::N
                        | Key::O
                        | Key::P
                        | Key::Q
                        | Key::R
                        | Key::S
                        | Key::T
                        | Key::U
                        | Key::V
                        | Key::W
                        | Key::X
                        | Key::Y
                        | Key::Z
                        | Key::OpenBracket
                        | Key::Backslash
                        | Key::CloseBracket
                        | Key::Space
                )
            {
                let control = match key {
                    Key::A => 1,
                    Key::B => 2,
                    Key::C => 3,
                    Key::D => 4,
                    Key::E => 5,
                    Key::F => 6,
                    Key::G => 7,
                    Key::H => 8,
                    Key::I => 9,
                    Key::J => 10,
                    Key::K => 11,
                    Key::L => 12,
                    Key::M => 13,
                    Key::N => 14,
                    Key::O => 15,
                    Key::P => 16,
                    Key::Q => 17,
                    Key::R => 18,
                    Key::S => 19,
                    Key::T => 20,
                    Key::U => 21,
                    Key::V => 22,
                    Key::W => 23,
                    Key::X => 24,
                    Key::Y => 25,
                    Key::Z => 26,
                    Key::OpenBracket => 27,
                    Key::Backslash => 28,
                    Key::CloseBracket => 29,
                    Key::Space => 0,
                    _ => return None,
                };
                return Some(vec![control]);
            }
            let arrow = match key {
                Key::ArrowUp => Some('A'),
                Key::ArrowDown => Some('B'),
                Key::ArrowRight => Some('C'),
                Key::ArrowLeft => Some('D'),
                Key::Home => Some('H'),
                Key::End => Some('F'),
                _ => None,
            };
            if let Some(arrow) = arrow {
                let modifier = 1
                    + usize::from(modifiers.shift)
                    + 2 * usize::from(modifiers.alt)
                    + 4 * usize::from(modifiers.ctrl);
                return Some(if modifier > 1 {
                    format!("\x1b[1;{modifier}{arrow}").into_bytes()
                } else {
                    format!("\x1b{}{arrow}", if application_cursor { 'O' } else { '[' })
                        .into_bytes()
                });
            }
            let text = match key {
                Key::Enter => "\r",
                Key::Backspace => "\x7f",
                Key::Escape => "\x1b",
                Key::Tab if modifiers.shift => "\x1b[Z",
                Key::Tab => "\t",
                Key::Insert => "\x1b[2~",
                Key::Delete => "\x1b[3~",
                Key::PageUp => "\x1b[5~",
                Key::PageDown => "\x1b[6~",
                Key::F1 => "\x1bOP",
                Key::F2 => "\x1bOQ",
                Key::F3 => "\x1bOR",
                Key::F4 => "\x1bOS",
                Key::F5 => "\x1b[15~",
                Key::F6 => "\x1b[17~",
                Key::F7 => "\x1b[18~",
                Key::F8 => "\x1b[19~",
                Key::F9 => "\x1b[20~",
                Key::F10 => "\x1b[21~",
                Key::F11 => "\x1b[23~",
                Key::F12 => "\x1b[24~",
                _ => return None,
            };
            let mut bytes = Vec::new();
            if modifiers.alt {
                bytes.push(27);
            }
            bytes.extend_from_slice(text.as_bytes());
            Some(bytes)
        }
        _ => None,
    }
}

fn color(color: vt100::Color, default: Color32, bright: bool) -> Color32 {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        vt100::Color::Idx(mut n) => {
            if bright && n < 8 {
                n += 8;
            }
            let rgb = match n {
                0..=15 => [
                    [30, 30, 30],
                    [205, 70, 70],
                    [80, 175, 100],
                    [200, 165, 65],
                    [85, 135, 215],
                    [175, 100, 200],
                    [65, 165, 175],
                    [195, 195, 195],
                    [105, 105, 105],
                    [245, 105, 105],
                    [115, 215, 135],
                    [235, 205, 110],
                    [130, 175, 245],
                    [215, 145, 235],
                    [110, 215, 225],
                    [245, 245, 245],
                ][n as usize],
                16..=231 => {
                    let n = n - 16;
                    let channel = |n| if n == 0 { 0 } else { 55 + n * 40 };
                    [channel(n / 36), channel(n / 6 % 6), channel(n % 6)]
                }
                _ => [8 + (n - 232) * 10; 3],
            };
            Color32::from_rgb(rgb[0], rgb[1], rgb[2])
        }
    }
}

fn paint(
    ui: &egui::Ui,
    rect: egui::Rect,
    cell_size: Vec2,
    font: &FontId,
    screen: &vt100::Screen,
    selection: Selection,
    focused: bool,
) {
    let colors = theme::palette(ui.ctx());
    let painter = ui.painter().with_clip_rect(rect);
    let (rows, cols) = screen.size();
    let selection = selection.map(|(a, b)| if a <= b { (a, b) } else { (b, a) });
    for row in 0..rows.get() {
        for col in 0..cols.get() {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let pos = rect.min + Vec2::new(col as f32 * cell_size.x, row as f32 * cell_size.y);
            let size = Vec2::new(
                cell_size.x * if cell.is_wide() { 2.0 } else { 1.0 },
                cell_size.y,
            );
            let cell_rect = egui::Rect::from_min_size(pos, size);
            let mut fg = color(cell.fgcolor(), colors.text, cell.bold());
            let mut bg = color(cell.bgcolor(), colors.bg, false);
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.dim() {
                fg = fg.gamma_multiply(0.6);
            }
            if selection.is_some_and(|(a, b)| a <= (row, col) && (row, col) < b) {
                bg = colors.accent_bg;
            }
            if bg != colors.bg {
                painter.rect_filled(cell_rect, 0, bg);
            }
            if cell.has_contents() {
                painter.text(
                    pos,
                    egui::Align2::LEFT_TOP,
                    cell.contents(),
                    font.clone(),
                    fg,
                );
                if cell.underline() {
                    painter.line_segment(
                        [cell_rect.left_bottom(), cell_rect.right_bottom()],
                        Stroke::new(1.0, fg),
                    );
                }
            }
        }
    }
    if !screen.hide_cursor() && screen.scrollback() == 0 {
        let (row, col) = screen.cursor_position();
        let cursor = egui::Rect::from_min_size(
            rect.min + Vec2::new(col as f32 * cell_size.x, row as f32 * cell_size.y),
            cell_size,
        );
        if focused {
            painter.rect_filled(cursor, 0, colors.accent.gamma_multiply(0.5));
        } else {
            painter.rect_stroke(
                cursor,
                0,
                Stroke::new(1.0, colors.muted),
                egui::StrokeKind::Inside,
            );
        }
    }
}

#[cfg(test)]
mod tests;
