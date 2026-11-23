//! Opt-in Pui Remote HTTP + WebSocket server for mobile companion apps.
//! Phase 1 binds directly (Tailscale-first); the JSON protocol is transport-agnostic
//! so Phase 2 E2EE Cloud Relay can reuse the exact same frames.
pub mod files;
pub mod http;
pub mod origin;
pub mod qr;
pub mod tailscale;
pub mod ws;

#[cfg(test)]
mod tests;

pub use origin::{OriginPlatform, SessionOrigin};

use crate::protocol::{Message, Model, PendingMessage, SlashCommand};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_PORT: u16 = 4817;

pub const MOBILE_LOCAL_COMMANDS: &[(&str, &str)] = &[
    ("new", "Start a new chat"),
    ("model", "Choose model and thinking effort"),
    ("compact", "Compact context; optional instructions"),
    ("reload", "Reload resources in this chat; keeps history"),
    ("stop", "Stop the agent and clear waiting messages"),
    ("files", "Open workspace file browser"),
    ("help", "Show composer help"),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindMode {
    #[default]
    Tailscale,
    AllInterfaces,
    Localhost,
}

impl BindMode {
    pub const ALL: [Self; 3] = [Self::Tailscale, Self::AllInterfaces, Self::Localhost];

    pub fn label(self) -> &'static str {
        match self {
            Self::Tailscale => "Tailscale only (recommended)",
            Self::AllInterfaces => "All network interfaces (LAN + Tailscale)",
            Self::Localhost => "Localhost only (for tailscale serve / tunnel)",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Tailscale => {
                "Binds strictly to your Tailscale 100.x.y.z IPv4 address. Unreachable from public Wi-Fi."
            }
            Self::AllInterfaces => {
                "Binds to 0.0.0.0 so phones on the same LAN or Tailscale can connect. Always protected by bearer token."
            }
            Self::Localhost => {
                "Binds to 127.0.0.1 only. Use with `tailscale serve` for automatic HTTPS/WSS or a reverse proxy."
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteConfig {
    pub enabled: bool,
    pub bind_mode: BindMode,
    pub port: u16,
    pub token: String,
    pub host_name: String,
    /// Set true when exposing behind `tailscale serve` or an HTTPS reverse proxy.
    pub use_tls_in_qr: bool,
    /// Optional explicit hostname override for QR pairing (e.g. `my-mac.tail1234.ts.net`).
    pub public_host: String,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_mode: BindMode::Tailscale,
            port: DEFAULT_PORT,
            token: generate_token(),
            host_name: tailscale::default_host_name(),
            use_tls_in_qr: false,
            public_host: String::new(),
        }
    }
}

impl RemoteConfig {
    pub fn ensure_populated(&mut self) {
        if self.token.trim().len() < 32 {
            self.token = generate_token();
        }
        if self.port == 0 {
            self.port = DEFAULT_PORT;
        }
        if self.host_name.trim().is_empty() {
            self.host_name = tailscale::default_host_name();
        }
    }

    pub fn regenerate_token(&mut self) {
        self.token = generate_token();
    }
}

pub fn generate_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectedDevice {
    pub id: String,
    pub platform: OriginPlatform,
    pub device: String,
    pub remote_addr: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSummary {
    pub path: String,
    pub title: String,
    pub project: String,
    pub project_name: String,
    pub cwd: String,
    pub status: String,
    pub busy: bool,
    pub unread: bool,
    pub has_pending_dialog: bool,
    pub model: String,
    pub modified_ms: i64,
    pub created_ms: Option<i64>,
    pub origin: SessionOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session: Option<String>,
    pub is_active: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArtifactItem {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
    pub message_index: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlashCommandItem {
    pub name: String,
    pub description: String,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WaitingItem {
    pub index: usize,
    pub text: String,
    pub mode: String,
    pub image_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionDetail {
    pub path: String,
    pub title: String,
    pub cwd: String,
    pub project: String,
    pub status: String,
    pub busy: bool,
    pub ready: bool,
    pub model: String,
    pub model_name: String,
    pub provider: String,
    pub thinking: String,
    pub levels: Vec<String>,
    pub permission_mode: String,
    pub cost: f64,
    pub context_tokens: Option<f64>,
    pub context_window: f64,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub origin: SessionOrigin,
    pub messages: Vec<Message>,
    pub pending_messages: Vec<PendingMessage>,
    #[serde(default)]
    pub waiting_messages: Vec<WaitingItem>,
    pub dialogs: Vec<Value>,
    pub commands: Vec<SlashCommand>,
    pub models: Vec<Model>,
    pub artifacts: Vec<ArtifactItem>,
    pub revision: u64,
}

#[derive(Clone, Debug, Default)]
pub struct RemoteSnapshot {
    pub revision: u64,
    pub host_name: String,
    pub tailscale_ip: Option<String>,
    pub active_path: Option<String>,
    pub projects: Vec<PathBuf>,
    pub sessions: Vec<SessionSummary>,
    pub details: BTreeMap<String, SessionDetail>,
    pub allowed_artifacts: BTreeSet<PathBuf>,
    pub usage: crate::usage::CachedUsage,
}

#[derive(Clone, Debug, Deserialize)]
pub struct IncomingImage {
    pub data_base64: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub mime: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub name: Option<String>,
}

#[derive(Clone, Debug)]
pub enum RemoteCommand {
    CreateSession {
        cwd: PathBuf,
        permission_mode: Option<String>,
        prompt: Option<String>,
        origin: SessionOrigin,
    },
    OpenSession {
        path: PathBuf,
    },
    SendMessage {
        path: Option<PathBuf>,
        text: String,
        mode: Option<String>,
        images: Vec<IncomingImage>,
        origin: Option<SessionOrigin>,
    },
    Abort {
        path: Option<PathBuf>,
    },
    AnswerDialog {
        path: Option<PathBuf>,
        dialog_id: String,
        response: Value,
    },
    UpdateSettings {
        path: Option<PathBuf>,
        permission_mode: Option<String>,
        model: Option<String>,
        provider: Option<String>,
        thinking: Option<String>,
    },
    RemoveQueued {
        path: Option<PathBuf>,
        index: usize,
    },
    SteerQueued {
        path: Option<PathBuf>,
        index: usize,
    },
    RefreshUsage,
}

struct WsClient {
    id: String,
    tx: Sender<String>,
    subscribed_path: Option<String>,
    device: ConnectedDevice,
}

struct Shared {
    token: RwLock<String>,
    snapshot: RwLock<RemoteSnapshot>,
    clients: Mutex<Vec<WsClient>>,
    cmd_tx: Sender<RemoteCommand>,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl Shared {
    fn broadcast(&self, frame: &str, filter_path: Option<&str>) {
        let mut clients = self.clients.lock().unwrap_or_else(|p| p.into_inner());
        clients.retain(|client| {
            if let Some(path) = filter_path {
                if let Some(sub) = client.subscribed_path.as_deref() {
                    if sub != path {
                        return true;
                    }
                }
            }
            client.tx.send(frame.to_owned()).is_ok()
        });
    }

    fn connected_devices(&self) -> Vec<ConnectedDevice> {
        self.clients
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .map(|c| c.device.clone())
            .collect()
    }
}

pub struct RemoteServer {
    pub config: RemoteConfig,
    pub bound_addr: Option<SocketAddr>,
    pub tailscale_ip: Option<String>,
    pub last_error: Option<String>,
    pub cmds: Receiver<RemoteCommand>,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    listener_thread: Option<thread::JoinHandle<()>>,
    last_sessions_hash: u64,
    last_detail_revisions: BTreeMap<String, u64>,
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        self.stop_listener();
    }
}

impl RemoteServer {
    pub fn new(mut config: RemoteConfig, wake: impl Fn() + Send + Sync + 'static) -> Self {
        config.ensure_populated();
        let (cmd_tx, cmds) = mpsc::channel();
        let tailscale_ip = tailscale::detect_tailscale_ip().map(|ip| ip.to_string());
        let shared = Arc::new(Shared {
            token: RwLock::new(config.token.clone()),
            snapshot: RwLock::new(RemoteSnapshot {
                host_name: config.host_name.clone(),
                tailscale_ip: tailscale_ip.clone(),
                ..Default::default()
            }),
            clients: Mutex::new(Vec::new()),
            cmd_tx,
            wake: Box::new(wake),
        });
        let mut server = Self {
            config,
            bound_addr: None,
            tailscale_ip,
            last_error: None,
            cmds,
            shared,
            stop: Arc::new(AtomicBool::new(false)),
            listener_thread: None,
            last_sessions_hash: 0,
            last_detail_revisions: BTreeMap::new(),
        };
        if server.config.enabled {
            server.start_listener();
        }
        server
    }

    pub fn refresh_tailscale_ip(&mut self) {
        self.tailscale_ip = tailscale::detect_tailscale_ip().map(|ip| ip.to_string());
        if let Ok(mut snap) = self.shared.snapshot.write() {
            snap.tailscale_ip = self.tailscale_ip.clone();
        }
    }

    pub fn apply_config(&mut self, mut next: RemoteConfig) {
        next.ensure_populated();
        let needs_restart = next.enabled != self.config.enabled
            || next.bind_mode != self.config.bind_mode
            || next.port != self.config.port;
        if let Ok(mut token) = self.shared.token.write() {
            *token = next.token.clone();
        }
        if let Ok(mut snap) = self.shared.snapshot.write() {
            snap.host_name = next.host_name.clone();
        }
        self.config = next;
        if needs_restart {
            self.stop_listener();
            if self.config.enabled {
                self.start_listener();
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.bound_addr.is_some()
    }

    pub fn connected_devices(&self) -> Vec<ConnectedDevice> {
        self.shared.connected_devices()
    }

    pub fn pairing_host(&self) -> Option<String> {
        let custom = self.config.public_host.trim();
        if !custom.is_empty() {
            return Some(custom.to_owned());
        }
        if let Some(ts) = &self.tailscale_ip {
            return Some(ts.clone());
        }
        self.bound_addr.map(|addr| {
            if addr.ip().is_unspecified() {
                "127.0.0.1".to_owned()
            } else {
                addr.ip().to_string()
            }
        })
    }

    pub fn pairing_uri(&self) -> Option<String> {
        let host = self.pairing_host()?;
        let port = self
            .bound_addr
            .map(|a| a.port())
            .unwrap_or(self.config.port);
        Some(tailscale::pairing_uri(
            &host,
            port,
            &self.config.token,
            &self.config.host_name,
            self.config.use_tls_in_qr,
        ))
    }

    pub fn ws_url(&self) -> Option<String> {
        let host = self.pairing_host()?;
        let port = self
            .bound_addr
            .map(|a| a.port())
            .unwrap_or(self.config.port);
        Some(tailscale::ws_url(&host, port, self.config.use_tls_in_qr))
    }

    pub fn update_snapshot(&mut self, mut snapshot: RemoteSnapshot) {
        snapshot.host_name = self.config.host_name.clone();
        snapshot.tailscale_ip = self.tailscale_ip.clone();
        let sessions_json = serde_json::to_string(&snapshot.sessions).unwrap_or_default();
        let sessions_hash = hash_str(&sessions_json);
        let sessions_changed = sessions_hash != self.last_sessions_hash;
        self.last_sessions_hash = sessions_hash;

        let mut changed_details = Vec::new();
        for (path, detail) in &snapshot.details {
            if self.last_detail_revisions.get(path) != Some(&detail.revision) {
                self.last_detail_revisions
                    .insert(path.clone(), detail.revision);
                changed_details.push((path.clone(), detail.clone()));
            }
        }
        let revision = snapshot.revision;
        let sessions_clone = if sessions_changed {
            Some(snapshot.sessions.clone())
        } else {
            None
        };

        if let Ok(mut guard) = self.shared.snapshot.write() {
            *guard = snapshot;
        }

        if let Some(sessions) = sessions_clone {
            let payload = json!({
                "type": "sessions_updated",
                "revision": revision,
                "sessions": sessions,
            });
            self.shared.broadcast(&payload.to_string(), None);
        }
        for (path, detail) in changed_details {
            let payload = json!({
                "type": "session_updated",
                "revision": detail.revision,
                "path": path,
                "detail": detail,
            });
            self.shared.broadcast(&payload.to_string(), Some(&path));
        }
    }

    fn resolve_bind_ip(&mut self) -> Result<IpAddr, String> {
        match self.config.bind_mode {
            BindMode::Localhost => Ok(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            BindMode::AllInterfaces => Ok(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
            BindMode::Tailscale => {
                if self.tailscale_ip.is_none() {
                    self.tailscale_ip = tailscale::detect_tailscale_ip().map(|ip| ip.to_string());
                }
                if let Some(ip_str) = &self.tailscale_ip {
                    ip_str
                        .parse::<IpAddr>()
                        .map_err(|_| format!("Invalid Tailscale IP: {ip_str}"))
                } else {
                    Err("Tailscale IPv4 (100.x.y.z) not detected. Start Tailscale or switch bind mode to Localhost / All interfaces.".into())
                }
            }
        }
    }

    pub fn start_listener(&mut self) {
        self.stop_listener();
        self.last_error = None;
        let ip = match self.resolve_bind_ip() {
            Ok(ip) => ip,
            Err(err) => {
                self.last_error = Some(err);
                return;
            }
        };
        let addr = SocketAddr::new(ip, self.config.port);
        let listener = match TcpListener::bind(addr) {
            Ok(l) => l,
            Err(err) => {
                self.last_error = Some(format!("Could not bind {addr}: {err}"));
                return;
            }
        };
        if let Err(err) = listener.set_nonblocking(true) {
            self.last_error = Some(format!("Could not configure listener: {err}"));
            return;
        }
        let local_addr = listener.local_addr().ok();
        self.bound_addr = local_addr;

        let stop = Arc::new(AtomicBool::new(false));
        self.stop = stop.clone();
        let shared = self.shared.clone();

        self.listener_thread = Some(thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        let shared = shared.clone();
                        let stop = stop.clone();
                        thread::spawn(move || handle_connection(stream, peer, shared, stop));
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(40));
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        }));
    }

    pub fn stop_listener(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.listener_thread.take() {
            let _ = handle.join();
        }
        self.bound_addr = None;
        if let Ok(mut clients) = self.shared.clients.lock() {
            clients.clear();
        }
    }
}

fn hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

pub fn extract_artifacts(messages: &[Message]) -> (Vec<ArtifactItem>, BTreeSet<PathBuf>) {
    let mut artifacts = Vec::new();
    let mut allowed_paths = BTreeSet::new();

    for (msg_idx, msg) in messages.iter().enumerate() {
        for (img_idx, img) in msg.images.iter().enumerate() {
            artifacts.push(ArtifactItem {
                id: format!("msg-img-{msg_idx}-{img_idx}"),
                kind: "image".into(),
                title: img
                    .file
                    .as_ref()
                    .map(|f| f.name.clone())
                    .unwrap_or_else(|| format!("Image {}", img_idx + 1)),
                html: None,
                height: None,
                path: img
                    .file
                    .as_ref()
                    .map(|f| f.path.to_string_lossy().into_owned()),
                mime: Some(img.mime.clone()),
                data_base64: Some(img.data.to_string()),
                message_index: msg_idx,
            });
        }
        for (tool_idx, tool) in msg.tools.iter().enumerate() {
            if let Some(visual) = &tool.visual {
                artifacts.push(ArtifactItem {
                    id: format!("visual-{msg_idx}-{tool_idx}"),
                    kind: "visual_html".into(),
                    title: if visual.title.trim().is_empty() {
                        "Interactive visual".into()
                    } else {
                        visual.title.clone()
                    },
                    html: Some(visual.html.clone()),
                    height: visual.height,
                    path: None,
                    mime: Some("text/html; charset=utf-8".into()),
                    data_base64: None,
                    message_index: msg_idx,
                });
            }
            if let Some(target) = &tool.open_target {
                let target_path = PathBuf::from(target);
                let is_http = target.starts_with("http://") || target.starts_with("https://");
                if !is_http {
                    allowed_paths.insert(target_path.clone());
                }
                let kind = if files::is_html_path(&target_path) {
                    "file_html"
                } else if files::is_image_path(&target_path) {
                    "image"
                } else {
                    "file"
                };
                let title = target_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| target.clone());
                artifacts.push(ArtifactItem {
                    id: format!("open-{msg_idx}-{tool_idx}"),
                    kind: kind.into(),
                    title,
                    html: None,
                    height: None,
                    path: Some(target.clone()),
                    mime: Some(files::mime_for_path(&target_path).into()),
                    data_base64: None,
                    message_index: msg_idx,
                });
            }
            for (img_idx, img) in tool.images.iter().enumerate() {
                artifacts.push(ArtifactItem {
                    id: format!("tool-img-{msg_idx}-{tool_idx}-{img_idx}"),
                    kind: "image".into(),
                    title: img
                        .file
                        .as_ref()
                        .map(|f| f.name.clone())
                        .unwrap_or_else(|| format!("{} image", tool.name)),
                    html: None,
                    height: None,
                    path: img
                        .file
                        .as_ref()
                        .map(|f| f.path.to_string_lossy().into_owned()),
                    mime: Some(img.mime.clone()),
                    data_base64: Some(img.data.to_string()),
                    message_index: msg_idx,
                });
            }
        }
    }
    (artifacts, allowed_paths)
}

fn handle_connection(
    mut stream: TcpStream,
    peer: SocketAddr,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));

    let req = match http::read_request(&mut stream) {
        Ok(req) => req,
        Err(_) => return,
    };

    if req.method == "OPTIONS" {
        let _ = http::write_options(&mut stream);
        return;
    }

    if req.method == "GET" && req.path == "/v1/health" {
        let _ = http::write_json(
            &mut stream,
            200,
            "OK",
            &json!({
                "ok": true,
                "protocol_version": PROTOCOL_VERSION,
                "pui_version": env!("CARGO_PKG_VERSION"),
            }),
        );
        return;
    }

    // Constant-time token validation for every other route.
    let expected = shared.token.read().map(|g| g.clone()).unwrap_or_default();
    let authorized = req
        .bearer_token()
        .is_some_and(|provided| http::constant_time_eq(provided, &expected));
    if !authorized {
        let _ = http::write_json(
            &mut stream,
            401,
            "Unauthorized",
            &json!({ "error": "Invalid or missing bearer token." }),
        );
        return;
    }

    if req.method == "GET" && req.path == "/v1/ws" && req.is_websocket_upgrade() {
        let key = req.header("sec-websocket-key").unwrap_or("");
        if ws::write_handshake(&mut stream, key).is_ok() {
            run_websocket(stream, peer, shared, stop);
        }
        return;
    }

    route_http(&req, &mut stream, &shared);
}

fn run_websocket(stream: TcpStream, peer: SocketAddr, shared: Arc<Shared>, stop: Arc<AtomicBool>) {
    let Ok(mut read_stream) = stream.try_clone() else {
        return;
    };
    let mut write_stream = stream;
    let _ = read_stream.set_read_timeout(Some(Duration::from_secs(60)));
    let _ = write_stream.set_write_timeout(Some(Duration::from_secs(15)));

    let (tx, rx) = mpsc::channel::<String>();
    let client_id = uuid::Uuid::new_v4().to_string();
    let device = ConnectedDevice {
        id: client_id.clone(),
        platform: OriginPlatform::Android,
        device: peer.ip().to_string(),
        remote_addr: peer.to_string(),
    };
    {
        let mut clients = shared.clients.lock().unwrap_or_else(|p| p.into_inner());
        clients.push(WsClient {
            id: client_id.clone(),
            tx: tx.clone(),
            subscribed_path: None,
            device,
        });
    }
    (shared.wake)();

    // Send initial hello + current sessions snapshot.
    if let Ok(snap) = shared.snapshot.read() {
        let hello = json!({
            "type": "hello",
            "protocol_version": PROTOCOL_VERSION,
            "pui_version": env!("CARGO_PKG_VERSION"),
            "host_name": snap.host_name,
            "active_path": snap.active_path,
        });
        let _ = tx.send(hello.to_string());
        let sessions = json!({
            "type": "sessions_updated",
            "revision": snap.revision,
            "sessions": snap.sessions,
        });
        let _ = tx.send(sessions.to_string());
    }

    let writer_stop = stop.clone();
    let writer = thread::spawn(move || {
        while !writer_stop.load(Ordering::Relaxed) {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(frame) => {
                    if ws::write_text(&mut write_stream, &frame).is_err() {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let _ = ws::write_close(&mut write_stream);
    });

    while !stop.load(Ordering::Relaxed) {
        match ws::read_message(&mut read_stream) {
            Ok(ws::WsMessage::Close) => break,
            Ok(ws::WsMessage::Ping(payload)) => {
                let mut reply = match read_stream.try_clone() {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let _ = ws::write_pong(&mut reply, &payload);
            }
            Ok(ws::WsMessage::Pong(_)) => {}
            Ok(ws::WsMessage::Text(text)) => {
                if let Ok(msg) = serde_json::from_str::<Value>(&text) {
                    match msg["type"].as_str().unwrap_or("") {
                        "ping" => {
                            let _ = tx.send(json!({ "type": "pong", "ts": msg["ts"] }).to_string());
                        }
                        "subscribe" => {
                            let path = msg["path"].as_str().map(str::to_owned);
                            if let Ok(mut clients) = shared.clients.lock() {
                                if let Some(c) = clients.iter_mut().find(|c| c.id == client_id) {
                                    c.subscribed_path = path.clone();
                                }
                            }
                            if let Some(path) = path {
                                if let Ok(snap) = shared.snapshot.read() {
                                    if let Some(detail) = snap.details.get(&path) {
                                        let _ = tx.send(
                                            json!({
                                                "type": "session_updated",
                                                "revision": detail.revision,
                                                "path": path,
                                                "detail": detail,
                                            })
                                            .to_string(),
                                        );
                                    }
                                }
                            }
                        }
                        "client_hello" => {
                            let platform = msg["platform"]
                                .as_str()
                                .and_then(|p| {
                                    serde_json::from_value::<OriginPlatform>(json!(p)).ok()
                                })
                                .unwrap_or(OriginPlatform::Android);
                            let name = msg["device"]
                                .as_str()
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(|s| s.chars().take(64).collect::<String>());
                            if let Ok(mut clients) = shared.clients.lock() {
                                if let Some(c) = clients.iter_mut().find(|c| c.id == client_id) {
                                    c.device.platform = platform;
                                    if let Some(name) = name {
                                        c.device.device = name;
                                    }
                                }
                            }
                            (shared.wake)();
                        }
                        _ => {}
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                continue;
            }
            Err(_) => break,
        }
    }

    if let Ok(mut clients) = shared.clients.lock() {
        clients.retain(|c| c.id != client_id);
    }
    drop(tx);
    let _ = writer.join();
    (shared.wake)();
}

fn route_http(req: &http::Request, stream: &mut TcpStream, shared: &Shared) {
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/v1/info") => {
            let snap = shared.snapshot.read().unwrap_or_else(|p| p.into_inner());
            let projects: Vec<String> = snap
                .projects
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            let _ = http::write_json(
                stream,
                200,
                "OK",
                &json!({
                    "protocol_version": PROTOCOL_VERSION,
                    "pui_version": env!("CARGO_PKG_VERSION"),
                    "host_name": snap.host_name,
                    "tailscale_ip": snap.tailscale_ip,
                    "active_path": snap.active_path,
                    "projects": projects,
                    "permission_modes": [
                        { "key": "auto", "label": "Auto" },
                        { "key": "manual", "label": "Manual" },
                        { "key": "acceptEdits", "label": "Accept edits" },
                        { "key": "plan", "label": "Plan" },
                        { "key": "dontAsk", "label": "Don't ask" },
                        { "key": "bypassPermissions", "label": "Bypass" }
                    ]
                }),
            );
        }
        ("GET", "/v1/usage") => {
            let snap = shared.snapshot.read().unwrap_or_else(|p| p.into_inner());
            let _ = http::write_json(stream, 200, "OK", &json!(snap.usage));
        }
        ("POST", "/v1/usage/refresh") => {
            let _ = shared.cmd_tx.send(RemoteCommand::RefreshUsage);
            (shared.wake)();
            let _ = http::write_json(stream, 202, "Accepted", &json!({ "ok": true }));
        }
        ("GET", "/v1/sessions") => {
            let snap = shared.snapshot.read().unwrap_or_else(|p| p.into_inner());
            let _ = http::write_json(
                stream,
                200,
                "OK",
                &json!({
                    "revision": snap.revision,
                    "active_path": snap.active_path,
                    "sessions": snap.sessions,
                }),
            );
        }
        ("GET", "/v1/sessions/detail") => {
            let snap = shared.snapshot.read().unwrap_or_else(|p| p.into_inner());
            let target = req
                .query_param("path")
                .map(str::to_owned)
                .or_else(|| snap.active_path.clone());
            let Some(path) = target else {
                let _ = http::write_json(
                    stream,
                    404,
                    "Not Found",
                    &json!({ "error": "No active session." }),
                );
                return;
            };
            if let Some(detail) = snap.details.get(&path) {
                let _ = http::write_json(stream, 200, "OK", &json!(detail));
                return;
            }
            // Fall back to reading saved session history from disk if not currently live.
            let path_buf = PathBuf::from(&path);
            let known = snap.sessions.iter().find(|s| s.path == path);
            if let Some(summary) = known {
                let messages = crate::history::load(&path_buf)
                    .map(|p| p.messages)
                    .unwrap_or_default();
                let (artifacts, _) = extract_artifacts(&messages);
                let detail = SessionDetail {
                    path: summary.path.clone(),
                    title: summary.title.clone(),
                    cwd: summary.cwd.clone(),
                    project: summary.project.clone(),
                    status: summary.status.clone(),
                    busy: false,
                    ready: true,
                    model: summary.model.clone(),
                    origin: summary.origin.clone(),
                    messages,
                    artifacts,
                    revision: snap.revision,
                    ..Default::default()
                };
                let _ = http::write_json(stream, 200, "OK", &json!(detail));
            } else {
                let _ = http::write_json(
                    stream,
                    404,
                    "Not Found",
                    &json!({ "error": "Session not found." }),
                );
            }
        }
        ("POST", "/v1/sessions") => {
            let Ok(body) = serde_json::from_slice::<Value>(&req.body) else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Invalid JSON body." }),
                );
                return;
            };
            let cwd = body["cwd"].as_str().map(PathBuf::from).or_else(|| {
                shared
                    .snapshot
                    .read()
                    .ok()
                    .and_then(|s| s.projects.first().cloned())
            });
            let Some(cwd) = cwd else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Missing project cwd." }),
                );
                return;
            };
            let mut origin: SessionOrigin = body
                .get("origin")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or(SessionOrigin {
                    platform: OriginPlatform::Android,
                    device: None,
                    client: Some("pui-mobile".into()),
                    created_at: None,
                })
                .sanitized();
            if origin.created_at.is_none() {
                origin.created_at = Some(chrono::Utc::now().timestamp_millis());
            }
            let permission_mode = body["permission_mode"].as_str().map(str::to_owned);
            let prompt = body["prompt"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            let _ = shared.cmd_tx.send(RemoteCommand::CreateSession {
                cwd,
                permission_mode,
                prompt,
                origin: origin.clone(),
            });
            (shared.wake)();
            let _ = http::write_json(
                stream,
                202,
                "Accepted",
                &json!({ "ok": true, "origin": origin }),
            );
        }
        ("POST", "/v1/sessions/open") => {
            let Ok(body) = serde_json::from_slice::<Value>(&req.body) else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Invalid JSON." }),
                );
                return;
            };
            let Some(path) = body["path"].as_str().filter(|s| !s.is_empty()) else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Missing path." }),
                );
                return;
            };
            let _ = shared.cmd_tx.send(RemoteCommand::OpenSession {
                path: PathBuf::from(path),
            });
            (shared.wake)();
            let _ = http::write_json(stream, 202, "Accepted", &json!({ "ok": true }));
        }
        ("POST", "/v1/sessions/message") => {
            let Ok(body) = serde_json::from_slice::<Value>(&req.body) else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Invalid JSON." }),
                );
                return;
            };
            let text = body["text"].as_str().unwrap_or("").to_owned();
            let images: Vec<IncomingImage> = body
                .get("images")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            if text.trim().is_empty() && images.is_empty() {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Message text or image is required." }),
                );
                return;
            }
            let path = body["path"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from);
            let mode = body["mode"].as_str().map(str::to_owned);
            let origin = body
                .get("origin")
                .and_then(|v| serde_json::from_value::<SessionOrigin>(v.clone()).ok())
                .map(SessionOrigin::sanitized);
            let _ = shared.cmd_tx.send(RemoteCommand::SendMessage {
                path,
                text,
                mode,
                images,
                origin,
            });
            (shared.wake)();
            let _ = http::write_json(stream, 202, "Accepted", &json!({ "ok": true }));
        }
        ("POST", "/v1/sessions/abort") => {
            let body = serde_json::from_slice::<Value>(&req.body).unwrap_or(Value::Null);
            let path = body["path"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from);
            let _ = shared.cmd_tx.send(RemoteCommand::Abort { path });
            (shared.wake)();
            let _ = http::write_json(stream, 202, "Accepted", &json!({ "ok": true }));
        }
        ("POST", "/v1/sessions/dialog") => {
            let Ok(body) = serde_json::from_slice::<Value>(&req.body) else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Invalid JSON." }),
                );
                return;
            };
            let Some(dialog_id) = body["dialog_id"].as_str().filter(|s| !s.is_empty()) else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Missing dialog_id." }),
                );
                return;
            };
            let path = body["path"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from);
            let response = body.get("response").cloned().unwrap_or(Value::Null);
            let _ = shared.cmd_tx.send(RemoteCommand::AnswerDialog {
                path,
                dialog_id: dialog_id.to_owned(),
                response,
            });
            (shared.wake)();
            let _ = http::write_json(stream, 202, "Accepted", &json!({ "ok": true }));
        }
        ("POST", "/v1/sessions/settings") => {
            let Ok(body) = serde_json::from_slice::<Value>(&req.body) else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Invalid JSON." }),
                );
                return;
            };
            let _ = shared.cmd_tx.send(RemoteCommand::UpdateSettings {
                path: body["path"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from),
                permission_mode: body["permission_mode"].as_str().map(str::to_owned),
                model: body["model"].as_str().map(str::to_owned),
                provider: body["provider"].as_str().map(str::to_owned),
                thinking: body["thinking"].as_str().map(str::to_owned),
            });
            (shared.wake)();
            let _ = http::write_json(stream, 202, "Accepted", &json!({ "ok": true }));
        }
        ("POST", "/v1/sessions/queue/remove") => {
            let Ok(body) = serde_json::from_slice::<Value>(&req.body) else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Invalid JSON." }),
                );
                return;
            };
            let Some(index) = body["index"].as_u64().map(|n| n as usize) else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Missing queue index." }),
                );
                return;
            };
            let path = body["path"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from);
            let _ = shared
                .cmd_tx
                .send(RemoteCommand::RemoveQueued { path, index });
            (shared.wake)();
            let _ = http::write_json(stream, 202, "Accepted", &json!({ "ok": true }));
        }
        ("POST", "/v1/sessions/queue/steer") => {
            let Ok(body) = serde_json::from_slice::<Value>(&req.body) else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Invalid JSON." }),
                );
                return;
            };
            let Some(index) = body["index"].as_u64().map(|n| n as usize) else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Missing queue index." }),
                );
                return;
            };
            let path = body["path"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from);
            let _ = shared
                .cmd_tx
                .send(RemoteCommand::SteerQueued { path, index });
            (shared.wake)();
            let _ = http::write_json(stream, 202, "Accepted", &json!({ "ok": true }));
        }
        ("GET", "/v1/completions/commands") => {
            let query = req
                .query_param("q")
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            let snap = shared.snapshot.read().unwrap_or_else(|p| p.into_inner());
            let target_path = req
                .query_param("path")
                .map(str::to_owned)
                .or_else(|| snap.active_path.clone());
            let mut items: Vec<SlashCommandItem> = MOBILE_LOCAL_COMMANDS
                .iter()
                .map(|(name, desc)| SlashCommandItem {
                    name: (*name).into(),
                    description: (*desc).into(),
                    source: "pui".into(),
                })
                .collect();
            let mut seen: BTreeSet<String> = items.iter().map(|i| i.name.clone()).collect();
            if let Some(detail) = target_path.as_deref().and_then(|p| snap.details.get(p)) {
                for cmd in &detail.commands {
                    if seen.insert(cmd.name.clone()) {
                        items.push(SlashCommandItem {
                            name: cmd.name.clone(),
                            description: cmd.description.clone().unwrap_or_default(),
                            source: "pi".into(),
                        });
                    }
                }
            }
            if !query.is_empty() {
                items.retain(|item| {
                    item.name.to_ascii_lowercase().contains(&query)
                        || item.description.to_ascii_lowercase().contains(&query)
                });
            }
            let _ = http::write_json(stream, 200, "OK", &json!({ "commands": items }));
        }
        ("GET", "/v1/completions/files") => {
            let snap = shared.snapshot.read().unwrap_or_else(|p| p.into_inner());
            let cwd = req
                .query_param("cwd")
                .map(PathBuf::from)
                .or_else(|| snap.projects.first().cloned());
            let Some(cwd) = cwd else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Missing cwd." }),
                );
                return;
            };
            let query = req.query_param("q").unwrap_or("");
            match files::search_mentions(&cwd, query, &snap.projects) {
                Ok((items, limited)) => {
                    let _ = http::write_json(
                        stream,
                        200,
                        "OK",
                        &json!({ "files": items, "limited": limited }),
                    );
                }
                Err((status, msg)) => {
                    let _ = http::write_json(stream, status, "Error", &json!({ "error": msg }));
                }
            }
        }
        ("GET", "/v1/files/tree") => {
            let snap = shared.snapshot.read().unwrap_or_else(|p| p.into_inner());
            let cwd = req
                .query_param("cwd")
                .map(PathBuf::from)
                .or_else(|| snap.projects.first().cloned());
            let Some(cwd) = cwd else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Missing cwd." }),
                );
                return;
            };
            let dir = req.query_param("dir").unwrap_or("");
            match files::list_tree(&cwd, dir, &snap.projects) {
                Ok(tree) => {
                    let _ = http::write_json(stream, 200, "OK", &json!(tree));
                }
                Err((status, msg)) => {
                    let _ = http::write_json(stream, status, "Error", &json!({ "error": msg }));
                }
            }
        }
        ("GET", "/v1/files/content") => {
            let snap = shared.snapshot.read().unwrap_or_else(|p| p.into_inner());
            let cwd = req
                .query_param("cwd")
                .map(PathBuf::from)
                .or_else(|| snap.projects.first().cloned())
                .unwrap_or_default();
            let Some(path) = req.query_param("path") else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Missing path." }),
                );
                return;
            };
            match files::read_file_content(&cwd, path, &snap.projects, &snap.allowed_artifacts) {
                Ok(resp) => {
                    let _ = http::write_json(stream, 200, "OK", &json!(resp));
                }
                Err((status, msg)) => {
                    let _ = http::write_json(stream, status, "Error", &json!({ "error": msg }));
                }
            }
        }
        ("GET", "/v1/files/raw") => {
            let snap = shared.snapshot.read().unwrap_or_else(|p| p.into_inner());
            let cwd = req
                .query_param("cwd")
                .map(PathBuf::from)
                .or_else(|| snap.projects.first().cloned())
                .unwrap_or_default();
            let Some(path) = req.query_param("path") else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Missing path." }),
                );
                return;
            };
            match files::read_file_raw(&cwd, path, &snap.projects, &snap.allowed_artifacts) {
                Ok((mime, bytes)) => {
                    let _ = http::write_bytes(stream, 200, "OK", &mime, &bytes);
                }
                Err((status, msg)) => {
                    let _ = http::write_json(stream, status, "Error", &json!({ "error": msg }));
                }
            }
        }
        ("GET", "/v1/git/changes") => {
            let snap = shared.snapshot.read().unwrap_or_else(|p| p.into_inner());
            let cwd = req
                .query_param("cwd")
                .map(PathBuf::from)
                .or_else(|| snap.projects.first().cloned());
            let Some(cwd) = cwd else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Missing cwd." }),
                );
                return;
            };
            match files::git_changes(&cwd, &snap.projects) {
                Ok(resp) => {
                    let _ = http::write_json(stream, 200, "OK", &json!(resp));
                }
                Err((status, msg)) => {
                    let _ = http::write_json(stream, status, "Error", &json!({ "error": msg }));
                }
            }
        }
        ("GET", "/v1/git/diff") => {
            let snap = shared.snapshot.read().unwrap_or_else(|p| p.into_inner());
            let cwd = req
                .query_param("cwd")
                .map(PathBuf::from)
                .or_else(|| snap.projects.first().cloned());
            let (Some(cwd), Some(path)) = (cwd, req.query_param("path")) else {
                let _ = http::write_json(
                    stream,
                    400,
                    "Bad Request",
                    &json!({ "error": "Missing cwd or path." }),
                );
                return;
            };
            let status_code = req.query_param("status").unwrap_or(" M");
            match files::git_diff(&cwd, path, status_code, &snap.projects) {
                Ok(diff) => {
                    let _ =
                        http::write_json(stream, 200, "OK", &json!({ "path": path, "diff": diff }));
                }
                Err((status, msg)) => {
                    let _ = http::write_json(stream, status, "Error", &json!({ "error": msg }));
                }
            }
        }
        _ => {
            let _ = http::write_json(
                stream,
                404,
                "Not Found",
                &json!({ "error": "Unknown endpoint." }),
            );
        }
    }
}
