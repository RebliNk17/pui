//! One worker owns each Pi process. UI commands never block the rendering thread.
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

pub enum Event {
    Record(Value),
    History(Vec<crate::protocol::Message>),
    Error(String),
    Exited,
}

enum Request {
    Record(Value),
    Shutdown,
    Eof,
}

pub struct Client {
    tx: Sender<Request>,
    pub events: Receiver<Event>,
    next_id: u64,
    worker: Option<thread::JoinHandle<()>>,
}

#[cfg(not(test))]
pub fn pi_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("PUI_PI_BIN") {
        return path.into();
    }
    if let Some(home) = dirs::home_dir() {
        let installed =
            home.join(".pi/agent/bin")
                .join(if cfg!(windows) { "pi.cmd" } else { "pi" });
        if installed.is_file() {
            return installed;
        }
    }
    PathBuf::from(if cfg!(windows) { "pi.cmd" } else { "pi" })
}

impl Client {
    #[cfg(test)]
    pub(crate) fn stub() -> Self {
        Self::stub_with_events().0
    }
    #[cfg(test)]
    pub(crate) fn stub_with_events() -> (Self, Sender<Event>) {
        let (client, events, _) = Self::stub_with_commands();
        (client, events)
    }
    #[cfg(test)]
    pub(crate) fn stub_with_records() -> (Self, Sender<Event>, Receiver<Value>) {
        Self::stub_with_commands()
    }
    #[cfg(test)]
    pub(crate) fn stub_with_commands() -> (Self, Sender<Event>, Receiver<Value>) {
        let (tx, rx) = mpsc::channel();
        let (commands_tx, commands) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let inject = events_tx.clone();
        let worker = thread::spawn(move || {
            while let Ok(request) = rx.recv() {
                match request {
                    Request::Shutdown => break,
                    Request::Record(record) => {
                        let _ = commands_tx.send(record);
                    }
                    Request::Eof => {}
                }
            }
            let _ = events_tx.send(Event::Exited);
        });
        (
            Self {
                tx,
                events,
                next_id: 0,
                worker: Some(worker),
            },
            inject,
            commands,
        )
    }
    #[cfg(not(test))]
    pub fn start_with(
        cwd: &Path,
        session: Option<&Path>,
        ctx: eframe::egui::Context,
        launch: Launch,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let cwd = cwd.to_path_buf();
        let session = session.map(Path::to_path_buf);
        let binary = pi_binary();
        let tx_eof = tx.clone();
        let worker = thread::spawn(move || {
            if let Err(error) = run(
                &binary,
                &launch,
                &cwd,
                session.as_deref(),
                rx,
                tx_eof,
                events_tx.clone(),
                ctx.clone(),
            ) {
                let _ = events_tx.send(Event::Error(error));
            }
            let _ = events_tx.send(Event::Exited);
            ctx.request_repaint();
        });
        Self {
            tx,
            events,
            next_id: 0,
            worker: Some(worker),
        }
    }

    /// Session changes must never join a child/extension shutdown on the UI thread.
    pub fn retire(self) {
        thread::spawn(move || drop(self));
    }

    pub fn send(&mut self, record: Value) -> Result<(), String> {
        self.enqueue(record).map(|_| ())
    }
    pub fn enqueue(&mut self, mut record: Value) -> Result<String, String> {
        if record["type"] != "extension_ui_response" {
            self.next_id += 1;
            record["id"] = format!("pui-{}", self.next_id).into();
        }
        let id = record["id"].as_str().unwrap_or_default().to_owned();
        self.tx
            .send(Request::Record(record))
            .map_err(|_| "Pi is not connected. Reconnect to try again.".to_owned())?;
        Ok(id)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.tx.send(Request::Shutdown);
        // Reap before the host exits: detached workers can leave orphaned agents.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) fn child_path() -> Option<std::ffi::OsString> {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<PathBuf> = std::env::split_paths(&inherited).collect();
    // Finder doesn't inherit shell startup files (nvm/Homebrew are often absent).
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local/share/pi-node/current/bin"));
        paths.push(home.join(".volta/bin"));
        if let Ok(versions) = std::fs::read_dir(home.join(".nvm/versions/node")) {
            let mut nodes: Vec<_> = versions
                .flatten()
                .map(|v| v.path().join("bin"))
                .filter(|p| p.join("node").is_file())
                .collect();
            nodes.sort();
            paths.extend(nodes.into_iter().rev());
        }
    }
    if cfg!(unix) {
        paths.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]);
    }
    std::env::join_paths(paths).ok()
}

