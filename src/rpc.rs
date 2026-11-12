//! A supervisor owns each Pi process; a separate writer keeps shutdown independent of stdin.
//! UI commands never block the rendering thread.
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
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
}

enum Control {
    Shutdown,
    Eof,
    WriteFailed(String),
}

pub struct Client {
    tx: Sender<Request>,
    control: Sender<Control>,
    pub events: Receiver<Event>,
    next_id: u64,
    worker: Option<thread::JoinHandle<()>>,
}

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
        let (control, _control_rx) = mpsc::channel();
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
                }
            }
            let _ = events_tx.send(Event::Exited);
        });
        (
            Self {
                tx,
                control,
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
        let writer_tx = tx.clone();
        let (control, control_rx) = mpsc::channel();
        let worker_control = control.clone();
        let worker = thread::spawn(move || {
            if let Err(error) = run(
                &binary,
                &launch,
                &cwd,
                session.as_deref(),
                rx,
                writer_tx,
                control_rx,
                worker_control,
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
            control,
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
        // Control must bypass queued records and a writer blocked on a full pipe.
        let _ = self.control.send(Control::Shutdown);
        let _ = self.tx.send(Request::Shutdown);
        // Reap before the host exits: detached supervisors can leave orphaned agents.
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
    writer_tx: Sender<Request>,
    control_rx: Receiver<Control>,
    control: Sender<Control>,
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
    let stdin = child.stdin.take().ok_or("Missing Pi stdin")?;
    let stdout = child.stdout.take().ok_or("Missing Pi stdout")?;
    let stderr = child.stderr.take().ok_or("Missing Pi stderr")?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let writer_cancelled = cancelled.clone();
    let writer_control = control.clone();
    thread::spawn(move || {
        if let Err(error) = write_requests(stdin, rx, &writer_cancelled) {
            let _ = writer_control.send(Control::WriteFailed(format!(
                "Could not write to Pi: {error}"
            )));
        }
    });
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
        let _ = control.send(Control::Eof);
    });
    // Drain diagnostics, but don't persist or display arbitrary stderr: extensions may log credentials.
    thread::spawn(move || {
        let _ = std::io::copy(&mut BufReader::new(stderr), &mut std::io::sink());
    });
    let mut shutdown_at = None;
    let mut failure = None;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if shutdown_at.is_none() {
                    failure = Some(format!(
                        "Pi exited ({status}). Reconnect to start a new process."
                    ));
                }
                break;
            }
            Ok(None) => {}
            Err(error) => {
                failure = Some(error.to_string());
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
        if shutdown_at.is_some_and(|at: Instant| at.elapsed() > Duration::from_secs(3)) {
            cancelled.store(true, Ordering::Release);
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        let request = if shutdown_at.is_some() {
            control_rx.recv_timeout(Duration::from_millis(25))
        } else {
            // No periodic wakeups while idle. EOF, write failure, or Drop wakes us.
            control_rx
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        if matches!(&request, Ok(Control::Eof | Control::WriteFailed(_))) {
            // A closed/broken transport cannot drain, even during graceful shutdown.
            cancelled.store(true, Ordering::Release);
        }
        if shutdown_at.is_none() {
            failure = match request {
                Ok(Control::Eof) => {
                    Some("Pi closed its output stream. Reconnect to continue.".into())
                }
                Ok(Control::WriteFailed(error)) => Some(error),
                Ok(Control::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => None,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
            };
            // Graceful shutdown drains accepted FIFO records (notably abort and
            // clear_queue) before this marker closes stdin. A blocked writer
            // cannot delay the independent supervisor's kill/reap deadline.
            let _ = writer_tx.send(Request::Shutdown);
            shutdown_at = Some(Instant::now());
        }
    }
    cancelled.store(true, Ordering::Release);
    let _ = writer_tx.send(Request::Shutdown);
    // Never join the pipe writer here: on non-Unix a blocking write may outlive
    // the child if a descendant inherited stdin. The supervisor still kills and
    // reaps its child on deadline. Unix writes also observe cancellation, so an
    // inherited pipe cannot retain a blocked writer there.
    if let Some(error) = failure {
        Err(error)
    } else {
        Ok(())
    }
}

fn write_requests(
    mut stdin: std::process::ChildStdin,
    rx: Receiver<Request>,
    cancelled: &AtomicBool,
) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let fd = stdin.as_raw_fd();
        // SAFETY: stdin owns this live descriptor for the entire writer lifetime.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    while let Ok(Request::Record(record)) = rx.recv() {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        // Encode once, not a syscall for every JSON token. No size cap or drops.
        let mut frame = serde_json::to_vec(&record).map_err(std::io::Error::other)?;
        frame.push(b'\n');
        let mut remaining = frame.as_slice();
        while !remaining.is_empty() {
            if cancelled.load(Ordering::Acquire) {
                return Ok(());
            }
            match stdin.write(remaining) {
                Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
                Ok(written) => remaining = &remaining[written..],
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                #[cfg(unix)]
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    use std::os::fd::AsRawFd;
                    let mut fd = libc::pollfd {
                        fd: stdin.as_raw_fd(),
                        events: libc::POLLOUT,
                        revents: 0,
                    };
                    // SAFETY: poll borrows one valid descriptor and is bounded so
                    // cancellation remains effective even if no reader ever drains it.
                    if unsafe { libc::poll(&mut fd, 1, 25) } < 0 {
                        let error = std::io::Error::last_os_error();
                        if error.kind() != std::io::ErrorKind::Interrupted {
                            return Err(error);
                        }
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }
    Ok(()) // Closing stdin requests an orderly shutdown.
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    // Launched with python -c rather than relying on a platform-specific executable fixture.
    #[cfg(unix)]
    const PIPE_FIXTURE: &str = r#"
import json, os, select, sys, time
mode = sys.argv[1]
print(json.dumps({'type': 'ready', 'pid': os.getpid()}), flush=True)
if mode == 'no-read':
    select.select([sys.stdin], [], [], 10)
    print(json.dumps({'type': 'stdin-readable'}), flush=True)
    time.sleep(60)
elif mode == 'eof':
    os.close(1)
    time.sleep(60)
else:
    if mode == 'drain':
        time.sleep(0.025)  # Drop should start before the fixture drains accepted records.
    for line in sys.stdin.buffer:
        print(json.dumps({'type': 'echo', 'record': json.loads(line)}), flush=True)
    print(json.dumps({'type': 'stdin-eof'}), flush=True)
"#;

    #[cfg(unix)]
    fn fixture_client(mode: &str) -> (Client, Receiver<Result<(), String>>, libc::pid_t) {
        let (tx, rx) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let (control, control_rx) = mpsc::channel();
        let (finished_tx, finished) = mpsc::channel();
        let writer_tx = tx.clone();
        let worker_control = control.clone();
        let launch = Launch {
            args: vec!["-c".into(), PIPE_FIXTURE.into(), mode.into()],
            ..Launch::default()
        };
        let worker = thread::spawn(move || {
            let result = run(
                Path::new("python3"),
                &launch,
                Path::new(env!("CARGO_MANIFEST_DIR")),
                None,
                rx,
                writer_tx,
                control_rx,
                worker_control,
                events_tx,
                eframe::egui::Context::default(),
            );
            let _ = finished_tx.send(result);
        });
        let client = Client {
            tx,
            control,
            events,
            next_id: 0,
            worker: Some(worker),
        };
        let Event::Record(ready) = client.events.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("fixture should announce its pid");
        };
        let pid = ready["pid"].as_i64().unwrap() as libc::pid_t;
        (client, finished, pid)
    }

    #[cfg(unix)]
    fn assert_reaped(pid: libc::pid_t) {
        // SAFETY: WNOHANG only checks this fixture's child; it never blocks.
        assert_eq!(
            unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) },
            -1
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[cfg(unix)]
    fn drop_with_deadline(client: Client, pid: libc::pid_t) -> Duration {
        let (done_tx, done) = mpsc::channel();
        let start = Instant::now();
        thread::spawn(move || {
            drop(client);
            let _ = done_tx.send(());
        });
        if done.recv_timeout(Duration::from_secs(5)).is_err() {
            // Bound regression failures too: release any blocked pipe before panicking.
            unsafe { libc::kill(pid, libc::SIGKILL) };
            let _ = done.recv_timeout(Duration::from_secs(2));
            panic!("Client::drop exceeded its shutdown deadline");
        }
        let elapsed = start.elapsed();
        assert_reaped(pid);
        elapsed
    }

    #[cfg(unix)]
    #[test]
    fn drop_reaps_child_that_never_reads_a_full_stdin_pipe() {
        let (mut client, finished, pid) = fixture_client("no-read");
        let record = json!({"type":"prompt", "message":"x".repeat(4 * 1024 * 1024)});
        let start = Instant::now();
        client.enqueue(record).unwrap();
        assert!(
            start.elapsed() < Duration::from_millis(250),
            "enqueue must not write stdin"
        );
        // The fixture observes bytes arriving without consuming any. The frame
        // exceeds pipe capacity, so the writer must remain pending behind it.
        let Event::Record(record) = client.events.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("fixture should observe stdin without reading it");
        };
        assert_eq!(record["type"], "stdin-readable");
        for _ in 0..256 {
            client.enqueue(json!({"type":"get_state"})).unwrap();
        }
        let elapsed = drop_with_deadline(client, pid);
        assert!(
            elapsed >= Duration::from_secs(3),
            "fixture should require the kill deadline"
        );
        assert!(
            finished
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .is_ok()
        );
    }

    #[cfg(unix)]
    #[test]
    fn writer_preserves_large_records_in_order_and_idle_stdin_gets_eof() {
        let (mut client, finished, pid) = fixture_client("echo");
        let payload = "one\u{2028}two\u{2029}three\\\"\n".repeat(8192);
        for sequence in 0..16 {
            client
                .enqueue(json!({"type":"prompt", "sequence":sequence, "message":payload}))
                .unwrap();
        }
        for sequence in 0..16 {
            let Event::Record(record) = client.events.recv_timeout(Duration::from_secs(5)).unwrap()
            else {
                panic!("fixture should echo each record");
            };
            assert_eq!(record["record"]["sequence"], sequence);
            assert_eq!(record["record"]["id"], format!("pui-{}", sequence + 1));
            assert_eq!(record["record"]["message"], payload);
        }
        // The writer is now blocked in recv, not polling. Shutdown must wake it
        // and close stdin, allowing the fixture to exit without a forced kill.
        assert!(drop_with_deadline(client, pid) < Duration::from_secs(2));
        assert!(
            finished
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .is_ok()
        );
    }

    #[cfg(unix)]
    #[test]
    fn drop_delivers_queued_abort_and_clear_queue_before_stdin_eof() {
        // Repeat immediate drops to cover scheduling where priority shutdown
        // reaches the supervisor before the writer can drain accepted records.
        for _ in 0..24 {
            let (mut client, finished, pid) = fixture_client("drain");
            let (_unused_tx, unused_events) = mpsc::channel();
            let events = std::mem::replace(&mut client.events, unused_events);
            let payload = "x".repeat(128 * 1024);
            client
                .enqueue(json!({"type":"prompt", "message":payload}))
                .unwrap();
            client.enqueue(json!({"type":"clear_queue"})).unwrap();
            client.enqueue(json!({"type":"abort"})).unwrap();
            assert!(drop_with_deadline(client, pid) < Duration::from_secs(2));
            assert!(
                finished
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap()
                    .is_ok()
            );
            for (index, kind) in ["prompt", "clear_queue", "abort"].into_iter().enumerate() {
                let Event::Record(record) = events.recv_timeout(Duration::from_secs(2)).unwrap()
                else {
                    panic!("fixture should echo each accepted record");
                };
                assert_eq!(record["type"], "echo");
                assert_eq!(record["record"]["type"], kind);
                assert_eq!(record["record"]["id"], format!("pui-{}", index + 1));
                if index == 0 {
                    assert_eq!(record["record"]["message"], payload);
                }
            }
            let Event::Record(record) = events.recv_timeout(Duration::from_secs(2)).unwrap() else {
                panic!("fixture should report EOF after all accepted records");
            };
            assert_eq!(record["type"], "stdin-eof");
        }
    }

    #[cfg(unix)]
    #[test]
    fn stdout_eof_still_shuts_down_and_reaps_a_live_child() {
        let (client, finished, pid) = fixture_client("eof");
        let result = finished.recv_timeout(Duration::from_secs(5));
        if result.is_err() {
            unsafe { libc::kill(pid, libc::SIGKILL) };
            panic!("stdout EOF should start the shutdown deadline");
        }
        assert!(
            result
                .unwrap()
                .unwrap_err()
                .contains("closed its output stream")
        );
        assert_reaped(pid);
        drop(client);
    }

    #[cfg(unix)]
    #[test]
    fn process_transport_streams_and_shuts_down() {
        let (tx, rx) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let writer_tx = tx.clone();
        let (control, control_rx) = mpsc::channel();
        let worker_control = control.clone();
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
                writer_tx,
                control_rx,
                worker_control,
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
        control.send(Control::Shutdown).unwrap();
        assert!(worker.join().unwrap().is_ok());
    }

    #[test]
    fn retiring_a_client_does_not_block_the_ui_for_shutdown() {
        let (tx, _rx) = mpsc::channel();
        let (_events, rx) = mpsc::channel();
        let (control, _control_rx) = mpsc::channel();
        let client = Client {
            tx,
            control,
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
