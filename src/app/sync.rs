//! Cross-process live-session ownership. One Pi writer; other windows observe and
//! send explicit actions to its owner. Loopback transport is authenticated with a
//! per-window token in a user-only registry. No idle timers or filesystem polling.
use super::*;
use fs2::FileExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    hash::{Hash, Hasher},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const MAX_FRAME: usize = 192 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct LivePath {
    pub path: PathBuf,
    pub revision: u64,
    pub label: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub cwd: PathBuf,
    #[serde(default)]
    pub model: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Endpoint {
    id: String,
    port: u16,
    token: String,
    pub paths: Vec<LivePath>,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Snapshot {
    pub revision: u64,
    pub state: State,
    pub waiting: Vec<run_status::WaitingMessage>,
    pub settings: settings::Settings,
    #[serde(default)]
    pub access: Option<String>,
    #[serde(default)]
    pub worktree: Option<PathBuf>,
    #[serde(default)]
    pub notifications: notifications::NotificationSettings,
    #[serde(default)]
    pub attention: Option<attention::Metadata>,
    pub ultra: bool,
    pub ready: bool,
    pub cwd: PathBuf,
    pub label: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "action", content = "data")]
pub(super) enum Action {
    Queue(run_status::WaitingMessage),
    Remove(usize),
    Edit(usize),
    SendNow(usize),
    Rpc(Value),
    Settings(settings::Settings),
    Notifications(notifications::NotificationSettings),
}
#[derive(Serialize, Deserialize)]
struct Wire {
    token: String,
    path: PathBuf,
    revision: u64,
    action: Option<Action>,
}
#[derive(Serialize, Deserialize)]
struct Reply {
    snapshot: Option<Snapshot>,
    edited: Option<run_status::WaitingMessage>,
    error: Option<String>,
}
struct RemoteCommand {
    path: PathBuf,
    revision: u64,
    action: Option<Action>,
    reply: mpsc::Sender<Reply>,
}
enum Work {
    Publish(LivePath),
    Scan,
    Forget(PathBuf),
    Fetch(PathBuf, Arc<Endpoint>, u64),
    Act(PathBuf, Arc<Endpoint>, u64, Action),
    Stop,
}
enum ResultEvent {
    Registry(Vec<Endpoint>, bool, u64),
    PresenceUnknown,
    Snapshot(PathBuf, u64, Box<Snapshot>),
    Action(PathBuf, Box<Reply>, Option<run_status::WaitingMessage>),
    Failure(PathBuf, String),
}
struct Owned {
    _lock: File,
    stamp: u64,
    revision: u64,
    published: std::time::Instant,
}
pub(super) struct WindowSync {
    directory: Option<PathBuf>,
    id: String,
    owned: BTreeMap<PathBuf, Owned>,
    endpoints: BTreeMap<PathBuf, Arc<Endpoint>>,
    revisions: BTreeMap<PathBuf, u64>,
    pending: BTreeSet<PathBuf>,
    tx: mpsc::Sender<Work>,
    rx: Receiver<ResultEvent>,
    commands: Receiver<RemoteCommand>,
    _watcher: Option<crate::file_watch::Watcher>,
    _presence: Option<File>,
    worker: Option<std::thread::JoinHandle<()>>,
    listener: Option<std::thread::JoinHandle<()>>,
    stopped: Arc<std::sync::atomic::AtomicBool>,
    port: u16,
    pub error: Option<String>,
    mirror: Option<PathBuf>,
    unsent: Vec<run_status::WaitingMessage>,
    // Reuse identities already resolved by claim; attention never probes disk.
    attention_aliases: std::collections::VecDeque<(PathBuf, PathBuf)>,
    // False until an event-driven scan proves no other registered window exists.
    dock_single_window: bool,
    dock_watch_ready: bool,
    presence_epoch: Arc<AtomicU64>,
    dock_epoch_accepted: Option<u64>,
}
fn key(path: &Path) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    format!("{:016x}", h.finish())
}
pub(super) fn identity(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| {
        path.parent()
            .and_then(|p| p.canonicalize().ok())
            .zip(path.file_name())
            .map(|(parent, name)| parent.join(name))
            .unwrap_or_else(|| path.to_path_buf())
    })
}
fn private_open(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
fn write_frame<T: Serialize>(stream: &mut TcpStream, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "Cannot encode window data")?;
    if bytes.len() > MAX_FRAME {
        return Err("Window data exceeds sync limit".into());
    }
    stream
        .write_all(&(bytes.len() as u64).to_be_bytes())
        .and_then(|_| stream.write_all(&bytes))
        .map_err(|_| "Window connection closed".into())
}
fn read_frame<T: for<'de> Deserialize<'de>>(stream: &mut TcpStream) -> Result<T, String> {
    let mut size = [0; 8];
    stream
        .read_exact(&mut size)
        .map_err(|_| "Window connection closed")?;
    let size = u64::from_be_bytes(size);
    if size > MAX_FRAME as u64 {
        return Err("Window frame too large".into());
    }
    let mut bytes = vec![0; size as usize];
    stream
        .read_exact(&mut bytes)
        .map_err(|_| "Incomplete window data")?;
    serde_json::from_slice(&bytes).map_err(|_| "Invalid window data".into())
}
fn call(
    endpoint: &Endpoint,
    path: &Path,
    revision: u64,
    action: Option<Action>,
) -> Result<Reply, String> {
    let mut stream = TcpStream::connect_timeout(
        &([127, 0, 0, 1], endpoint.port).into(),
        Duration::from_secs(2),
    )
    .map_err(|_| "The owning window closed or is unavailable. Reopen this chat to reconnect.")?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    write_frame(
        &mut stream,
        &Wire {
            token: endpoint.token.clone(),
            path: path.to_path_buf(),
            revision,
            action,
        },
    )?;
    read_frame(&mut stream)
}
fn write_endpoint(directory: &Path, endpoint: &Endpoint) -> std::io::Result<()> {
    // Streaming serde_json directly into File issues a syscall for every JSON
    // token. Encode first so each publication needs just one bounded write.
    let bytes = serde_json::to_vec(endpoint).map_err(std::io::Error::other)?;
    let temp = directory.join(format!("{}.tmp", endpoint.id));
    let mut file = private_open(&temp)?;
    file.set_len(0)?;
    file.write_all(&bytes)?;
    std::fs::rename(temp, directory.join(format!("{}.json", endpoint.id)))
}
fn registry_changed(event: &notify::Event, own: &Path) -> bool {
    event.need_rescan()
        || (event.kind.is_create() || event.kind.is_modify() || event.kind.is_remove())
            && event.paths.iter().any(|p| {
                p.file_stem() != own.file_stem()
                    && p.extension().is_some_and(|e| e == "json" || e == "owner")
            })
}
fn queue_scan(tx: &mpsc::Sender<Work>, pending: &AtomicBool) {
    // Bound watcher bursts to one queued scan without coalescing explicit actions.
    if !pending.swap(true, Ordering::AcqRel) && tx.send(Work::Scan).is_err() {
        pending.store(false, Ordering::Release);
    }
}
fn run_scan<T>(pending: &AtomicBool, scan: impl FnOnce() -> T) -> T {
    // Changes after this point must be able to request a follow-up scan.
    pending.store(false, Ordering::Release);
    scan()
}
fn index_endpoint(endpoints: &mut BTreeMap<PathBuf, Arc<Endpoint>>, endpoint: Endpoint) {
    let endpoint = Arc::new(endpoint);
    for live in &endpoint.paths {
        endpoints.insert(live.path.clone(), Arc::clone(&endpoint));
    }
}
fn registry(directory: &Path, own: &str) -> Vec<Endpoint> {
    registry_with_presence(directory, own).0
}
fn registry_with_presence(directory: &Path, own: &str) -> (Vec<Endpoint>, bool) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return (Vec::new(), false);
    };
    let mut endpoints = Vec::new();
    let mut single = true;
    for entry in entries {
        let Ok(entry) = entry else {
            single = false;
            continue;
        };
        let path = entry.path();
        let extension = path.extension().and_then(|e| e.to_str());
        if !matches!(extension, Some("json" | "owner"))
            || path.file_stem().and_then(|s| s.to_str()) == Some(own)
        {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            single = false;
            continue;
        };
        let Ok(presence) = File::open(directory.join(format!("{id}.owner"))) else {
            single = false;
            continue;
        };
        if presence.try_lock_exclusive().is_ok() {
            continue;
        }
        // Count presence before decoding/indexing: empty-path, malformed and
        // unreadable peer metadata cannot authorize a misleading local Dock total.
        single = false;
        if extension != Some("json") {
            continue;
        }
        if let Some(endpoint) = File::open(path)
            .ok()
            .and_then(|file| serde_json::from_reader(file.take(1024 * 1024)).ok())
        {
            endpoints.push(endpoint);
        }
    }
    (endpoints, single)
}
impl WindowSync {
    pub fn new(ctx: &egui::Context, disabled: bool) -> Self {
        Self::new_at(ctx, disabled, None)
    }
    fn new_at(ctx: &egui::Context, disabled: bool, location: Option<PathBuf>) -> Self {
        let (tx, work) = mpsc::channel();
        let (results, rx) = mpsc::channel();
        let (remote, commands) = mpsc::channel();
        let id = uuid::Uuid::new_v4().to_string();
        let mut this = Self {
            directory: None,
            id: id.clone(),
            owned: Default::default(),
            endpoints: Default::default(),
            revisions: Default::default(),
            pending: Default::default(),
            tx: tx.clone(),
            rx,
            commands,
            _watcher: None,
            _presence: None,
            worker: None,
            listener: None,
            stopped: Arc::new(false.into()),
            port: 0,
            error: None,
            mirror: None,
            unsent: Vec::new(),
            attention_aliases: Default::default(),
            dock_single_window: false,
            dock_watch_ready: false,
            presence_epoch: Arc::new(AtomicU64::new(1)),
            dock_epoch_accepted: None,
        };
        if disabled {
            return this;
        }
        let result = (|| -> Result<(), String> {
            let directory = location
                .or_else(|| dirs::data_local_dir().map(|p| p.join("pui/live-v1")))
                .ok_or("No app data directory")?;
            std::fs::create_dir_all(&directory)
                .map_err(|_| "Cannot create live window registry")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                    .map_err(|_| "Cannot secure window registry")?;
            }
            let presence = private_open(&directory.join(format!("{id}.owner")))
                .map_err(|_| "Cannot establish window presence")?;
            presence
                .try_lock_exclusive()
                .map_err(|_| "Cannot lock window presence")?;
            this._presence = Some(presence);
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .map_err(|_| "Cannot start window synchronization")?;
            this.port = listener
                .local_addr()
                .map_err(|_| "Cannot get window address")?
                .port();
            let mut endpoint = Endpoint {
                id: id.clone(),
                port: this.port,
                token: uuid::Uuid::new_v4().to_string(),
                paths: Vec::new(),
            };
            // Publish even a blank window: Dock eligibility must not mistake a
            // live pathless window for absence. Existing watcher handles changes.
            write_endpoint(&directory, &endpoint)
                .map_err(|_| "Cannot publish live window presence")?;
            let token = endpoint.token.clone();
            let stopped = this.stopped.clone();
            let repaint = ctx.clone();
            this.listener = Some(std::thread::spawn(move || {
                for connection in listener.incoming() {
                    if stopped.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    let Ok(mut stream) = connection else {
                        continue;
                    };
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
                    let Ok(request) = read_frame::<Wire>(&mut stream) else {
                        continue;
                    };
                    if request.token != token {
                        continue;
                    }
                    {
                        let action = request.action;
                        let (reply, response) = mpsc::channel();
                        if remote
                            .send(RemoteCommand {
                                path: request.path,
                                revision: request.revision,
                                action,
                                reply,
                            })
                            .is_err()
                        {
                            break;
                        }
                        repaint.request_repaint();
                        let result = response.recv_timeout(Duration::from_secs(8)).unwrap_or(Reply { snapshot: None, edited: None, error: Some("Owner did not acknowledge the action. It was not retried; check the owning window before resending.".into()) });
                        let _ = write_frame(&mut stream, &result);
                    }
                }
            }));
            let dir = directory.clone();
            let repaint = ctx.clone();
            let scan_pending = Arc::new(AtomicBool::new(false));
            let worker_scan_pending = Arc::clone(&scan_pending);
            let watcher_results = results.clone();
            let scan_epoch = this.presence_epoch.clone();
            this.worker = Some(std::thread::spawn(move || {
                let metadata = dir.join(format!("{id}.json"));
                while let Ok(job) = work.recv() {
                    match job {
                        Work::Stop => break,
                        Work::Publish(live) => {
                            endpoint.paths.retain(|p| p.path != live.path);
                            endpoint.paths.push(live);
                            let _ = write_endpoint(&dir, &endpoint);
                            // No self-repaint on publication; peer watchers handle updates.
                            continue;
                        }
                        Work::Forget(path) => {
                            endpoint.paths.retain(|p| p.path != path);
                            let _ = write_endpoint(&dir, &endpoint);
                            continue;
                        }
                        Work::Scan => {
                            let (endpoints, single, epoch) = run_scan(&worker_scan_pending, || {
                                let epoch = scan_epoch.load(Ordering::Acquire);
                                let (endpoints, single) =
                                    registry_with_presence(&dir, &endpoint.id);
                                (endpoints, single, epoch)
                            });
                            let _ = results.send(ResultEvent::Registry(endpoints, single, epoch));
                        }
                        Work::Fetch(path, peer, revision) => {
                            let event = match call(&peer, &path, revision, None) {
                                Ok(Reply {
                                    snapshot: Some(snapshot),
                                    ..
                                }) => ResultEvent::Snapshot(
                                    path,
                                    snapshot.revision,
                                    Box::new(snapshot),
                                ),
                                Ok(reply) => ResultEvent::Failure(
                                    path,
                                    reply
                                        .error
                                        .unwrap_or_else(|| "Owner has no live snapshot".into()),
                                ),
                                Err(error) => ResultEvent::Failure(path, error),
                            };
                            let _ = results.send(event);
                        }
                        Work::Act(path, peer, revision, action) => {
                            let retained = if let Action::Queue(message) = &action {
                                Some(message.clone())
                            } else {
                                None
                            };
                            let reply = call(&peer, &path, revision, Some(action)).unwrap_or_else(
                                |error| Reply {
                                    snapshot: None,
                                    edited: None,
                                    error: Some(format!(
                                        "{error} Delivery may be uncertain; it was not retried."
                                    )),
                                },
                            );
                            let _ =
                                results.send(ResultEvent::Action(path, Box::new(reply), retained));
                        }
                    }
                    repaint.request_repaint();
                }
                let _ = std::fs::remove_file(metadata);
            }));
            let own_metadata = directory.join(format!("{}.json", this.id));
            let watcher_scan_pending = Arc::clone(&scan_pending);
            let watcher_ctx = ctx.clone();
            let watcher_epoch = this.presence_epoch.clone();
            let watcher = crate::file_watch::Watcher::new(
                directory.clone(),
                notify::RecursiveMode::NonRecursive,
                ctx,
                move |event: notify::Result<notify::Event>| {
                    match event {
                        Ok(event) if registry_changed(&event, &own_metadata) => {
                            // Invalidate immediately, even when the scan is
                            // queued behind a slow peer request. Old scans may
                            // update session metadata, never authorize Dock.
                            watcher_epoch.fetch_add(1, Ordering::AcqRel);
                            watcher_ctx.request_repaint();
                            queue_scan(&tx, &watcher_scan_pending);
                        }
                        Err(_) => {
                            watcher_epoch.fetch_add(1, Ordering::AcqRel);
                            let _ = watcher_results.send(ResultEvent::PresenceUnknown);
                            watcher_ctx.request_repaint();
                        }
                        _ => {}
                    }
                },
            );
            this.directory = Some(directory);
            this._watcher = Some(watcher);
            // Registration completion requests the first scan in poll_window_sync.
            // Do not authorize Dock from a pre-watch snapshot with a presence gap.
            Ok(())
        })();
        if let Err(error) = result {
            this.error = Some(error);
        }
        this
    }
    pub fn enabled(&self) -> bool {
        self.directory.is_some()
    }
    pub(super) fn dock_single_window(&self) -> bool {
        self.dock_single_window
            && self.dock_watch_ready
            && self.error.is_none()
            && self.dock_epoch_accepted == Some(self.presence_epoch.load(Ordering::Acquire))
    }
    /// Reuse an owned lock or acquire a temporary lock. Never delete another
    /// window's live transcript, even if its cached status says Idle.
    pub(super) fn deletion_lock(&self, path: &Path) -> Result<Option<File>, String> {
        let path = identity(path);
        if self.owned.contains_key(&path) {
            return Ok(None);
        }
        #[cfg(test)]
        if self.directory.is_none() {
            return Ok(None);
        }
        let directory = self
            .directory
            .as_ref()
            .ok_or("Live session coordination is unavailable; deletion was refused")?;
        let file = private_open(&directory.join(format!("{}.lock", key(&path))))
            .map_err(|_| "Cannot lock chat for deletion")?;
        file.try_lock_exclusive().map_err(
            |_| "This chat is open in another Pui window. Close it there before deleting it.",
        )?;
        Ok(Some(file))
    }
    pub(super) fn forget_deleted(&mut self, path: &Path) {
        let path = identity(path);
        self.owned.remove(&path);
        self.attention_aliases
            .retain(|(raw, canonical)| raw != &path && canonical != &path);
        self.endpoints.remove(&path);
        self.pending.remove(&path);
        self.revisions.remove(&path);
        if self.mirror.as_ref() == Some(&path) {
            self.mirror = None;
        }
        let _ = self.tx.send(Work::Forget(path));
    }
    pub fn detach_mirror(&mut self) {
        self.mirror = None;
        self.revisions.clear();
    }
    pub fn owns(&self, path: &Path) -> bool {
        !self.owned.is_empty() && self.owned.contains_key(&identity(path))
    }
    pub fn peer(&self, path: &Path) -> bool {
        self.mirror.as_ref().is_some_and(|mirror| {
            let path = identity(path);
            mirror == &path && !self.owned.contains_key(&path)
        })
    }
    pub(super) fn attention_identity(&self, path: &Path) -> PathBuf {
        self.attention_aliases
            .iter()
            .find(|(raw, _)| raw == path)
            .map(|(_, canonical)| canonical.clone())
            .unwrap_or_else(|| attention::lexical_identity(path))
    }
    pub fn claim(&mut self, path: &Path) -> Result<bool, String> {
        let raw = path;
        let path = identity(path);
        if attention::valid_path(raw) && attention::valid_path(&path) {
            self.attention_aliases.retain(|(alias, _)| alias != raw);
            self.attention_aliases
                .push_back((raw.to_path_buf(), path.clone()));
            if self.attention_aliases.len() > 256 {
                self.attention_aliases.pop_front();
            }
        }
        if self.owned.contains_key(&path) {
            return Ok(true);
        }
        #[cfg(test)]
        if self.directory.is_none() {
            return Ok(true);
        }
        let directory = self.directory.as_ref().ok_or_else(|| {
            self.error
                .clone()
                .unwrap_or_else(|| "Live session coordination unavailable".into())
        })?;
        let file = private_open(&directory.join(format!("{}.lock", key(&path))))
            .map_err(|_| "Cannot lock session ownership")?;
        match file.try_lock_exclusive() {
            Ok(()) => {
                self.owned.insert(
                    path,
                    Owned {
                        _lock: file,
                        stamp: 0,
                        revision: 0,
                        published: std::time::Instant::now(),
                    },
                );
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                self.mirror = Some(path.clone());
                for endpoint in registry(directory, &self.id) {
                    index_endpoint(&mut self.endpoints, endpoint);
                }
                Ok(false)
            }
            Err(_) => Err("Cannot establish exclusive session ownership".into()),
        }
    }
    fn fetch(&mut self, path: &Path) {
        let path = identity(path);
        if self.pending.contains(&path) {
            return;
        }
        if let Some(peer) = self.endpoints.get(&path) {
            let revision = peer
                .paths
                .iter()
                .find(|p| p.path == path)
                .map(|p| p.revision)
                .unwrap_or(0);
            self.pending.insert(path.clone());
            let _ = self.tx.send(Work::Fetch(path, peer.clone(), revision));
        }
    }
    pub fn live_sessions(&self) -> Vec<LivePath> {
        self.endpoints
            .iter()
            .filter(|(path, _)| !self.owned.contains_key(*path))
            .filter_map(|(path, endpoint)| endpoint.paths.iter().find(|p| &p.path == path))
            .cloned()
            .collect()
    }
    pub fn act(&mut self, path: &Path, action: Action) -> Result<(), String> {
        let path = identity(path);
        let peer = self
            .endpoints
            .get(&path)
            .ok_or("The owner is starting or closed. Reopen this chat to reconnect.")?;
        self.tx
            .send(Work::Act(
                path.clone(),
                peer.clone(),
                *self.revisions.get(&path).unwrap_or(&0),
                action,
            ))
            .map_err(|_| "Window synchronization stopped".into())
    }
}
impl Drop for WindowSync {
    fn drop(&mut self) {
        self._watcher.take();
        let _ = self.tx.send(Work::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.stopped
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if self.port != 0 {
            let _ = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, self.port));
        }
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
        self._presence.take();
        if let Some(directory) = &self.directory {
            // Unlock alone produces no watcher event. Removing our released
            // lease lets peers recompute single-window eligibility after close.
            let _ = std::fs::remove_file(directory.join(format!("{}.owner", self.id)));
        }
    }
}

