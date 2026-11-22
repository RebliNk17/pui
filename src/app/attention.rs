//! Bounded, locally observed attention. Never infer unread from saved history.
use super::*;
use std::{
    collections::VecDeque,
    hash::{Hash, Hasher},
    path::Path,
};

mod decoder;
mod dock;
mod search;
mod sidebar;
mod ui;
pub(super) use decoder::Stored;
pub(super) use dock::Window as DockWindow;
pub(super) use search::SearchEvent;
pub(super) use sidebar::paint_unread;

#[cfg(test)]
mod dropdown_tests;
#[cfg(test)]
mod tests;

const MAX_RECORDS: usize = 256;
const MAX_KEY_BYTES: usize = 4096;
const MAX_AGE_SECONDS: u64 = 30 * 24 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Kind {
    Completed,
    NeedsInput,
    Failed,
}
impl Serialize for Kind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            Self::Completed => "completed",
            Self::NeedsInput => "needs_input",
            Self::Failed => "failed",
        })
    }
}
impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Completed => "Completed",
            Self::NeedsInput => "Needs input",
            Self::Failed => "Failed",
        }
    }
}

/// The only durable/synchronized payload: no transcript, title, error or approval data.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub(super) struct Metadata {
    path: PathBuf,
    cwd: PathBuf,
    revision: String,
    kind: Kind,
    at: u64,
    read: bool,
}

