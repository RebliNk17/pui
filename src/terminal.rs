//! A real PTY shell, with bounded VT screen/scrollback and blocking worker I/O.
//! Output is local UI state only: it is never added to the Pi conversation.
use eframe::egui;
use portable_pty::{CommandBuilder, PtySize};
use std::{
    io::{Read, Write},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
};

pub const SCROLLBACK: usize = 2_000;
const MAX_INPUT: usize = 1024 * 1024;

pub struct State {
    pub parser: vt100::Parser<Replies>,
    pub exited: Option<String>,
    pub error: Option<String>,
}
impl State {
    fn new(size: PtySize, input: SyncSender<Input>) -> Self {
        Self {
            parser: vt100::Parser::new_with_callbacks(
                size.rows.max(1).try_into().unwrap(),
                size.cols.max(1).try_into().unwrap(),
                SCROLLBACK,
                Replies(input),
            ),
            exited: None,
            error: None,
        }
    }
}

enum Input {
    Write(Vec<u8>),
    Resize(PtySize),
    Shutdown,
}

pub struct Replies(SyncSender<Input>);
impl vt100::Callbacks for Replies {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        if i1.is_some() || i2.is_some() {
            return;
        }
        let param = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        let reply = match (c, param) {
            ('n', 5) => b"\x1b[0n".to_vec(),
            ('n', 6) => {
                let (row, col) = screen.cursor_position();
                format!("\x1b[{};{}R", row + 1, col + 1).into_bytes()
            }
            ('c', 0) => b"\x1b[?1;2c".to_vec(),
            _ => return,
        };
        let _ = self.0.try_send(Input::Write(reply));
    }
}

pub struct Session {
    pub state: Arc<Mutex<State>>,
    input: SyncSender<Input>,
    killer: Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>,
    finished: Arc<AtomicBool>,
    size: PtySize,
}
impl Session {
    /// Call on a worker: opening a PTY and spawning a shell can block.
    pub fn start(ctx: &egui::Context, root: &Path, size: PtySize) -> Result<Self, String> {
        if !root.is_dir() {
            return Err("The project folder is no longer available.".into());
        }
        let mut command = CommandBuilder::new_default_prog();
        #[cfg(unix)]
        {
            command = CommandBuilder::new(command.get_shell());
            command.arg("-l");
        }
        command.cwd(root);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "Pui");
        // The PTY's actual size, not inherited dimensions, is authoritative.
        command.env_remove("LINES");
        command.env_remove("COLUMNS");
        Self::spawn(ctx, command, size)
    }

    fn spawn(ctx: &egui::Context, command: CommandBuilder, size: PtySize) -> Result<Self, String> {
        let pair = portable_pty::native_pty_system()
            .openpty(size)
            .map_err(|e| format!("Cannot open terminal: {e}"))?;
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| e.to_string())?;
        drop(pair.slave);
        let killer = child.clone_killer();
        let (input, rx) = mpsc::sync_channel(64);
        let state = Arc::new(Mutex::new(State::new(size, input.clone())));
        let finished = Arc::new(AtomicBool::new(false));

        let output = state.clone();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let mut buffer = [0; 16 * 1024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        output.lock().unwrap().parser.process(&buffer[..n]);
                        repaint.request_repaint();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    // Unix PTYs can report EIO instead of EOF after shell exit.
                    Err(_) => break,
                }
            }
        });

        let output = state.clone();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            while let Ok(input) = rx.recv() {
                let result = match input {
                    Input::Write(bytes) => writer.write_all(&bytes).and_then(|()| writer.flush()),
                    Input::Resize(size) => {
                        let result = pair.master.resize(size).map_err(std::io::Error::other);
                        if result.is_ok() {
                            output.lock().unwrap().parser.set_size(
                                size.rows.max(1).try_into().unwrap(),
                                size.cols.max(1).try_into().unwrap(),
                            );
                            repaint.request_repaint();
                        }
                        result
                    }
                    Input::Shutdown => break,
                };
                if let Err(e) = result {
                    output.lock().unwrap().error = Some(format!("Terminal I/O failed: {e}"));
                    repaint.request_repaint();
                    break;
                }
            }
        });

        let output = state.clone();
        let repaint = ctx.clone();
        let done = finished.clone();
        std::thread::spawn(move || {
            let status = child.wait();
            done.store(true, Ordering::Release);
            output.lock().unwrap().exited = Some(match status {
                Ok(status) => format!("Shell exited · {status}"),
                Err(e) => format!("Could not wait for shell: {e}"),
            });
            repaint.request_repaint();
        });
        Ok(Self {
            state,
            input,
            killer: Some(killer),
            finished,
            size,
        })
    }

    pub fn send(&self, bytes: Vec<u8>) -> Result<(), String> {
        if bytes.is_empty() {
            return Ok(());
        }
        if bytes.len() > MAX_INPUT {
            return Err("Paste is too large (maximum 1 MiB).".into());
        }
        if self.finished.load(Ordering::Acquire) {
            return Err("The shell has exited. Start a new terminal to continue.".into());
        }
        self.input
            .try_send(Input::Write(bytes))
            .map_err(|_| "Terminal input is busy or disconnected. Try again.".into())
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let size = PtySize {
            rows: rows.clamp(1, 200),
            cols: cols.clamp(1, 400),
            ..Default::default()
        };
        if size != self.size && self.input.try_send(Input::Resize(size)).is_ok() {
            self.size = size;
        }
    }

    pub fn running(&self) -> bool {
        !self.finished.load(Ordering::Acquire)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        // Shutdown and reap off the UI thread. The waiter owns/reaps the child.
        let killer = self.killer.take();
        let finished = self.finished.clone();
        let input = self.input.clone();
        std::thread::spawn(move || {
            if !finished.load(Ordering::Acquire)
                && let Some(mut killer) = killer
            {
                let _ = killer.kill();
            }
            let _ = input.send(Input::Shutdown);
        });
    }
}

#[cfg(test)]
mod tests;