impl Pui {
    pub(super) fn other_window_status(&self, path: &Path) -> Option<&str> {
        if self.sync.endpoints.is_empty() {
            return None;
        }
        let path = identity(path);
        if self.sync.owned.contains_key(&path) {
            return None;
        }
        self.sync
            .endpoints
            .get(&path)?
            .paths
            .iter()
            .find(|p| p.path == path)
            .map(|p| p.label.as_str())
    }
    pub(super) fn recover_shared_draft(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        if self.sync.unsent.is_empty() {
            return;
        }
        ui.horizontal(|ui|{
            ui.label(RichText::new(format!("{} unsent/uncertain message(s) retained",self.sync.unsent.len())).size(11.0).color(colors.accent));
            if ui.small_button("Restore draft").clicked(){
                if self.draft.is_empty()&&self.draft_images.is_empty(){let message=self.sync.unsent.remove(0);self.context_chips.clear();self.draft=message.text;self.draft_images.set_images(message.images);self.focus_composer=true;}else{self.state.error=Some("Clear the current draft first. Retained messages are not automatically retried.".into());}
            }
        });
    }
    pub(super) fn is_mirror(&self) -> bool {
        self.client.is_none()
            && self
                .state
                .session_file
                .as_ref()
                .is_some_and(|p| self.sync.peer(Path::new(p)))
    }
    pub(super) fn attach_or_claim(&mut self, path: &Path) -> bool {
        match self.sync.claim(path) {
            Ok(true) => true,
            Ok(false) => {
                self.sync.fetch(path);
                self.state.notice =
                    Some("Shared chat · connecting to its owning Pui window".into());
                false
            }
            Err(error) => {
                self.loading_chat = false;
                self.state.error = Some(error);
                false
            }
        }
    }
    pub(super) fn remote_action(&mut self, action: Action) -> bool {
        let Some(path) = self.state.session_file.as_ref().map(PathBuf::from) else {
            return false;
        };
        if !self.is_mirror() {
            return false;
        }
        if !self.connected {
            self.state.error = Some(
                "Owner unavailable. Reopen this chat to reconnect; your message was retained."
                    .into(),
            );
            if let Action::Queue(message) = action {
                self.sync.unsent.push(message);
            }
            return true;
        }
        if let Err(error) = self.sync.act(&path, action.clone()) {
            self.state.error = Some(error);
            if let Action::Queue(message) = action {
                self.sync.unsent.push(message);
            }
        }
        true
    }
    pub(super) fn sync_live_owner(&self) -> bool {
        self.sync.enabled()
            && self.client.is_some()
            && self
                .state
                .session_file
                .as_ref()
                .is_some_and(|p| self.sync.owns(Path::new(p)))
    }
    pub(super) fn publish_live(&mut self) {
        if self.demo || !self.sync.enabled() || self.client.is_none() || self.loading_chat {
            return;
        }
        let Some(path) = self
            .state
            .session_file
            .as_ref()
            .map(|p| identity(Path::new(p)))
        else {
            return;
        };
        if !self.sync.owned.contains_key(&path) && !self.attach_or_claim(&path) {
            if let Some(client) = self.client.take() {
                client.retire();
            }
            return;
        }
        let queue = self
            .waiting
            .iter()
            .find(|(p, _)| identity(p) == path)
            .map(|(_, q)| q);
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (
            self.state.transcript_revision,
            self.state.prose_revision,
            self.state.outputs_revision,
            self.state.model.as_str(),
            self.state.provider.as_str(),
            self.state.thinking.as_str(),
            self.state.ready(),
            self.state.busy,
            self.state.compacting,
            self.state.pending,
            self.ultracode,
            self.ultracode_ready,
        )
            .hash(&mut hash);
        for receipt in &self.state.pending_messages {
            receipt.id.hash(&mut hash);
            receipt.text.hash(&mut hash);
            receipt.status.label().hash(&mut hash);
            receipt.observed.hash(&mut hash);
            for image in &receipt.images {
                image.key.hash(&mut hash);
            }
        }
        self.attention.metadata(&path).hash(&mut hash);
        self.state.error.hash(&mut hash);
        self.state.notice.hash(&mut hash);
        format!("{:?}", self.state.dialogs).hash(&mut hash);
        self.agent_settings(&self.agents.active)
            .payload()
            .to_string()
            .hash(&mut hash);
        self.agents
            .nodes
            .get(&self.agents.active)
            .map(|n| &n.access)
            .hash(&mut hash);
        self.prefs.settings.delivery.label().hash(&mut hash);
        self.prefs.notifications.sound.hash(&mut hash);
        self.prefs.notifications.desktop.hash(&mut hash);
        self.state.session_name.hash(&mut hash);
        self.state.model_name.hash(&mut hash);
        self.prefs.cwd.hash(&mut hash);
        for command in &self.state.commands {
            command.name.hash(&mut hash);
            command.description.hash(&mut hash);
        }
        self.state.permission_classifier.hash(&mut hash);
        self.state.permission_cost.to_bits().hash(&mut hash);
        self.state.compaction_settings_error.hash(&mut hash);
        self.state.compact_percent.hash(&mut hash);
        self.state.cost.to_bits().hash(&mut hash);
        self.state.context_tokens.map(f64::to_bits).hash(&mut hash);
        self.state.context_window.to_bits().hash(&mut hash);
        if let Some(parts) = &self.state.context_breakdown {
            for value in parts.values() {
                value.to_bits().hash(&mut hash);
            }
        }
        for message in queue.into_iter().flatten() {
            message.text.hash(&mut hash);
            message.mode.label().hash(&mut hash);
            for image in &message.images {
                image.key.hash(&mut hash);
            }
        }
        let stamp = hash.finish();
        let owner = self.sync.owned.get_mut(&path).unwrap();
        if owner.revision > 0 && owner.stamp == stamp {
            return;
        }
        let coalesce = Duration::from_millis(120);
        if owner.revision > 0 && !self.state.ready() && owner.published.elapsed() < coalesce {
            self.ctx
                .request_repaint_after(coalesce - owner.published.elapsed().min(coalesce));
            return;
        }
        owner.stamp = stamp;
        owner.revision += 1;
        owner.published = std::time::Instant::now();
        let revision = self.sync.owned[&path].revision;
        let _ = self.sync.tx.send(Work::Publish(LivePath {
            path,
            revision,
            label: if self.state.dialogs.is_empty()
                && self.state.ready()
                && self.state.error.is_some()
            {
                "Failed".into()
            } else {
                self.run_label().0.into()
            },
            title: self.state.chat_title().unwrap_or_default(),
            cwd: self.prefs.cwd.clone(),
            model: self.state.model_name.clone(),
        }));
    }
    pub(super) fn poll_window_sync(&mut self) {
        if let Some(status) = self
            .sync
            ._watcher
            .as_ref()
            .and_then(|watcher| watcher.poll())
        {
            match status {
                Ok(()) => {
                    self.sync.dock_watch_ready = true;
                    // Include publications made while registration was pending.
                    let _ = self.sync.tx.send(Work::Scan);
                }
                Err(_) => {
                    self.sync.dock_single_window = false;
                    self.sync.dock_watch_ready = false;
                    self.sync.error = Some("Cannot watch other Pui windows".into());
                }
            }
        }
        let events: Vec<_> = self.sync.rx.try_iter().collect();
        for event in events {
            match event {
                ResultEvent::PresenceUnknown => {
                    self.sync.dock_single_window = false;
                    self.sync.dock_watch_ready = false;
                }
                ResultEvent::Registry(endpoints, single, epoch) => {
                    self.sync.dock_single_window = single;
                    self.sync.dock_epoch_accepted = Some(epoch);
                    let before: BTreeSet<_> = self.sync.endpoints.keys().cloned().collect();
                    self.sync.endpoints.clear();
                    for endpoint in endpoints {
                        index_endpoint(&mut self.sync.endpoints, endpoint);
                    }
                    if self.client.is_none() {
                        if let Some(path) = self.state.session_file.as_ref().map(PathBuf::from) {
                            let changed = self
                                .sync
                                .endpoints
                                .get(&path)
                                .and_then(|e| e.paths.iter().find(|p| p.path == path))
                                .is_some_and(|p| {
                                    Some(&p.revision) != self.sync.revisions.get(&path)
                                });
                            if changed {
                                self.sync.fetch(&path);
                            }
                            if self.sync.peer(&path) && !self.sync.endpoints.contains_key(&path) {
                                self.connected = false;
                                self.loading_chat = false;
                                self.state.busy = false;
                                self.state.pending = false;
                                self.state.compacting = false;
                                self.state.error=Some("The owning Pui window closed. Reopen this chat to reconnect; queued work is not replayed.".into());
                            }
                        }
                    }
                    let after: BTreeSet<_> = self.sync.endpoints.keys().cloned().collect();
                    if before != after {
                        self.refresh_sessions();
                    }
                }
                ResultEvent::Snapshot(path, revision, snapshot) => {
                    self.sync.pending.remove(&path);
                    self.sync.revisions.insert(path.clone(), revision);
                    if self.is_mirror()
                        && self
                            .state
                            .session_file
                            .as_ref()
                            .is_some_and(|p| identity(Path::new(p)) == path)
                    {
                        if self.state.messages.len() > snapshot.state.messages.len()
                            || (self.state.transcript_revision == 0
                                && !self.state.messages.is_empty())
                        {
                            self.transcript
                                .replace_history(&self.state.messages, &snapshot.state.messages);
                        }
                        if let Some(metadata) = snapshot.attention {
                            // The owner supplies event identity, never derive unread
                            // from status/transcript or a transport revision.
                            self.attention.import_for(&path, metadata);
                        }
                        self.state = snapshot.state;
                        self.state.notice = Some(format!(
                            "Shared chat · {} in another Pui window",
                            snapshot.label
                        ));
                        self.waiting.insert(path.clone(), snapshot.waiting);
                        if let Some(node) = self.agents.nodes.get_mut(&self.agents.active) {
                            node.permission = Some(snapshot.settings.permission);
                            if let Some(access) = snapshot.access {
                                if node.access != access && access != "workspace" {
                                    node.launch = Some(agents::worker_launch(
                                        access == "read-only",
                                        &node.name,
                                        None,
                                        None,
                                    ));
                                }
                                node.access = access;
                            }
                            node.cwd = snapshot.cwd.clone();
                            node.worktree = snapshot.worktree;
                        }
                        self.prefs.cwd = snapshot.cwd;
                        let permission = self.prefs.settings.permission;
                        let has_agent = self.agents.nodes.contains_key(&self.agents.active);
                        self.prefs.settings = snapshot.settings;
                        if has_agent {
                            self.prefs.settings.permission = permission;
                        }
                        self.prefs.notifications = snapshot.notifications;
                        self.ultracode = snapshot.ultra;
                        self.ultracode_ready = snapshot.ready;
                        self.connected = true;
                        self.loading_chat = false;
                        self.pending_session = None;
                        self.preview_rx = None;
                        self.output_revision = u64::MAX;
                        let newer = self
                            .sync
                            .endpoints
                            .get(&path)
                            .and_then(|e| e.paths.iter().find(|p| p.path == path))
                            .is_some_and(|p| p.revision > revision);
                        if newer {
                            self.sync.fetch(&path);
                        }
                    }
                }
                ResultEvent::Action(path, reply, retained) => {
                    self.pending_dialog = None;
                    if reply.error.is_some() {
                        if let Some(message) = retained {
                            self.sync.unsent.push(message);
                        }
                    }
                    if let Some(edited) = reply.edited.clone() {
                        self.sync.unsent.push(edited);
                    }
                    if self
                        .state
                        .session_file
                        .as_ref()
                        .is_some_and(|p| identity(Path::new(p)) == path)
                    {
                        if let Some(error) = reply.error {
                            self.state.error = Some(error);
                        }
                        if reply.edited.is_some() {
                            self.current_dialog.clear();
                        }
                        if reply.edited.is_some()
                            && self.draft.is_empty()
                            && self.draft_images.is_empty()
                        {
                            let edited = self.sync.unsent.pop().unwrap();
                            self.context_chips.clear();
                            self.draft = edited.text;
                            self.draft_images.set_images(edited.images);
                            self.focus_composer = true;
                        }
                        self.sync.fetch(&path);
                    }
                }
                ResultEvent::Failure(path, error) => {
                    self.sync.pending.remove(&path);
                    if self.client.is_none()
                        && self
                            .state
                            .session_file
                            .as_ref()
                            .is_some_and(|p| identity(Path::new(p)) == path)
                    {
                        self.loading_chat = false;
                        self.connected = false;
                        self.state.busy = false;
                        self.state.pending = false;
                        self.state.compacting = false;
                        self.state.error = Some(error);
                    }
                }
            }
        }
        let commands: Vec<_> = self.sync.commands.try_iter().collect();
        for command in commands {
            let path = command.path.clone();
            let reply = self
                .with_live_chat(&path, |app| {
                    if let Some(action) = command.action {
                        match app.apply_remote_action(&command.path, command.revision, action) {
                            Ok(edited) => Reply {
                                snapshot: None,
                                edited,
                                error: None,
                            },
                            Err(error) => Reply {
                                snapshot: None,
                                edited: None,
                                error: Some(error),
                            },
                        }
                    } else {
                        match app.live_snapshot(&command.path) {
                            Ok(snapshot) => Reply {
                                snapshot: Some(snapshot),
                                edited: None,
                                error: None,
                            },
                            Err(error) => Reply {
                                snapshot: None,
                                edited: None,
                                error: Some(error),
                            },
                        }
                    }
                })
                .unwrap_or(Reply {
                    snapshot: None,
                    edited: None,
                    error: Some("Chat is no longer available in its owning window.".into()),
                });
            self.publish_live();
            let _ = command.reply.send(reply);
        }
    }
    fn live_snapshot(&self, path: &Path) -> Result<Snapshot, String> {
        if !self.sync.owns(path)
            || self
                .state
                .session_file
                .as_ref()
                .is_none_or(|p| identity(Path::new(p)) != path)
        {
            return Err("Chat is no longer active in its owner".into());
        }
        Ok(Snapshot {
            revision: self.sync.owned[path].revision,
            state: self.state.clone(),
            waiting: self
                .waiting
                .iter()
                .find(|(p, _)| identity(p) == path)
                .map(|(_, q)| q.clone())
                .unwrap_or_default(),
            settings: self.agent_settings(&self.agents.active),
            access: self
                .agents
                .nodes
                .get(&self.agents.active)
                .map(|n| n.access.clone()),
            worktree: self
                .agents
                .nodes
                .get(&self.agents.active)
                .and_then(|n| n.worktree.clone()),
            notifications: self.prefs.notifications,
            attention: self
                .attention
                .metadata(path)
                .map(attention::Metadata::unread_snapshot),
            ultra: self.ultracode,
            ready: self.ultracode_ready,
            cwd: self.prefs.cwd.clone(),
            label: self.run_label().0.into(),
        })
    }
    fn apply_remote_action(
        &mut self,
        path: &Path,
        revision: u64,
        action: Action,
    ) -> Result<Option<run_status::WaitingMessage>, String> {
        if !self.sync.owns(path)
            || self
                .state
                .session_file
                .as_ref()
                .is_none_or(|p| identity(Path::new(p)) != path)
        {
            return Err(
                "This chat is not active in its owning window. Switch to it there first.".into(),
            );
        }
        if self.deletion.in_progress() || self.deletion.deleting_chat(path) {
            return Err("Deletion is in progress for this chat in the owning window.".into());
        }
        if self.loading_chat || self.resource_reload.pending() {
            return Err("Owning window is connecting; try again when ready.".into());
        }
        if self.history_pending.is_some() || self.restoring_code() {
            return Err("History navigation is in progress in the owning window".into());
        }
        let queue_path = self.state.session_file.as_ref().map(PathBuf::from).unwrap();
        let mut edited = None;
        match action {
            Action::Queue(message) => {
                if message.text.len() > 256000 || message.images.len() > 8 {
                    return Err("Message exceeds sync limits".into());
                }
                self.waiting
                    .entry(queue_path.clone())
                    .or_default()
                    .push(message);
            }
            Action::Remove(index) | Action::Edit(index) | Action::SendNow(index) => {
                if self.sync.owned[path].revision != revision {
                    return Err("The shared queue changed. Review it and try again.".into());
                }
                let queue = self.waiting.entry(queue_path.clone()).or_default();
                if index >= queue.len() {
                    return Err("Message is no longer waiting".into());
                }
                match action {
                    Action::Remove(_) => {
                        queue.remove(index);
                    }
                    Action::Edit(_) => {
                        edited = Some(queue.remove(index));
                    }
                    Action::SendNow(_) => {
                        if self.state.compacting
                            || !self.state.dialogs.is_empty()
                            || (!self.state.ready() && !self.state.busy)
                        {
                            return Err("Wait for compaction/approval before sending now.".into());
                        }
                        if !self.send_now(index) {
                            return Err("Owner could not interrupt/send; message retained".into());
                        }
                    }
                    _ => unreachable!(),
                }
            }
            Action::Notifications(settings) => {
                let enabled = !self.prefs.notifications.desktop && settings.desktop;
                self.prefs.notifications = settings;
                if enabled {
                    self.notifications.configure(&self.ctx);
                }
            }
            Action::Settings(settings) => {
                if self.readonly_worker() && settings.permission != settings::PermissionMode::Plan {
                    return Err(
                        "Read-only worker upgrades must be confirmed in the owning window.".into(),
                    );
                }
                let before = self.prefs.settings.clone();
                self.set_permission(settings.permission);
                self.prefs.settings.delivery = settings.delivery;
                self.prefs.settings.compact_percent = settings.compact_percent;
                self.prefs.settings.keep_recent_tokens = settings.keep_recent_tokens;
                if before.payload() != self.prefs.settings.payload() {
                    self.apply_settings();
                }
            }
            Action::Rpc(record) => {
                if self.history_pending.is_some() {
                    return Err("History navigation is in progress in the owning window".into());
                }
                let kind = protocol::string(&record, "type");
                if !matches!(
                    kind,
                    "abort"
                        | "clear_queue"
                        | "set_model"
                        | "set_thinking_level"
                        | "compact"
                        | "extension_ui_response"
                        | "prompt"
                ) {
                    return Err("Unsupported shared action".into());
                }
                if matches!(kind, "set_model" | "set_thinking_level") {
                    if let Some(reason) = self.model_change_blocked() {
                        return Err(reason.into());
                    }
                    if kind == "set_thinking_level"
                        && !self
                            .state
                            .levels
                            .iter()
                            .any(|level| record["level"] == *level)
                    {
                        return Err(
                            "This effort level is not supported by the current model.".into()
                        );
                    }
                }
                if kind == "prompt" {
                    let message = protocol::string(&record, "message");
                    if !matches!(message, "/pui-ultracode on" | "/pui-ultracode off") {
                        return Err("Unsupported shared command".into());
                    }
                    self.ensure_agent();
                }
                if kind == "extension_ui_response"
                    && !self.state.dialogs.iter().any(|d| d["id"] == record["id"])
                {
                    return Err("This approval dialog is no longer active".into());
                }
                if kind == "abort" {
                    self.pause_agent_jobs(&self.agents.active.clone());
                }
                if kind == "clear_queue" {
                    self.cancel_send_now();
                    self.waiting.remove(&queue_path);
                }
                if kind == "extension_ui_response" {
                    if self.workflow_answer(&record) {
                        self.state.dialogs.retain(|d| d["id"] != record["id"]);
                        return Ok(edited);
                    }
                    self.state.dialogs.retain(|d| d["id"] != record["id"]);
                }
                self.send(record);
            }
        }
        Ok(edited)
    }
}