#[derive(Default, Clone)]
pub struct Launch {
    pub args: Vec<String>,
    pub readonly: bool,
    pub worker: bool,
    pub task_role: Option<String>,
    pub settings: Value,
}

#[cfg(not(test))]
fn adapter_path() -> Result<PathBuf, String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(macos) = exe.parent() {
            let bundled = macos.join("../Resources/ultracode.ts");
            if bundled.is_file() {
                return Ok(bundled);
            }
        }
    }
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/ultracode.ts");
    if source.is_file() {
        Ok(source)
    } else {
        Err("Pui's bundled Ultracode adapter is missing; rebuild the app.".into())
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    binary: &Path,
    launch: &Launch,
    cwd: &Path,
    session: Option<&Path>,
    rx: Receiver<Request>,
    eof: Sender<Request>,
    events: Sender<Event>,
    ctx: eframe::egui::Context,
) -> Result<(), String> {
    let mut command = Command::new(binary);
    if let Some(path) = child_path() {
        command.env("PATH", path);
    }
    #[cfg(not(test))]
    command.arg("--extension").arg(adapter_path()?);
    command.env("PUI_AGENT_CHILD", if launch.worker { "1" } else { "0" });
    command.env("PUI_TASK_ROLE", launch.task_role.as_deref().unwrap_or(""));
    if let Some(data) = dirs::data_local_dir() {
        command.env("PUI_CHECKPOINT_DIR", data.join("pui/checkpoints-v1"));
    }
    command.env(
        "PUI_SETTINGS",
        if launch.settings.is_object() {
            launch.settings.to_string()
        } else {
            "{}".into()
        },
    );
    if let Some(sdk) = crate::usage::sdk() {
        command.env("PUI_PI_SDK", sdk);
    }
    command.args(&launch.args).env(
        "PUI_AGENT_ACCESS",
        if launch.readonly {
            "read-only"
        } else {
            "worktree"
        },
    );
    command
        .args(["--mode", "rpc"])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(session) = session {
        command.arg("--session").arg(session);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().map_err(|e| {
        format!(
            "Could not start {}: {e}. Install Pi or set PUI_PI_BIN.",
            binary.display()
        )
    })?;
    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take().ok_or("Missing Pi stdout")?;
    let stderr = child.stderr.take().ok_or("Missing Pi stderr")?;
    let output_events = events.clone();
    let output_ctx = ctx.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).split(b'\n') {
            match line {
                Ok(line) if line.is_empty() => continue,
                Ok(line) => match serde_json::from_slice::<Value>(&line) {
                    Ok(record) => {
                        let event = if record["type"] == "response"
                            && record["command"] == "get_entries"
                            && record["success"] == true
                        {
                            Event::History(crate::history::from_entries(
                                record["data"]["entries"]
                                    .as_array()
                                    .map(Vec::as_slice)
                                    .unwrap_or_default(),
                                record["data"]["leafId"].as_str(),
                            ))
                        } else {
                            Event::Record(record)
                        };
                        if output_events.send(event).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = output_events
                            .send(Event::Error("Pi emitted an invalid JSONL record.".into()));
                    }
                },
                Err(error) => {
                    let _ = output_events.send(Event::Error(format!("Pi stdout: {error}")));
                    break;
                }
            }
            output_ctx.request_repaint_after(Duration::from_millis(16));
        }
        let _ = eof.send(Request::Eof);
    });
    // Drain diagnostics, but don't persist or display arbitrary stderr: extensions may log credentials.
    thread::spawn(move || {
        let _ = std::io::copy(&mut BufReader::new(stderr), &mut std::io::sink());
    });
    let mut shutdown_at = None;
    let mut failure = None;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if shutdown_at.is_none() {
                return Err(format!(
                    "Pi exited ({status}). Reconnect to start a new process."
                ));
            }
            break;
        }
        if shutdown_at.is_some_and(|at: Instant| at.elapsed() > Duration::from_secs(3)) {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        let request = if shutdown_at.is_some() {
            rx.recv_timeout(Duration::from_millis(25))
        } else {
            rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        match request {
            Ok(Request::Record(record)) if shutdown_at.is_none() => {
                if let Some(input) = &mut stdin {
                    let result = serde_json::to_writer(&mut *input, &record)
                        .map_err(std::io::Error::other)
                        .and_then(|_| input.write_all(b"\n"))
                        .and_then(|_| input.flush());
                    if let Err(error) = result {
                        failure = Some(format!("Could not write to Pi: {error}"));
                        stdin.take();
                        shutdown_at = Some(Instant::now());
                    }
                }
            }
            Ok(Request::Eof) if shutdown_at.is_none() => {
                failure = Some("Pi closed its output stream. Reconnect to continue.".into());
                stdin.take();
                shutdown_at = Some(Instant::now());
            }
            Ok(Request::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected)
                if shutdown_at.is_none() =>
            {
                stdin.take(); // EOF requests an orderly shutdown.
                shutdown_at = Some(Instant::now());
            }
            _ => {}
        }
    }
    if let Some(error) = failure {
        Err(error)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[cfg(unix)]
    #[test]
    fn process_transport_streams_and_shuts_down() {
        let (tx, rx) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let eof = tx.clone();
        let worker = thread::spawn(move || {
            run(
                Path::new(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/pi-rpc.py"
                )),
                &Launch::default(),
                Path::new(env!("CARGO_MANIFEST_DIR")),
                None,
                rx,
                eof,
                events_tx,
                eframe::egui::Context::default(),
            )
        });
        tx.send(Request::Record(
            json!({"id":"smoke","type":"prompt","message":"hello","images":[{"type":"image","data":"aGVsbG8=","mimeType":"image/png"}]}),
        ))
        .unwrap();
        let mut settled = false;
        let mut got_unicode = false;
        let mut got_image = false;
        loop {
            let event = events
                .recv_timeout(Duration::from_secs(5))
                .expect("fixture should reply");
            if let Event::Record(record) = event {
                if record["type"] == "agent_settled" {
                    settled = true;
                }
                if record["type"] == "message_start" && record["message"]["role"] == "user" {
                    got_image = record["message"]["content"][1]["data"] == "aGVsbG8=";
                }
                if record["type"] == "message_update" {
                    got_unicode =
                        record["assistantMessageEvent"]["delta"] == "one\u{2028}two\u{2029}three";
                }
                if record["type"] == "response" {
                    assert_eq!(record["id"], "smoke");
                    break;
                }
            }
        }
        assert!(settled && got_unicode && got_image);
        tx.send(Request::Record(
            json!({"id":"history","type":"get_entries"}),
        ))
        .unwrap();
        match events.recv_timeout(Duration::from_secs(5)).unwrap() {
            Event::History(messages) => {
                assert_eq!(messages.len(), 4);
                assert_eq!(messages[0].text, "saved history");
                assert_eq!(messages[1].text, "original answer");
                assert_eq!(messages[2].role, "compaction");
                assert!(messages[2].text.is_empty());
                assert_eq!(messages[3].text, "after compaction");
            }
            _ => panic!("History should be hydrated on the reader thread"),
        }
        tx.send(Request::Shutdown).unwrap();
        assert!(worker.join().unwrap().is_ok());
    }

    #[test]
    fn retiring_a_client_does_not_block_the_ui_for_shutdown() {
        let (tx, _rx) = mpsc::channel();
        let (_events, rx) = mpsc::channel();
        let client = Client {
            tx,
            events: rx,
            next_id: 0,
            worker: Some(thread::spawn(|| thread::sleep(Duration::from_millis(250)))),
        };
        let start = Instant::now();
        client.retire();
        assert!(start.elapsed() < Duration::from_millis(50));
    }
    #[test]
    fn jsonl_preserves_unicode_separators() {
        let records = format!(
            "{}\r\n{}\n",
            json!({"text":"one\u{2028}two\u{2029}three"}),
            json!({"type":"agent_settled"})
        );
        let parsed: Vec<Value> = BufReader::new(records.as_bytes())
            .split(b'\n')
            .filter_map(|r| serde_json::from_slice(&r.ok()?).ok())
            .collect();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["text"], "one\u{2028}two\u{2029}three");
    }
}
