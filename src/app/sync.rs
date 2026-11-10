//! Cross-process live-session ownership. One Pi writer; other windows observe and
//! send explicit actions to its owner. Loopback transport is authenticated with a
//! per-window token in a user-only registry. No idle timers or filesystem polling.
use super::*;
use fs2::FileExt;
use notify::Watcher;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    hash::{Hash, Hasher},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::Arc,
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
    pub notifications: notifications::NotificationSettings,
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
    Fetch(PathBuf, Endpoint, u64),
    Act(PathBuf, Endpoint, u64, Action),
    Stop,
}
enum ResultEvent {
    Registry(Vec<Endpoint>),
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
    endpoints: BTreeMap<PathBuf, Endpoint>,
    revisions: BTreeMap<PathBuf, u64>,
    pending: BTreeSet<PathBuf>,
    tx: mpsc::Sender<Work>,
    rx: Receiver<ResultEvent>,
    commands: Receiver<RemoteCommand>,
    _watcher: Option<notify::RecommendedWatcher>,
    _presence: Option<File>,
    worker: Option<std::thread::JoinHandle<()>>,
    listener: Option<std::thread::JoinHandle<()>>,
    stopped: Arc<std::sync::atomic::AtomicBool>,
    port: u16,
    pub error: Option<String>,
    mirror: Option<PathBuf>,
    unsent: Vec<run_status::WaitingMessage>,
}
fn key(path: &Path) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    format!("{:016x}", h.finish())
}
pub(super) fn identity(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
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
fn registry(directory: &Path, own: &str) -> Vec<Endpoint> {
    std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()?.to_str()? != "json" || path.file_stem()?.to_str()? == own {
                return None;
            }
            let id = path.file_stem()?.to_str()?;
            let presence = File::open(directory.join(format!("{id}.owner"))).ok()?;
            if presence.try_lock_exclusive().is_ok() {
                return None;
            }
            let file = File::open(path).ok()?;
            serde_json::from_reader(file.take(1024 * 1024)).ok()
        })
        .collect()
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
            this.worker = Some(std::thread::spawn(move || {
                let metadata = dir.join(format!("{id}.json"));
                while let Ok(job) = work.recv() {
                    match job {
                        Work::Stop => break,
                        Work::Publish(live) => {
                            endpoint.paths.retain(|p| p.path != live.path);
                            endpoint.paths.push(live);
                            let temp = dir.join(format!("{}.tmp", endpoint.id));
                            if let Ok(mut file) = private_open(&temp) {
                                let _ = file.set_len(0);
                                if serde_json::to_writer(&mut file, &endpoint).is_ok() {
                                    let _ = std::fs::rename(&temp, &metadata);
                                }
                            }
                        }
                        Work::Scan => {
                            let _ =
                                results.send(ResultEvent::Registry(registry(&dir, &endpoint.id)));
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
            let repaint = ctx.clone();
            let mut watcher =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    if let Ok(event) = event {
                        if (event.kind.is_create()
                            || event.kind.is_modify()
                            || event.kind.is_remove())
                            && event
                                .paths
                                .iter()
                                .any(|p| p.extension().is_some_and(|e| e == "json"))
                        {
                            let _ = tx.send(Work::Scan);
                            repaint.request_repaint();
                        }
                    }
                })
                .map_err(|_| "Cannot watch other Pui windows")?;
            watcher
                .watch(&directory, notify::RecursiveMode::NonRecursive)
                .map_err(|_| "Cannot watch window registry")?;
            this.directory = Some(directory);
            this._watcher = Some(watcher);
            let _ = this.tx.send(Work::Scan);
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
    pub fn detach_mirror(&mut self) {
        self.mirror = None;
        self.revisions.clear();
    }
    pub fn owns(&self, path: &Path) -> bool {
        self.owned.contains_key(&identity(path))
    }
    pub fn peer(&self, path: &Path) -> bool {
        self.mirror.as_deref() == Some(identity(path).as_path()) && !self.owns(path)
    }
    pub fn claim(&mut self, path: &Path) -> Result<bool, String> {
        let path = identity(path);
        if self.owns(&path) {
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
                    for live in &endpoint.paths {
                        self.endpoints.insert(live.path.clone(), endpoint.clone());
                    }
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
            .filter(|(path, _)| !self.owns(path))
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
    }
}

impl Pui {
    pub(super) fn other_window_status(&self, path: &Path) -> Option<&str> {
        let path = identity(path);
        if self.sync.owns(&path) {
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
                if self.draft.is_empty()&&self.draft_images.is_empty(){let message=self.sync.unsent.remove(0);self.draft=message.text;self.draft_images.set_images(message.images);self.focus_composer=true;}else{self.state.error=Some("Clear the current draft first. Retained messages are not automatically retried.".into());}
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
        if !self.sync.owns(&path) && !self.attach_or_claim(&path) {
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
        self.state.error.hash(&mut hash);
        self.state.notice.hash(&mut hash);
        format!("{:?}", self.state.dialogs).hash(&mut hash);
        self.prefs.settings.payload().to_string().hash(&mut hash);
        self.prefs.settings.delivery.label().hash(&mut hash);
        self.prefs.notifications.sound.hash(&mut hash);
        self.prefs.notifications.desktop.hash(&mut hash);
        self.state.session_name.hash(&mut hash);
        self.state.model_name.hash(&mut hash);
        self.prefs.cwd.hash(&mut hash);
        self.state.permission_classifier.hash(&mut hash);
        self.state.permission_cost.to_bits().hash(&mut hash);
        self.state.compaction_settings_error.hash(&mut hash);
        self.state.cost.to_bits().hash(&mut hash);
        self.state.context_tokens.map(f64::to_bits).hash(&mut hash);
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
            title: self.state.session_name.clone(),
            cwd: self.prefs.cwd.clone(),
            model: self.state.model_name.clone(),
        }));
    }
    pub(super) fn poll_window_sync(&mut self) {
        let events: Vec<_> = self.sync.rx.try_iter().collect();
        for event in events {
            match event {
                ResultEvent::Registry(endpoints) => {
                    let before: BTreeSet<_> = self.sync.endpoints.keys().cloned().collect();
                    self.sync.endpoints.clear();
                    for endpoint in endpoints {
                        for live in &endpoint.paths {
                            self.sync
                                .endpoints
                                .insert(live.path.clone(), endpoint.clone());
                        }
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
                        self.state = snapshot.state;
                        self.state.notice = Some(format!(
                            "Shared chat · {} in another Pui window",
                            snapshot.label
                        ));
                        self.waiting.insert(path.clone(), snapshot.waiting);
                        self.prefs.settings = snapshot.settings;
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
            settings: self.prefs.settings.clone(),
            notifications: self.prefs.notifications,
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
        if self.loading_chat {
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
                        let message = queue.remove(index);
                        let kind = if self.state.ready() {
                            "prompt"
                        } else {
                            "steer"
                        };
                        if !self.deliver_message(message.clone(), kind) {
                            self.waiting
                                .entry(queue_path.clone())
                                .or_default()
                                .insert(index, message);
                            return Err("Owner could not send; message retained".into());
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
                let changed = self.prefs.settings.payload() != settings.payload();
                self.prefs.settings = settings;
                if changed {
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
                if kind == "clear_queue" {
                    self.waiting.remove(&queue_path);
                }
                if kind == "extension_ui_response" {
                    self.state.dialogs.retain(|d| d["id"] != record["id"]);
                }
                self.send(record);
            }
        }
        Ok(edited)
    }
}

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