#[cfg(test)]
#[path = "sync/dock_epoch_tests.rs"]
mod dock_epoch_tests;
#[cfg(test)]
mod dock_tests;
#[cfg(test)]
mod tests {
    use super::*;
    fn location() -> (PathBuf, PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("pui-window-tests-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("chat.jsonl");
        std::fs::write(&path, "").unwrap();
        (identity(&directory), identity(&path))
    }
    fn app(directory: &Path, path: &Path) -> Pui {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.state = State {
            session_file: Some(path.display().to_string()),
            ..Default::default()
        };
        app.sync = WindowSync::new_at(&ctx, false, Some(directory.join("registry")));
        assert!(app.sync.error.is_none(), "{:?}", app.sync.error);
        app.connected = true;
        app.ultracode_ready = true;
        app
    }
    fn spin(a: &mut Pui, b: &mut Pui, done: impl Fn(&Pui, &Pui) -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !done(a, b) && std::time::Instant::now() < deadline {
            a.poll_window_sync();
            b.poll_window_sync();
            a.publish_live();
            b.publish_live();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            done(a, b),
            "cross-window operation timed out: {:?} / {:?}",
            a.state.error,
            b.state.error
        );
    }
    #[cfg(unix)]
    #[test]
    fn attention_claim_identity_cache_reuses_real_alias_without_new_path_probes() {
        let (directory, path) = location();
        let alias = directory.join("alias.jsonl");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        let mut owner = app(&directory, &alias);
        assert!(owner.attach_or_claim(&alias));
        assert_eq!(owner.sync.attention_identity(&alias), path);
        for n in 0..300 {
            assert!(
                owner
                    .sync
                    .claim(&directory.join(format!("missing-{n}.jsonl")))
                    .unwrap()
            );
        }
        assert_eq!(owner.sync.attention_aliases.len(), 256);
        owner.sync.forget_deleted(&path);
        assert!(!owner.sync.attention_aliases.iter().any(|(_, p)| p == &path));
    }
    #[test]
    fn observed_attention_mirrors_same_revision_without_replaying_or_sharing_reads() {
        let (directory, path) = location();
        let mut owner = app(&directory, &path);
        owner.client = Some(rpc::Client::stub());
        owner.ensure_agent();
        assert!(owner.attach_or_claim(&path));
        owner.completion_event(&json!({"type":"agent_start"}));
        owner.completion_event(&json!({"type":"agent_settled"}));
        owner.publish_live();
        let original = owner.attention.metadata(&path).unwrap();
        let mut mirror = app(&directory, &path);
        assert!(!mirror.attach_or_claim(&path));
        spin(&mut owner, &mut mirror, |_, b| {
            b.attention.unread() == 1 && b.connected
        });
        assert_eq!(mirror.attention.metadata(&path).unwrap(), original);
        let _ = mirror.ctx.clone().run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| mirror.attention_rendered(ui));
        });
        assert_eq!(mirror.attention.unread(), 0);
        assert_eq!(owner.attention.unread(), 1);
        owner.state.notice = Some("transport revision only".into());
        owner.publish_live();
        let wanted = owner.sync.owned[&path].revision;
        spin(&mut owner, &mut mirror, |_, b| {
            b.sync.revisions.get(&path).copied().unwrap_or(0) >= wanted
        });
        assert_eq!(mirror.attention.unread(), 0);
        mirror.completion_event(&json!({"type":"agent_start"}));
        mirror.completion_event(&json!({"type":"agent_settled"}));
        assert_eq!(
            mirror.attention.metadata(&path).unwrap().unread_snapshot(),
            original
        );
        let mut snapshot = serde_json::to_value(owner.live_snapshot(&path).unwrap()).unwrap();
        snapshot["attention"]["path"] = json!("x".repeat(5000));
        let snapshot: Snapshot = serde_json::from_value(snapshot).unwrap();
        mirror
            .attention
            .import_for(&path, snapshot.attention.unwrap());
        assert_eq!(mirror.attention.unread(), 0);
        owner.shutdown_agents();
        mirror.shutdown_agents();
    }
    #[test]
    fn kanban_discovers_other_windows_without_attaching_or_duplicating_sessions() {
        let (directory, path) = location();
        let mut owner = app(&directory, &path);
        let mut observer = app(&directory, &path);
        observer.state = State::default();
        observer.connected = false;
        owner.client = Some(rpc::Client::stub());
        owner.prefs.cwd = directory.clone();
        owner.state.session_name = "Live agent in another window".into();
        owner.state.model_name = "Example model".into();
        owner.state.busy = true;
        assert!(owner.attach_or_claim(&path));
        owner.publish_live();
        spin(&mut owner, &mut observer, |_, b| {
            !b.sync.live_sessions().is_empty()
        });
        let cards = observer.board_cards();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "Live agent in another window");
        assert_eq!(cards[0].project, directory);
        assert_eq!(cards[0].model, "Example model");
        assert_eq!(cards[0].status, kanban::Status::Working);
        assert!(observer.client.is_none());
        assert!(
            observer.sync.mirror.is_none(),
            "Board discovery must not attach a session"
        );
        owner.state.dialogs.push_back(questions::demo_dialog());
        std::thread::sleep(Duration::from_millis(130));
        owner.publish_live();
        spin(&mut owner, &mut observer, |_, b| {
            b.board_cards()
                .iter()
                .any(|c| c.status == kanban::Status::Waiting)
        });
        observer.state.session_file = Some(path.display().to_string());
        observer.connected = true;
        assert_eq!(
            observer.board_cards().len(),
            1,
            "Mirrored paths must be deduplicated"
        );
        owner.state.dialogs.clear();
        owner.state.busy = false;
        owner.state.error = Some("Provider disconnected".into());
        owner.publish_live();
        spin(&mut owner, &mut observer, |_, b| {
            b.sync.live_sessions().iter().any(|p| p.label == "Failed")
        });
        observer.state = State::default();
        observer.connected = false;
        assert_eq!(observer.board_cards()[0].status, kanban::Status::Error);
        // Registry additions are backwards compatible with older metadata.
        let legacy: LivePath =
            serde_json::from_value(json!({"path":"old.jsonl","revision":1,"label":"Idle"}))
                .unwrap();
        assert!(legacy.title.is_empty());
    }
    fn endpoint(paths: impl IntoIterator<Item = PathBuf>) -> Endpoint {
        Endpoint {
            id: "peer".into(),
            port: 1,
            token: "test-token".into(),
            paths: paths
                .into_iter()
                .map(|path| LivePath {
                    path,
                    revision: 1,
                    label: "Working…".into(),
                    title: String::new(),
                    cwd: PathBuf::new(),
                    model: String::new(),
                })
                .collect(),
        }
    }
    #[test]
    fn endpoint_index_and_queued_work_share_one_paths_allocation() {
        let (directory, path) = location();
        let mut sync = WindowSync::new(&egui::Context::default(), true);
        let (tx, work) = mpsc::channel();
        sync.tx = tx;
        let paths = std::iter::once(path.clone())
            .chain((1..1024).map(|i| directory.join(format!("chat-{i}.jsonl"))));
        index_endpoint(&mut sync.endpoints, endpoint(paths));
        let shared = Arc::clone(&sync.endpoints[&path]);
        assert_eq!(sync.endpoints.len(), 1024);
        assert_eq!(Arc::strong_count(&shared), 1025);
        assert!(sync.endpoints.values().all(|e| Arc::ptr_eq(e, &shared)));
        sync.fetch(&path);
        sync.act(&path, Action::Remove(0)).unwrap();
        let Work::Fetch(_, fetched, 1) = work.try_recv().unwrap() else {
            panic!("expected fetch");
        };
        let Work::Act(_, acted, 0, Action::Remove(0)) = work.try_recv().unwrap() else {
            panic!("expected action");
        };
        assert!(Arc::ptr_eq(&shared, &fetched));
        assert!(Arc::ptr_eq(&shared, &acted));
    }
    #[test]
    fn scan_bursts_coalesce_without_losing_actions_or_changes_during_scan() {
        let (directory, path) = location();
        let presence = private_open(&directory.join("peer.owner")).unwrap();
        presence.try_lock_exclusive().unwrap();
        let mut peer = endpoint([path.clone()]);
        write_endpoint(&directory, &peer).unwrap();
        let (tx, work) = mpsc::channel();
        let pending = AtomicBool::new(false);
        for _ in 0..1000 {
            queue_scan(&tx, &pending);
        }
        assert!(matches!(work.try_recv().unwrap(), Work::Scan));
        assert!(work.try_recv().is_err());
        let first = run_scan(&pending, || {
            let first = registry(&directory, "own");
            // The first scan has already read the old metadata. A publication
            // before it finishes must still cause another scan.
            peer.paths[0].revision = 2;
            write_endpoint(&directory, &peer).unwrap();
            for i in 0..1000 {
                tx.send(Work::Act(
                    path.clone(),
                    Arc::new(endpoint([path.clone()])),
                    1,
                    Action::Remove(i),
                ))
                .unwrap();
                queue_scan(&tx, &pending);
            }
            first
        });
        assert_eq!(first[0].paths[0].revision, 1);
        let mut scans = 0;
        let mut actions = Vec::new();
        while let Ok(job) = work.try_recv() {
            match job {
                Work::Scan => {
                    scans += 1;
                    let latest = run_scan(&pending, || registry(&directory, "own"));
                    assert_eq!(latest[0].paths[0].revision, 2);
                }
                Work::Act(_, _, _, Action::Remove(i)) => actions.push(i),
                _ => panic!("unexpected work"),
            }
        }
        assert_eq!(scans, 1);
        assert_eq!(actions, (0..1000).collect::<Vec<_>>());
        assert!(!pending.load(Ordering::Acquire));
        queue_scan(&tx, &pending);
        assert!(matches!(work.try_recv().unwrap(), Work::Scan));
        drop(work);
        pending.store(false, Ordering::Release);
        queue_scan(&tx, &pending);
        assert!(!pending.load(Ordering::Acquire));
    }
    #[cfg(unix)]
    #[test]
    fn ownership_checks_resolve_retargeted_symlinks_freshly() {
        let (directory, path) = location();
        let next = directory.join("next.jsonl");
        std::fs::write(&next, "").unwrap();
        let alias = directory.join("alias.jsonl");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        // Exercise real ownership/deletion locks without starting OS watchers.
        let mut owner = WindowSync::new(&egui::Context::default(), true);
        owner.directory = Some(directory.clone());
        assert!(owner.claim(&alias).unwrap());
        index_endpoint(&mut owner.endpoints, endpoint([path.clone(), next.clone()]));
        let mut observer = WindowSync::new(&egui::Context::default(), true);
        observer.mirror = Some(path.clone());
        assert!(owner.owns(&alias));
        assert!(observer.peer(&alias));
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&next, &alias).unwrap();
        assert!(!owner.owns(&alias));
        assert!(!observer.peer(&alias));
        assert!(owner.deletion_lock(&alias).unwrap().is_some());
        assert!(owner.owns(&path));
        assert!(!owner.live_sessions().iter().any(|p| p.path == path));
        assert!(owner.live_sessions().iter().any(|p| p.path == next));
        assert!(owner.claim(&alias).unwrap());
        assert!(owner.owned.contains_key(&next));
    }
    #[test]
    fn registry_watcher_ignores_own_publications_and_temporary_files() {
        use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};
        let root = Path::new("registry");
        let own = root.join("own.json");
        let peer = root.join("peer.json");
        for kind in [
            notify::EventKind::Create(CreateKind::File),
            notify::EventKind::Modify(ModifyKind::Any),
            notify::EventKind::Remove(RemoveKind::File),
        ] {
            let event = notify::Event::new(kind);
            assert!(!registry_changed(
                &event.clone().add_path(own.clone()),
                &own
            ));
            assert!(!registry_changed(
                &event.clone().add_path(root.join("own.tmp")),
                &own
            ));
            assert!(registry_changed(
                &event.clone().add_path(peer.clone()),
                &own
            ));
            assert!(!registry_changed(
                &event.clone().add_path(root.join("own.owner")),
                &own
            ));
            assert!(registry_changed(
                &event.clone().add_path(root.join("peer.owner")),
                &own
            ));
            assert!(registry_changed(
                &event.add_path(own.clone()).add_path(peer.clone()),
                &own
            ));
        }
        assert!(!registry_changed(
            &notify::Event::new(notify::EventKind::Access(AccessKind::Any)).add_path(peer),
            &own
        ));
    }
    #[test]
    fn publication_updates_registry_without_waking_its_own_ui() {
        let (directory, path) = location();
        let ctx = egui::Context::default();
        let (wake, wakes) = mpsc::channel();
        ctx.set_request_repaint_callback(move |_| {
            let _ = wake.send(());
        });
        // Clear egui's initial repaint before starting the registry worker.
        for _ in 0..3 {
            let _ = ctx.run(egui::RawInput::default(), |_| {});
        }
        wakes.try_iter().for_each(drop);
        let mut sync = WindowSync::new_at(&ctx, false, Some(directory.join("registry")));
        assert!(sync.error.is_none(), "{:?}", sync.error);
        wakes.recv_timeout(Duration::from_secs(5)).unwrap();
        for _ in 0..3 {
            let _ = ctx.run(egui::RawInput::default(), |_| {});
        }
        assert!(!ctx.has_requested_repaint());
        sync.tx
            .send(Work::Publish(LivePath {
                path: path.clone(),
                revision: 1,
                label: "Working…".into(),
                title: String::new(),
                cwd: PathBuf::new(),
                model: String::new(),
            }))
            .unwrap();
        // Stop is a barrier: the publication must finish before we inspect the
        // repaint state. The watcher remains active throughout publication.
        sync.tx.send(Work::Stop).unwrap();
        sync.worker.take().unwrap().join().unwrap();
        assert!(!ctx.has_requested_repaint());
        let endpoint = Endpoint {
            id: sync.id.clone(),
            port: sync.port,
            token: "test-token".into(),
            paths: vec![LivePath {
                path,
                revision: 1,
                label: "Working…".into(),
                title: String::new(),
                cwd: PathBuf::new(),
                model: String::new(),
            }],
        };
        let registry = sync.directory.as_ref().unwrap();
        write_endpoint(registry, &endpoint).unwrap();
        let metadata = registry.join(format!("{}.json", sync.id));
        let saved: Endpoint = serde_json::from_reader(File::open(&metadata).unwrap()).unwrap();
        assert_eq!(saved.paths[0].revision, 1);
        let mut endpoint = endpoint;
        endpoint.paths.clear();
        write_endpoint(registry, &endpoint).unwrap();
        let saved: Endpoint = serde_json::from_reader(File::open(metadata).unwrap()).unwrap();
        assert!(
            saved.paths.is_empty(),
            "shorter updates must truncate old JSON"
        );
    }
    #[test]
    fn two_windows_share_history_status_images_and_keep_drafts_local() {
        let (directory, path) = location();
        let mut owner = app(&directory, &path);
        let mut mirror = app(&directory, &path);
        owner.client = Some(rpc::Client::stub());
        assert!(owner.attach_or_claim(&path));
        owner.state.busy = true;
        owner.state.run_observed = true;
        owner.state.cost = 4.25;
        owner.state.context_tokens = Some(12345.0);
        owner.state.outputs_revision = 9;
        owner.state.messages.push(Message {
            role: "user".into(),
            text: "live message".into(),
            images: vec![
                crate::media::Attachment::from_bytes(include_bytes!("../../assets/icon.png"))
                    .unwrap(),
            ],
            ..Default::default()
        });
        owner.state.transcript_revision = 1;
        owner.publish_live();
        mirror.draft = "local unsent draft".into();
        assert!(!mirror.attach_or_claim(&path));
        assert!(mirror.client.is_none());
        spin(&mut owner, &mut mirror, |_, b| {
            b.state.messages.len() == 1 && b.connected
        });
        assert!(mirror.is_mirror());
        assert!(mirror.state.busy);
        assert_eq!(mirror.state.cost, 4.25);
        assert_eq!(mirror.state.context_tokens, Some(12345.0));
        assert_eq!(mirror.state.messages[0].images.len(), 1);
        assert_eq!(mirror.draft, "local unsent draft");
        assert!(owner.draft.is_empty());
        owner.state.messages[0].text = "updated live message".into();
        owner.state.transcript_revision += 1;
        owner.state.busy = false;
        owner.state.run_observed = false;
        owner.publish_live();
        spin(&mut owner, &mut mirror, |_, b| {
            b.state.messages[0].text == "updated live message" && !b.state.busy
        });
    }
    #[test]
    fn context_only_changes_publish_without_a_new_message_or_usage_total() {
        let (directory, path) = location();
        let mut owner = app(&directory, &path);
        let mut mirror = app(&directory, &path);
        owner.client = Some(rpc::Client::stub());
        assert!(owner.attach_or_claim(&path));
        owner.state.context_tokens = Some(4200.0);
        owner.publish_live();
        assert!(!mirror.attach_or_claim(&path));
        spin(&mut owner, &mut mirror, |_, b| {
            b.state.context_tokens == Some(4200.0)
        });
        owner.state.context_window = 10000.0;
        owner.state.context_breakdown = Some(protocol::ContextBreakdown {
            system: 100.0,
            tool_results: 2000.0,
            ..Default::default()
        });
        owner.publish_live();
        spin(&mut owner, &mut mirror, |_, b| {
            b.state.context_breakdown.is_some()
        });
        assert_eq!(mirror.state.context_window, 10000.0);
        assert_eq!(
            mirror
                .state
                .context_breakdown
                .as_ref()
                .unwrap()
                .tool_results,
            2000.0
        );
        owner.state.context_breakdown = None;
        owner.publish_live();
        spin(&mut owner, &mut mirror, |_, b| {
            b.state.context_breakdown.is_none()
        });
        owner.state.compact_percent = Some(85);
        owner.publish_live();
        spin(&mut owner, &mut mirror, |_, b| {
            b.state.compact_percent == Some(85)
        });
    }
    #[test]
    fn alert_settings_change_while_busy_and_only_owner_notifies() {
        let (directory, path) = location();
        let mut owner = app(&directory, &path);
        let mut mirror = app(&directory, &path);
        owner.client = Some(rpc::Client::stub());
        assert!(owner.attach_or_claim(&path));
        owner.state.busy = true;
        owner.publish_live();
        assert!(!mirror.attach_or_claim(&path));
        spin(&mut owner, &mut mirror, |_, b| {
            b.state.busy && b.sync.revisions.contains_key(&path)
        });
        let settings = notifications::NotificationSettings {
            sound: false,
            desktop: true,
        };
        let before = mirror.prefs.notifications;
        mirror.prefs.notifications = settings;
        mirror.propagate_notifications(&mirror.ctx.clone(), before);
        spin(&mut owner, &mut mirror, |a, b| {
            a.prefs.notifications == settings && b.prefs.notifications == settings
        });
        owner.completion_event(&json!({"type":"agent_start"}));
        owner.completion_event(&json!({"type":"agent_settled","aborted":false}));
        mirror.completion_event(&json!({"type":"agent_start"}));
        mirror.completion_event(&json!({"type":"agent_settled","aborted":false}));
        assert_eq!(owner.notifications.sent.len(), 1);
        assert!(!owner.notifications.sent[0].sound);
        assert!(owner.notifications.sent[0].desktop);
        assert!(mirror.notifications.sent.is_empty());
    }
    #[test]
    fn mirrored_compaction_never_promotes_a_manual_worker_to_the_window_default() {
        let (directory, path) = location();
        let mut owner = app(&directory, &path);
        let mut mirror = app(&directory, &path);
        owner.ensure_agent();
        mirror.ensure_agent();
        for app in [&mut owner, &mut mirror] {
            app.prefs.settings.permission = settings::PermissionMode::Bypass;
            let node = app.agents.nodes.get_mut(&app.agents.active).unwrap();
            node.access = "worktree".into();
            node.permission = Some(settings::PermissionMode::Manual);
        }
        owner.client = Some(rpc::Client::stub());
        assert!(owner.attach_or_claim(&path));
        owner.publish_live();
        assert!(!mirror.attach_or_claim(&path));
        spin(&mut owner, &mut mirror, |_, b| {
            b.sync.revisions.contains_key(&path)
        });
        let before = mirror.prefs.settings.clone();
        mirror.prefs.settings.compact_percent = 71;
        mirror.propagate_settings(&before);
        spin(&mut owner, &mut mirror, |a, b| {
            a.prefs.settings.compact_percent == 71 && b.prefs.settings.compact_percent == 71
        });
        assert!(owner.active_permission() == settings::PermissionMode::Manual);
        assert!(mirror.active_permission() == settings::PermissionMode::Manual);
        assert!(owner.prefs.settings.permission == settings::PermissionMode::Bypass);
        assert!(mirror.prefs.settings.permission == settings::PermissionMode::Bypass);
        owner.shutdown_agents();
        mirror.shutdown_agents();
    }
    #[test]
    fn mirrored_worker_upgrade_refreshes_workspace_and_reconnect_tools() {
        let (directory, path) = location();
        let workspace = directory.join("isolated");
        std::fs::create_dir_all(&workspace).unwrap();
        let mut owner = app(&directory, &path);
        let mut mirror = app(&directory, &path);
        owner.ensure_agent();
        let mirror_id = mirror.ensure_agent();
        for app in [&mut owner, &mut mirror] {
            let node = app.agents.nodes.get_mut(&app.agents.active).unwrap();
            node.access = "read-only".into();
            node.permission = Some(settings::PermissionMode::Plan);
            node.launch = Some(agents::worker_launch(true, "reader", None, None));
        }
        owner.client = Some(rpc::Client::stub());
        assert!(owner.attach_or_claim(&path));
        owner.publish_live();
        assert!(!mirror.attach_or_claim(&path));
        spin(&mut owner, &mut mirror, |_, b| {
            b.sync.revisions.contains_key(&path)
        });
        let node = owner.agents.nodes.get_mut(&owner.agents.active).unwrap();
        node.access = "worktree".into();
        node.permission = Some(settings::PermissionMode::Manual);
        node.cwd = workspace.clone();
        node.worktree = Some(workspace.clone());
        node.launch = Some(agents::worker_launch(false, "reader", None, None));
        owner.prefs.cwd = workspace.clone();
        owner.publish_live();
        spin(&mut owner, &mut mirror, |_, b| !b.readonly_worker());
        let node = &mirror.agents.nodes[&mirror_id];
        assert_eq!(node.cwd, workspace);
        assert_eq!(node.worktree.as_ref(), Some(&workspace));
        assert!(!node.launch.as_ref().unwrap().readonly);
        assert_eq!(mirror.prefs.cwd, workspace);
        assert!(mirror.active_permission() == settings::PermissionMode::Manual);
        owner.shutdown_agents();
        owner.sync.forget_deleted(&path);
        mirror.sync.detach_mirror();
        assert!(mirror.resume_agent());
        assert!(
            !mirror.agents.nodes[&mirror_id]
                .launch
                .as_ref()
                .unwrap()
                .readonly
        );
        mirror.shutdown_agents();
    }
    #[test]
    fn shared_permissions_and_compaction_can_change_while_owner_is_working() {
        let (directory, path) = location();
        let mut owner = app(&directory, &path);
        let mut mirror = app(&directory, &path);
        owner.client = Some(rpc::Client::stub());
        assert!(owner.attach_or_claim(&path));
        owner.state.busy = true;
        owner.publish_live();
        assert!(!mirror.attach_or_claim(&path));
        spin(&mut owner, &mut mirror, |_, b| {
            b.state.busy && b.sync.revisions.contains_key(&path)
        });
        let settings = settings::Settings {
            permission: settings::PermissionMode::Plan,
            compact_percent: 75,
            ..Default::default()
        };
        mirror.remote_action(Action::Settings(settings.clone()));
        spin(&mut owner, &mut mirror, |a, b| {
            a.prefs.settings == settings && b.prefs.settings == settings
        });
        assert!(owner.state.busy);
        assert!(owner.client.is_some());
    }
    #[test]
    fn shared_send_now_interrupts_owner_and_publishes_immediate_receipt() {
        let (directory, path) = location();
        let mut owner = app(&directory, &path);
        let mut mirror = app(&directory, &path);
        let (client, tx, commands) = rpc::Client::stub_with_commands();
        owner.client = Some(client);
        owner.state.busy = true;
        owner.state.run_observed = true;
        assert!(owner.attach_or_claim(&path));
        owner.waiting.insert(
            path.clone(),
            vec![run_status::WaitingMessage {
                text: "interrupt from attached window".into(),
                ..Default::default()
            }],
        );
        owner.publish_live();
        assert!(!mirror.attach_or_claim(&path));
        spin(&mut owner, &mut mirror, |_, b| {
            b.waiting.get(&path).is_some_and(|q| q.len() == 1)
        });
        mirror.remote_action(Action::SendNow(0));
        spin(&mut owner, &mut mirror, |a, b| {
            a.interrupt.is_some() && b.state.pending_messages.len() == 1
        });
        assert_eq!(
            mirror.state.pending_messages[0].status,
            protocol::DeliveryStatus::Interrupting
        );
        assert_eq!(
            commands.recv_timeout(Duration::from_secs(1)).unwrap()["type"],
            "abort_bash"
        );
        let abort = commands.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(abort["type"], "abort");
        assert!(owner.deliveries.is_empty());
        tx.send(rpc::Event::Record(
            json!({"type":"agent_settled","aborted":true}),
        ))
        .unwrap();
        tx.send(rpc::Event::Record(
            json!({"type":"response","command":"abort","id":abort["id"],"success":true}),
        ))
        .unwrap();
        owner.poll_rpc();
        assert_eq!(owner.deliveries.len(), 1);
        let id = owner.deliveries[0].id.clone();
        tx.send(rpc::Event::Record(json!({"type":"response","command":"prompt","id":id,"success":true,"data":{"disposition":"started"}}))).unwrap();
        owner.poll_rpc();
        spin(&mut owner, &mut mirror, |_, b| {
            b.state
                .pending_messages
                .first()
                .is_some_and(|p| p.status == protocol::DeliveryStatus::AwaitingAgent)
        });
        assert!(mirror.state.messages.is_empty());
        tx.send(rpc::Event::Record(json!({"type":"message_start","message":{"role":"user","content":"interrupt from attached window","timestamp":42}}))).unwrap();
        owner.poll_rpc();
        spin(&mut owner, &mut mirror, |_, b| {
            b.state.messages.len() == 1 && b.state.pending_messages.is_empty()
        });
    }

    #[test]
    fn shared_queue_edit_remove_and_stale_revision_are_authoritative() {
        let (directory, path) = location();
        let mut owner = app(&directory, &path);
        let mut mirror = app(&directory, &path);
        owner.client = Some(rpc::Client::stub());
        assert!(owner.attach_or_claim(&path));
        owner.publish_live();
        assert!(!mirror.attach_or_claim(&path));
        spin(&mut owner, &mut mirror, |_, b| {
            b.is_mirror() && b.connected && !b.loading_chat && b.sync.revisions.contains_key(&path)
        });
        mirror.draft = "queued by second window".into();
        mirror.queue_draft_mode(settings::DeliveryMode::Queue);
        spin(&mut owner, &mut mirror, |a, b| {
            a.waiting.get(&path).is_some_and(|q| q.len() == 1)
                && b.waiting.get(&path).is_some_and(|q| q.len() == 1)
        });
        let revision = owner.sync.owned[&path].revision;
        assert!(
            owner
                .apply_remote_action(&path, revision - 1, Action::Remove(0))
                .is_err()
        );
        assert_eq!(owner.waiting[&path].len(), 1);
        assert!(mirror.remote_action(Action::Edit(0)));
        spin(&mut owner, &mut mirror, |a, b| {
            a.waiting[&path].is_empty() && b.draft == "queued by second window"
        });
        assert!(owner.draft.is_empty());
    }
    #[test]
    fn owner_switching_chats_still_serves_original_without_changing_visible_chat() {
        let (directory, path) = location();
        let next = directory.join("next.jsonl");
        std::fs::write(&next, "").unwrap();
        let mut owner = app(&directory, &path);
        let mut mirror = app(&directory, &path);
        owner.client = Some(rpc::Client::stub());
        assert!(owner.attach_or_claim(&path));
        owner.state.messages.push(Message {
            role: "assistant".into(),
            text: "original running response".into(),
            ..Default::default()
        });
        owner.state.transcript_revision = 1;
        owner.publish_live();
        let root = owner.ensure_agent();
        owner.park_tree();
        owner.state = State {
            session_file: Some(next.display().to_string()),
            ..Default::default()
        };
        owner.client = Some(rpc::Client::stub());
        owner.connected = true;
        owner.ensure_agent();
        assert!(!mirror.attach_or_claim(&path));
        spin(&mut owner, &mut mirror, |_, b| b.state.messages.len() == 1);
        assert_eq!(mirror.state.messages[0].text, "original running response");
        assert_eq!(owner.state.session_file.as_deref(), next.to_str());
        mirror.draft = "message for original".into();
        mirror.queue_draft_mode(settings::DeliveryMode::Queue);
        spin(&mut owner, &mut mirror, |a, _| {
            a.waiting.get(&path).is_some_and(|q| q.len() == 1)
        });
        assert_eq!(owner.state.session_file.as_deref(), next.to_str());
        assert_ne!(owner.agents.active, root);
        owner.shutdown_agents();
    }
    #[test]
    fn stale_owner_registry_is_ignored_and_closed_owner_does_not_replay_work() {
        let (directory, path) = location();
        let mut owner = app(&directory, &path);
        let mut mirror = app(&directory, &path);
        owner.client = Some(rpc::Client::stub());
        assert!(owner.attach_or_claim(&path));
        owner.publish_live();
        assert!(!mirror.attach_or_claim(&path));
        spin(&mut owner, &mut mirror, |_, b| {
            b.sync.revisions.contains_key(&path)
        });
        owner.client.take();
        owner.sync = WindowSync::new(&owner.ctx, true);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while mirror.connected && std::time::Instant::now() < deadline {
            mirror.poll_window_sync();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!mirror.connected);
        assert!(mirror.client.is_none());
        assert!(mirror.state.error.as_deref().unwrap().contains("closed"));
        assert!(mirror.attach_or_claim(&path));
    }
    #[test]
    fn closing_attached_only_window_does_not_prompt_to_stop_owner() {
        let (directory, path) = location();
        let mut owner = app(&directory, &path);
        let mut mirror = app(&directory, &path);
        owner.client = Some(rpc::Client::stub());
        assert!(owner.attach_or_claim(&path));
        owner.state.busy = true;
        owner.publish_live();
        assert!(!mirror.attach_or_claim(&path));
        spin(&mut owner, &mut mirror, |_, b| b.state.busy);
        let _ = mirror.ctx.clone().run(
            egui::RawInput {
                viewports: std::iter::once((
                    egui::ViewportId::ROOT,
                    egui::ViewportInfo {
                        events: vec![egui::ViewportEvent::Close],
                        ..Default::default()
                    },
                ))
                .collect(),
                ..Default::default()
            },
            |ctx| mirror.agent_close_guard(ctx),
        );
        assert!(!mirror.agents.close_prompt);
        assert!(owner.client.is_some());
    }
    #[test]
    fn shared_effort_changes_route_while_busy_and_revalidate_the_owners_supported_levels() {
        let (directory, path) = location();
        let mut owner = app(&directory, &path);
        let mut mirror = app(&directory, &path);
        let (client, _events, requests) = rpc::Client::stub_with_records();
        owner.client = Some(client);
        owner.loading_chat = false;
        owner.state.busy = true;
        owner.state.levels = vec!["low".into(), "high".into()];
        owner.state.thinking = "high".into();
        assert!(owner.attach_or_claim(&path));
        owner.publish_live();
        assert!(!mirror.attach_or_claim(&path));
        spin(&mut owner, &mut mirror, |_, b| {
            b.state.busy && b.state.levels.len() == 2
        });
        mirror.send(json!({"type":"set_thinking_level","level":"low"}));
        let received = std::cell::RefCell::new(None);
        spin(&mut owner, &mut mirror, |_, _| {
            if received.borrow().is_none() {
                *received.borrow_mut() = requests.try_recv().ok();
            }
            received.borrow().is_some()
        });
        let sent = received.into_inner().unwrap();
        assert_eq!(sent["type"], "set_thinking_level");
        assert_eq!(sent["level"], "low");
        assert!(owner.state.busy);
        assert_eq!(owner.state.thinking, "high");
        assert!(requests.try_recv().is_err());
        assert!(
            owner
                .apply_remote_action(
                    &path,
                    0,
                    Action::Rpc(json!({"type":"set_thinking_level","level":"xhigh"}))
                )
                .is_err()
        );
        owner.state.compacting = true;
        assert!(
            owner
                .apply_remote_action(
                    &path,
                    0,
                    Action::Rpc(json!({"type":"set_thinking_level","level":"low"}))
                )
                .is_err()
        );
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn shared_rpc_cannot_change_sessions_or_approve_obsolete_dialogs() {
        let (directory, path) = location();
        let mut owner = app(&directory, &path);
        owner.client = Some(rpc::Client::stub());
        assert!(owner.attach_or_claim(&path));
        assert!(
            owner
                .apply_remote_action(
                    &path,
                    0,
                    Action::Rpc(json!({"type":"switch_session","sessionPath":"elsewhere"}))
                )
                .is_err()
        );
        assert!(
            owner
                .apply_remote_action(
                    &path,
                    0,
                    Action::Rpc(json!({"type":"prompt","message":"unrestricted user prompt"}))
                )
                .is_err()
        );
        assert!(
            owner
                .apply_remote_action(
                    &path,
                    0,
                    Action::Rpc(
                        json!({"type":"extension_ui_response","id":"obsolete","confirmed":true})
                    )
                )
                .is_err()
        );
        owner
            .state
            .dialogs
            .push_back(json!({"id":"approval","method":"confirm"}));
        assert!(
            owner
                .apply_remote_action(
                    &path,
                    0,
                    Action::Rpc(
                        json!({"type":"extension_ui_response","id":"approval","confirmed":true})
                    )
                )
                .is_ok()
        );
        assert!(owner.state.dialogs.is_empty());
    }
    #[test]
    fn exclusive_session_lock_is_respected_by_a_separate_process() {
        let (directory, path) = location();
        let ctx = egui::Context::default();
        let mut owner = WindowSync::new_at(&ctx, false, Some(directory.join("registry")));
        assert!(owner.claim(&path).unwrap());
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::sync::tests::second_process_lock_probe",
                "--nocapture",
            ])
            .env("PUI_TEST_SYNC_DIR", directory.join("registry"))
            .env("PUI_TEST_SYNC_PATH", path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[test]
    fn deletion_cannot_take_another_windows_session_and_releases_its_own_claim() {
        let (directory, path) = location();
        let mut owner = WindowSync::new_at(
            &egui::Context::default(),
            false,
            Some(directory.join("registry")),
        );
        let other = WindowSync::new_at(
            &egui::Context::default(),
            false,
            Some(directory.join("registry")),
        );
        assert!(owner.claim(&path).unwrap());
        assert!(other.deletion_lock(&path).is_err());
        assert!(owner.deletion_lock(&path).unwrap().is_none());
        owner.forget_deleted(&path);
        let lock = other.deletion_lock(&path).unwrap();
        assert!(lock.is_some());
        assert!(owner.deletion_lock(&path).is_err());
    }
    #[test]
    fn second_process_lock_probe() {
        let Some(dir) = std::env::var_os("PUI_TEST_SYNC_DIR") else {
            return;
        };
        let path = PathBuf::from(std::env::var_os("PUI_TEST_SYNC_PATH").unwrap());
        let mut other = WindowSync::new_at(&egui::Context::default(), false, Some(dir.into()));
        assert!(
            !other.claim(&path).unwrap(),
            "second process must not start another Pi writer"
        );
    }
}