impl Metadata {
    pub(super) fn unread_snapshot(mut self) -> Self {
        // Reads are window-local; only an observed event crosses windows.
        self.read = false;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Owner {
    Saved(PathBuf),
    Runtime(String),
}
#[derive(Clone)]
struct Entry {
    owner: Owner,
    metadata: Metadata,
}
impl Entry {
    fn unread_at(&self, time: u64) -> bool {
        !self.metadata.read
            && self.metadata.at <= time
            && time.saturating_sub(self.metadata.at) <= MAX_AGE_SECONDS
    }
}
#[derive(Default)]
pub(super) struct Attention {
    entries: VecDeque<Entry>,
    pub open: bool,
    focus_close: bool,
    trigger_focus: Option<egui::Id>,
    restore_focus: Option<egui::Id>,
    trigger_owner: Option<Owner>,
    last_button: Option<egui::Id>,
    reverse_from: Option<egui::Id>,
    reverse_focus: Option<egui::Id>,
    button_hit: Option<(egui::Rect, egui::LayerId, egui::Rect, u64)>,
    anchor: Option<(egui::Rect, egui::LayerId)>,
    geometry: Option<(egui::Rect, f32, bool)>,
    pub(super) defer_layout: bool,
    pub(super) popup_rect: Option<egui::Rect>,
    focus_ids: Vec<egui::Id>,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub(super) fn lexical_identity(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}
pub(super) fn valid_path(path: &Path) -> bool {
    path.to_str().is_some_and(|s| {
        !s.is_empty() && s.len() <= MAX_KEY_BYTES && !s.chars().any(char::is_control)
    }) && path.is_absolute()
}
fn valid_metadata(metadata: &Metadata, time: u64) -> bool {
    valid_path(&metadata.path)
        && valid_path(&metadata.cwd)
        && uuid::Uuid::parse_str(&metadata.revision).is_ok()
        && metadata.at > 0
        && metadata.at <= time
        && time.saturating_sub(metadata.at) <= MAX_AGE_SECONDS
}
impl Attention {
    pub fn restore(records: Vec<Metadata>) -> Self {
        let mut this = Self::default();
        for record in records.into_iter().rev().take(MAX_RECORDS).rev() {
            if valid_metadata(&record, now()) {
                this.import(record);
            }
        }
        this
    }
    pub fn persisted(&self) -> Vec<Metadata> {
        self.entries
            .iter()
            .filter_map(|entry| match &entry.owner {
                Owner::Saved(_) if valid_metadata(&entry.metadata, now()) => {
                    Some(entry.metadata.clone())
                }
                _ => None,
            })
            .collect()
    }
    pub fn metadata(&self, path: &Path) -> Option<Metadata> {
        if self.entries.is_empty() {
            return None;
        }
        let path = lexical_identity(path);
        self.entries
            .iter()
            .find(|e| e.owner == Owner::Saved(path.clone()))
            .map(|e| e.metadata.clone())
    }
    pub fn import_for(&mut self, path: &Path, metadata: Metadata) {
        if lexical_identity(&metadata.path) == lexical_identity(path) {
            self.import(metadata);
        }
    }
    pub fn import(&mut self, mut metadata: Metadata) {
        if !valid_metadata(&metadata, now()) {
            return;
        }
        metadata.path = lexical_identity(&metadata.path);
        let owner = Owner::Saved(metadata.path.clone());
        if let Some(existing) = self.entries.iter().find(|e| e.owner == owner) {
            // A snapshot revision is not a new attention event. Keep this window's
            // explicit read watermark even if the owner has not read it there.
            if existing.metadata.revision == metadata.revision || existing.metadata.at > metadata.at
            {
                return;
            }
        }
        self.insert(Entry { owner, metadata });
    }
    fn insert(&mut self, entry: Entry) {
        self.entries.retain(|e| e.owner != entry.owner);
        self.entries.push_back(entry);
        while self.entries.len() > MAX_RECORDS {
            self.entries.pop_front();
        }
    }
    fn observe(&mut self, owner: Owner, cwd: &Path, kind: Kind) {
        if !valid_path(cwd) {
            return;
        }
        let path = match &owner {
            Owner::Saved(path) if !valid_path(path) => return,
            Owner::Saved(path) => path.clone(),
            Owner::Runtime(_) => PathBuf::new(),
        };
        self.insert(Entry {
            owner,
            metadata: Metadata {
                path,
                cwd: cwd.to_path_buf(),
                revision: uuid::Uuid::new_v4().to_string(),
                kind,
                at: now(),
                read: false,
            },
        });
    }
    fn promote(&mut self, id: &str, path: &Path) {
        if !valid_path(path) {
            return;
        }
        let owner = Owner::Runtime(id.into());
        let Some(index) = self.entries.iter().position(|e| e.owner == owner) else {
            return;
        };
        let mut entry = self.entries.remove(index).unwrap();
        entry.owner = Owner::Saved(lexical_identity(path));
        entry.metadata.path = lexical_identity(path);
        if self
            .entries
            .iter()
            .any(|e| e.owner == entry.owner && e.metadata.at > entry.metadata.at)
        {
            return;
        }
        self.insert(entry);
    }
    pub fn forget_path(&mut self, path: &Path) {
        let path = lexical_identity(path);
        self.entries
            .retain(|e| e.owner != Owner::Saved(path.clone()));
    }
    pub fn forget_agents(&mut self, ids: &std::collections::BTreeSet<String>) {
        self.entries
            .retain(|e| !matches!(&e.owner, Owner::Runtime(id) if ids.contains(id)));
    }
    fn mark_read(&mut self, owner: &Owner) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|e| &e.owner == owner)
            && !entry.metadata.read
        {
            entry.metadata.read = true;
            return true;
        }
        false
    }
    fn mark_all_read(&mut self) {
        for entry in &mut self.entries {
            entry.metadata.read = true;
        }
    }
    pub(super) fn unread(&self) -> usize {
        let time = now();
        self.entries.iter().filter(|e| e.unread_at(time)).count()
    }
}

/// Transient observation state travels with Chat, including parked chats. Request
/// IDs are hashed, not retained; reconnect/history/snapshots cannot synthesize runs.
#[derive(Default)]
pub(super) struct Observed {
    running: bool,
    failed: bool,
    failure_reported: bool,
    cancelled: bool,
    requests: VecDeque<u64>,
}
impl Observed {
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    fn event(&mut self, record: &Value) -> Option<Kind> {
        match protocol::string(record, "type") {
            "agent_start" if !self.running => {
                self.running = true;
                self.failed = false;
                self.failure_reported = false;
                self.cancelled = false;
            }
            "message_end" if self.running && record["message"]["role"] == "assistant" => {
                self.failed = record["message"]["stopReason"] == "error"
                    || record["message"]["errorMessage"].is_string();
                self.cancelled |= record["message"]["stopReason"] == "aborted";
            }
            "agent_settled" if self.running => {
                self.running = false;
                return if self.cancelled || self.failure_reported || record["aborted"] == true {
                    None
                } else if self.failed {
                    Some(Kind::Failed)
                } else {
                    Some(Kind::Completed)
                };
            }
            _ => {}
        }
        None
    }
    fn request(&mut self, record: &Value) -> bool {
        if record["type"] != "extension_ui_request"
            || !matches!(
                protocol::string(record, "method"),
                "confirm" | "select" | "input" | "editor"
            )
        {
            return false;
        }
        self.remember("request", protocol::string(record, "id"))
    }
    fn remember(&mut self, scope: &str, id: &str) -> bool {
        if id.is_empty() || id.len() > MAX_KEY_BYTES {
            return false;
        }
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        scope.hash(&mut hash);
        id.hash(&mut hash);
        let key = hash.finish();
        if self.requests.contains(&key) {
            return false;
        }
        self.requests.push_back(key);
        if self.requests.len() > MAX_RECORDS {
            self.requests.pop_front();
        }
        true
    }
    fn failure(&mut self) -> bool {
        if !self.running || self.cancelled || self.failure_reported {
            return false;
        }
        self.failed = true;
        self.failure_reported = true;
        true
    }
}

impl Pui {
    /// In-memory native preview only: no saved paths, providers or Git activity.
    pub(super) fn demo_attention(&mut self) {
        if !self.demo {
            return;
        }
        self.attention = Attention::default();
        self.attention_observed = Observed::default();
        self.state.session_file = None;
        self.state.session_name = "Synthetic catch-up preview".into();
        self.state.notice = Some("Synthetic demo only · no agent work was run.".into());
        self.prefs.hide_inspector = true;
        self.sessions.clear();
        let original = self.ensure_agent();
        self.observe_attention(Kind::Completed);
        let visible = self.take_chat();
        self.agents.parked.insert(original.clone(), visible);
        self.agents.active.clear();
        self.state = State {
            session_name: "Synthetic needs-input example".into(),
            notice: Some("Synthetic demo only · no pending approval or provider request.".into()),
            ..Default::default()
        };
        let preview = self.ensure_agent();
        self.observe_attention(Kind::NeedsInput);
        let parked = self.take_chat();
        self.agents.parked.insert(preview, parked);
        self.agents.active = original.clone();
        let visible = self.agents.parked.remove(&original).unwrap();
        self.put_chat(visible);
        self.attention.open = true;
        self.attention.focus_close = true;
        self.focus_composer = false;
    }
    fn attention_owner(&self) -> Option<Owner> {
        if let Some(path) = self.state.session_file.as_ref().map(PathBuf::from) {
            return valid_path(&path).then(|| Owner::Saved(self.sync.attention_identity(&path)));
        }
        (!self.agents.active.is_empty() && self.agents.active.len() <= MAX_KEY_BYTES)
            .then(|| Owner::Runtime(self.agents.active.clone()))
    }
    fn observe_attention(&mut self, kind: Kind) {
        self.ensure_agent();
        self.promote_attention();
        if let Some(owner) = self.attention_owner() {
            self.attention.observe(owner, &self.prefs.cwd, kind);
        }
    }
    pub(super) fn promote_attention(&mut self) {
        if let Some(path) = self.state.session_file.as_ref() {
            let identity = self.sync.attention_identity(Path::new(path));
            self.attention.promote(&self.agents.active, &identity);
        }
    }
    pub(super) fn attention_event(&mut self, record: &Value) {
        if self.demo || self.is_mirror() || self.client.is_none() {
            return;
        }
        if record["type"] == "response"
            && record["command"] == "prompt"
            && record["success"] == false
            && self
                .attention_observed
                .remember("prompt_failed", protocol::string(record, "id"))
        {
            self.attention_observed.failure_reported = true;
            self.observe_attention(Kind::Failed);
        }
        if let Some(kind) = self.attention_observed.event(record) {
            self.observe_attention(kind);
        }
    }
    pub(super) fn attention_request(&mut self, record: &Value) {
        if !self.demo
            && !self.is_mirror()
            && self.client.is_some()
            && self.attention_observed.request(record)
        {
            self.observe_attention(Kind::NeedsInput);
        }
    }
    pub(super) fn attention_failure(&mut self) {
        if !self.demo
            && !self.is_mirror()
            && self.client.is_some()
            && self.attention_observed.failure()
        {
            self.observe_attention(Kind::Failed);
        }
    }
    pub(super) fn attention_rendered(&mut self, ui: &egui::Ui) {
        let ctx = ui.ctx();
        if !self.agents.background
            && !self.loading_chat
            && !self.attention.open
            && !self.settings_open
            && !self.kanban.open
            && !self.automation.open
            && !self.tasks.open
            && self.question_designs.is_none()
            && !self.changes.review_comments.confirming()
            && !self.section_view.confirming()
            && !self.tasks.attachment_confirming()
            && !self.task_operations_confirming()
            && !self.agents.close_prompt
            && !self.terminals.confirming_stop()
            && !self.history_view.is_open()
            && !self.deletion.confirming()
            && !self.deletion.in_progress()
            && !self.automation.confirming()
            && self.bypass_pending.is_none()
            && ctx.memory(|m| m.is_above_modal_layer(ui.layer_id()))
            && !ctx.memory(|m| m.top_modal_layer().is_some())
            && !prompt_ui::modal_owns_current_pass(ctx)
            && !self.search_open
            && !self.menu.about
            && !self.updates.open
            && !self.transcript.gallery.is_open()
            && !self.draft_images.gallery.is_open()
            && !egui::Popup::is_any_open(ctx)
            && ctx.input(|i| i.focused)
            && let Some(owner) = self.attention_owner()
            && self.attention.mark_read(&owner)
        {
            // The sidebar/header painted earlier in this pass. Refresh once
            // for a real read transition, never continuously for visible chat.
            ctx.request_repaint();
        }
    }
}
